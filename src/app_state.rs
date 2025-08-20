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
//! - Async RwLocks for frequently accessed resources (graphs, agents, connections)
//! - Sync RwLocks for registries requiring immediate consistency
//! - Double-check pattern prevents race conditions in resource creation
//! - Debug assertions detect lock contention during development

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock as SyncRwLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::error::Error;
use std::fs;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, error, warn};
use uuid::Uuid;

use crate::{
    agent::agent::Agent,
    graph_manager::GraphManager,
    config::{load_config, Config},
    storage::{GraphRegistry, AgentRegistry, TransactionLog, TransactionCoordinator, graph_registry::GraphInfo, transaction},
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
    pub agents: Arc<RwLock<HashMap<Uuid, Agent>>>,  // Active agents
    pub agent_registry: Arc<SyncRwLock<AgentRegistry>>,  // Agent lifecycle management
    
    pub config: Config,
    pub data_dir: PathBuf,  // Resolved absolute path
    
    // Server-specific components (optional)
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
    pub auth_token: Arc<RwLock<Option<String>>>,  // Authentication token
    
    // Test infrastructure
    pub operation_freeze: Arc<RwLock<bool>>,  // Freeze operations after transaction creation
    
    // Shutdown coordination
    pub shutdown_initiated: Arc<AtomicBool>,  // Flag to prevent new transactions
}

impl AppState {
    /// Create new AppState for CLI usage (no server components)
    pub async fn new_cli(config_path: Option<String>, data_dir_override: Option<String>) -> Result<Arc<Self>, Box<dyn Error + Send + Sync>> {
        Self::new_internal(config_path, data_dir_override, false).await
    }
    
    /// Create new AppState for server usage (with WebSocket components)  
    pub async fn new_server(config_path: Option<String>, data_dir_override: Option<String>) -> Result<Arc<Self>, Box<dyn Error + Send + Sync>> {
        Self::new_internal(config_path, data_dir_override, true).await
    }
    
    async fn new_internal(config_path: Option<String>, data_dir_override: Option<String>, with_server: bool) -> Result<Arc<Self>, Box<dyn Error + Send + Sync>> {
        // Load configuration
        let mut config = load_config(config_path);
        
        // Apply data_dir override if provided
        if let Some(cli_data_dir) = &data_dir_override {
            info!("🗂️  Overriding data directory: {}", cli_data_dir);
            config.data_dir = cli_data_dir.clone();
        }
        
        // Initialize data directory
        let data_dir = if std::path::Path::new(&config.data_dir).is_absolute() {
            PathBuf::from(&config.data_dir)
        } else {
            std::env::current_dir()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to get current directory: {e}")))?
                .join(&config.data_dir)
        };
        fs::create_dir_all(&data_dir)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create data directory: {e}")))?;
        
        // Initialize graph registry
        let registry_path = data_dir.join("graph_registry.json");
        let mut registry = GraphRegistry::load_or_create(&registry_path, &data_dir)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Graph registry error: {e:?}")))?;
        
        // Ensure at least one graph is open
        registry.ensure_graph_open()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to ensure graph open: {e:?}")))?;
        
        let graph_registry = Arc::new(SyncRwLock::new(registry));
        
        // Initialize agent registry
        let agent_registry_path = data_dir.join("agent_registry.json");
        let mut agent_registry = AgentRegistry::load_or_create(&agent_registry_path, &data_dir)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Agent registry error: {e:?}")))?;
        
        // Ensure prime agent exists (creates it on first run)
        agent_registry.ensure_default_agent()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to ensure prime agent: {e:?}")))?;
        
        
        // Save agent registry after potential prime agent creation
        agent_registry.save()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to save agent registry: {e:?}")))?;
        
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
            let registry = graph_registry.read()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to read registry: {}", e)))?;
            registry.get_open_graphs()
        };
        
        let app_state = Arc::new(AppState {
            graph_resources,
            graph_registry,
            agents,
            agent_registry,
            config,
            data_dir: data_dir.clone(),
            ws_ready_tx: Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
            operation_freeze: Arc::new(RwLock::new(false)),
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
        });
        
        // Load all open graphs
        for graph_id in initial_open_graphs {
            app_state.get_or_create_graph_manager(&graph_id).await?;
            info!("Loaded open graph: {}", graph_id);
        }
        
