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
//! - **Resource Bundling**: Each graph bundles GraphManager + TransactionCoordinator atomically
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
//! - **Per-Graph WAL**: Each graph has isolated write-ahead logging
//! - **ACID Guarantees**: `with_graph_transaction()` wraps operations
//! - **Crash Recovery**: `run_graph_recovery()` for specific graphs, `run_all_graphs_recovery()` for startup
//! - **Graceful Shutdown**: Coordinates transaction completion across all graphs
//! 
//! ### Server Mode Features
//! - **WebSocket Management**: Active connection tracking with agent association
//! - **Authentication**: Token-based auth with automatic rotation
//! - **Async Processing**: High-throughput command execution
//! 
//! ## Key Methods
//! 
//! ### Initialization
//! - `new_cli()`, `new_server()` - Factory methods for different runtime modes
//! 
//! ### Graph Operations
//! - `create_new_graph()` - Complete graph creation with prime agent authorization
//! - `delete_graph_completely()` - Archive graph and remove from memory
//! - `open_graph()`, `close_graph()` - Explicit state management
//! - `get_or_create_graph_manager()` - Lazy resource loading
//! 
//! ### Agent Operations
//! - `activate_agent()`, `deactivate_agent()` - Memory management
//! - `get_or_load_agent()` - Lazy agent loading
//! 
//! ### Transaction & Recovery
//! - `with_graph_transaction()` - ACID wrapper for graph operations
//! - `run_graph_recovery()` - Replay pending transactions for specific graph
//! - `run_all_graphs_recovery()` - Startup recovery for all graphs (both open and closed)
//! 
//! ### Shutdown Coordination
//! - `initiate_graceful_shutdown()` - Stop new transactions, track active ones
//! - `wait_for_transactions()` - Wait for completion with timeout
//! - `cleanup_and_save()` - Save all graphs, agents, and registries
//! 
//! ## Architecture Patterns
//! 
//! ### Resource Bundling
//! Graph resources are bundled (GraphManager + TransactionCoordinator) to ensure
//! they're always created/destroyed atomically, preventing inconsistent states.
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
use std::sync::{Arc, RwLock as SyncRwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, error, warn};
use uuid::Uuid;

use crate::error::*;

use crate::{
    agent::agent::Agent,
    graph_manager::GraphManager,
    config::Config,
    storage::{GraphRegistry, AgentRegistry, TransactionLog, TransactionCoordinator, graph_registry::GraphInfo, transaction},
    lock::{RwLockExt, AsyncRwLockExt, lock_registries_for_write},
};

// Re-export the real WsConnection from server module
pub use crate::server::websocket::WsConnection;

/// Bundle of resources for a single graph
/// This ensures graph manager and transaction coordinator are always created/destroyed together
pub struct GraphResources {
    pub manager: RwLock<GraphManager>,
    pub coordinator: Arc<TransactionCoordinator>,
}

/// Central application state that coordinates all Cymbiont components
pub struct AppState {
    // Core graph management - bundled resources ensure consistency
    pub graph_resources: Arc<RwLock<HashMap<Uuid, GraphResources>>>,
    pub graph_registry: Arc<SyncRwLock<GraphRegistry>>,
    
    // Agent management - parallel to graph management
    pub agents: Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,  // Active agents with per-agent locks
    pub agent_registry: Arc<SyncRwLock<AgentRegistry>>,  // Agent lifecycle management
    
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
        
        // Initialize graph registry
        let registry_path = data_dir.join("graph_registry.json");
        let mut registry = GraphRegistry::load_or_create(&registry_path, &data_dir)
            ?;
        
        // Ensure at least one graph is open
        registry.ensure_graph_open()
            ?;
        
        let graph_registry = Arc::new(SyncRwLock::new(registry));
        
        // Initialize agent registry
        let agent_registry_path = data_dir.join("agent_registry.json");
        let mut agent_registry = AgentRegistry::load_or_create(&agent_registry_path, &data_dir)
            ?;
        
