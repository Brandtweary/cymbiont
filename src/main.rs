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
//! # Run as HTTP/WebSocket server (not MCP)
//! cymbiont --server
//!
//! # Run HTTP server with specific duration
//! cymbiont --server --duration 60
//! ```
//!
//! ## Program Architecture
//!
//! The program follows a 5-phase startup sequence:
//! 1. **Initialization**: Create `AppState` with `CommandProcessor` for CQRS
//! 2. **Common Startup**: `CommandProcessor` initializes CQRS system and loads agent
//! 3. **Command Handling**: Process CLI-specific commands via `CommandQueue`
//! 4. **Runtime Loop**: Enter mode-specific event loop (server or CLI)
//! 5. **Cleanup**: Graceful shutdown with command completion
//!
//! Key functions organize the flow:
//! - `AppState::new_with_config()`: Initialize CQRS and start `CommandProcessor`
//! - `run_startup_sequence()`: Common startup tasks
//! - `cli::handle_cli_commands()`: Execute CLI commands through `CommandQueue`
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
//! After graceful cleanup, the process uses `std::process::exit(0)` due to sled database background threads.

use crate::error::{CymbiontError, Result};
use clap::Parser;
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::signal;
use tokio::time::sleep;
use tracing::{debug, error, info, trace, warn};

// Internal modules
mod agent;
mod app_state;
mod cli;
mod config;
mod cqrs;
mod error;
mod graph;
mod import;
mod http_server;
mod utils;

use app_state::AppState;
use autodebugger::{init_logging, init_logging_with_file, RotatingFileConfig, VerbosityConfig as AutodebuggerVerbosityConfig};
use cli::{handle_cli_commands, Args};

fn main() -> Result<()> {
    // Create Tokio runtime explicitly for proper shutdown control
    let runtime = Runtime::new()
        .map_err(|e| CymbiontError::Other(format!("Failed to create runtime: {e}")))?;

    // Run async main logic
    let result = runtime.block_on(async_main());

    // Force runtime shutdown with timeout
    runtime.shutdown_timeout(Duration::from_secs(2));

    result
}

#[allow(clippy::cognitive_complexity)]
async fn async_main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Load configuration once to get logging settings
    let config = config::load_config(args.config.clone());

    // Initialize logging with verbosity config and output destination
    let verbosity_config = AutodebuggerVerbosityConfig {
        info_threshold: config.verbosity.info_threshold,
        debug_threshold: config.verbosity.debug_threshold,
        trace_threshold: config.verbosity.trace_threshold,
    };

    let verbosity_layer = if args.mcp {
        // MCP mode: dual logging to stderr + file with maximum verbosity
        std::env::set_var("RUST_LOG", "trace");  // Force maximum verbosity for MCP debugging
        
        let file_config = RotatingFileConfig {
            log_directory: match args.data_dir.as_deref() {
                Some(dir) => format!("{}/logs", dir),
                None => "logs".to_string(),
            },
            filename: "mcp_server.log".to_string(),
            max_files: 10,
            max_size_mb: 5,
            console_output: true,
            truncate_on_limit: true,
        };
        
        init_logging_with_file(
            None,  // Let RUST_LOG=trace take effect via env var
            Some(verbosity_config),
            Some(&config.tracing.output),
            file_config,
        )
    } else {
        // Standard mode: console only
        // TODO: Add rotating file logging for all modes - server, agent, and CLI (currently only MCP has it)
        init_logging(None, Some(verbosity_config), Some(&config.tracing.output))
    };

    // Track start time for total runtime measurement
    let start_time = Instant::now();

    // Phase 1: Create AppState based on mode (using pre-loaded config)
    let app_state = AppState::new_with_config(config, args.data_dir.clone(), args.server).await?;

    // Phase 2: Common startup sequence (shared between modes)
    run_startup_sequence(&app_state);

    // Phase 3: Handle one-off commands (CLI mode only)
    if !args.server && handle_cli_commands(&app_state, &args).await? {
        // Command requested early exit, run cleanup
        app_state.shutdown().await;
        utils::remove_pid_file();
        info!("🧹 CLI shutdown complete");

        let total_runtime = start_time.elapsed();
        info!("💫 Total runtime: {:.2}s", total_runtime.as_secs_f64());

        if let Some(report) = verbosity_layer.check_and_report() {
            warn!("{}", report);
        }

        trace!("Forcing clean process exit");
        process::exit(0);
    }

    // Phase 4: Enter runtime loop
    if args.server {
        run_server_loop(&app_state, &args).await?;
        info!("🧹 HTTP server shutdown complete");
    } else if args.mcp {
        run_mcp_server(&app_state, &args).await?;
        info!("🧹 MCP server shutdown complete");
    } else if args.agent {
        // Agent mode - spawns Claude subprocess with MCP
        // Handler in cli.rs does all the work
        if handle_cli_commands(&app_state, &args).await? {
            info!("🧹 Agent shutdown complete");
        }
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
    process::exit(0)
}

/// Common startup sequence for both server and CLI modes
fn run_startup_sequence(app_state: &Arc<AppState>) {
    info!("🧠 Cymbiont initialized");
    info!("📁 Data directory: {}", app_state.data_dir.display());

    // CommandProcessor::start() was called during AppState initialization and handles:
    // 1. Loading persisted state from JSON files
    // 2. Restoring runtime state (open graphs)
    // 3. Loading agent state

    // Future startup checks can go here:
    // - Check disk space
    // - Validate configuration
    // - Run integrity checks
    // - etc.
}

