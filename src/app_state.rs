//! Application State Management - Multi-Agent Knowledge Graph Coordinator
//! 
//! AppState is the central coordination hub for Cymbiont's multi-agent knowledge graph engine.
//! It orchestrates graphs, agents, transactions, WebSocket connections, and authentication
//! while delegating actual implementation to specialized components.
//! 
//! ## Core Responsibilities
//! 
//! ### Graph Lifecycle Management
//! - **Multi-Graph Coordination**: Manages multiple isolated knowledge graphs simultaneously
//! - **Resource Management**: Direct HashMap of graph managers without bundling
//! - **State Management**: Open/closed graph states with automatic recovery
//! - **Complete Workflows**: `create_new_graph()`, `delete_graph_completely()`, `open_graph()`, `close_graph()`
//! 
//! ### Agent Lifecycle Management  
//! - **Agent Coordination**: Manages active agents in memory with persistent registry
//! - **Authorization System**: Runtime permission checks via AgentRegistry
//! - **Prime Agent**: Auto-created default agent with full graph access
//! - **Complete Workflows**: `activate_agent()`, `deactivate_agent()`, agent loading/saving
//! 
//! ### Transaction Coordination
//! - **Global WAL**: Single transaction log for all operations
//! - **ACID Guarantees**: TransactionCoordinator ensures consistency
//! - **Crash Recovery**: `run_unified_recovery()` replays all pending transactions
//! - **Graceful Shutdown**: Coordinates transaction completion
//! 
//! ### Server Mode Features
//! - **WebSocket Management**: Active connection tracking with agent association
//! - **Authentication**: Token-based auth with automatic rotation
//! - **Async Processing**: High-throughput command execution
//! 
//! ## Key Methods
//! 
//! ### Initialization
//! - `new_with_config()` - Factory method with config loading
//! 
//! ### Graph Operations
//! - `create_new_graph()` - Complete graph creation with prime agent authorization
//! - `delete_graph_completely()` - Archive graph and remove from memory
//! - `open_graph()`, `close_graph()` - Explicit state management
//! - `get_or_create_graph_manager()` - Lazy resource loading
//! 
//! ### Agent Operations
//! - `activate_agent()`, `deactivate_agent()` - Memory management with WAL rebuild
//! 
//! ### Transaction & Recovery
//! - `run_unified_recovery()` - Replay all pending transactions from global WAL
//! 
//! ### Shutdown Coordination
//! - `initiate_graceful_shutdown()` - Stop new transactions, track active ones
//! - `wait_for_transactions()` - Wait for completion with timeout
//! - `cleanup_and_save()` - Save all graphs, agents, and registries
//! 
//! ## Architecture Patterns
//! 
//! ### Registry Pattern
//! Both GraphRegistry and AgentRegistry serve as single sources of truth for
//! metadata, with bidirectional consistency between authorization mappings.
//! 
//! ### Delegation Pattern
//! AppState coordinates but doesn't implement domain logic:
//! - Graph operations → GraphOps trait  
//! - Graph storage → GraphManager
//! - Transactions → TransactionCoordinator
//! - Agent interactions → Agent struct
//! - Authorization → AgentRegistry
//! - Recovery execution → RecoveryContext in storage module
//! 
//! ### Concurrency Design
//! - **Per-agent locking**: Each agent has its own RwLock, enabling parallel operations
//! - **HashMap discipline**: The `agents` HashMap lock is ONLY for Arc management (get/insert/remove)
//! - **Operation pattern**: Get Arc from HashMap → drop HashMap lock → work with individual agent
//! - Async RwLocks for frequently accessed resources (graphs, connections)
//! - Sync RwLocks for registries requiring immediate consistency
//! - Double-check pattern prevents race conditions in resource creation
//! - Debug assertions detect lock contention during development
//! 
//! ### Lock Ordering Rules
//! To prevent deadlocks when acquiring MULTIPLE locks simultaneously:
//! - **Registry locks**: Always acquire `graph_registry` before `agent_registry`
//!   (use `lock_registries_for_write()` helper)
//! - **Agent locks**: The `agents` HashMap lock should only be held briefly to get/insert/remove
//!   Arc references. Never hold it while acquiring individual agent locks.
//! - **Graph locks**: Similar pattern - brief HashMap access, then work with individual resources
//! 
//! Note: We never actually hold multiple agent locks or mix registry/agent locks simultaneously.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, error};
use uuid::Uuid;

