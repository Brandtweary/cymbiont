/**
 * @module main
 * @description Backend server orchestration for the PKM Knowledge Graph Plugin
 * 
 * This module serves as the central orchestrator for the PKM backend server, managing
 * application state and coordinating between specialized modules. After refactoring,
 * this module focuses on high-level control flow while delegating specific responsibilities
 * to dedicated modules.
 * 
 * Key responsibilities:
 * - Server lifecycle management (startup, shutdown, graceful termination)
 * - Application state management (AppState with graph manager, Logseq process, channels)
 * - Coordination between modules (config, logging, api, utils, graph_manager)
 * - Duration-based execution modes for development and testing
 * - Signal handling for clean shutdowns (Ctrl+C)
 * - Logseq process launching and termination
 * 
 * Module dependencies:
 * - config: Configuration loading and validation
 * - logging: Custom tracing setup
 * - utils: Port management, process utilities, and Logseq executable discovery
 * - api: HTTP routes and handlers
 * - graph_manager: Petgraph-based knowledge graph storage
 * 
 * The server supports two execution modes:
 * - Indefinite: Runs until terminated (production mode)
 * - Duration-based: Runs for specified seconds (development/testing)
 * 
 * When Logseq auto-launch is enabled, the server:
 * - Uses utils module to discover Logseq executable
 * - Launches Logseq after server startup
 * - Waits for plugin initialization before starting duration timer
 * - Terminates Logseq gracefully on shutdown
 */

use async_trait::async_trait;
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::process::exit;
use tokio::sync::{oneshot, RwLock};
use std::error::Error;
use std::fs;
use std::time::{Duration, Instant};
use tracing::{info, warn, error, debug};
use clap::Parser;
use std::collections::HashMap;

// Import our modules
mod pkm_data;
mod graph_manager;
mod config;
mod logging;
mod api;
mod utils;
mod websocket;
mod transaction_log;
mod transaction;
mod saga;
mod kg_api;
mod graph_registry;
mod edn;
mod session_manager;

use graph_manager::GraphManager;
use graph_registry::GraphRegistry;
use session_manager::{SessionManager, DbIdentifier};
use config::{load_config, validate_js_plugin_config, Config};
use logging::init_logging;
use api::create_router;
use utils::{SERVER_INFO_FILE, terminate_previous_instance, write_server_info, find_available_port};
use transaction_log::TransactionLog;
use transaction::TransactionCoordinator;
use saga::{SagaCoordinator, WorkflowSagas};

// CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run server for a specific duration in seconds (for testing)
    #[arg(long)]
    duration: Option<u64>,
    
    /// Force a full database sync on next plugin connection
    #[arg(long)]
    force_full_sync: bool,
    
    /// Force an incremental sync on next plugin connection
    #[arg(long)]
    force_incremental_sync: bool,
    
    /// Test WebSocket by sending a command after connection
    #[arg(long)]
    test_websocket: Option<String>,
    
    /// Launch with specific Logseq database by name
    #[arg(long, conflicts_with = "graph_path")]
    graph: Option<String>,
    
    /// Launch with specific Logseq database by path
    #[arg(long)]
    graph_path: Option<String>,
}

// Application state that will be shared between handlers
pub struct AppState {
    pub graph_managers: Arc<RwLock<HashMap<String, RwLock<GraphManager>>>>,
    pub graph_registry: Arc<Mutex<GraphRegistry>>,
    pub active_graph_id: Arc<RwLock<Option<String>>>,
    pub session_manager: Arc<SessionManager>,
    pub logseq_child: Mutex<Option<std::process::Child>>,
    pub plugin_init_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub sync_complete_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub force_full_sync: bool,
    pub force_incremental_sync: bool,
    pub config: Config,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, websocket::WsConnection>>>>,
    pub transaction_coordinators: Arc<RwLock<HashMap<String, Arc<TransactionCoordinator>>>>,
    pub saga_coordinator: Arc<SagaCoordinator>,
    pub workflow_sagas: Arc<WorkflowSagas>,
    pub correlation_to_saga: Arc<RwLock<HashMap<String, String>>>,
}

impl AppState {
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
        
        // Create new GraphManager (absolute path)
        let data_dir = std::env::current_dir()
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to get current directory: {e}")))?
            .join("data").join("graphs").join(graph_id);
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
        *active = Some(graph_id);
    }
}