/// Unified runtime loop with shutdown handling for all modes
async fn run_with_shutdown<F, C, CF>(
    app_state: &Arc<AppState>,
    args: &Args,
    main_task: F,
    cleanup: C,
) -> Result<()>
where
    F: std::future::Future<Output = Result<()>>,
    C: Fn() -> CF,
    CF: std::future::Future<Output = ()>,
{
    // Handle duration and shutdown uniformly (0 = infinite)
    let duration = args
        .duration
        .or(app_state.config.development.default_duration)
        .and_then(|d| if d == 0 { None } else { Some(d) });
    
    // Create signal handlers outside tokio::select! to avoid lifetime issues
    let mut sigterm1 = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    let mut sigterm2 = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    
    if let Some(duration) = duration {
        tokio::select! {
            result = main_task => {
                if let Err(e) = result {
                    error!("Main task error: {}", e);
                }
            }
            () = sleep(Duration::from_secs(duration)) => {
                info!("⏱️ Duration limit reached");
            }
            _ = signal::ctrl_c() => {
                info!("🛑 Received SIGINT (Ctrl+C)");
                if handle_graceful_shutdown(app_state).await {
                    cleanup().await;
                    process::exit(1);
                }
            }
            _ = sigterm1.recv() => {
                info!("🛑 Received SIGTERM");
                if handle_graceful_shutdown(app_state).await {
                    cleanup().await;
                    process::exit(1);
                }
            }
        }
    } else {
        // Run indefinitely
        tokio::select! {
            result = main_task => {
                if let Err(e) = result {
                    error!("Main task error: {}", e);
                }
            }
            _ = signal::ctrl_c() => {
                info!("🛑 Received SIGINT (Ctrl+C)");
                if handle_graceful_shutdown(app_state).await {
                    cleanup().await;
                    process::exit(1);
                }
            }
            _ = sigterm2.recv() => {
                info!("🛑 Received SIGTERM");
                if handle_graceful_shutdown(app_state).await {
                    cleanup().await;
                    process::exit(1);
                }
            }
        }
    }

    // Standard cleanup sequence
    cleanup().await;
    app_state.shutdown().await;

    Ok(())
}

/// Run the server event loop
async fn run_server_loop(app_state: &Arc<AppState>, args: &Args) -> Result<()> {
    // Start server and get handle
    let (server_handle, server_info_file) = http_server::start_server(app_state.clone()).await?;

    // Cleanup function specific to server mode
    let cleanup = || async {
        http_server::cleanup_server_info(&server_info_file);
    };

    // Convert JoinHandle to a Future that returns Result<()>
    let server_task = async {
        match server_handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                error!("Server error: {}", e);
                Err(CymbiontError::Other(format!("Server error: {}", e)))
            }
            Err(e) => {
                error!("Server task panicked: {}", e);
                Err(CymbiontError::Other(format!("Server task panicked: {}", e)))
            }
        }
    };

    // Run with unified shutdown handling
    run_with_shutdown(
        app_state,
        args,
        server_task,
        cleanup,
    ).await
}

/// Run the CLI event loop
async fn run_cli_loop(app_state: &Arc<AppState>, args: &Args) -> Result<()> {
    // Write PID file for CLI mode
    if args.duration.is_none() && app_state.config.development.default_duration.is_none() {
        utils::write_pid_file().map_err(|e| CymbiontError::Other(e.to_string()))?;
        info!("Running indefinitely. Press Ctrl+C to exit.");
    }

    // Cleanup function specific to CLI mode
    let cleanup = || async {
        utils::remove_pid_file();
    };

    // Create a future that never completes (CLI has no main task)
    let never = std::future::pending::<Result<()>>();

    // Run with unified shutdown handling
    run_with_shutdown(
        app_state,
        args,
        never,
        cleanup,
    ).await
}

/// Run the MCP server
async fn run_mcp_server(app_state: &Arc<AppState>, args: &Args) -> Result<()> {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    debug!("🔧 DEBUG: MCP server startup initiated with debug logging enabled");
    info!("🤖 Starting MCP server on stdio at {}", timestamp);
    
    // No cleanup needed for MCP mode
    let cleanup = || async {};

    // Run the MCP server as the main task
    let mcp_task = async {
        debug!("🔧 DEBUG: Starting MCP server task");
        match agent::mcp::server::run_mcp_server(app_state.clone()).await {
            Ok(_) => {
                info!("MCP client disconnected");
                debug!("MCP server task completed successfully");
                Ok(())
            }
            Err(e) => {
                error!("MCP server error: {}", e);
                debug!("MCP server task failed with error: {}", e);
                Err(e)
            }
        }
    };

    // Run with unified shutdown handling
    run_with_shutdown(
        app_state,
        args,
        mcp_task,
        cleanup,
    ).await
}

/// Handle graceful shutdown with transaction completion
/// Returns true if should exit immediately (force quit), false to continue with normal cleanup
async fn handle_graceful_shutdown(app_state: &Arc<AppState>) -> bool {
    // Brief grace period for spawned tasks to reach transaction boundary
    sleep(Duration::from_millis(100)).await;

    let active_count = app_state.initiate_graceful_shutdown().await;
    if active_count > 0 {
        info!(
            "⏳ Waiting for {} transactions to complete... Press Ctrl+C again to force quit",
            active_count
        );

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
