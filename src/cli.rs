//! CLI Command Handling Module
//!
//! This module contains all CLI argument parsing and command execution logic
//! for Cymbiont. It handles agent management, graph operations, and various
//! administrative commands through a clean command-line interface.
//!
//! ## Architecture
//!
//! The CLI module is responsible for:
//! - Parsing command-line arguments using clap
//! - Executing one-off commands (agent/graph management)
//! - Displaying system status when no commands are provided
//! - Determining whether to exit after command completion
//!
//! ## Command Categories
//!
//! ### Graph Operations
//! - Import Logseq graphs from directories
//! - Delete graphs by name or ID
//!
//! ### Agent Management
//! - Create/delete agents
//! - Activate/deactivate agents (memory management)
//! - Display agent information
//!
//! ### Agent Authorization
//! - Authorize agents for specific graphs
//! - Deauthorize agents from graphs
//! - Manage multi-agent access control
//!
//! ## Execution Model
//!
//! Commands that modify state (create/delete/authorize) typically cause the
//! program to exit after completion. Import and status commands allow the
//! program to continue running, supporting the persistent daemon model.

use std::error::Error;
use std::sync::Arc;
use clap::Parser;
use tracing::{info, error};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::graph_operations::GraphOps;
use crate::import;

/// CLI arguments  
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Run as HTTP/WebSocket server
    #[arg(long)]
    pub server: bool,
    
    /// Override data directory path (defaults to config value)
    #[arg(long)]
    pub data_dir: Option<String>,
    
    /// Path to configuration file
    #[arg(long)]
    pub config: Option<String>,
    
    /// Import a Logseq graph from directory
    #[arg(long, value_name = "PATH")]
    pub import_logseq: Option<String>,
    
    /// Delete a graph by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    pub delete_graph: Option<String>,
    
    // Server-specific args (only used when --server is provided)
    /// Run server for a specific duration in seconds (for testing)
    #[arg(long)]
    pub duration: Option<u64>,
    
    // Agent admin commands
    /// Create a new agent with the given name
    #[arg(long, value_name = "NAME")]
    pub create_agent: Option<String>,
    
    /// Optional description for the new agent (used with --create-agent)
    #[arg(long, value_name = "DESCRIPTION", requires = "create_agent")]
    pub agent_description: Option<String>,
    
    /// Delete an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    pub delete_agent: Option<String>,
    
    /// Activate an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    pub activate_agent: Option<String>,
    
    /// Deactivate an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    pub deactivate_agent: Option<String>,
    
    /// Show information about an agent by name or ID
    #[arg(long, value_name = "NAME_OR_ID")]
    pub agent_info: Option<String>,
    
    /// Authorize an agent for a graph (specify agent and graph by name or ID)
    #[arg(long, value_name = "AGENT_NAME_OR_ID")]
    pub authorize_agent: Option<String>,
    
    /// Graph to authorize the agent for (used with --authorize-agent)
    #[arg(long, value_name = "GRAPH_NAME_OR_ID", requires = "authorize_agent")]
    pub for_graph: Option<String>,
    
    /// Deauthorize an agent from a graph (specify agent and graph by name or ID)
    #[arg(long, value_name = "AGENT_NAME_OR_ID")]
    pub deauthorize_agent: Option<String>,
    
    /// Graph to deauthorize the agent from (used with --deauthorize-agent)
    #[arg(long, value_name = "GRAPH_NAME_OR_ID", requires = "deauthorize_agent")]
    pub from_graph: Option<String>,
}

/// Handle all CLI-specific commands
/// Returns true if should exit after command completion
pub async fn handle_cli_commands(app_state: &Arc<AppState>, args: &Args) -> Result<bool, Box<dyn Error + Send + Sync>> {
    // Handle Logseq import if requested (continues running after)
    if let Some(logseq_path) = &args.import_logseq {
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
        // Don't return true - continue running
    }
    
    // Handle graph deletion if requested (continues running after)
    if let Some(graph_identifier) = &args.delete_graph {
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
        // Don't return true - continue running
    }
    
    // Handle agent admin commands (these exit after completion)
    
    // Create agent
    if let Some(agent_name) = &args.create_agent {
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
                args.agent_description.clone().or(Some("An intelligent assistant".to_string())),
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
        return Ok(true);  // Exit after creating agent
    }
    
    // Delete agent
    if let Some(agent_identifier) = &args.delete_agent {
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
        return Ok(true);  // Exit after deleting agent
    }
    
    // Activate agent
    if let Some(agent_identifier) = &args.activate_agent {
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
        return Ok(true);  // Exit after activating agent
    }
    
    // Deactivate agent
    if let Some(agent_identifier) = &args.deactivate_agent {
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
        return Ok(true);  // Exit after deactivating agent
    }
    
    // Authorize agent for graph
    if let Some(agent_identifier) = &args.authorize_agent {
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
        let graph_identifier = args.for_graph.as_deref()
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
        return Ok(true);  // Exit after authorization
    }
    
    // Deauthorize agent from graph
    if let Some(agent_identifier) = &args.deauthorize_agent {
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
        let graph_identifier = args.from_graph.as_deref()
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
        return Ok(true);  // Exit after deauthorization
    }
    
    // Show agent info
    if let Some(agent_identifier) = &args.agent_info {
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
        
        return Ok(true);  // Exit after showing info
    }
    
    // If no commands, show status
    show_cli_status(app_state).await?;
    
    Ok(false)
}

/// Show CLI status information
pub async fn show_cli_status(app_state: &Arc<AppState>) -> Result<(), Box<dyn Error + Send + Sync>> {
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
    } else {
        info!("📂 No open graphs");
    }
    
    Ok(())
}