// Implement GraphSwitchNotifier for Arc<AppState>
#[async_trait]
impl session_manager::GraphSwitchNotifier for Arc<AppState> {
    async fn notify_graph_switch(&self, target_graph_id: &str, target_graph_name: &str, target_graph_path: &str) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let command = websocket::Command::GraphSwitchRequested {
            target_graph_id: target_graph_id.to_string(),
            target_graph_name: target_graph_name.to_string(),
            target_graph_path: target_graph_path.to_string(),
        };
        websocket::broadcast_command(self, command).await
    }
}

// Cleanup function to handle graceful shutdown
fn cleanup_and_exit(app_state: Option<Arc<AppState>>, start_time: Instant) {
    let total_runtime = start_time.elapsed();
    info!("🧹 Cleaning up... (total runtime: {:.2}s)", total_runtime.as_secs_f64());
    
    // Save graph registry and all loaded graphs
    if let Some(state) = app_state {
        // Use tokio::task::block_in_place for blocking operations in async context
        tokio::task::block_in_place(|| {
            // Create a new runtime handle for the cleanup
            let handle = tokio::runtime::Handle::current();
            
            // Save all loaded graphs
            handle.block_on(async {
                let managers = state.graph_managers.read().await;
                for (graph_id, manager_lock) in managers.iter() {
                    match manager_lock.write().await.save_graph() {
                        Ok(_) => info!("✅ Saved graph: {}", graph_id),
                        Err(e) => error!("Failed to save graph {}: {}", graph_id, e),
                    }
                }
            });
        });
        
        // Save graph registry
        if let Ok(registry_guard) = state.graph_registry.lock() {
            let registry_path = PathBuf::from("data").join("graph_registry.json");
            if let Err(e) = registry_guard.save(&registry_path) {
                error!("Failed to save graph registry: {}", e);
            } else {
                info!("✅ Graph registry saved");
            }
        }
        
        // Terminate Logseq if it was launched by us
        if let Ok(mut child_guard) = state.logseq_child.lock() {
            if let Some(mut child) = child_guard.take() {
                match child.kill() {
                    Ok(_) => info!("✅ Logseq terminated successfully"),
                    Err(e) => error!("Error terminating Logseq: {}", e),
                }
            }
        }
    }
    
    if let Err(e) = fs::remove_file(SERVER_INFO_FILE) {
        error!("Error removing server info file: {e}");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Start runtime timer
    let start_time = Instant::now();
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    init_logging();
    
    // Load configuration
    let config = load_config();
    
    // Validate JavaScript plugin configuration
    if let Err(e) = validate_js_plugin_config(&config) {
        warn!("Failed to validate JavaScript plugin configuration: {}", e);
    }
    
    // Terminate any previous instance
    if fs::metadata(SERVER_INFO_FILE).is_ok() {
        terminate_previous_instance();
        let _ = fs::remove_file(SERVER_INFO_FILE);
    }
    
    // Initialize data directory (absolute path from executable location)
    let data_dir = std::env::current_dir()
        .map_err(|e| Box::<dyn Error>::from(format!("Failed to get current directory: {e}")))?
        .join("data");
    fs::create_dir_all(&data_dir)
        .map_err(|e| Box::<dyn Error>::from(format!("Failed to create data directory: {e}")))?;
    
    // Initialize graph registry
    let registry_path = data_dir.join("graph_registry.json");
    let graph_registry = Arc::new(Mutex::new(
        GraphRegistry::load_or_create(&registry_path)
            .map_err(|e| Box::<dyn Error>::from(format!("Graph registry error: {e:?}")))?
    ));
    
    // Initialize session manager
    let session_manager = Arc::new(SessionManager::new(
        graph_registry.clone(),
        config.logseq.clone(),
    ));
    
    // Initialize multi-graph managers and coordinators
    let graph_managers = Arc::new(RwLock::new(HashMap::new()));
    let transaction_coordinators = Arc::new(RwLock::new(HashMap::new()));
    
    // Initialize saga coordinator (shared across all graphs for now)
    // TODO: Consider per-graph saga coordinators in the future
    let dummy_transaction_log = Arc::new(TransactionLog::new(data_dir.join("saga_transaction_log"))
        .map_err(|e| Box::<dyn Error>::from(format!("Saga transaction log error: {e:?}")))?);
    let dummy_coordinator = Arc::new(TransactionCoordinator::new(dummy_transaction_log));
    let saga_coordinator = Arc::new(SagaCoordinator::new(dummy_coordinator.clone()));
    let workflow_sagas = Arc::new(WorkflowSagas::new(
        saga_coordinator.clone(),
        dummy_coordinator.clone(),
    ));
    
    // Log if force sync is enabled
    if args.force_full_sync {
        info!("⚡ Force full sync enabled - next plugin connection will trigger a full database sync");
    }
    if args.force_incremental_sync {
        info!("⚡ Force incremental sync enabled - next plugin connection will trigger an incremental sync");
    }
    
    // Create shared application state
    let app_state = Arc::new(AppState {
        graph_managers: graph_managers.clone(),
        graph_registry: graph_registry.clone(),
        active_graph_id: Arc::new(RwLock::new(None)),
        session_manager: session_manager.clone(),
        logseq_child: Mutex::new(None),
        plugin_init_tx: Mutex::new(None),
        sync_complete_tx: Mutex::new(None),
        ws_ready_tx: Mutex::new(None),
        force_full_sync: args.force_full_sync,
        force_incremental_sync: args.force_incremental_sync,
        config: config.clone(),
        ws_connections: Some(Arc::new(RwLock::new(HashMap::new()))),
        transaction_coordinators: transaction_coordinators.clone(),
        saga_coordinator: saga_coordinator.clone(),
        workflow_sagas: workflow_sagas.clone(),
        correlation_to_saga: Arc::new(RwLock::new(HashMap::new())),
    });
    
    // Set up exit handler
    let app_state_clone = app_state.clone();
    ctrlc::set_handler(move || {
        info!("🛑 Received shutdown signal");
        cleanup_and_exit(Some(app_state_clone.clone()), start_time);
        exit(0);
    }).expect("Error setting Ctrl-C handler");
    
    // Define the application routes
    let app = create_router(app_state.clone());

    // Find available port
    let port = find_available_port(&config.backend)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    // Write server info file for JS plugin
    write_server_info("127.0.0.1", port)?;
    
    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await
        .map_err(|e| Box::<dyn Error>::from(format!("Listener error: {e}")))?;
    
    info!("🚀 Backend server listening on {}", addr);
    
    // Launch Logseq with session manager
    let plugin_init_rx = if config.logseq.auto_launch {
        // Determine CLI target database
        let cli_target = if let Some(name) = args.graph {
            Some(DbIdentifier::Name(name))
        } else if let Some(path) = args.graph_path {
            Some(DbIdentifier::Path(path))
        } else {
            None
        };
        
        // Launch Logseq (this is async, but we'll handle it synchronously for now)
        let logseq_child = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match session_manager.launch_logseq(cli_target).await {
                    Ok(child) => child,
                    Err(e) => {
                        error!("Failed to launch Logseq: {}", e);
                        None
                    }
                }
            })
        });
        
        // Store the child process handle if we got one
        if let Some(child) = logseq_child {
            if let Ok(mut child_guard) = app_state.logseq_child.lock() {
                *child_guard = Some(child);
            }
        }
        
        // Create initialization channel
        let (tx, rx) = oneshot::channel::<()>();
        if let Ok(mut tx_guard) = app_state.plugin_init_tx.lock() {
            *tx_guard = Some(tx);
        }
        Some(rx)
    } else {
        None
    };
    
    // Determine duration: explicit CLI arg takes precedence over config default
    let duration_secs = args.duration.or(config.development.default_duration);
    
    // Warn if using default duration in release build
    #[cfg(not(debug_assertions))]
    if let Some(duration) = config.development.default_duration {
        warn!("Development default_duration ({} seconds) detected in release build - this should be null in production!", duration);
    }
    
    // Run server with appropriate configuration
    if let Some(duration) = duration_secs {
        if let Some(rx) = plugin_init_rx {
            // Wait for plugin initialization before starting timer
            run_with_duration(listener, app, app_state.clone(), rx, duration, args.test_websocket).await?;
        } else {
            // No Logseq, start timer immediately
            debug!("Server will run for {} seconds", duration);
            run_server_with_timeout(listener, app, duration).await?;
        }
    } else {
        // Run indefinitely
        if let Some(rx) = plugin_init_rx {
            // Monitor plugin initialization in background
            tokio::spawn(async move {
                match rx.await {
                    Ok(_) => debug!("Plugin initialization confirmed"),
                    Err(_) => debug!("Plugin initialization channel closed"),
                }
            });
        }
        
        axum::serve(listener, app).await
            .map_err(|e| Box::<dyn Error>::from(format!("Server error: {e}")))?;
    }
    
    // Clean up before exiting
    cleanup_and_exit(Some(app_state), start_time);
    
    Ok(())
}

