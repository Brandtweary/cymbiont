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
 * Both CLI and server modes handle SIGINT (Ctrl+C) to trigger cleanup_and_save() before exit.
 * After graceful cleanup, the process uses std::process::exit(0) due to sled database background threads.
 */

use std::error::Error;
use clap::Parser;
use tracing::{info, error, warn};

// Internal modules
mod app_state;
mod config;
mod graph_manager;
mod logging;
mod import;
mod server;
mod storage;
mod utils;

use app_state::AppState;
use logging::init_logging;

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
        
        // Run server (will handle its own tokio::select! for shutdown)
        server::run_server_with_duration(app_state.clone(), args.duration).await?;
        
        // Cleanup for server mode
        app_state.cleanup_and_save().await;
        info!("🧹 Server shutdown complete");
    } else {
        // Run as CLI
        let app_state = AppState::new_cli(args.config, args.data_dir.clone()).await?;
        
        info!("🧠 Cymbiont CLI initialized");
        info!("📁 Data directory: {}", app_state.data_dir.display());
        
        // Handle Logseq import if requested
        if let Some(logseq_path) = args.import_logseq {
            let path = std::path::Path::new(&logseq_path);
            let result = import::import_logseq_graph(&app_state, path, None).await?;
            
            // Report any errors that occurred during import
            if !result.errors.is_empty() {
                warn!("Import completed with {} errors:", result.errors.len());
                for err in &result.errors {
                    warn!("  - {}", err);
                }
            }
            
            info!("✅ Import complete. Continuing to run...");
        }
        
        let graphs = {
            let registry_guard = app_state.graph_registry.lock().unwrap();
            registry_guard.get_all_graphs()
        };
        
        if graphs.is_empty() {
            info!("📊 No graphs found");
        } else {
            info!("📊 Found {} registered graph(s)", graphs.len());
        }
        
        let active_graph = {
            let registry_guard = app_state.graph_registry.lock().unwrap();
            if let Some(active_id) = registry_guard.get_active_graph_id() {
                registry_guard.get_graph(active_id).cloned()
            } else {
                None
            }
        };
        
        if let Some(active_graph) = active_graph {
            info!("🎯 Active graph: {} ({})", active_graph.name, active_graph.id);
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
            tokio::signal::ctrl_c().await?;
            info!("🛑 Received shutdown signal");
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