        // Ensure prime agent exists (creates it on first run)
        agent_registry.ensure_default_agent()
            ?;
        
        
        // Save agent registry after potential prime agent creation
        agent_registry.save()
            ?;
        
        let agent_registry = Arc::new(SyncRwLock::new(agent_registry));
        
        // Initialize graph resources map (managers + coordinators bundled)
        let graph_resources = Arc::new(RwLock::new(HashMap::new()));
        
        // Initialize agents map
        let agents = Arc::new(RwLock::new(HashMap::new()));
        
        
        // Create WebSocket connections if server mode
        let ws_connections = if with_server {
            Some(Arc::new(RwLock::new(HashMap::new())))
        } else {
            None
        };
        
        // Get the open graphs from registry
        let initial_open_graphs = {
            let registry = graph_registry.read_or_panic("app state init - read graph registry");
            registry.get_open_graphs()
        };
        
        let app_state = Arc::new(AppState {
            graph_resources,
            graph_registry,
            agents,
            agent_registry,
            config,
            data_dir: data_dir.clone(),
            ws_ready_tx: std::sync::Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
            operation_freeze: Arc::new(RwLock::new(false)),
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
        });
        
        // Load all open graphs
        for graph_id in initial_open_graphs {
            app_state.get_or_create_graph_manager(&graph_id).await?;
        }
        
        // Load all active agents (ensure at least prime agent is active)
        let initial_active_agents = {
            let mut registry = app_state.agent_registry.write_or_panic("app state init - write agent registry");
            
            // If no agents are active, activate the prime agent
            let active_agents = registry.get_active_agents();
            if active_agents.is_empty() {
                if let Some(prime_id) = registry.get_prime_agent_id() {
                    registry.activate_agent(&prime_id)
                        .map_err(|e| CymbiontError::Other(format!("Failed to activate prime agent: {e:?}")))?;
                    // Save after activation
                    registry.save()
                        ?;
                }
            }
            
            registry.get_active_agents()
        };
        
        for agent_id in initial_active_agents {
            if let Err(e) = app_state.get_or_load_agent(&agent_id).await {
                error!("Failed to load agent {}: {}", agent_id, e);
                // Continue loading other agents even if one fails
            } else {
            }
        }
        
        // Initialize authentication if in server mode
        if with_server {
            use crate::server::auth::initialize_auth;
            let token = initialize_auth(&app_state).await?;
            if !app_state.config.auth.disabled {
                let mut token_guard = app_state.auth_token.write_or_panic("initialize auth token").await;
                *token_guard = Some(token);
            }
        }
        
