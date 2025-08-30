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
//! 1. **Initialization**: Create AppState based on mode (server vs CLI)
//! 2. **Common Startup**: Run shared initialization (recovery, orphan check)
//! 3. **Command Handling**: Process CLI-specific commands (CLI mode only)
//! 4. **Runtime Loop**: Enter mode-specific event loop (server or CLI)
//! 5. **Cleanup**: Save state and terminate gracefully
//!
//! Key functions organize the flow:
//! - `run_startup_sequence()`: Common initialization for both modes
//! - `check_orphaned_graphs()`: Warns about graphs with no authorized agents
//! - `cli::handle_cli_commands()`: Processes all CLI commands (in cli module)
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
//! - First Ctrl+C: Initiates graceful shutdown, waits for active transactions to complete (up to 30s)
//! - Second Ctrl+C: Forces immediate termination with transaction log flush
//!
//! After graceful cleanup, the process uses std::process::exit(0) due to sled database background threads.

use crate::error::*;
use crate::lock::AsyncRwLockExt;
use clap::Parser;
use tracing::{info, error, warn, trace};

// Internal modules
mod agent;
mod app_state;
mod cli;
mod config;
mod error;
mod graph;
mod import;
mod lock;
mod server;
mod storage;
mod utils;

use app_state::AppState;
use cli::{Args, handle_cli_commands};
use autodebugger::{init_logging, VerbosityConfig as AutodebuggerVerbosityConfig};
use agent::agent_registry::AgentRegistry;
use graph::graph_registry::GraphRegistry;

fn main() -> Result<()> {
    // Create Tokio runtime explicitly for proper shutdown control
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| CymbiontError::Other(format!("Failed to create runtime: {}", e)))?;
    
    // Run async main logic
    let result = runtime.block_on(async_main());
    
    // Force runtime shutdown with timeout
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    
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
    let start_time = std::time::Instant::now();
    
    // Phase 1: Create AppState based on mode (using pre-loaded config)
    let app_state = AppState::new_with_config(config, args.data_dir.clone(), args.server).await?;
    
    // Phase 2: Common startup sequence (shared between modes)
    run_startup_sequence(&app_state).await?;
    
    // Phase 3: Handle one-off commands (CLI mode only)
    if !args.server {
        if handle_cli_commands(&app_state, &args).await? {
            // Command requested early exit, run cleanup
            app_state.cleanup_and_save().await;
            utils::remove_pid_file();
            info!("🧹 CLI shutdown complete");
            
            let total_runtime = start_time.elapsed();
            info!("💫 Total runtime: {:.2}s", total_runtime.as_secs_f64());
            
            if let Some(report) = verbosity_layer.check_and_report() {
                warn!("{}", report);
            }
            
            trace!("Forcing process exit (sled workaround)");
            std::process::exit(0);
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
    std::process::exit(0)
}

/// Common startup sequence for both server and CLI modes
async fn run_startup_sequence(app_state: &std::sync::Arc<AppState>) -> Result<()> {
    info!("🧠 Cymbiont initialized");
    info!("📁 Data directory: {}", app_state.data_dir.display());
    
    // Three-phase startup:
    // Phase 1: Rebuild entire state from committed transactions
    if let Err(e) = storage::recovery::rebuild_from_wal_complete(&app_state).await {
        error!("Failed to rebuild from WAL: {}", e);
    }
    
    // Phase 2: Recover any pending transactions (crash recovery)
    if let Err(e) = storage::recovery::recover_pending_transactions(&app_state).await {
        error!("Failed to recover pending transactions: {}", e);
    }
    
    // Phase 3: Bootstrap and activation
    // Ensure prime agent exists (creates on first run)
    AgentRegistry::ensure_default_agent(
        app_state.agent_registry.clone()
    ).await?;
    
    // Activate all agents that should be active
    let agents_to_activate = {
        let registry = app_state.agent_registry.read_or_panic("get active agents").await;
        registry.get_active_agents()
    };
    
    for agent_id in agents_to_activate {
        // Use the new static method that manages its own locking
        if let Err(e) = AgentRegistry::activate_agent_complete(
            app_state.agent_registry.clone(),
            agent_id,
            false  // don't skip_wal - this is a real persistent activation
        ).await {
            error!("Failed to activate agent {}: {}", agent_id, e);
        }
    }
    
    // Open all graphs that should be open
    let graphs_to_open = {
        let registry = app_state.graph_registry.read_or_panic("get open graphs").await;
        registry.get_open_graphs()
    };
    
    for graph_id in graphs_to_open {
        if let Err(e) = GraphRegistry::open_graph_complete(
            app_state.graph_registry.clone(),
            graph_id,
            false  // don't skip_wal - this is a real persistent open operation
        ).await {
            error!("Failed to open graph {}: {}", graph_id, e);
        }
    }
    
    // Check for orphaned graphs (graphs with no authorized agents)
    check_orphaned_graphs(app_state).await;
    
    // Future startup checks can go here:
    // - Check disk space
    // - Validate configuration
    // - Run integrity checks
    // - etc.
    
    Ok(())
}

/// Check for graphs with no authorized agents and warn
async fn check_orphaned_graphs(app_state: &std::sync::Arc<AppState>) {
    let agent_registry = app_state.agent_registry.read_or_panic("check orphaned graphs - agent registry").await;
    let graph_registry = app_state.graph_registry.read_or_panic("check orphaned graphs - graph registry").await;
    let orphaned_graphs = agent_registry.find_orphaned_graphs(&graph_registry);
    
    if !orphaned_graphs.is_empty() {
        warn!("⚠️  Found {} orphaned graph(s) with no authorized agents:", orphaned_graphs.len());
        for graph_id in &orphaned_graphs {
            if let Some(graph_info) = graph_registry.get_graph(graph_id) {
                warn!("  - {} ({})", graph_info.name, graph_id);
            } else {
                warn!("  - {}", graph_id);
            }
        }
        warn!("  Consider authorizing an agent using: cymbiont --authorize-agent <AGENT> --for-graph <GRAPH>");
    }
}


/// Run the server event loop
async fn run_server_loop(
    app_state: &std::sync::Arc<AppState>,
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
            _ = tokio::time::sleep(std::time::Duration::from_secs(duration)) => {
                info!("⏱️ Duration limit reached");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("🛑 Received shutdown signal");
                if handle_graceful_shutdown(app_state).await {
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
                if handle_graceful_shutdown(app_state).await {
                    server::cleanup_server_info(&server_info_file);
                    std::process::exit(1);
                }
            }
        }
    }
    
    // Cleanup for server mode
    server::cleanup_server_info(&server_info_file);
    app_state.cleanup_and_save().await;
    
    Ok(())
}

/// Run the CLI event loop
async fn run_cli_loop(
    app_state: &std::sync::Arc<AppState>,
    args: &Args,
) -> Result<()> {
    // Handle duration for CLI mode
    if let Some(duration) = args.duration.or(app_state.config.development.default_duration) {
        tokio::time::sleep(std::time::Duration::from_secs(duration)).await;
        info!("⏱️ Duration limit reached");
    } else {
        // Run indefinitely (for future interactive features)
        utils::write_pid_file()
            .map_err(|e| CymbiontError::Other(e.to_string()))?;
        
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
    
    Ok(())
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
