/**
 * # Cymbiont - Terminal-First Knowledge Graph Engine
 * 
 * Cymbiont is a terminal-first knowledge graph engine designed for AI agents.
 * It provides a clean command-line interface for managing and querying
 * knowledge graphs with support for import, export, and graph operations.
 * 
 * ## Usage
 * 
 * ```bash
 * # Basic usage (shows graph info then runs indefinitely)
 * cymbiont
 * 
 * # Override data directory
 * cymbiont --data-dir /path/to/data
 * 
 * # Import Logseq graph (then continues running)
 * cymbiont --import-logseq /path/to/logseq/graph
 * 
 * # Run for specific duration (for testing)
 * cymbiont --duration 10
 * 
 * # Run as HTTP/WebSocket server
 * cymbiont --server
 * 
 * # Run server with specific duration
 * cymbiont --server --duration 60
 * ```
 * 
 * ## Lifecycle Behavior
 * 
 * The CLI always runs continuously after performing any requested operations:
 * - With --duration flag or config: Runs for specified seconds then exits
 * - Without duration: Runs indefinitely until Ctrl+C
 * - With --import-logseq: Performs import, then continues running
 * 
 * This design allows the CLI to serve as a persistent knowledge graph engine
 * that can handle future interactive features while maintaining simplicity.
 * 
 * ## Graceful Shutdown
 * 
 * Both CLI and server modes handle SIGINT (Ctrl+C) uniformly:
 * - First Ctrl+C: Initiates graceful shutdown, waits for active transactions to complete (up to 30s)
 * - Second Ctrl+C: Forces immediate termination with transaction log flush
 * 
 * After graceful cleanup, the process uses std::process::exit(0) due to sled database background threads.
 */

use std::error::Error;
use clap::Parser;
use tracing::{info, error, warn};

// Internal modules
mod app_state;
mod config;
mod graph_manager;
mod graph_operations;
mod logging;
mod import;
mod server;
mod storage;
mod utils;

use app_state::AppState;
use logging::init_logging;
use graph_operations::GraphOperationsExt;

// CLI arguments  
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run as HTTP/WebSocket server
    #[arg(long)]
    server: bool,
    
    /// Override data directory path (defaults to config value)
    #[arg(long)]
    data_dir: Option<String>,
    
    /// Path to configuration file
    #[arg(long)]
    config: Option<String>,
    
    /// Import a Logseq graph from directory
    #[arg(long, value_name = "PATH")]
    import_logseq: Option<String>,
    
    /// Delete a graph by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    delete_graph: Option<String>,
    
    // Server-specific args (only used when --server is provided)
    /// Run server for a specific duration in seconds (for testing)
    #[arg(long)]
    duration: Option<u64>,
    
    
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Create Tokio runtime explicitly for proper shutdown control
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Failed to create runtime: {}", e)))?;
    
    // Run async main logic
    let result = runtime.block_on(async_main());
    
    // Force runtime shutdown with timeout
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    
    result
}