        Ok(app_state)
    }
    
    
    /// Get or create graph resources (manager + coordinator) for the given graph ID
    /// 
    /// Resources are bundled together to ensure they're always created/destroyed atomically.
    // TODO 💾: Implement LRU cache eviction for graph managers to prevent unbounded memory growth
    // TODO 📊: Add memory pressure monitoring and automatic graph unloading
    pub async fn get_or_create_graph_manager(&self, graph_id: &Uuid) -> Result<()> {
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        
        // Check if resources already exist
        if resources.contains_key(graph_id) {
            return Ok(());
        }
        
        // Drop read lock before acquiring write lock
        drop(resources);
        
        // Acquire write lock to create new resources
        let mut resources = self.graph_resources.write_or_panic("get or create graph manager - write resources").await;
        
        // Double-check pattern - another thread may have created it
        if resources.contains_key(graph_id) {
            return Ok(());
        }
        
        // Create new GraphManager and TransactionCoordinator together
        let data_dir = self.data_dir.join("graphs").join(graph_id.to_string());
        fs::create_dir_all(&data_dir)?;
        
        let graph_manager = GraphManager::new(data_dir.clone())?;
        
        // Transaction log is inside the graph-specific directory
        let transaction_log_dir = data_dir.join("transaction_log");
        fs::create_dir_all(&transaction_log_dir)?;
        let transaction_log = Arc::new(TransactionLog::new(transaction_log_dir)
            ?);
        
        let transaction_coordinator = Arc::new(TransactionCoordinator::new(transaction_log));
        
        // Bundle them together and insert atomically
        resources.insert(*graph_id, GraphResources {
            manager: RwLock::new(graph_manager),
            coordinator: transaction_coordinator,
        });
        
        Ok(())
    }
    
    /// Get the transaction coordinator for a specific graph
    pub async fn get_transaction_coordinator(&self, graph_id: &Uuid) -> Option<Arc<TransactionCoordinator>> {
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        resources.get(graph_id).map(|r| r.coordinator.clone())
    }
    
    /// Check if a graph is open
    pub fn is_graph_open(&self, graph_id: &Uuid) -> bool {
        if let Ok(registry) = self.graph_registry.read() {
            registry.is_graph_open(graph_id)
        } else {
            false
        }
    }
    
    /// Open a graph (ensure manager is loaded and update registry)
    /// 
    /// This performs the following steps:
    /// 1. Creates/loads the graph manager and transaction coordinator
    /// 2. Updates the registry to mark the graph as open
    /// 
    /// Note: Transaction recovery should be run separately via run_graph_recovery()
    /// if needed after this method completes successfully.
    pub async fn open_graph(&self, graph_id: &Uuid) -> Result<()> {
        // First ensure the graph manager and coordinator exist
        // This will either:
        // - Return immediately if already loaded
        // - Load from disk if closed
        // - Create new empty graph if doesn't exist
        self.get_or_create_graph_manager(graph_id).await?;
        
        // Update registry (single source of truth)
        // Debug assertion is now built into write_or_panic
        let mut registry = self.graph_registry.write_or_panic("open graph - write registry");
        registry.open_graph(graph_id)
            .map_err(|e| CymbiontError::Other(format!("Failed to open graph: {}", e)))?;
        
        Ok(())
    }
    
    /// Run transaction recovery for a specific graph
    /// 
    /// Simplified using downstream helper functions.
    pub async fn run_graph_recovery(self: &Arc<Self>, graph_id: &Uuid) -> Result<usize> {
        let coordinator = self.get_transaction_coordinator(graph_id).await
            .ok_or_else(|| format!("No transaction coordinator for graph {}", graph_id))?;
        
        // Delegate core recovery logic to helper
        let count = transaction::run_single_graph_recovery_helper(&coordinator, self, graph_id).await?;
        
        // Save graph if transactions were recovered
        if count > 0 {
            let resources = self.graph_resources.read_or_panic("read graph resources").await;
            if let Some(graph_resources) = resources.get(graph_id) {
                let mut manager = graph_resources.manager.write_or_panic("run graph recovery - write manager").await;
                transaction::save_graph_after_recovery_helper(&mut *manager, graph_id).await;
            }
        }
        
        Ok(count)
    }
    
    /// Run recovery for all graphs on startup
    /// 
    /// This includes both open and closed graphs to ensure no pending transactions are lost.
    /// Closed graphs are temporarily opened for recovery, then closed again.
    pub async fn run_all_graphs_recovery(self: &Arc<Self>) -> Result<usize> {
        let all_graphs = {
            let registry_guard = self.graph_registry.read_or_panic("recovery - read graph registry");
            registry_guard.get_all_graphs()
        };
        
        info!("🔄 Running recovery for {} graphs", all_graphs.len());
        let mut total_recovered = 0;
        
        for graph_info in all_graphs {
            let graph_id = graph_info.id;
            let graph_name = graph_info.name.clone();
            
            // Check if graph is already open
            let was_open = {
                let registry_guard = self.graph_registry.read_or_panic("recovery - check open");
                registry_guard.is_graph_open(&graph_id)
            };
            
            // If graph is closed, temporarily open it for recovery
            if !was_open {
                if let Err(e) = self.open_graph(&graph_id).await {
                    error!("Failed to open graph {} for recovery: {}", graph_name, e);
                    continue;
                }
            }
            
            // Ensure the graph manager is loaded
            if let Err(e) = self.get_or_create_graph_manager(&graph_id).await {
                error!("Failed to create graph manager for {}: {}", graph_name, e);
                if !was_open {
                    // Try to close it again if we opened it
                    let _ = self.close_graph(&graph_id).await;
                }
                continue;
            }
            
            // Run recovery using the centralized method
            match self.run_graph_recovery(&graph_id).await {
                Ok(count) if count > 0 => {
                    info!("✅ Successfully replayed {} transactions for graph {}", count, graph_name);
                    total_recovered += count;
                }
                Err(e) => {
                    error!("❌ Failed to recover transactions for {}: {}", graph_name, e);
                }
                _ => {} // No pending transactions
            }
            
            // If graph was originally closed, close it again
            if !was_open {
                if let Err(e) = self.close_graph(&graph_id).await {
                    error!("Failed to close graph {} after recovery: {}", graph_name, e);
                }
            }
        }
        
        info!("✅ Recovery complete for all graphs - total transactions recovered: {}", total_recovered);
        Ok(total_recovered)
    }
    
    /// Create a new knowledge graph with automatic prime agent authorization
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn create_new_graph(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<GraphInfo> {
        // Delegate complete workflow to GraphRegistry
        let (graph_info, _prime_agent_id) = {
            let (mut graph_registry, mut agent_registry) = lock_registries_for_write(
                &self.graph_registry,
                &self.agent_registry
            )?;
            
            let graph_info = graph_registry.create_new_graph_complete(name, description, &mut agent_registry)
                .map_err(|e| CymbiontError::Other(format!("Failed to create graph: {}", e)))?;
            
            let prime_id = agent_registry.get_prime_agent_id();
            (graph_info, prime_id)
        };
        
        // Create graph manager resources (AppState-specific coordination)
        self.get_or_create_graph_manager(&graph_info.id).await?;
        
        // Don't try to set prime agent default here - it causes deadlock if called from a tool
        // The caller can handle setting defaults if needed
        
        Ok(graph_info)
    }
    
    /// Delete a knowledge graph completely
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn delete_graph_completely(&self, graph_id: &Uuid) -> Result<()> {
        // Delegate complete workflow to GraphRegistry
        {
            let mut registry = self.graph_registry.write()
                .map_err(|e| CymbiontError::Other(format!("Failed to write registry: {}", e)))?;
            
            registry.delete_graph_complete(graph_id)
                .map_err(|e| CymbiontError::Other(format!("Failed to delete graph: {}", e)))?;
        }
        
        // Remove from memory resources (AppState-specific coordination)
        {
            let mut resources = self.graph_resources.write_or_panic("get or create graph manager - write resources").await;
            resources.remove(graph_id);
        }
        
        Ok(())
    }
    
    /// Close a graph (save and update registry)
    /// 
    /// Resources are bundled so removing from graph_resources removes BOTH
    /// the manager and coordinator atomically, preventing inconsistent state.
    pub async fn close_graph(&self, graph_id: &Uuid) -> Result<()> {
        // Get the resources to save and close
        let mut resources = self.graph_resources.write_or_panic("get or create graph manager - write resources").await;
        
        if let Some(graph_resources) = resources.get(graph_id) {
            // Save the graph before removing from memory
            graph_resources.manager.write_or_panic("close graph - save").await.save_graph()?;
            
            // Flush and close the transaction coordinator
            if let Err(e) = graph_resources.coordinator.close().await {
                error!("Failed to close transaction log for graph {}: {}", graph_id, e);
                // Continue anyway - we still want to remove it from memory
            }
        }
        
        // Remove all resources atomically
        resources.remove(graph_id);
        drop(resources);
        
        // Step 4: Update registry (single source of truth)
        // Debug assertion is now built into write_or_panic
        let mut registry = self.graph_registry.write_or_panic("close graph - write registry");
        registry.close_graph(graph_id)
            .map_err(|e| CymbiontError::Other(format!("Failed to close graph: {}", e)))?;
        
        
        Ok(())
    }
    
    // ========== Agent Management Methods (Parallel to Graph Methods) ==========
    
    /// Get or load an agent for the given agent ID
    /// 
    /// Loads the agent from disk if not already in memory.
    pub async fn get_or_load_agent(&self, agent_id: &Uuid) -> Result<()> {
        let agents = self.agents.read_or_panic("get or load agent - read agents").await;
        
        // Check if agent already loaded
        if agents.contains_key(agent_id) {
            return Ok(());
        }
        
        // Drop read lock before acquiring write lock
        drop(agents);
        
        // Acquire write lock to load agent
        let mut agents = self.agents.write_or_panic("get or load agent - write agents").await;
        
        // Double-check pattern - another thread may have loaded it
        if agents.contains_key(agent_id) {
            return Ok(());
        }
        
        // Get agent info from registry
        let agent_info = {
            let registry = self.agent_registry.read_or_panic("get or load agent - read registry");
            registry.get_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not found in registry", agent_id))?
                .clone()
        };
        
        // Load agent from disk
        let mut agent = Agent::load(&agent_info.data_path)
            .map_err(|e| CymbiontError::Other(format!("Failed to load agent {}: {:?}", agent_id, e)))?;
        
        // If agent has no default but is authorized for graphs, set first as default
        if agent.get_default_graph_id().is_none() && !agent_info.authorized_graphs.is_empty() {
            agent.set_default_graph_id(Some(agent_info.authorized_graphs[0]));
            info!("Set default graph for loaded agent {} to {}", 
                  agent_id, agent_info.authorized_graphs[0]);
        }
        
        agents.insert(*agent_id, Arc::new(RwLock::new(agent)));
        
        Ok(())
    }
    
    /// Activate an agent (load into memory and update registry)
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn activate_agent(&self, agent_id: &Uuid) -> Result<()> {
        // First ensure the agent is loaded (AppState-specific coordination)
        self.get_or_load_agent(agent_id).await?;
        
        // Delegate workflow to AgentRegistry
        let mut registry = self.agent_registry.write_or_panic("activate agent - write registry");
        registry.activate_agent_complete(agent_id)
            .map_err(|e| CymbiontError::Other(format!("Failed to activate agent: {:?}", e)))?;
        
        Ok(())
    }
    
    /// Deactivate an agent (save and unload from memory)
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn deactivate_agent(&self, agent_id: &Uuid) -> Result<()> {
        // Save and unload agent from memory (AppState-specific coordination)
        let mut agents = self.agents.write_or_panic("deactivate agent - write agents").await;
        if let Some(agent_arc) = agents.remove(agent_id) {
            // Try to extract agent from Arc - this will fail if other references exist
            match Arc::try_unwrap(agent_arc) {
                Ok(agent_lock) => {
                    // Successfully unwrapped - we own the agent
                    let mut agent = agent_lock.into_inner();
                    agent.save()
                        .map_err(|e| CymbiontError::Other(format!("Failed to save agent before deactivation: {:?}", e)))?;
                }
                Err(agent_arc) => {
                    // Agent is still referenced elsewhere - save through the Arc
                    {
                        let mut agent = agent_arc.write_or_panic("deactivate agent - save").await;
                        agent.save()
                            .map_err(|e| CymbiontError::Other(format!("Failed to save agent before deactivation: {:?}", e)))?;
                    } // Drop the guard here
                    warn!("Agent {} has active references during deactivation", agent_id);
                    // Put it back since we couldn't unwrap it
                    agents.insert(*agent_id, agent_arc);
                }
            }
        }
        drop(agents);
        
        // Delegate workflow to AgentRegistry
        let mut registry = self.agent_registry.write_or_panic("activate agent - write registry");
        registry.deactivate_agent_complete(agent_id)
            .map_err(|e| CymbiontError::Other(format!("Failed to deactivate agent: {:?}", e)))?;
        
        Ok(())
    }
    
    /// Check if an agent is active (loaded in memory)
    pub fn is_agent_active(&self, agent_id: &Uuid) -> bool {
        if let Ok(registry) = self.agent_registry.read() {
            registry.is_agent_active(agent_id)
        } else {
            false
        }
    }
    
    /// Authorize an agent for a graph and set default if needed
    /// 
    /// This method handles the complete authorization workflow:
    /// 1. Updates both registries for bidirectional tracking
    /// 2. If agent is loaded and has no default, sets this graph as default
    /// 3. Saves all changes to disk
    pub async fn authorize_agent_for_graph(&self, agent_id: &Uuid, graph_id: &Uuid) -> Result<()> {
        // Update registries
        {
            let (mut graph_registry, mut agent_registry) = lock_registries_for_write(
                &self.graph_registry,
                &self.agent_registry
            )?;
            
            agent_registry.authorize_agent_for_graph(agent_id, graph_id, &mut graph_registry)?;
            
            // Save both registries
            agent_registry.save()?;
            graph_registry.save()?;
        }
        
        // If agent is loaded, check if it needs a default graph
        let agents = self.agents.read_or_panic("authorize agent - read agents").await;
        if let Some(agent_arc) = agents.get(agent_id) {
            let mut agent = agent_arc.write_or_panic("authorize agent - set default").await;
            if agent.get_default_graph_id().is_none() {
                agent.set_default_graph_id(Some(*graph_id));
                info!("Set default graph for agent {} to {}", agent_id, graph_id);
            }
        }
        
        Ok(())
    }
    
    /// Save all graphs and registry on shutdown
    pub async fn cleanup_and_save(&self) {
        // Close all WebSocket connections first
        if let Some(ref connections) = self.ws_connections {
            let mut conn_map = connections.write_or_panic("cleanup connections").await;
            let connection_count = conn_map.len();
            
            if connection_count > 0 {
                // Send shutdown signal to all connections
                for (_, conn) in conn_map.iter() {
                    let _ = conn.shutdown_tx.send(true);
                }
                
                // Clear the connections
                conn_map.clear();
                drop(conn_map);
                
                // Give tasks a moment to shut down gracefully
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        
        // Save all loaded graphs and close transaction logs
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        
        for (graph_id, graph_resources) in resources.iter() {
            // Save graph
            match graph_resources.manager.write_or_panic("save graph on cleanup").await.save_graph() {
                Ok(_) => {
                    info!("✅ Saved graph: {}", graph_id);
                }
                Err(e) => error!("Failed to save graph {}: {}", graph_id, e),
            }
            
            // Flush and close transaction log
            if let Err(e) = graph_resources.coordinator.close().await {
                error!("Failed to close transaction log for graph {}: {}", graph_id, e);
            }
        }
        drop(resources);
        
        // Save graph registry
        if let Ok(registry_guard) = self.graph_registry.read() {
            if let Err(e) = registry_guard.save() {
                error!("Failed to save graph registry: {}", e);
            } else {
                info!("✅ Graph registry saved");
            }
        }
        
        // Save all active agents
        let agents = self.agents.read_or_panic("cleanup - read agents").await;
        for (agent_id, agent_arc) in agents.iter() {
            let mut agent = agent_arc.write_or_panic("cleanup - save agent").await;
            if let Err(e) = agent.save() {
                error!("Failed to save agent {}: {:?}", agent_id, e);
            }
        }
        drop(agents);
        
        // Save agent registry
        if let Ok(registry_guard) = self.agent_registry.read() {
            if let Err(e) = registry_guard.save() {
                error!("Failed to save agent registry: {}", e);
            } else {
                info!("✅ Agent registry saved");
            }
        }
    }
    
    /// Execute an operation with transaction on a specific graph
    pub async fn with_graph_transaction<F, T>(
        &self,
        graph_id: &Uuid,
        operation: crate::storage::Operation,
        executor: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut GraphManager) -> std::result::Result<T, String>,
    {
        // Check if shutdown has been initiated
        if self.shutdown_initiated.load(Ordering::Acquire) {
            warn!("Transaction rejected at AppState level - graceful shutdown in progress");
            return Err(CymbiontError::Other(
                "Shutdown in progress - no new transactions allowed".to_string()
            ));
        }
        
        // Verify graph is open
        if !self.is_graph_open(graph_id) {
            return Err(CymbiontError::Other(
                format!("Graph '{}' is not open", graph_id)
            ));
        }
        
        // Get transaction coordinator from bundled resources
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        let graph_resources = resources.get(graph_id)
            .ok_or_else(|| format!("Graph resources not found for graph '{}'", graph_id))?;
        
        // Clone coordinator to use in closure
        let coordinator = Arc::clone(&graph_resources.coordinator);
        drop(resources); // Release lock early
        
        // Create transaction (includes deduplication check)
        let tx_id = coordinator.create_transaction(operation).await?;
        
        // Check freeze state and wait if frozen
        while *self.operation_freeze.read_or_panic("check freeze state").await {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        // Get graph manager and execute operation
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        let graph_resources = resources.get(graph_id)
            .ok_or_else(|| format!("Graph resources not found for graph '{}'", graph_id))?;
        let mut manager = graph_resources.manager.write_or_panic("with graph transaction - write manager").await;
        
        
        // Execute the operation
        let result = executor(&mut *manager).map_err(|e| CymbiontError::Other(e));
        
        
        // Drop locks before updating transaction state
        drop(manager);
        drop(resources);
        
        // Complete transaction based on result
        coordinator.complete_transaction(&tx_id, result).await
                }
    
    /// Initiate graceful shutdown across all graph transaction coordinators
    /// Returns the total count of active transactions across all graphs
    pub async fn initiate_graceful_shutdown(&self) -> usize {
        // Set the local shutdown flag to prevent new transactions
        self.shutdown_initiated.store(true, Ordering::Release);
        
        let mut total_active = 0;
        
        // Initiate shutdown on all transaction coordinators
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        for (_graph_id, graph_resources) in resources.iter() {
            let active_count = graph_resources.coordinator.initiate_shutdown().await;
            if active_count > 0 {
            }
            total_active += active_count;
        }
        
        if total_active > 0 {
        }
        
        total_active
    }
    
    /// Wait for all transactions to complete across all graphs
    /// Returns true if all completed, false if timeout
    pub async fn wait_for_transactions(&self, timeout: Duration) -> bool {
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        
        // Create futures for waiting on each coordinator
        let mut wait_futures = Vec::new();
        for graph_resources in resources.values() {
            let coordinator = Arc::clone(&graph_resources.coordinator);
            wait_futures.push(async move {
                coordinator.wait_for_completion(timeout).await
            });
        }
        drop(resources);
        
        // Wait for all coordinators - all must complete for success
        let results = futures_util::future::join_all(wait_futures).await;
        results.iter().all(|&completed| completed)
    }
    
    /// Force flush all transaction coordinators for immediate shutdown
    pub async fn force_flush_transactions(&self) {
        let resources = self.graph_resources.read_or_panic("read graph resources").await;
        for (graph_id, graph_resources) in resources.iter() {
            if let Err(e) = graph_resources.coordinator.force_shutdown().await {
                error!("Failed to force flush transaction log for graph {}: {}", graph_id, e);
            }
        }
    }
    
}