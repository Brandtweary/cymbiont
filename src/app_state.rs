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
    graph_registry::GraphRegistry,
    config::{load_config, Config},
    transaction_log::TransactionLog,
    transaction::{TransactionCoordinator},
    saga::{SagaCoordinator, WorkflowSagas},
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
    
    // Transaction and saga management (always present)
    pub transaction_coordinators: Arc<RwLock<HashMap<String, Arc<TransactionCoordinator>>>>,
    // TODO: Remove allow(dead_code) once saga system is fully implemented
    #[allow(dead_code)]
    pub saga_coordinator: Arc<SagaCoordinator>,
    pub workflow_sagas: Arc<WorkflowSagas>,
    pub correlation_to_saga: Arc<RwLock<HashMap<String, String>>>,
    
    // Server-specific components (optional)
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
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
            GraphRegistry::load_or_create(&registry_path)
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Graph registry error: {e:?}")))?
        ));
        
        // Initialize multi-graph managers and coordinators
        let graph_managers = Arc::new(RwLock::new(HashMap::new()));
        let transaction_coordinators = Arc::new(RwLock::new(HashMap::new()));
        
        // Initialize saga coordinator (shared across all graphs)
        let dummy_transaction_log = Arc::new(TransactionLog::new(data_dir.join("saga_transaction_log"))
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Saga transaction log error: {e:?}")))?);
        let dummy_coordinator = Arc::new(TransactionCoordinator::new(dummy_transaction_log));
        let saga_coordinator = Arc::new(SagaCoordinator::new(dummy_coordinator.clone()));
        let workflow_sagas = Arc::new(WorkflowSagas::new(
            saga_coordinator.clone(),
            dummy_coordinator.clone(),
        ));
        
        // Create WebSocket connections if server mode
        let ws_connections = if with_server {
            Some(Arc::new(RwLock::new(HashMap::new())))
        } else {
            None
        };
        
        let app_state = Arc::new(AppState {
            graph_managers,
            graph_registry,
            active_graph_id: Arc::new(RwLock::new(None)),
            config,
            transaction_coordinators,
            saga_coordinator,
            workflow_sagas,
            correlation_to_saga: Arc::new(RwLock::new(HashMap::new())),
            ws_ready_tx: Mutex::new(None),
            ws_connections,
        });
        
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
        
        // Create new GraphManager using config data_dir
        let base_data_dir = if std::path::Path::new(&self.config.data_dir).is_absolute() {
            PathBuf::from(&self.config.data_dir)
        } else {
            std::env::current_dir()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to get current directory: {e}")))?
                .join(&self.config.data_dir)
        };
        let data_dir = base_data_dir.join("graphs").join(graph_id);
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
        
        info!("Created new GraphManager and TransactionCoordinator for graph: {}", graph_id);
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
        // Save all loaded graphs
        let managers = self.graph_managers.read().await;
        for (graph_id, manager_lock) in managers.iter() {
            match manager_lock.write().await.save_graph() {
                Ok(_) => info!("✅ Saved graph: {}", graph_id),
                Err(e) => error!("Failed to save graph {}: {}", graph_id, e),
            }
        }
        drop(managers);
        
        // Save graph registry
        if let Ok(registry_guard) = self.graph_registry.lock() {
            let base_data_dir = if std::path::Path::new(&self.config.data_dir).is_absolute() {
                PathBuf::from(&self.config.data_dir)
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(&self.config.data_dir)
            };
            let registry_path = base_data_dir.join("graph_registry.json");
            if let Err(e) = registry_guard.save(&registry_path) {
                error!("Failed to save graph registry: {}", e);
            } else {
                info!("✅ Graph registry saved");
            }
        }
    }
}