async fn async_main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    init_logging();
    
    
    // Track start time for total runtime measurement
    let start_time = std::time::Instant::now();
    
    // Branch based on server flag
    if args.server {
        // Create server app state
        let app_state = AppState::new_server(args.config.clone(), args.data_dir.clone()).await?;
        
        // Run recovery for all open graphs (same as CLI path)
        run_all_graphs_recovery(&app_state).await;
        
        // Start server and get handle
        let (server_handle, server_info_file) = server::start_server(app_state.clone()).await?;
        
        // Handle duration and shutdown uniformly with CLI mode
        if let Some(duration) = args.duration.or(app_state.config.development.default_duration) {
            tokio::select! {
                result = server_handle => {
                    if let Err(e) = result {
                        error!("Server task error: {}", e);
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(duration)) => {
                    info!("⏱️ Duration limit reached");
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("🛑 Received shutdown signal");
                    if handle_graceful_shutdown(&app_state).await {
                        // Force quit requested
                        server::cleanup_server_info(&server_info_file);
                        std::process::exit(1);
                    }
                }
            }
        } else {
            // Run indefinitely
            tokio::select! {
                result = server_handle => {
                    if let Err(e) = result {
                        error!("Server task error: {}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("🛑 Received shutdown signal");
                    if handle_graceful_shutdown(&app_state).await {
                        // Force quit requested
                        server::cleanup_server_info(&server_info_file);
                        std::process::exit(1);
                    }
                }
            }
        }
        
        // Cleanup for server mode
        server::cleanup_server_info(&server_info_file);
        app_state.cleanup_and_save().await;
        info!("🧹 Server shutdown complete");
    } else {
        // Run as CLI
        let app_state = AppState::new_cli(args.config, args.data_dir.clone()).await?;
        
        info!("🧠 Cymbiont CLI initialized");
        info!("📁 Data directory: {}", app_state.data_dir.display());
        
        // Handle Logseq import if requested
        if let Some(logseq_path) = args.import_logseq {
            let import_start = std::time::Instant::now();
            let path = std::path::Path::new(&logseq_path);
            let result = import::import_logseq_graph(&app_state, path, None).await?;
            
            // Report any errors that occurred during import
            if !result.errors.is_empty() {
                error!("Import completed with {} errors:", result.errors.len());
                for err in &result.errors {
                    error!("  - {}", err);
                }
            }
            
            info!("✅ Import complete in {:.3}s. Continuing to run...", import_start.elapsed().as_secs_f64());
        }
        
        // Handle graph deletion if requested
        if let Some(graph_identifier) = args.delete_graph {
            use crate::graph_operations::GraphOperationsExt;
            use uuid::Uuid;
            
            // Resolve the graph using centralized logic
            let graph_uuid = {
                let registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&graph_identifier).ok();
                
                // Use resolve_graph_target to handle both UUID and name
                registry.resolve_graph_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&graph_identifier) } else { None },
                    false  // No smart default for delete
                ).map_err(|e| format!("Failed to resolve graph: {}", e))?
            };
            
            info!("🗑️  Deleting graph: {}", graph_identifier);
            
            // Delete the graph using GraphOperations extension
            app_state.delete_graph(&graph_uuid).await
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?;
            
            info!("✅ Graph deleted successfully");
            info!("Continuing to run...");
        }
        
        let graphs = {
            let registry_guard = app_state.graph_registry.read().unwrap();
            registry_guard.get_all_graphs()
        };
        
        if graphs.is_empty() {
            info!("📊 No graphs found");
        } else {
            info!("📊 Found {} registered graph(s)", graphs.len());
        }
        
        // Get open graphs
        let open_graphs = app_state.list_open_graphs().await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?;
        
        if !open_graphs.is_empty() {
            {
                let registry_guard = app_state.graph_registry.read().unwrap();
                info!("📂 {} open graph(s):", open_graphs.len());
                for graph_id in &open_graphs {
                    if let Some(graph_info) = registry_guard.get_graph(graph_id) {
                        info!("  - {} ({})", graph_info.name, graph_info.id);
                    }
                }
            } // registry_guard drops here
            
            // Run recovery for all open graphs on startup
            run_all_graphs_recovery(&app_state).await;
        } else {
            info!("📂 No open graphs");
        }
        
        // Handle duration for CLI mode
        if let Some(duration) = args.duration.or(app_state.config.development.default_duration) {
            tokio::time::sleep(std::time::Duration::from_secs(duration)).await;
            info!("⏱️ Duration limit reached");
        } else {
            // Run indefinitely (for future interactive features)
            utils::write_pid_file()
                .map_err(|e| Box::<dyn Error + Send + Sync>::from(e.to_string()))?;
            
            info!("Running indefinitely. Press Ctrl+C to exit.");
            
            // First Ctrl+C - initiate graceful shutdown
            tokio::signal::ctrl_c().await?;
            info!("🛑 Received shutdown signal");
            
            if handle_graceful_shutdown(&app_state).await {
                // Force quit requested
                utils::remove_pid_file();
                std::process::exit(1);
            }
        }
        
        // Cleanup for CLI mode
        app_state.cleanup_and_save().await;
        utils::remove_pid_file();
        info!("🧹 CLI shutdown complete");
    }
    
    let total_runtime = start_time.elapsed();
    info!("💫 Total runtime: {:.2}s", total_runtime.as_secs_f64());
    
    // Force exit because sled/tokio threads won't terminate
    // This is the recommended workaround for sled issue #1234
    error!("FORCING PROCESS EXIT NOW");
    std::process::exit(0)
}

