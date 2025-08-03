//! Application State Management
//! 
//! This module provides the central AppState struct that coordinates all
//! components of the Cymbiont knowledge graph engine. It handles graph
//! management, configuration, transactions, and optional server functionality.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::error::Error;
use std::fs;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, error};

use crate::{
    graph_manager::GraphManager,
    config::{load_config, Config},
    storage::{GraphRegistry, TransactionLog, TransactionCoordinator},
};

// Re-export the real WsConnection from server module
pub use crate::server::websocket::WsConnection;

/// Central application state that coordinates all Cymbiont components
pub struct AppState {
    // Core graph management (always present)
    pub graph_managers: Arc<RwLock<HashMap<String, RwLock<GraphManager>>>>,
    pub graph_registry: Arc<Mutex<GraphRegistry>>,
    pub active_graph_id: Arc<RwLock<Option<String>>>,
    pub config: Config,
    pub data_dir: PathBuf,  // Resolved absolute path
    
    // Transaction management (always present)
    pub transaction_coordinators: Arc<RwLock<HashMap<String, Arc<TransactionCoordinator>>>>,
    
    // Server-specific components (optional)
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
    pub auth_token: Arc<RwLock<Option<String>>>,  // Authentication token
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
        let graph_registry = Arc::new(Mutex::new(
            GraphRegistry::load_or_create(&registry_path, &data_dir)
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Graph registry error: {e:?}")))?
        ));
        
        // Initialize multi-graph managers and coordinators
        let graph_managers = Arc::new(RwLock::new(HashMap::new()));
        let transaction_coordinators = Arc::new(RwLock::new(HashMap::new()));
        
        
        // Create WebSocket connections if server mode
        let ws_connections = if with_server {
            Some(Arc::new(RwLock::new(HashMap::new())))
        } else {
            None
        };
        
        // Get the active graph from registry
        let initial_active_graph = {
            let registry = graph_registry.lock()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to lock registry: {}", e)))?;
            registry.get_active_graph_id().map(|s| s.to_string())
        };
        
        let app_state = Arc::new(AppState {
            graph_managers,
            graph_registry,
            active_graph_id: Arc::new(RwLock::new(initial_active_graph.clone())),
            config,
            data_dir: data_dir.clone(),
            transaction_coordinators,
            ws_ready_tx: Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
        });
        
        // If there's an active graph, ensure its manager is loaded
        if let Some(graph_id) = initial_active_graph {
            app_state.get_or_create_graph_manager(&graph_id).await?;
            info!("Loaded active graph: {}", graph_id);
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
    
    /// Get or create a GraphManager for the given graph ID
    pub async fn get_or_create_graph_manager(&self, graph_id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let managers = self.graph_managers.read().await;
        
        // Check if manager already exists
        if managers.contains_key(graph_id) {
            return Ok(());
        }
        
        // Drop read lock before acquiring write lock
        drop(managers);
        
        // Acquire write lock to create new manager
        let mut managers = self.graph_managers.write().await;
        
        // Double-check pattern - another thread may have created it
        if managers.contains_key(graph_id) {
            return Ok(());
        }
        
        // Create new GraphManager using the resolved data_dir
        let data_dir = self.data_dir.join("graphs").join(graph_id);
        fs::create_dir_all(&data_dir)?;
        
        let graph_manager = GraphManager::new(data_dir.clone())
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create graph manager for {}: {:?}", graph_id, e)))?;
        
        managers.insert(graph_id.to_string(), RwLock::new(graph_manager));
        
        // Create transaction coordinator for this graph
        let transaction_log_dir = data_dir.join("transaction_log");
        fs::create_dir_all(&transaction_log_dir)?;
        let transaction_log = Arc::new(TransactionLog::new(transaction_log_dir)
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create transaction log for {}: {:?}", graph_id, e)))?);
        
        let transaction_coordinator = Arc::new(TransactionCoordinator::new(transaction_log));
        
        // Store the coordinator
        let mut coordinators = self.transaction_coordinators.write().await;
        coordinators.insert(graph_id.to_string(), transaction_coordinator);
        
        Ok(())
    }
    
    /// Get the active graph manager (returns None if no active graph)
    pub async fn get_active_graph_manager(&self) -> Option<String> {
        self.active_graph_id.read().await.clone()
    }
    
    /// Set the active graph ID
    pub async fn set_active_graph(&self, graph_id: String) {
        let mut active = self.active_graph_id.write().await;
        *active = Some(graph_id.clone());
        drop(active); // Release the write lock
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
        
        // Save all loaded graphs
        let managers = self.graph_managers.read().await;
        
        for (graph_id, manager_lock) in managers.iter() {
            match manager_lock.write().await.save_graph() {
                Ok(_) => {
                    info!("✅ Saved graph: {}", graph_id);
                }
                Err(e) => error!("Failed to save graph {}: {}", graph_id, e),
            }
        }
        drop(managers);
        
        // Flush and close transaction logs
        let coordinators = self.transaction_coordinators.read().await;
        for (graph_id, coordinator) in coordinators.iter() {
            if let Err(e) = coordinator.close().await {
                error!("Failed to close transaction log for graph {}: {}", graph_id, e);
            }
        }
        drop(coordinators);
        
        // Save graph registry
        if let Ok(registry_guard) = self.graph_registry.lock() {
            if let Err(e) = registry_guard.save() {
                error!("Failed to save graph registry: {}", e);
            } else {
                info!("✅ Graph registry saved");
            }
        }
    }
    
    /// Execute an operation with transaction on the active graph
    pub async fn with_active_graph_transaction<F, T>(
        &self,
        operation: crate::storage::Operation,
        executor: F,
    ) -> Result<T, Box<dyn Error + Send + Sync>>
    where
        F: FnOnce(&mut GraphManager) -> std::result::Result<T, String>,
    {
        // Get active graph ID
        let active_id = self.get_active_graph_manager().await
            .ok_or_else(|| "No active graph".to_string())?;
        
        // Get transaction coordinator
        let coordinators = self.transaction_coordinators.read().await;
        let coordinator = coordinators.get(&active_id)
            .ok_or_else(|| "Transaction coordinator not found".to_string())?;
        
        // Clone coordinator to use in closure
        let coordinator = Arc::clone(coordinator);
        
        // Get graph manager and execute within transaction
        let managers = self.graph_managers.read().await;
        let manager_lock = managers.get(&active_id)
            .ok_or_else(|| "Graph manager not found".to_string())?;
        let mut manager = manager_lock.write().await;
        
        // Execute with transaction
        coordinator.execute_with_transaction(operation, || {
            executor(&mut *manager)
        }).await
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}