// Run server with duration timer starting after plugin initialization
async fn run_with_duration(
    listener: tokio::net::TcpListener,
    app: Router,
    app_state: Arc<AppState>,
    plugin_initialized: oneshot::Receiver<()>,
    duration_secs: u64,
    test_websocket: Option<String>,
) -> Result<(), Box<dyn Error>> {
    // Create graceful shutdown signal
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    
    // Create sync completion channel BEFORE plugin starts
    let (sync_tx, sync_rx) = oneshot::channel::<()>();
    if let Ok(mut tx_guard) = app_state.sync_complete_tx.lock() {
        *tx_guard = Some(sync_tx);
    }
    
    // Serve with graceful shutdown capability
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        });
    
    // Spawn test WebSocket task if requested
    let test_task = if let Some(test_command) = test_websocket {
        let app_state_clone = app_state.clone();
        let (ws_ready_tx, ws_ready_rx) = oneshot::channel::<()>();
        
        // Store the channel for WebSocket ready signal
        if let Ok(mut tx_guard) = app_state.ws_ready_tx.lock() {
            *tx_guard = Some(ws_ready_tx);
        }
        
        Some(tokio::spawn(async move {
            // Wait for WebSocket connection to be ready
            match ws_ready_rx.await {
                Ok(_) => {
                    info!("🧪 WebSocket ready, executing test command");
                    test_websocket_command(&app_state_clone, &test_command).await;
                }
                Err(_) => {
                    error!("WebSocket ready signal never received");
                }
            }
        }))
    } else {
        None
    };
    
    // Run server and timer concurrently
    tokio::select! {
        result = server => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = async {
            // Wait for plugin to initialize
            match plugin_initialized.await {
                Ok(_) => {
                    debug!("🏃 Server will run for {} seconds after plugin initialization", duration_secs);
                    tokio::time::sleep(Duration::from_secs(duration_secs)).await;
                    debug!("⏱️ Duration limit reached, checking for active sync...");
                    
                    // Wait for sync completion with timeout
                    tokio::select! {
                        _ = sync_rx => {
                            debug!("✅ Sync completion received, shutting down gracefully");
                        }
                        _ = tokio::time::sleep(Duration::from_secs(10)) => {
                            debug!("⏱️ Timeout waiting for sync completion, shutting down anyway");
                        }
                    }
                },
                Err(_) => {
                    // If plugin init fails, still run with timer
                    debug!("⚠️ Plugin initialization failed, running with {} second timer anyway", duration_secs);
                    tokio::time::sleep(Duration::from_secs(duration_secs)).await;
                    debug!("⏱️ Duration limit reached, shutting down gracefully");
                }
            }
            
            // Signal server to start graceful shutdown
            let _ = shutdown_tx.send(());
        } => {}
    }
    
    // Clean up test task if it's still running
    if let Some(task) = test_task {
        task.abort();
    }
    
    Ok(())
}