use crate::error::*;

use crate::{
    config::Config,
    graph::graph_manager::GraphManager,
    graph::graph_registry::GraphRegistry,
    agent::agent::Agent,
    agent::agent_registry::AgentRegistry,
    storage::{
        TransactionLog, TransactionCoordinator,
        recovery::export_all_json,
    },
    lock::AsyncRwLockExt,
    server::{
        auth::initialize_auth,
        message_queue,
        websocket::WsConnection,
    },
};


/// Central application state that coordinates all Cymbiont components
///
/// ARCHITECTURAL RULE: AppState is a pure resource container.
/// DO NOT add helper methods here. All fields are public.
/// Business logic belongs in domain modules (GraphOps, Agent, registries).
pub struct AppState {
    // Core resources - directly owned by AppState
    pub graph_managers: Arc<RwLock<HashMap<Uuid, RwLock<GraphManager>>>>,
    pub agents: Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
    
    // Single global transaction coordinator for all operations
    pub transaction_coordinator: Arc<TransactionCoordinator>,
    
    // Registries for metadata and persistence
    pub graph_registry: Arc<RwLock<GraphRegistry>>,
    pub agent_registry: Arc<RwLock<AgentRegistry>>,
    
    pub config: Config,
    pub data_dir: PathBuf,  // Resolved absolute path
    
    // Server-specific components (optional)
    pub ws_ready_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
    pub auth_token: Arc<RwLock<Option<String>>>,  // Authentication token
    
    // Test infrastructure
    pub operation_freeze: Arc<RwLock<bool>>,  // Freeze operations after transaction creation
    
    // Shutdown coordination
    pub shutdown_initiated: Arc<AtomicBool>,  // Flag to prevent new transactions
}

impl AppState {
    
    /// Create new AppState with pre-loaded config (avoids duplicate config loading)
    pub async fn new_with_config(mut config: crate::config::Config, data_dir_override: Option<String>, with_server: bool) -> Result<Arc<Self>> {
        // Apply data_dir override if provided
        if let Some(cli_data_dir) = &data_dir_override {
            config.data_dir = cli_data_dir.clone();
        }
        
        Self::new_internal_with_config(config, with_server).await
    }
    
