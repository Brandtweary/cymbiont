//! Application State Management - Central Coordination Hub
//! 
//! This module provides the central AppState struct that acts as the coordination
//! layer for all components of the Cymbiont multi-agent knowledge graph engine.
//! AppState orchestrates the interaction between graphs, agents, transactions, and
//! real-time communication systems.
//! 
//! ## Central Nervous System Architecture
//! 
//! AppState coordinates all major subsystems:
//! - **Graph Resources**: Bundled managers + coordinators for atomic lifecycle management
//! - **Graph Registry**: Multi-graph metadata, open/closed state, and agent associations
//! - **Agent Management**: Active agent instances and lifecycle coordination
//! - **Agent Registry**: Agent metadata, authorization mappings, and persistence
//! - **WebSocket Connections**: Real-time communication with current agent tracking
//! - **Transaction Coordination**: ACID guarantees across all graphs
//! - **Authentication**: Token-based security for server mode
//! - **Configuration**: Runtime settings and environment management
//! - **Graceful Shutdown**: Coordinated cleanup across all subsystems
//! 
//! ## Design Philosophy: Coordination, Not Implementation
//! 
//! AppState provides essential wiring between components while delegating actual work:
//! - **PKM Operations** → GraphOperationsExt trait with phantom type authorization
//! - **Graph Storage** → GraphManager with petgraph engine
//! - **Transactions** → TransactionCoordinator with WAL logging
//! - **Agent Interactions** → Agent structs with LLM backends
//! - **Open/Closed State** → GraphRegistry (single source of truth)
//! - **Authorization** → AgentRegistry with bidirectional mappings
//! - **Real-time Communication** → WebSocket handlers with async execution
//! 
//! This separation ensures clean architecture, testability, and maintainability.
//! 
//! ## Resource Bundling Pattern
//! 
//! AppState implements resource bundling to ensure atomic lifecycle management:
//! 
//! ### GraphResources Bundle
//! Each graph bundles GraphManager (petgraph storage) with TransactionCoordinator
//! (WAL management). This prevents inconsistent states where one resource exists
//! without its counterpart.
//! 
//! ### Agent Resource Management
//! - **Active Agents**: HashMap of loaded Agent instances in memory
//! - **Agent Registry**: Persistent metadata and authorization mappings
//! - **Atomic Operations**: Lifecycle changes update both memory and registry
//! 
//! ## Multi-Graph Architecture
//! 
//! AppState coordinates multiple isolated knowledge graphs simultaneously:
//! 
//! ### Graph Lifecycle States
//! - **Closed**: Metadata exists but no manager/coordinator in memory
//! - **Open**: Manager and coordinator loaded, ready for operations
//! - **Archived**: Moved to archived_graphs/ directory (soft deletion)
//! 
//! ### Graph Resource Management
//! - **Lazy Loading**: Resources created on first access
//! - **Explicit Control**: Open/close operations provide deterministic management
//! - **Atomic Transitions**: State changes coordinated across registry and memory
//! - **Recovery**: Open graphs automatically loaded on system startup
//! 
//! ### Per-Graph Isolation
//! - **Storage**: Isolated data directory and transaction log per graph
//! - **Transactions**: Independent WAL and coordinator per graph
//! - **Authorization**: Agent permissions managed per graph
//! - **Concurrency**: Graph operations don't interfere with each other
//! 
//! ## Multi-Agent Coordination
//! 
//! AppState manages the complete agent ecosystem:
//! 
//! ### Agent Lifecycle Management
//! - **Registration**: Create metadata and data directories
//! - **Activation**: Load agent into memory for interaction
//! - **Deactivation**: Save state and unload from memory
//! - **Authorization**: Grant/revoke access to specific graphs
//! - **Archival**: Move agents to archived_agents/ directory
//! 
//! ### Prime Agent System
//! - **Auto-creation**: Created automatically on first startup
//! - **Deletion Protection**: Cannot be deleted for system stability
//! - **Default Authorization**: Auto-authorized for all new graphs
//! - **WebSocket Default**: Used as fallback when no specific agent selected
//! 
//! ### Authorization Framework
//! - **Bidirectional Mapping**: Agents ↔ graphs authorization tracking
//! - **Phantom Type Enforcement**: Compile-time authorization checking
//! - **Runtime Validation**: Authorization verified for all graph operations
//! - **Single Source of Truth**: AgentRegistry maintains authoritative state
//! 
//! ## Transaction Architecture
//! 
//! AppState coordinates ACID transactions across the entire system:
//! 
//! ### Per-Graph Transaction Isolation
//! - **Independent WAL**: Each graph has its own write-ahead log
//! - **Separate Coordinators**: Independent transaction management per graph
//! - **Atomic Operations**: Single-graph operations maintain ACID properties
//! - **Recovery**: Graph-specific transaction replay during startup/open
//! 
//! ### System-Wide Transaction Coordination
//! - **Graceful Shutdown**: Wait for all active transactions across all graphs
//! - **Freeze Mechanism**: Test infrastructure to pause transaction execution
//! - **Forced Termination**: Emergency shutdown with transaction log flush
//! - **Startup Recovery**: All graphs processed for pending transaction recovery
//! 
//! ### Transaction Processing Pipeline
//! 1. **Operation Request**: Graph operation initiated via GraphOps trait
//! 2. **Authorization Check**: Agent permissions verified via phantom types
//! 3. **Transaction Creation**: Operation logged to WAL with content deduplication
//! 4. **Freeze Check**: Wait if operations frozen for testing
//! 5. **Execution**: Operation applied to graph with error handling
//! 6. **Completion**: Transaction marked committed/aborted based on result
//! 7. **Persistence**: Graph state saved and transaction log updated
//! 
//! ## Server Mode Architecture
//! 
//! When running in server mode, AppState manages additional real-time components:
//! - **Connection Tracking**: HashMap of active WebSocket connections by UUID
//! - **Agent Association**: Each connection tracks current agent for operations
//! - **Authentication**: Token-based auth with automatic token rotation
//! - **Async Command Execution**: Commands spawn independent async tasks
//! - **High Throughput**: Concurrent operation processing without blocking
//! - **Agent Authorization**: All operations enforce agent permissions
//! 
//! ## Graceful Shutdown Architecture
//! 
//! AppState implements comprehensive graceful shutdown:
//! 
//! ### Shutdown Coordination Sequence
//! 1. **Signal Handling**: SIGINT triggers graceful shutdown initiation
//! 2. **Transaction Halt**: No new transactions accepted across all graphs
//! 3. **Connection Closure**: WebSocket connections notified and closed
//! 4. **Transaction Completion**: Wait for active transactions (up to 30 seconds)
//! 5. **Resource Persistence**: All graphs and agents saved to disk
//! 6. **Registry Persistence**: Graph and agent registries saved
//! 7. **Transaction Log Flush**: WAL logs flushed and closed
//! 8. **Process Termination**: Clean exit with std::process::exit(0)
//! 
//! ### Force Shutdown Path
//! Second SIGINT forces immediate termination with transaction flush.
//! 
//! ## Initialization and Startup
//! 
//! AppState initialization is carefully orchestrated:
//! 
//! ### Factory Methods
//! - **new_cli()**: CLI usage without server components
//! - **new_server()**: Full server capabilities
//! - **new_internal()**: Shared initialization logic
//! 
//! ### Startup Sequence
//! 1. **Configuration Loading**: YAML config loaded with CLI overrides
//! 2. **Data Directory Setup**: Directory structure created/validated
//! 3. **Registry Initialization**: Graph and agent registries loaded from disk
//! 4. **Prime Agent Creation**: Ensure prime agent exists for system stability
//! 5. **Graph Recovery**: Open graphs loaded and transaction recovery performed
//! 6. **Agent Activation**: Active agents loaded into memory
//! 7. **Authentication Setup**: Server mode authentication token generation
//! 8. **Component Wiring**: All subsystems connected and ready
//! 
//! ### Startup Validation
//! - At least one open graph ensures system has a default graph available
//! - Prime agent available guarantees authentication cannot deadlock
//! - Data directory integrity validates storage directory structure
//! - Registry consistency validates graph/agent metadata consistency
//! 
//! ## Concurrency and Thread Safety
//! 
//! AppState is designed for high-concurrency environments:
//! - **Async RwLocks**: For frequently accessed resources (graphs, agents, connections)
//! - **Sync RwLocks**: For registries requiring immediate consistency
//! - **Lock Ordering**: Consistent acquisition order prevents deadlocks
//! - **Double-check Pattern**: Prevents race conditions in resource creation
//! - **Lock Dropping**: Early release to minimize contention windows
//! - **Atomic Operations**: AtomicBool for shutdown coordination
//! - **Debug Assertions**: Development-time lock contention detection
//! 
//! ## Error Handling and Resilience
//! 
//! AppState implements comprehensive error handling with recovery strategies:
//! - **Configuration/Persistence/Authorization/Resource/Network Errors**: Categorized handling
//! - **Graceful Degradation**: Continue when non-critical components fail
//! - **Partial Success**: Agent loading continues even if individual agents fail
//! - **Clear Error Propagation**: Context provided for debugging
//! - **Atomic Operations**: Prevent partial state corruption
//! 
//! ## Extension Patterns
//! 
//! AppState supports clean extension:
//! - **Extension Trait Pattern**: Domain-specific operations via traits (GraphOperationsExt)
//! - **Factory Pattern**: Resource creation through factory methods
//! - **Registry Pattern**: Centralized metadata with single source of truth
//! - **Bidirectional Consistency**: Registry updates maintain referential integrity

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
    storage::{GraphRegistry, AgentRegistry, TransactionLog, TransactionCoordinator},
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
    /// Note: Transaction recovery is handled by the caller (GraphOperationsExt::open_graph)
    /// after this method completes successfully.
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
    /// This performs the following steps:
    /// 1. Loads the agent from disk (or creates new if doesn't exist)
    /// 2. Updates the registry to mark the agent as active
    pub async fn activate_agent(&self, agent_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // First ensure the agent is loaded
        self.get_or_load_agent(agent_id).await?;
        
        // Debug assertion to fail fast if another thread holds the write lock
        debug_assert!(
            self.agent_registry.try_write().is_ok(),
            "Agent registry write lock unavailable - another thread may be holding it"
        );
        
        // Update registry (single source of truth)
        let mut registry = self.agent_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
        registry.activate_agent(agent_id)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to activate agent: {:?}", e)))?;
        
        Ok(())
    }
    
    /// Deactivate an agent (save and unload from memory)
    /// 
    /// Mirrors close_graph() - saves before removing from memory
    pub async fn deactivate_agent(&self, agent_id: &Uuid) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Get the agent to save
        let mut agents = self.agents.write().await;
        
        if let Some(mut agent) = agents.remove(agent_id) {
            // Save the agent before removing from memory
            agent.save()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to save agent before deactivation: {:?}", e)))?;
            
            info!("Saved and unloaded agent: {}", agent_id);
        }
        
        drop(agents);
        
        // Debug assertion to fail fast if another thread holds the write lock
        debug_assert!(
            self.agent_registry.try_write().is_ok(),
            "Agent registry write lock unavailable - another thread may be holding it"
        );
        
        // Update registry (single source of truth)
        let mut registry = self.agent_registry.write()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to write agent registry: {}", e)))?;
        registry.deactivate_agent(agent_id)
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