// Send test WebSocket command
async fn test_websocket_command(app_state: &Arc<AppState>, command_type: &str) {
    use websocket::{Command, broadcast_command};
    
    let command = match command_type {
        "test" | "echo" => {
            info!("🧪 Sending test WebSocket command");
            Command::Test {
                message: format!("Test message from server at {:?}", std::time::SystemTime::now()),
            }
        },
        "page" | "create-page" => {
            info!("🧪 Sending create page command");
            let mut properties = HashMap::new();
            properties.insert("type".to_string(), "test-page".to_string());
            properties.insert("created-by".to_string(), "cymbiont-websocket".to_string());
            
            Command::CreatePage {
                name: "test-websocket".to_string(),
                properties: Some(properties),
                correlation_id: None,
            }
        },
        "block" | "create-block" => {
            info!("🧪 Sending create block command");
            Command::CreateBlock {
                content: format!("Test block created via WebSocket at {:?}", std::time::SystemTime::now()),
                parent_id: None,
                page_name: Some("test-websocket".to_string()),
                correlation_id: None,
                temp_id: None,
            }
        },
        _ => {
            error!("Unknown test command type: {} (use 'test', 'page', 'block')", command_type);
            return;
        }
    };
    
    match broadcast_command(app_state, command).await {
        Ok(()) => info!("✅ Test WebSocket command sent successfully"),
        Err(e) => error!("Failed to send test WebSocket command: {}", e),
    }
}

// Simple timeout for when Logseq is not launched
async fn run_server_with_timeout(
    listener: tokio::net::TcpListener,
    app: Router,
    duration_secs: u64,
) -> Result<(), Box<dyn Error>> {
    let server = axum::serve(listener, app);
    
    tokio::select! {
        result = server => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = tokio::time::sleep(Duration::from_secs(duration_secs)) => {
            info!("Duration limit reached, shutting down gracefully");
        }
    }
    
    Ok(())
}

