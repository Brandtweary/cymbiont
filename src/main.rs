//! # Cymbiont - Terminal-First Knowledge Graph Engine
//!
//! Cymbiont is a terminal-first knowledge graph engine designed for AI agents.
//! It provides a clean command-line interface for managing and querying
//! knowledge graphs with support for import, export, and graph operations.
//!
//! ## Usage
//!
//! ```bash
//! # Basic usage (shows graph info then runs indefinitely)
//! cymbiont
//!
//! # Override data directory
//! cymbiont --data-dir /path/to/data
//!
//! # Import Logseq graph (then continues running)
//! cymbiont --import-logseq /path/to/logseq/graph
//!
//! # Run for specific duration (for testing)
//! cymbiont --duration 10
//!
//! # Run as HTTP/WebSocket server
//! cymbiont --server
//!
//! # Run server with specific duration
//! cymbiont --server --duration 60
//! ```
//!
//! ## Program Architecture
//!
//! The program follows a 5-phase startup sequence:
//! 1. **Initialization**: Create AppState with CommandProcessor for CQRS
//! 2. **Common Startup**: CommandProcessor initializes CQRS system and loads agent
//! 3. **Command Handling**: Process CLI-specific commands via CommandQueue
//! 4. **Runtime Loop**: Enter mode-specific event loop (server or CLI)
//! 5. **Cleanup**: Graceful shutdown with command completion
//!
//! Key functions organize the flow:
//! - `AppState::new_with_config()`: Initialize CQRS and start CommandProcessor
//! - `run_startup_sequence()`: Common startup tasks
//! - `cli::handle_cli_commands()`: Execute CLI commands through CommandQueue
//! - `run_server_loop()` / `run_cli_loop()`: Mode-specific runtime handling
//!
//! ## Lifecycle Behavior
//!
//! The CLI always runs continuously after performing any requested operations:
//! - With --duration flag or config: Runs for specified seconds then exits
//! - Without duration: Runs indefinitely until Ctrl+C
//! - With --import-logseq: Performs import, then continues running
//!
//! This design allows the CLI to serve as a persistent knowledge graph engine
//! that can handle future interactive features while maintaining simplicity.
//!
//! ## Graceful Shutdown
//!
//! Both CLI and server modes handle SIGINT (Ctrl+C) uniformly:
//! - First Ctrl+C: Initiates graceful shutdown, waits for active commands to complete (up to 30s)
//! - Second Ctrl+C: Forces immediate termination with command log flush
//!
//! After graceful cleanup, the process uses std::process::exit(0) due to sled database background threads.

use crate::error::*;
use clap::Parser;
use tracing::{info, error, warn, trace};
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::signal;
use tokio::time::sleep;

// Internal modules
mod agent;
mod app_state;
mod cli;
mod config;
mod cqrs;
mod error;
mod graph;
mod import;
mod server;
mod utils;

use app_state::AppState;
use cli::{Args, handle_cli_commands};
use autodebugger::{init_logging, VerbosityConfig as AutodebuggerVerbosityConfig};

fn main() -> Result<()> {
    // Create Tokio runtime explicitly for proper shutdown control
    let runtime = Runtime::new()
        .map_err(|e| CymbiontError::Other(format!("Failed to create runtime: {}", e)))?;
    
    // Run async main logic
    let result = runtime.block_on(async_main());
    
    // Force runtime shutdown with timeout
    runtime.shutdown_timeout(Duration::from_secs(2));
    
    result
}

async fn async_main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Load configuration once to get logging settings
    let config = config::load_config(args.config.clone());
    
    // Initialize logging with verbosity config
    let verbosity_config = AutodebuggerVerbosityConfig {
        info_threshold: config.verbosity.info_threshold,
        debug_threshold: config.verbosity.debug_threshold,
        trace_threshold: config.verbosity.trace_threshold,
    };
    
    let verbosity_layer = init_logging(None, Some(verbosity_config));
    
    // Track start time for total runtime measurement
    let start_time = Instant::now();
    
    // Phase 1: Create AppState based on mode (using pre-loaded config)
    let app_state = AppState::new_with_config(config, args.data_dir.clone(), args.server).await?;
    
    // Phase 2: Common startup sequence (shared between modes)
    run_startup_sequence(&app_state).await?;
    
    // Phase 3: Handle one-off commands (CLI mode only)
    if !args.server {
        if handle_cli_commands(&app_state, &args).await? {
            // Command requested early exit, run cleanup
            app_state.shutdown().await;
            utils::remove_pid_file();
            info!("🧹 CLI shutdown complete");
            
            let total_runtime = start_time.elapsed();
            info!("💫 Total runtime: {:.2}s", total_runtime.as_secs_f64());
            
            if let Some(report) = verbosity_layer.check_and_report() {
                warn!("{}", report);
            }
            
            trace!("Forcing process exit (sled workaround)");
            process::exit(0);
        }
    }
    
    // Phase 4: Enter runtime loop
    if args.server {
        run_server_loop(&app_state, &args).await?;
        info!("🧹 Server shutdown complete");
    } else {
        run_cli_loop(&app_state, &args).await?;
        info!("🧹 CLI shutdown complete");
    }
    
    // Phase 5: Final cleanup
    let total_runtime = start_time.elapsed();
    info!("💫 Total runtime: {:.2}s", total_runtime.as_secs_f64());
    
    // Check log verbosity and report if excessive
    if let Some(report) = verbosity_layer.check_and_report() {
        warn!("{}", report);
    }
    
    // Force exit because sled/tokio threads won't terminate
    trace!("Forcing process exit (sled workaround)");
    process::exit(0)
}

