/**
 * # Cymbiont - Self-Organizing Knowledge Graph Agent
 * 
 * Cymbiont transforms your personal knowledge management (PKM) tool into a queryable 
 * knowledge graph, providing AI agents with rich contextual understanding of your notes, 
 * thoughts, and connections. Unlike traditional RAG approaches that treat documents as 
 * isolated text chunks, Cymbiont preserves and leverages the inherent graph structure 
 * of your knowledge base.
 * 
 * ## Getting Started
 * 
 * New to Cymbiont? Start with these documentation files:
 * 
 * - **[README.md](../README.md)** - Installation, setup, and basic usage
 * - **[CLAUDE.md](../CLAUDE.md)** - Development guide, build commands, and CLI reference  
 * - **[cymbiont_architecture.md](../cymbiont_architecture.md)** - Comprehensive technical architecture
 * 
 * ## Core Features
 * 
 * - **Real-time Sync**: Automatically syncs with PKM tools to maintain an up-to-date knowledge graph
 * - **Graph-Aware Context**: Provides AI agents with understanding of relationships between concepts
 * - **Multi-Graph Support**: Manage multiple knowledge bases simultaneously with complete isolation
 * - **Incremental Updates**: Efficiently tracks changes without full database rescans
 * - **Configurable Storage**: Flexible data directory configuration for different deployment scenarios
 * 
 * ## Architecture Overview
 * 
 * Cymbiont consists of three main components:
 * 1. **Backend Server** (Rust) - This codebase: graph management, API endpoints, sync coordination
 * 2. **Agent Integration** - Terminal-based agents for knowledge management
 * 3. **AI Agent Integration** - Future integration with aichat-agent library for LLM capabilities
 * 
 * ## Main Module Responsibilities
 * 
 * This main.rs module serves as the application entry point and orchestrator:
 * 
 * ### Core Functions
 * - Server lifecycle management (startup, shutdown, graceful termination)
 * - Application state coordination (AppState with multi-graph managers)
 * - CLI argument processing and configuration loading
 * - Data directory initialization and path resolution
 * - Process lifecycle and signal handling
 * - Signal handling for clean shutdowns (Ctrl+C)
 * 
 * ### Module Coordination
 * - **config**: YAML configuration loading and CLI overrides
 * - **api**: HTTP router creation and endpoint handlers  
 * - **graph_registry**: Multi-graph identification and switching
 * - **websocket**: Real-time communication with plugin
 * - **utils**: Process management and platform utilities
 * 
 * ### Execution Modes
 * - **Production**: Runs indefinitely until terminated
 * - **Development**: Auto-shutdown after configured duration (default: 3 seconds)
 * - **Testing**: Uses isolated data directories for test isolation
 * 
 * ## Quick Start
 * 
 * ```bash
 * # Standard operation
 * cargo run
 * 
 * # With specific graph
 * cargo run -- --graph "My Knowledge Base"
 * 
 * # Custom data directory  
 * cargo run -- --data-dir /path/to/data
 * 
 * # Shutdown running server
 * cargo run -- --shutdown-server
 * ```
 * 
 * For comprehensive usage instructions, see README.md in the project root.
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

use graph_manager::GraphManager;
use graph_registry::GraphRegistry;
use config::{load_config, Config};
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
    
    
    /// Shutdown a running Cymbiont server gracefully
    #[arg(long)]
    shutdown_server: bool,
    
    /// Override data directory path (defaults to config value)
    #[arg(long)]
    data_dir: Option<String>,
}

// Application state that will be shared between handlers
pub struct AppState {
    pub graph_managers: Arc<RwLock<HashMap<String, RwLock<GraphManager>>>>,
    pub graph_registry: Arc<Mutex<GraphRegistry>>,
    pub active_graph_id: Arc<RwLock<Option<String>>>,
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
        drop(active); // Release the write lock before calling session manager
        
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
        
        // Save graph registry using config data_dir
        if let Ok(registry_guard) = state.graph_registry.lock() {
            let base_data_dir = if std::path::Path::new(&state.config.data_dir).is_absolute() {
                PathBuf::from(&state.config.data_dir)
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(&state.config.data_dir)
            };
            let registry_path = base_data_dir.join("graph_registry.json");
            if let Err(e) = registry_guard.save(&registry_path) {
                error!("Failed to save graph registry: {}", e);
            } else {
                info!("✅ Graph registry saved");
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
    
    // Handle shutdown command
    if args.shutdown_server {
        if let Ok(info_str) = fs::read_to_string(SERVER_INFO_FILE) {
            if let Ok(info) = serde_json::from_str::<utils::ServerInfo>(&info_str) {
                println!("Shutting down Cymbiont server (PID: {})...", info.pid);
                if terminate_previous_instance() {
                    println!("Server shutdown successfully");
                    let _ = fs::remove_file(SERVER_INFO_FILE);
                    return Ok(());
                } else {
                    eprintln!("Failed to shutdown server or server not running");
                    return Err("Shutdown failed".into());
                }
            } else {
                eprintln!("Failed to parse server info file");
                return Err("Invalid server info".into());
            }
        } else {
            eprintln!("No running Cymbiont server found");
            return Err("No server to shutdown".into());
        }
    }
    
    // Initialize logging
    init_logging();
    
    // Load configuration
    let mut config = load_config();
    
    // Apply CLI data_dir override if provided
    if let Some(cli_data_dir) = &args.data_dir {
        info!("🗂️  Overriding data directory from CLI: {}", cli_data_dir);
        config.data_dir = cli_data_dir.clone();
    }
    
    
    // Terminate any previous instance
    if fs::metadata(SERVER_INFO_FILE).is_ok() {
        terminate_previous_instance();
        let _ = fs::remove_file(SERVER_INFO_FILE);
    }
    
    // Initialize data directory from config
    let data_dir = if std::path::Path::new(&config.data_dir).is_absolute() {
        PathBuf::from(&config.data_dir)
    } else {
        std::env::current_dir()
            .map_err(|e| Box::<dyn Error>::from(format!("Failed to get current directory: {e}")))?
            .join(&config.data_dir)
    };
    fs::create_dir_all(&data_dir)
        .map_err(|e| Box::<dyn Error>::from(format!("Failed to create data directory: {e}")))?;
    
    // Initialize graph registry
    let registry_path = data_dir.join("graph_registry.json");
    let graph_registry = Arc::new(Mutex::new(
        GraphRegistry::load_or_create(&registry_path)
            .map_err(|e| Box::<dyn Error>::from(format!("Graph registry error: {e:?}")))?
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
    
    // Create initialization channel for plugin compatibility
    let plugin_init_rx = None;
    
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
            // Start timer immediately
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

// Simple timeout for server
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