        // Load all active agents (ensure at least prime agent is active)
        let initial_active_agents = {
            let mut registry = app_state.agent_registry.write()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
            
            // If no agents are active, activate the prime agent
            let active_agents = registry.get_active_agents();
            if active_agents.is_empty() {
                if let Some(prime_id) = registry.get_prime_agent_id() {
                    info!("No agents active on startup, activating prime agent");
                    registry.activate_agent(&prime_id)
                        .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to activate prime agent: {e:?}")))?;
                    // Save after activation
                    registry.save()
                        .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to save agent registry: {e:?}")))?;
                }
            }
            
            registry.get_active_agents()
        };
        
        for agent_id in initial_active_agents {
            if let Err(e) = app_state.get_or_load_agent(&agent_id).await {
                error!("Failed to load agent {}: {}", agent_id, e);
                // Continue loading other agents even if one fails
            } else {
                info!("Loaded active agent: {}", agent_id);
            }
        }
        
        // Initialize authentication if in server mode
        if with_server {
            use crate::server::auth::initialize_auth;
            let token = initialize_auth(&app_state).await?;
            if !app_state.config.auth.disabled {
                let mut token_guard = app_state.auth_token.write().await;
                *token_guard = Some(token);
            }
        }
        
        Ok(app_state)
    }
    
    /// Get or create graph resources (manager + coordinator) for the given graph ID
    /// 
    /// Resources are bundled together to ensure they're always created/destroyed atomically.
    pub async fn get_or_create_graph_manager(&self, graph_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        let resources = self.graph_resources.read().await;
        
        // Check if resources already exist
        if resources.contains_key(graph_id) {
            return Ok(());
        }
        
        // Drop read lock before acquiring write lock
        drop(resources);
        
        // Acquire write lock to create new resources
        let mut resources = self.graph_resources.write().await;
        
        // Double-check pattern - another thread may have created it
        if resources.contains_key(graph_id) {
            return Ok(());
        }
        
        // Create new GraphManager and TransactionCoordinator together
        let data_dir = self.data_dir.join("graphs").join(graph_id.to_string());
        fs::create_dir_all(&data_dir)?;
        
        let graph_manager = GraphManager::new(data_dir.clone())
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create graph manager for {}: {:?}", graph_id, e)))?;
        
        // Transaction log is inside the graph-specific directory
        let transaction_log_dir = data_dir.join("transaction_log");
        fs::create_dir_all(&transaction_log_dir)?;
        let transaction_log = Arc::new(TransactionLog::new(transaction_log_dir)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create transaction log for {}: {:?}", graph_id, e)))?);
        
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
        let resources = self.graph_resources.read().await;
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
    pub async fn open_graph(&self, graph_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // First ensure the graph manager and coordinator exist
        // This will either:
        // - Return immediately if already loaded
        // - Load from disk if closed
        // - Create new empty graph if doesn't exist
        self.get_or_create_graph_manager(graph_id).await?;
        
        // Debug assertion to fail fast if another thread holds the write lock
        debug_assert!(
            self.graph_registry.try_write().is_ok(),
            "Registry write lock unavailable - another thread may be holding it"
        );
        
        // Update registry (single source of truth)
        let mut registry = self.graph_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write registry: {}", e)))?;
        registry.open_graph(graph_id)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to open graph: {}", e)))?;
        
        Ok(())
    }
    
    /// Run transaction recovery for a specific graph
    /// 
    /// Simplified using downstream helper functions.
    pub async fn run_graph_recovery(self: &Arc<Self>, graph_id: &Uuid) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let coordinator = self.get_transaction_coordinator(graph_id).await
            .ok_or_else(|| format!("No transaction coordinator for graph {}", graph_id))?;
        
        // Delegate core recovery logic to helper
        let count = transaction::run_single_graph_recovery_helper(&coordinator, self, graph_id).await?;
        
        // Save graph if transactions were recovered
        if count > 0 {
            let resources = self.graph_resources.read().await;
            if let Some(graph_resources) = resources.get(graph_id) {
                let mut manager = graph_resources.manager.write().await;
                transaction::save_graph_after_recovery_helper(&mut *manager, graph_id).await;
            }
        }
        
        Ok(count)
    }
    
    /// Run recovery for all graphs on startup
    /// 
    /// This includes both open and closed graphs to ensure no pending transactions are lost.
    /// Closed graphs are temporarily opened for recovery, then closed again.
    pub async fn run_all_graphs_recovery(self: &Arc<Self>) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let all_graphs = {
            let registry_guard = self.graph_registry.read()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to read registry for recovery: {}", e)))?;
            registry_guard.get_all_graphs()
        };
        
        info!("🔄 Running recovery for {} graphs", all_graphs.len());
        let mut total_recovered = 0;
        
        for graph_info in all_graphs {
            let graph_id = graph_info.id;
            let graph_name = graph_info.name.clone();
            
            // Check if graph is already open
            let was_open = {
                let registry_guard = self.graph_registry.read()
                    .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to read registry: {}", e)))?;
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
    ) -> Result<GraphInfo, Box<dyn Error + Send + Sync>> {
        // Delegate complete workflow to GraphRegistry
        let graph_info = {
            let mut graph_registry = self.graph_registry.write()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write graph registry: {}", e)))?;
            let mut agent_registry = self.agent_registry.write()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
            
            graph_registry.create_new_graph_complete(name, description, &mut agent_registry)
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create graph: {}", e)))?
        };
        
        // Create graph manager resources (AppState-specific coordination)
        self.get_or_create_graph_manager(&graph_info.id).await?;
        
        Ok(graph_info)
    }
    
    /// Delete a knowledge graph completely
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn delete_graph_completely(&self, graph_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Delegate complete workflow to GraphRegistry
        {
            let mut registry = self.graph_registry.write()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write registry: {}", e)))?;
            
            registry.delete_graph_complete(graph_id)
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to delete graph: {}", e)))?;
        }
        
        // Remove from memory resources (AppState-specific coordination)
        {
            let mut resources = self.graph_resources.write().await;
            resources.remove(graph_id);
        }
        
        Ok(())
    }
    
    /// Close a graph (save and update registry)
    /// 
    /// Resources are bundled so removing from graph_resources removes BOTH
    /// the manager and coordinator atomically, preventing inconsistent state.
    pub async fn close_graph(&self, graph_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Get the resources to save and close
        let mut resources = self.graph_resources.write().await;
        
        if let Some(graph_resources) = resources.get(graph_id) {
            // Save the graph before removing from memory
            graph_resources.manager.write().await.save_graph()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to save graph before closing: {}", e)))?;
            
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
        // Debug assertion to fail fast if another thread holds the write lock
        debug_assert!(
            self.graph_registry.try_write().is_ok(),
            "Registry write lock unavailable - another thread may be holding it"
        );
        
        let mut registry = self.graph_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write registry: {}", e)))?;
        registry.close_graph(graph_id)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to close graph: {}", e)))?;
        
        info!("Closed graph {} and removed all resources from memory", graph_id);
        
        Ok(())
    }
    
    // ========== Agent Management Methods (Parallel to Graph Methods) ==========
    
    /// Get or load an agent for the given agent ID
    /// 
    /// Loads the agent from disk if not already in memory.
    pub async fn get_or_load_agent(&self, agent_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        let agents = self.agents.read().await;
        
        // Check if agent already loaded
        if agents.contains_key(agent_id) {
            return Ok(());
        }
        
        // Drop read lock before acquiring write lock
        drop(agents);
        
        // Acquire write lock to load agent
        let mut agents = self.agents.write().await;
        
        // Double-check pattern - another thread may have loaded it
        if agents.contains_key(agent_id) {
            return Ok(());
        }
        
        // Get agent info from registry
        let agent_info = {
            let registry = self.agent_registry.read()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to read agent registry: {}", e)))?;
            registry.get_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not found in registry", agent_id))?
                .clone()
        };
        
        // Load agent from disk
        let agent = Agent::load(&agent_info.data_path)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to load agent {}: {:?}", agent_id, e)))?;
        
        agents.insert(*agent_id, agent);
        
        Ok(())
    }
    
    /// Activate an agent (load into memory and update registry)
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn activate_agent(&self, agent_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // First ensure the agent is loaded (AppState-specific coordination)
        self.get_or_load_agent(agent_id).await?;
        
        // Delegate workflow to AgentRegistry
        let mut registry = self.agent_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
        registry.activate_agent_complete(agent_id)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to activate agent: {:?}", e)))?;
        
        Ok(())
    }
    
    /// Deactivate an agent (save and unload from memory)
    /// 
    /// Simplified workflow using downstream registry methods.
    pub async fn deactivate_agent(&self, agent_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Save and unload agent from memory (AppState-specific coordination)
        let mut agents = self.agents.write().await;
        if let Some(mut agent) = agents.remove(agent_id) {
            agent.save()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to save agent before deactivation: {:?}", e)))?;
            info!("Saved and unloaded agent: {}", agent_id);
        }
        drop(agents);
        
        // Delegate workflow to AgentRegistry
        let mut registry = self.agent_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
        registry.deactivate_agent_complete(agent_id)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to deactivate agent: {:?}", e)))?;
        
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
    
    /// Save all graphs and registry on shutdown
    pub async fn cleanup_and_save(&self) {
        // Close all WebSocket connections first
        if let Some(ref connections) = self.ws_connections {
            let mut conn_map = connections.write().await;
            let connection_count = conn_map.len();
            
            if connection_count > 0 {
                info!("🔌 Shutting down {} WebSocket connection(s)...", connection_count);
                
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
        let resources = self.graph_resources.read().await;
        
        for (graph_id, graph_resources) in resources.iter() {
            // Save graph
            match graph_resources.manager.write().await.save_graph() {
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
        let mut agents = self.agents.write().await;
        for (agent_id, agent) in agents.iter_mut() {
            match agent.save() {
                Ok(_) => {
                    info!("✅ Saved agent: {}", agent_id);
                }
                Err(e) => error!("Failed to save agent {}: {:?}", agent_id, e),
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
    ) -> Result<T, Box<dyn Error + Send + Sync>>
    where
        F: FnOnce(&mut GraphManager) -> std::result::Result<T, String>,
    {
        // Check if shutdown has been initiated
        if self.shutdown_initiated.load(Ordering::Acquire) {
            warn!("Transaction rejected at AppState level - graceful shutdown in progress");
            return Err(Box::<dyn Error + Send + Sync>::from(
                "Shutdown in progress - no new transactions allowed"
            ));
        }
        
        // Verify graph is open
        if !self.is_graph_open(graph_id) {
            return Err(Box::<dyn Error + Send + Sync>::from(
                format!("Graph '{}' is not open", graph_id)
            ));
        }
        
        // Get transaction coordinator from bundled resources
        let resources = self.graph_resources.read().await;
        let graph_resources = resources.get(graph_id)
            .ok_or_else(|| format!("Graph resources not found for graph '{}'", graph_id))?;
        
        // Clone coordinator to use in closure
        let coordinator = Arc::clone(&graph_resources.coordinator);
        drop(resources); // Release lock early
        
        // Create transaction (includes deduplication check)
        let tx_id = coordinator.create_transaction(operation).await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?;
        
        // Check freeze state and wait if frozen
        while *self.operation_freeze.read().await {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        // Get graph manager and execute operation
        let resources = self.graph_resources.read().await;
        let graph_resources = resources.get(graph_id)
            .ok_or_else(|| format!("Graph resources not found for graph '{}'", graph_id))?;
        let mut manager = graph_resources.manager.write().await;
        
        
        // Execute the operation
        let result = executor(&mut *manager);
        
        
        // Drop locks before updating transaction state
        drop(manager);
        drop(resources);
        
        // Complete transaction based on result
        coordinator.complete_transaction(&tx_id, result).await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
    
    /// Initiate graceful shutdown across all graph transaction coordinators
    /// Returns the total count of active transactions across all graphs
    pub async fn initiate_graceful_shutdown(&self) -> usize {
        // Set the local shutdown flag to prevent new transactions
        self.shutdown_initiated.store(true, Ordering::Release);
        
        let mut total_active = 0;
        
        // Initiate shutdown on all transaction coordinators
        let resources = self.graph_resources.read().await;
        for (graph_id, graph_resources) in resources.iter() {
            let active_count = graph_resources.coordinator.initiate_shutdown().await;
            if active_count > 0 {
                info!("Graph {} has {} active transactions", graph_id, active_count);
            }
            total_active += active_count;
        }
        
        if total_active > 0 {
            info!("Total active transactions across all graphs: {}", total_active);
        }
        
        total_active
    }
    
    /// Wait for all transactions to complete across all graphs
    /// Returns true if all completed, false if timeout
    pub async fn wait_for_transactions(&self, timeout: Duration) -> bool {
        let resources = self.graph_resources.read().await;
        
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
        let resources = self.graph_resources.read().await;
        for (graph_id, graph_resources) in resources.iter() {
            if let Err(e) = graph_resources.coordinator.force_shutdown().await {
                error!("Failed to force flush transaction log for graph {}: {}", graph_id, e);
            }
        }
    }
    
}