/// Common startup sequence for both server and CLI modes
async fn run_startup_sequence(app_state: &Arc<AppState>) -> Result<()> {
    info!("🧠 Cymbiont initialized");
    info!("📁 Data directory: {}", app_state.data_dir.display());
    
    // CommandProcessor::start() was called during AppState initialization and handles:
    // 1. Replaying all commands from WAL for recovery
    // 2. Restoring runtime state (open graphs)
    // 3. Loading agent state
    
    // Future startup checks can go here:
    // - Check disk space
    // - Validate configuration
    // - Run integrity checks
    // - etc.
    
    Ok(())
}


/// Run the server event loop
async fn run_server_loop(
    app_state: &Arc<AppState>,
    args: &Args,
) -> Result<()> {
    // Start server and get handle
    let (server_handle, server_info_file) = server::start_server(app_state.clone()).await?;
    
    // Handle duration and shutdown
    if let Some(duration) = args.duration.or(app_state.config.development.default_duration) {
        tokio::select! {
            result = server_handle => {
                if let Err(e) = result {
                    error!("Server task error: {}", e);
                }
            }
            _ = sleep(Duration::from_secs(duration)) => {
                info!("⏱️ Duration limit reached");
            }
            _ = signal::ctrl_c() => {
                info!("🛑 Received shutdown signal");
                if handle_graceful_shutdown(app_state).await {
                    server::cleanup_server_info(&server_info_file);
                    process::exit(1);
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
            _ = signal::ctrl_c() => {
                info!("🛑 Received shutdown signal");
                if handle_graceful_shutdown(app_state).await {
                    server::cleanup_server_info(&server_info_file);
                    process::exit(1);
                }
            }
        }
    }
    
    // Cleanup for server mode
    server::cleanup_server_info(&server_info_file);
    app_state.shutdown().await;
    
    Ok(())
}

/// Run the CLI event loop
async fn run_cli_loop(
    app_state: &Arc<AppState>,
    args: &Args,
) -> Result<()> {
    // Handle duration for CLI mode
    if let Some(duration) = args.duration.or(app_state.config.development.default_duration) {
        sleep(Duration::from_secs(duration)).await;
        info!("⏱️ Duration limit reached");
    } else {
        // Run indefinitely (for future interactive features)
        utils::write_pid_file()
            .map_err(|e| CymbiontError::Other(e.to_string()))?;
        
        info!("Running indefinitely. Press Ctrl+C to exit.");
        
        // First Ctrl+C - initiate graceful shutdown
        signal::ctrl_c().await?;
        info!("🛑 Received shutdown signal");
        
        if handle_graceful_shutdown(&app_state).await {
            // Force quit requested
            utils::remove_pid_file();
            process::exit(1);
        }
    }
    
    // Cleanup for CLI mode
    app_state.shutdown().await;
    utils::remove_pid_file();
    
    Ok(())
}

/// Handle graceful shutdown with transaction completion
/// Returns true if should exit immediately (force quit), false to continue with normal cleanup
async fn handle_graceful_shutdown(app_state: &Arc<AppState>) -> bool {
    // Brief grace period for spawned tasks to reach transaction boundary
    sleep(Duration::from_millis(100)).await;
    
    let active_count = app_state.initiate_graceful_shutdown().await;
    if active_count > 0 {
        info!("⏳ Waiting for {} transactions to complete... Press Ctrl+C again to force quit", active_count);
        
        tokio::select! {
            completed = app_state.wait_for_transactions(Duration::from_secs(30)) => {
                if completed {
                    info!("✅ All transactions completed");
                } else {
                    warn!("⚠️ Timeout waiting for transactions");
                }
                false // Continue with normal cleanup
            }
            _ = signal::ctrl_c() => {
                error!("⚡ Force quit requested");
                app_state.force_flush_transactions().await;
                true // Force immediate exit
            }
        }
    } else {
        false // No active transactions, continue with normal cleanup
    }
}
