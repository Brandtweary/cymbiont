//! Application State Management
//! 
//! This module provides the central AppState struct that acts as the coordination
//! layer for all components of the Cymbiont knowledge graph engine. 
//! 
//! ## Architecture Role
//! 
//! AppState serves as the "central nervous system" that connects:
//! - Graph resources (bundled managers + coordinators via GraphResources)
//! - Graph registry (multi-graph metadata and open/closed state tracking)
//! - WebSocket connections (real-time communication)
//! - Configuration (runtime settings)
//! 
//! ## Design Philosophy
//! 
//! AppState is intentionally a coordination layer, not a business logic layer.
//! It provides the wiring between components but delegates actual work:
//! - PKM operations → Handled by GraphOperationsExt trait
//! - Graph storage → Handled by GraphManager
//! - Transactions → Handled by TransactionCoordinator
//! - Open/closed state → Handled by GraphRegistry (single source of truth)
//! 
//! ## Resource Management
//! 
//! The GraphResources struct bundles graph managers with their transaction
//! coordinators to ensure atomic lifecycle management. This prevents
//! inconsistent states where one resource exists without the other.
//! 
//! ## Key Methods
//! 
//! - `with_graph_transaction(graph_id)` - Wraps operations in transactions for specific graph
//! - `get_or_create_graph_manager()` - Lazy initialization of bundled graph resources
//! - `open_graph()` / `close_graph()` - Explicit graph lifecycle management
//! - `get_transaction_coordinator()` - Access to per-graph WAL
//! - `cleanup_and_save()` - Graceful shutdown coordination
//! 
//! ## Extension Pattern
//! 
//! Domain-specific operations are added via extension traits rather than
//! methods on AppState itself. See GraphOperationsExt for PKM operations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock as SyncRwLock, Mutex};
use std::error::Error;
use std::fs;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, error};
use uuid::Uuid;

use crate::{
    graph_manager::GraphManager,
    config::{load_config, Config},
    storage::{GraphRegistry, TransactionLog, TransactionCoordinator},
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
    pub config: Config,
    pub data_dir: PathBuf,  // Resolved absolute path
    
    // Server-specific components (optional)
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
    pub auth_token: Arc<RwLock<Option<String>>>,  // Authentication token
    
    // Test infrastructure
    pub operation_freeze: Arc<RwLock<bool>>,  // Freeze operations after transaction creation
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
        
        // Initialize graph resources map (managers + coordinators bundled)
        let graph_resources = Arc::new(RwLock::new(HashMap::new()));
        
        
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
            config,
            data_dir: data_dir.clone(),
            ws_ready_tx: Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
            operation_freeze: Arc::new(RwLock::new(false)),
        });
        
        // Load all open graphs
        for graph_id in initial_open_graphs {
            app_state.get_or_create_graph_manager(&graph_id).await?;
            info!("Loaded open graph: {}", graph_id);
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
    
}