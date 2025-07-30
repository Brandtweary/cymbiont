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
 * # Basic usage (shows graph info)
 * cymbiont
 * 
 * # Override data directory
 * cymbiont --data-dir /path/to/data
 * 
 * # For HTTP/WebSocket server functionality, use:
 * cymbiont-server
 * ```
 * 
 * ## Architecture
 * 
 * This CLI tool focuses on the core knowledge graph functionality without
 * network dependencies. For HTTP API and WebSocket features, use the
 * `cymbiont-server` binary instead.
 */

use std::error::Error;
use clap::Parser;
use tracing::{info, error};

// Internal modules
mod app_state;
mod config;
mod graph_manager;
mod graph_registry;
mod logging;
mod pkm_data;
mod saga;
mod server;
mod transaction;
mod transaction_log;
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
    
    // Server-specific args (only used when --server is provided)
    /// Run server for a specific duration in seconds (for testing)
    #[arg(long)]
    duration: Option<u64>,
    
    /// Test WebSocket by sending a command after connection
    #[arg(long)]
    test_websocket: Option<String>,
    
    /// Shutdown a running Cymbiont instance gracefully
    #[arg(long)]
    shutdown: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    init_logging();
    
    // Handle shutdown command
    if args.shutdown {
        // First check for simple PID file (CLI mode)
        if let Ok(pid) = utils::read_pid_file() {
            info!("Shutting down Cymbiont (PID: {})...", pid);
            // We need to terminate by PID directly
            #[cfg(target_family = "unix")]
            {
                use std::process::Command;
                if Command::new("kill").arg(pid.to_string()).status().is_ok() {
                    info!("Shutdown signal sent successfully");
                    utils::remove_pid_file();
                    return Ok(());
                }
            }
            #[cfg(target_family = "windows")]
            {
                use std::process::Command;
                if Command::new("taskkill").args(&["/PID", &pid.to_string(), "/F"]).status().is_ok() {
                    info!("Shutdown successfully");
                    utils::remove_pid_file();
                    return Ok(());
                }
            }
        }
        
        // Fall back to server info file (server mode)
        if let Ok(info_str) = std::fs::read_to_string(utils::SERVER_INFO_FILE) {
            if let Ok(info) = serde_json::from_str::<utils::ServerInfo>(&info_str) {
                info!("Shutting down Cymbiont server (PID: {})...", info.pid);
                if utils::terminate_previous_instance() {
                    info!("Server shutdown successfully");
                    let _ = std::fs::remove_file(utils::SERVER_INFO_FILE);
                    return Ok(());
                } else {
                    error!("Failed to shutdown server");
                    return Err("Shutdown failed".into());
                }
            }
        }
        
        error!("No running Cymbiont instance found");
        return Err("No instance to shutdown".into());
    }
    
    // Branch based on server flag
    if args.server {
        // Run server with all setup/teardown handled internally
        server::run_server_with_duration(args.data_dir, args.duration).await?;
    } else {
        // Run as CLI
        let app_state = AppState::new_cli(args.data_dir.clone()).await?;
        
        info!("🧠 Cymbiont CLI initialized");
        info!("📁 Data directory: {}", app_state.config.data_dir);
        
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
            
            // Set up Ctrl+C handler to clean up PID file
            ctrlc::set_handler(move || {
                utils::remove_pid_file();
                std::process::exit(0);
            }).expect("Error setting Ctrl-C handler");
            
            info!("Running indefinitely. Press Ctrl+C to exit.");
            tokio::signal::ctrl_c().await?;
        }
    }
    
    Ok(())
}