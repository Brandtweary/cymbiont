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
use tracing::{info, error, warn, trace};

// Internal modules
mod agent;
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
use graph_operations::GraphOps;

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
    
    // Agent admin commands
    /// Create a new agent with the given name
    #[arg(long, value_name = "NAME")]
    create_agent: Option<String>,
    
    /// Optional description for the new agent (used with --create-agent)
    #[arg(long, value_name = "DESCRIPTION", requires = "create_agent")]
    agent_description: Option<String>,
    
    /// Delete an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    delete_agent: Option<String>,
    
    /// Activate an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    activate_agent: Option<String>,
    
    /// Deactivate an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    deactivate_agent: Option<String>,
    
    /// Show information about an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    agent_info: Option<String>,
    
    /// Authorize an agent for a graph (specify agent and graph by name or ID)
    #[arg(long, value_name = "AGENT_NAME_OR_ID")]
    authorize_agent: Option<String>,
    
    /// Graph to authorize the agent for (used with --authorize-agent)
    #[arg(long, value_name = "GRAPH_NAME_OR_ID", requires = "authorize_agent")]
    for_graph: Option<String>,
    
    /// Deauthorize an agent from a graph (specify agent and graph by name or ID)
    #[arg(long, value_name = "AGENT_NAME_OR_ID")]
    deauthorize_agent: Option<String>,
    
    /// Graph to deauthorize the agent from (used with --deauthorize-agent)
    #[arg(long, value_name = "GRAPH_NAME_OR_ID", requires = "deauthorize_agent")]
    from_graph: Option<String>,
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
    
    // Initialize logging with verbosity checking
    let verbosity_layer = init_logging(None);
    
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
            use crate::graph_operations::GraphOps;
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
        
        // Handle agent admin commands
        
        // Create agent
        if let Some(agent_name) = args.create_agent {
            let agent_info = {
                let mut registry = app_state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                
                registry.register_agent(
                    None,  // Let it generate a new UUID
                    Some(agent_name.clone()),
                    args.agent_description.clone(),
                ).map_err(|e| format!("Failed to create agent: {:?}", e))?
            };
            
            // Create the actual Agent instance
            {
                use crate::agent::{agent::Agent, llm::LLMConfig};
                
                // Ensure agent directory exists
                std::fs::create_dir_all(&agent_info.data_path)
                    .map_err(|e| format!("Failed to create agent directory: {}", e))?;
                
                // Create agent with default MockLLM config
                let mut agent = Agent::new(
                    agent_info.id,
                    agent_name.clone(),
                    LLMConfig::default(),  // MockLLM by default
                    agent_info.data_path.clone(),
                    args.agent_description.or(Some("An intelligent assistant".to_string())),
                );
                
                // Save the agent to disk
                agent.save()
                    .map_err(|e| format!("Failed to save agent: {:?}", e))?;
            }
            
            // Save the registry after creating agent
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            
            info!("✅ Created agent '{}' with ID: {}", agent_info.name, agent_info.id);
            return Ok(());  // Exit after creating agent
        }
        
        // Delete agent
        if let Some(agent_identifier) = args.delete_agent {
            use uuid::Uuid;
            
            let resolved_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    false  // No smart default for delete
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Don't allow deleting the prime agent
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                if Some(resolved_id) == registry.get_prime_agent_id() {
                    return Err("Cannot delete the prime agent".into());
                }
            }
            
            // Remove agent from memory if loaded
            app_state.deactivate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to deactivate agent: {:?}", e))?;
            
            // Remove from registry and archive data
            {
                let mut registry = app_state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                registry.remove_agent(&resolved_id)
                    .map_err(|e| format!("Failed to remove agent: {:?}", e))?;
            }
            
            // Save registry after deletion
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            
            info!("✅ Deleted agent: {}", resolved_id);
            return Ok(());  // Exit after deleting agent
        }
        
        // Activate agent
        if let Some(agent_identifier) = args.activate_agent {
            use uuid::Uuid;
            
            let resolved_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    false  // No smart default for activate
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            app_state.activate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to activate agent: {:?}", e))?;
            
            info!("✅ Activated agent: {}", resolved_id);
            return Ok(());  // Exit after activating agent
        }
        
        // Deactivate agent
        if let Some(agent_identifier) = args.deactivate_agent {
            use uuid::Uuid;
            
            let resolved_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    false  // No smart default for deactivate
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Don't allow deactivating the prime agent if it's the only active agent
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                if Some(resolved_id) == registry.get_prime_agent_id() {
                    let active_agents = registry.get_active_agents();
                    if active_agents.len() == 1 {
                        return Err("Cannot deactivate the prime agent when it's the only active agent".into());
                    }
                }
            }
            
            app_state.deactivate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to deactivate agent: {:?}", e))?;
            
            info!("✅ Deactivated agent: {}", resolved_id);
            return Ok(());  // Exit after deactivating agent
        }
        
        // Authorize agent for graph
        if let Some(agent_identifier) = args.authorize_agent {
            use uuid::Uuid;
            
            // Resolve agent (no smart default for authorize)
            let resolved_agent_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    false  // No smart default for authorize
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Resolve graph (requires --for-graph)
            let graph_identifier = args.for_graph
                .ok_or_else(|| "Must specify --for-graph with --authorize-agent")?;
            
            let resolved_graph_id = {
                let registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&graph_identifier).ok();
                
                registry.resolve_graph_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&graph_identifier) } else { None },
                    false  // No smart default for graph
                ).map_err(|e| format!("Failed to resolve graph: {:?}", e))?
            };
            
            // Authorize agent for graph
            {
                let mut agent_registry = app_state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                let mut graph_registry = app_state.graph_registry.write()
                    .map_err(|e| format!("Failed to write graph registry: {}", e))?;
                
                agent_registry.authorize_agent_for_graph(
                    &resolved_agent_id,
                    &resolved_graph_id,
                    &mut graph_registry,
                ).map_err(|e| format!("Failed to authorize agent: {:?}", e))?;
            }
            
            // Save both registries
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            {
                let registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save graph registry: {:?}", e))?;
            }
            
            info!("✅ Authorized agent {} for graph {}", resolved_agent_id, resolved_graph_id);
            return Ok(());  // Exit after authorization
        }
        
        // Deauthorize agent from graph
        if let Some(agent_identifier) = args.deauthorize_agent {
            use uuid::Uuid;
            
            // Resolve agent (no smart default for deauthorize)
            let resolved_agent_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    false  // No smart default for deauthorize
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Resolve graph (requires --from-graph)
            let graph_identifier = args.from_graph
                .ok_or_else(|| "Must specify --from-graph with --deauthorize-agent")?;
            
            let resolved_graph_id = {
                let registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&graph_identifier).ok();
                
                registry.resolve_graph_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&graph_identifier) } else { None },
                    false  // No smart default for graph
                ).map_err(|e| format!("Failed to resolve graph: {:?}", e))?
            };
            
            // Deauthorize agent from graph
            {
                let mut agent_registry = app_state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                let mut graph_registry = app_state.graph_registry.write()
                    .map_err(|e| format!("Failed to write graph registry: {}", e))?;
                
                agent_registry.deauthorize_agent_from_graph(
                    &resolved_agent_id,
                    &resolved_graph_id,
                    &mut graph_registry,
                ).map_err(|e| format!("Failed to deauthorize agent: {:?}", e))?;
            }
            
            // Save both registries
            {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            {
                let registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save graph registry: {:?}", e))?;
            }
            
            info!("✅ Deauthorized agent {} from graph {}", resolved_agent_id, resolved_graph_id);
            return Ok(());  // Exit after deauthorization
        }
        
        // Show agent info
        if let Some(agent_identifier) = args.agent_info {
            use uuid::Uuid;
            
            // Resolve agent (allows smart default to prime if not specified)
            let resolved_id = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Try to parse as UUID first
                let uuid_opt = Uuid::parse_str(&agent_identifier).ok();
                
                registry.resolve_agent_target(
                    uuid_opt.as_ref(),
                    if uuid_opt.is_none() { Some(&agent_identifier) } else { None },
                    true  // Allow smart default (prime agent) for info command
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Get agent info from registry
            let (agent_info, is_active) = {
                let registry = app_state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let info = registry.get_agent(&resolved_id)
                    .ok_or_else(|| format!("Agent {} not found", resolved_id))?
                    .clone();
                let active = registry.is_agent_active(&resolved_id);
                (info, active)
            };
            
            // Display agent information
            info!("🤖 Agent Information");
            info!("  ID: {}", agent_info.id);
            info!("  Name: {}", agent_info.name);
            if let Some(desc) = &agent_info.description {
                info!("  Description: {}", desc);
            }
            info!("  Is Prime: {}", agent_info.is_prime);
            info!("  Is Active: {}", is_active);
            info!("  Created: {}", agent_info.created);
            info!("  Last Active: {}", agent_info.last_active);
            
            // Show authorized graphs
            if !agent_info.authorized_graphs.is_empty() {
                info!("  Authorized Graphs:");
                let graph_registry = app_state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
                for graph_id in &agent_info.authorized_graphs {
                    if let Some(graph) = graph_registry.get_graph(graph_id) {
                        info!("    - {} ({})", graph.name, graph_id);
                    } else {
                        info!("    - {} (not found)", graph_id);
                    }
                }
            } else {
                info!("  Authorized Graphs: None");
            }
            
            // Show conversation stats if agent is loaded
            if is_active {
                let agents = app_state.agents.read().await;
                if let Some(agent) = agents.get(&resolved_id) {
                    info!("  Conversation Messages: {}", agent.conversation_history.len());
                }
            }
            
            return Ok(());  // Exit after showing info
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
    
    // Check log verbosity and report if excessive
    if let Some(report) = verbosity_layer.check_and_report() {
        warn!("{}", report);
    }
    
    // Force exit because sled/tokio threads won't terminate
    // This is the recommended workaround for sled issue #1234
    trace!("Forcing process exit (sled workaround)");
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