    async fn new_internal_with_config(config: crate::config::Config, with_server: bool) -> Result<Arc<Self>> {
        
        // Initialize data directory
        let data_dir = if std::path::Path::new(&config.data_dir).is_absolute() {
            PathBuf::from(&config.data_dir)
        } else {
            std::env::current_dir()
                .map_err(|e| CymbiontError::Other(format!("Failed to get current directory: {e}")))?
                .join(&config.data_dir)
        };
        fs::create_dir_all(&data_dir)
            .map_err(|e| CymbiontError::Other(format!("Failed to create data directory: {e}")))?;
        
        // Initialize empty registries (will be populated from WAL)
        let graph_registry = Arc::new(RwLock::new(GraphRegistry::new()));
        let agent_registry = Arc::new(RwLock::new(AgentRegistry::new()));
        
        // Initialize global transaction coordinator
        let transaction_log_dir = data_dir.join("transaction_log");
        fs::create_dir_all(&transaction_log_dir)?;
        let wal = Arc::new(TransactionLog::new(transaction_log_dir)?);
        let transaction_coordinator = Arc::new(TransactionCoordinator::new(wal));
        
        // Initialize managers and agents - owned directly by AppState
        let graph_managers = Arc::new(RwLock::new(HashMap::new()));
        let agents = Arc::new(RwLock::new(HashMap::new()));
        
        // Set resources for registries (they'll get AppState reference later)
        {
            let mut graph_reg = graph_registry.write_or_panic("set graph registry resources").await;
            graph_reg.set_resources(&data_dir, transaction_coordinator.clone());
            // No longer giving managers to registry - AppState owns them
        }
        {
            let mut agent_reg = agent_registry.write_or_panic("set agent registry resources").await;
            agent_reg.set_resources(&data_dir, transaction_coordinator.clone());
            // No longer giving agents to registry - AppState owns them
        }
        
        // Create WebSocket connections if server mode
        let ws_connections = if with_server {
            Some(Arc::new(RwLock::new(HashMap::new())))
        } else {
            None
        };
        
        let app_state = Arc::new(AppState {
            graph_managers,
            agents,
            transaction_coordinator,
            graph_registry,
            agent_registry,
            config,
            data_dir: data_dir.clone(),
            ws_ready_tx: std::sync::Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
            operation_freeze: Arc::new(RwLock::new(false)),
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
        });
        
        // Give registries access to AppState for resource access
        {
            let mut graph_reg = app_state.graph_registry.write_or_panic("set graph registry app_state").await;
            graph_reg.set_app_state(&app_state);
        }
        {
            let mut agent_reg = app_state.agent_registry.write_or_panic("set agent registry app_state").await;
            agent_reg.set_app_state(&app_state);
        }
        
        // Give transaction coordinator access to freeze state for testing
        app_state.transaction_coordinator.set_freeze_state(app_state.operation_freeze.clone()).await;
        
        // Note: Graphs and agents will be loaded from WAL during startup in main.rs
        
        // Initialize authentication if in server mode
        if with_server {
            let token = initialize_auth(&app_state).await?;
            if !app_state.config.auth.disabled {
                let mut token_guard = app_state.auth_token.write_or_panic("initialize auth token").await;
                *token_guard = Some(token);
            }
        }
        
        // Initialize message queue with AppState reference
        #[allow(deprecated)] // Temporary until CQRS refactor
        message_queue::initialize_message_queue(&app_state);
        
        Ok(app_state)
    }
    // NOTE: All business logic methods have been removed from AppState.
    // AppState is now a pure resource container. Access the public fields directly.
    // For operations:
    // - Graph operations: Use GraphOps trait (implemented on Arc<AppState>)
    // - Agent operations: Access agents through app_state.agents
    // - Registry operations: Access registries through app_state.graph_registry/agent_registry
    
    /// Cleanup and export all data on shutdown
    pub async fn cleanup_and_save(&self) {
        // Shutdown agent message queue workers first
        #[allow(deprecated)] // Temporary until CQRS refactor
        message_queue::shutdown_workers().await;
        
        // Close all WebSocket connections
        if let Some(ref connections) = self.ws_connections {
            let mut conn_map = connections.write_or_panic("cleanup connections").await;
            let connection_count = conn_map.len();
            
            if connection_count > 0 {
                use axum::extract::ws::Message;
                
                // Send Close frame to all connections before shutting down
                for (_, conn) in conn_map.iter() {
                    // Send WebSocket Close frame
                    let close_msg = Message::Close(None);
                    let _ = conn.sender.send(close_msg);
                    
                    // Then send shutdown signal
                    let _ = conn.shutdown_tx.send(true);
                }
                
                // Clear the connections
                conn_map.clear();
                drop(conn_map);
                
                // Give tasks a moment to shut down gracefully
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        
        // Export all data to JSON for debugging
        if let Err(e) = export_all_json(self).await {
            error!("Failed to export JSON: {}", e);
        }
        
        // Flush and close global transaction log
        if let Err(e) = self.transaction_coordinator.close().await {
            error!("Failed to close global transaction log: {}", e);
        }
    }
    
    /// Initiate graceful shutdown on the global transaction coordinator
    /// Returns the count of active transactions
    pub async fn initiate_graceful_shutdown(&self) -> usize {
        // Set the local shutdown flag to prevent new transactions
        self.shutdown_initiated.store(true, Ordering::Release);
        
        // Initiate shutdown on the global coordinator
        let active_count = self.transaction_coordinator.initiate_shutdown().await;
        
        if active_count > 0 {
            info!("⏳ {} active transactions to complete", active_count);
        }
        
        active_count
    }
    
    /// Wait for all transactions to complete
    /// Returns true if all completed, false if timeout
    pub async fn wait_for_transactions(&self, timeout: Duration) -> bool {
        self.transaction_coordinator.wait_for_completion(timeout).await
    }
    
    /// Force flush the global transaction coordinator for immediate shutdown
    pub async fn force_flush_transactions(&self) {
        if let Err(e) = self.transaction_coordinator.force_shutdown().await {
            error!("Failed to force flush global transaction log: {}", e);
        }
    }
    
}