/// Handle graceful shutdown with transaction completion
/// Returns true if should exit immediately (force quit), false to continue with normal cleanup
async fn handle_graceful_shutdown(app_state: &std::sync::Arc<AppState>) -> bool {
    // Brief grace period for spawned tasks to reach transaction boundary
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    let active_count = app_state.initiate_graceful_shutdown().await;
    if active_count > 0 {
        info!("⏳ Waiting for {} transactions to complete... Press Ctrl+C again to force quit", active_count);
        
        tokio::select! {
            completed = app_state.wait_for_transactions(std::time::Duration::from_secs(30)) => {
                if completed {
                    info!("✅ All transactions completed");
                } else {
                    warn!("⚠️ Timeout waiting for transactions");
                }
                false // Continue with normal cleanup
            }
            _ = tokio::signal::ctrl_c() => {
                error!("⚡ Force quit requested");
                app_state.force_flush_transactions().await;
                true // Force immediate exit
            }
        }
    } else {
        false // No active transactions, continue with normal cleanup
    }
}

/// Run recovery for all graphs on startup (shared between CLI and server)
/// This includes both open and closed graphs to ensure no pending transactions are lost
async fn run_all_graphs_recovery(app_state: &std::sync::Arc<AppState>) {
    let all_graphs = {
        let registry_guard = match app_state.graph_registry.read() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to read registry for recovery: {}", e);
                return;
            }
        };
        registry_guard.get_all_graphs()
    };
    
    info!("🔄 Running recovery for {} graphs", all_graphs.len());
    
    for graph_info in all_graphs {
        let graph_id = graph_info.id;
        let graph_name = graph_info.name.clone();
        
        // Check if graph is already open
        let was_open = {
            let registry_guard = match app_state.graph_registry.read() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Failed to read registry: {}", e);
                    continue;
                }
            };
            registry_guard.is_graph_open(&graph_id)
        };
        
        // If graph is closed, temporarily open it for recovery
        if !was_open {
            if let Err(e) = app_state.open_graph(graph_id).await {
                error!("Failed to open graph {} for recovery: {}", graph_name, e);
                continue;
            }
        }
        
        // Ensure the graph manager is loaded
        if let Err(e) = app_state.get_or_create_graph_manager(&graph_id).await {
            error!("Failed to create graph manager for {}: {}", graph_name, e);
            if !was_open {
                // Try to close it again if we opened it
                let _ = app_state.close_graph(graph_id).await;
            }
            continue;
        }
        
        // Run recovery
        if let Some(coordinator) = app_state.get_transaction_coordinator(&graph_id).await {
            match coordinator.recover_pending_transactions().await {
                Ok(pending_transactions) => {
                    if !pending_transactions.is_empty() {
                        info!("🔄 Replaying {} pending transactions for graph {}", 
                              pending_transactions.len(), graph_name);
                        
                        for transaction in pending_transactions {
                            if let Err(e) = app_state.replay_transaction(&graph_id, transaction, coordinator.clone()).await {
                                error!("Failed to replay transaction: {}", e);
                            }
                        }
                        
                        // Save the graph after recovery
                        let resources = app_state.graph_resources.read().await;
                        if let Some(graph_resources) = resources.get(&graph_id) {
                            match graph_resources.manager.write().await.save_graph() {
                                Ok(_) => info!("💾 Saved graph {} after recovery", graph_name),
                                Err(e) => error!("Failed to save graph {} after recovery: {}", graph_name, e),
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to recover transactions for {}: {}", graph_name, e);
                }
            }
        }
        
        // If graph was originally closed, close it again
        if !was_open {
            if let Err(e) = app_state.close_graph(graph_id).await {
                error!("Failed to close graph {} after recovery: {}", graph_name, e);
            }
        }
    }
    
    info!("✅ Recovery complete for all graphs");
}