//! Command Router - Enforcing CQRS at Compile Time
//!
//! The router module serves two critical purposes: it routes commands to their
//! handlers, and it enforces the CQRS architecture through the RouterToken pattern.
//! This is where commands are transformed into actual state changes, with compile-time
//! guarantees that all mutations flow through the command processor.
//!
//! ## The RouterToken Pattern
//!
//! RouterToken is a zero-sized type that can ONLY be created within this module.
//! All business logic methods require a RouterToken as their first parameter,
//! making it impossible to bypass CQRS:
//!
//! ```rust
//! // This function can ONLY be called from the router
//! pub fn create_block_with_resolution(
//!     _token: &RouterToken,  // Proof of CQRS routing
//!     manager: &mut GraphManager,
//!     content: String,
//!     // ... other params
//! ) -> Result<NodeIndex> {
//!     // Business logic here
//! }
//! ```
//!
//! ### Why RouterToken?
//!
//! Without RouterToken, developers might accidentally:
//! - Call business logic directly, bypassing the command queue
//! - Forget to log operations to WAL
//! - Create race conditions by modifying state concurrently
//!
//! RouterToken makes these mistakes impossible at compile time. If you don't
//! have a token, you can't call the function. The only way to get a token is
//! to be the router. It's Rust's type system as architecture enforcement.
//!
//! ## Command Routing Flow
//!
//! ```text
//! Command arrives from processor
//!         |
//!         v
//! Router matches command type
//!         |
//!         v
//! Router creates RouterToken
//!         |
//!         v
//! Router calls business logic with token
//!         |
//!         v
//! Business logic executes (validated by token)
//!         |
//!         v
//! Router returns CommandResult
//! ```
//!
//! ## Handler Patterns
//!
//! ### Graph Commands
//! Graph operations that modify content:
//! ```rust
//! GraphCommand::CreateBlock { graph_id, agent_id, content, ... } => {
//!     // 1. Check agent authorization
//!     // 2. Get graph manager
//!     // 3. Call helper with RouterToken
//!     // 4. Return result
//! }
//! ```
//!
//! ### Registry Commands
//! Lifecycle operations for graphs and agents:
//! ```rust
//! GraphRegistryCommand::CreateGraph { name, description, resolved_id } => {
//!     // 1. Use resolved_id (from resolution phase)
//!     // 2. Create graph with RouterToken
//!     // 3. Update registry
//!     // 4. Authorize prime agent
//! }
//! ```
//!
//! ### Agent Commands
//! Modifications to agent state:
//! ```rust
//! AgentCommand::AddMessage { agent_id, message } => {
//!     // 1. Get agent from HashMap
//!     // 2. Deserialize message
//!     // 3. Add to conversation with RouterToken
//!     // 4. Save agent state
//! }
//! ```
//!
//! ## Authorization Checking
//!
//! Most commands require authorization checks:
//! ```rust
//! // Check if agent can modify graph
//! if !agent_registry.is_agent_authorized(&agent_id, &graph_id) {
//!     return Err(GraphError::Unauthorized(...));
//! }
//! ```
//!
//! The router centralizes these checks, ensuring consistent security enforcement.
//!
//! ## Arc<RwLock> Coordination
//!
//! The router handles all the Arc<RwLock> patterns:
//! - Getting references from HashMaps
//! - Acquiring write locks when needed
//! - Dropping locks promptly to avoid contention
//! - Coordinating between multiple resources
//!
//! This keeps the complexity in one place rather than scattered throughout
//! the codebase.
//!
//! ## Error Handling
//!
//! Commands can fail for various reasons:
//! - **Authorization**: Agent lacks permission
//! - **Not Found**: Target entity doesn't exist
//! - **Invalid State**: Operation not valid in current state
//! - **Business Logic**: Domain-specific failures
//!
//! All errors are converted to CommandResult with appropriate messages.
//!
//! ## Design Benefits
//!
//! ### Separation of Concerns
//! - Router: Command dispatch and resource coordination
//! - Business logic: Domain operations (with RouterToken)
//! - Processor: State ownership and WAL management
//!
//! ### Type Safety
//! RouterToken ensures all mutations go through CQRS at compile time,
//! not just by convention or code review.
//!
//! ### Maintainability
//! Adding new commands is straightforward:
//! 1. Define command in commands.rs
//! 2. Add match arm here
//! 3. Implement handler (requiring RouterToken)
//!
//! The compiler guides you through the process.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::*;
use crate::graph::graph_manager::GraphManager;
use crate::graph::graph_registry::GraphRegistry;
use crate::graph::graph_operations;
use crate::agent::agent::Agent;
use crate::agent::agent_registry::AgentRegistry;
use crate::agent::llm::{Message, LLMConfig};
use super::commands::{CommandResult, GraphCommand, AgentCommand, RegistryCommand,
                      GraphRegistryCommand, AgentRegistryCommand};
use super::processor::ProcessorError;

/// Private token that proves a call came through the CQRS router.
/// This type can ONLY be constructed within the router module,
/// preventing any code outside the CQRS system from calling
/// business logic methods directly.
#[derive(Debug)]
pub struct RouterToken(());

impl RouterToken {
    /// Private constructor - only callable within this module
    fn new() -> Self {
        RouterToken(())
    }
    
    /// Internal constructor for CQRS system use (processor recovery)
    pub(super) fn new_internal() -> Self {
        RouterToken(())
    }
}

/// Route a graph command to its handler
pub async fn route_graph_command(
    managers: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    agent_registry: &Arc<RwLock<AgentRegistry>>,
    command: GraphCommand,
) -> Result<CommandResult> {
    match command {
        GraphCommand::CreateBlock { graph_id, agent_id, content, parent_id, page_name, properties } => {
            // Check agent authorization
            check_agent_authorization(agent_registry, agent_id, graph_id).await?;
            
            // Get the graph manager
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError("Graph not found".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Create block using graph_operations helper
            let token = RouterToken::new();
            let block_id = graph_operations::execute_create_block(
                &token,
                &mut *graph_manager,
                content,
                parent_id,
                page_name,
                properties,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ 
                    "block_id": block_id
                })),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphCommand::UpdateBlock { graph_id, agent_id, block_id, content } => {
            check_agent_authorization(agent_registry, agent_id, graph_id).await?;
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError("Graph not found".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Update block using graph_operations helper
            let token = RouterToken::new();
            graph_operations::execute_update_block(
                &token,
                &mut *graph_manager,
                &block_id,
                content,
                graph_id,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ 
                    "block_id": block_id
                })),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphCommand::DeleteBlock { graph_id, agent_id, block_id } => {
            check_agent_authorization(agent_registry, agent_id, graph_id).await?;
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError("Graph not found".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Delete block using graph_operations helper
            let token = RouterToken::new();
            graph_operations::execute_delete_block(
                &token,
                &mut *graph_manager,
                &block_id,
                graph_id,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ "block_id": block_id })),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphCommand::CreatePage { graph_id, agent_id, page_name, properties } => {
            check_agent_authorization(agent_registry, agent_id, graph_id).await?;
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError("Graph not found".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Create page using graph_operations helper
            let token = RouterToken::new();
            graph_operations::execute_create_page(
                &token,
                &mut *graph_manager,
                page_name.clone(),
                properties,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ "page_name": page_name })),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphCommand::DeletePage { graph_id, agent_id, page_name } => {
            check_agent_authorization(agent_registry, agent_id, graph_id).await?;
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError("Graph not found".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Delete page using graph_operations helper
            let token = RouterToken::new();
            graph_operations::execute_delete_page(
                &token,
                &mut *graph_manager,
                &page_name,
                graph_id,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ "page_name": page_name })),
                error: None,
                child_commands: vec![],
            })
        }
    }
}

/// Route an agent command to its handler
pub async fn route_agent_command(
    agents: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
    command: AgentCommand,
) -> Result<CommandResult> {
    match command {
        AgentCommand::AddMessage { agent_id, message } => {
            let agents_guard = agents.read().await;
            let agent_arc = agents_guard.get(&agent_id)
                .ok_or_else(|| ProcessorError("Agent not found".to_string()))?
                .clone();
            let mut agent = agent_arc.write().await;
            
            // Deserialize and add message
            let msg: Message = serde_json::from_value(message)?;
            let token = RouterToken::new();
            agent.add_message(&token, msg).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::ClearHistory { agent_id } => {
            let agents_guard = agents.read().await;
            let agent_arc = agents_guard.get(&agent_id)
                .ok_or_else(|| ProcessorError("Agent not found".to_string()))?
                .clone();
            let mut agent = agent_arc.write().await;
            
            let token = RouterToken::new();
            agent.clear_history(&token).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::SetLLMConfig { agent_id, config } => {
            let agents_guard = agents.read().await;
            let agent_arc = agents_guard.get(&agent_id)
                .ok_or_else(|| ProcessorError("Agent not found".to_string()))?
                .clone();
            let mut agent = agent_arc.write().await;
            
            // Deserialize and set config
            let llm_config: LLMConfig = serde_json::from_value(config)?;
            let token = RouterToken::new();
            agent.set_llm_config(&token, llm_config).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::SetSystemPrompt { agent_id, prompt } => {
            let agents_guard = agents.read().await;
            let agent_arc = agents_guard.get(&agent_id)
                .ok_or_else(|| ProcessorError("Agent not found".to_string()))?
                .clone();
            let mut agent = agent_arc.write().await;
            
            let token = RouterToken::new();
            agent.set_system_prompt(&token, prompt).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::SetDefaultGraph { agent_id, graph_id } => {
            let agents_guard = agents.read().await;
            let agent_arc = agents_guard.get(&agent_id)
                .ok_or_else(|| ProcessorError("Agent not found".to_string()))?
                .clone();
            let mut agent = agent_arc.write().await;
            
            let token = RouterToken::new();
            agent.set_default_graph_id(&token, graph_id).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
    }
}

/// Route a registry command to its handler
pub async fn route_registry_command(
    graph_managers: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    agents: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    agent_registry: &Arc<RwLock<AgentRegistry>>,
    command: RegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        RegistryCommand::Graph(graph_cmd) => {
            route_graph_registry_command(
                graph_managers,
                graph_registry,
                agent_registry,
                graph_cmd,
                data_dir,
            ).await
        }
        RegistryCommand::Agent(agent_cmd) => {
            route_agent_registry_command(
                agents,
                agent_registry,
                graph_registry,
                agent_cmd,
                data_dir,
            ).await
        }
    }
}

/// Route a graph registry command
async fn route_graph_registry_command(
    graph_managers: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    agent_registry: &Arc<RwLock<AgentRegistry>>,
    command: GraphRegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        GraphRegistryCommand::CreateGraph { name, description, resolved_id } => {
            // Use the complete workflow
            let mut graph_reg = graph_registry.write().await;
            let mut agent_reg = agent_registry.write().await;
            let mut managers = graph_managers.write().await;
            
            // resolved_id should always be Some after resolution
            let graph_id = resolved_id.expect("CreateGraph command should be resolved before execution");
            
            let token = RouterToken::new();
            let graph_info = graph_reg.create_graph_complete(
                &token,
                graph_id,
                name,
                description,
                &mut *managers,
                &mut *agent_reg,
                data_dir,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::to_value(graph_info)?),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphRegistryCommand::RegisterGraph { graph_id, name, description } => {
            // Low-level registry operation
            let graph_dir = data_dir.join("graphs").join(graph_id.to_string());
            let mut registry = graph_registry.write().await;
            registry.register_graph(Some(graph_id), name, description, &graph_dir).await?;
            
            // Create the empty graph manager
            let manager = Arc::new(RwLock::new(GraphManager::new(&graph_dir)?));
            let mut managers = graph_managers.write().await;
            managers.insert(graph_id, manager);
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ "graph_id": graph_id })),
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphRegistryCommand::RemoveGraph { graph_id } => {
            // Use registry method that handles both memory and archival
            let mut registry = graph_registry.write().await;
            let mut managers = graph_managers.write().await;
            let mut agent_reg = agent_registry.write().await;
            let token = RouterToken::new();
            registry.remove_graph(&token, &graph_id, &mut *managers, &mut *agent_reg).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphRegistryCommand::OpenGraph { graph_id } => {
            // Use complete workflow
            let mut registry = graph_registry.write().await;
            let mut managers = graph_managers.write().await;
            let token = RouterToken::new();
            registry.open_graph_complete(
                &token,
                graph_id,
                &mut *managers,
                data_dir,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        GraphRegistryCommand::CloseGraph { graph_id } => {
            // Use registry method that handles memory cleanup
            let mut registry = graph_registry.write().await;
            let mut managers = graph_managers.write().await;
            let token = RouterToken::new();
            registry.close_graph(&token, &graph_id, &mut *managers).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
    }
}

/// Route an agent registry command
async fn route_agent_registry_command(
    agents: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
    agent_registry: &Arc<RwLock<AgentRegistry>>,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    command: AgentRegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        AgentRegistryCommand::CreateAgent { name, description, resolved_id } => {
            // Use the complete workflow
            let mut registry = agent_registry.write().await;
            let mut agents_map = agents.write().await;
            
            // resolved_id should always be Some after resolution
            let agent_id = resolved_id.expect("CreateAgent command should be resolved before execution");
            
            let token = RouterToken::new();
            let agent_info = registry.create_agent_complete(
                &token,
                agent_id,
                name,
                description,
                None,  // system_prompt
                &mut *agents_map,
                data_dir,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::to_value(agent_info)?),
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::RegisterAgent { agent_id, name, description } => {
            // Low-level registry operation
            let mut registry = agent_registry.write().await;
            let token = RouterToken::new();
            registry.register_agent(&token, Some(agent_id), name, description).await?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::json!({ "agent_id": agent_id })),
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::DeleteAgent { agent_id } => {
            let mut registry = agent_registry.write().await;
            let mut agents_map = agents.write().await;
            let mut graph_reg = graph_registry.write().await;
            let token = RouterToken::new();
            registry.remove_agent(&token, &agent_id, &mut *agents_map, &mut *graph_reg).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::ActivateAgent { agent_id } => {
            // Use complete workflow
            let mut registry = agent_registry.write().await;
            let mut agents_map = agents.write().await;
            let token = RouterToken::new();
            registry.activate_agent_complete(&token, agent_id, &mut *agents_map).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::DeactivateAgent { agent_id } => {
            // Use complete workflow that handles memory cleanup
            let mut registry = agent_registry.write().await;
            let mut agents_map = agents.write().await;
            let token = RouterToken::new();
            registry.deactivate_agent_complete(&token, agent_id, &mut *agents_map).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::AuthorizeAgent { agent_id, graph_id } => {
            // Update both registries
            let mut agent_reg = agent_registry.write().await;
            let mut graph_reg = graph_registry.write().await;
            
            // Check if this will be the agent's first graph (for default graph setting)
            let needs_default = agent_reg.get_agent(&agent_id)
                .map(|a| a.authorized_graphs.is_empty())
                .unwrap_or(false);
            
            let token = RouterToken::new();
            agent_reg.authorize_agent_for_graph(
                &token,
                &agent_id,
                &graph_id,
                &mut *graph_reg,
            ).await?;
            
            // If this was the first graph, set it as default
            let child_commands = if needs_default {
                vec![super::commands::Command::Agent(AgentCommand::SetDefaultGraph {
                    agent_id,
                    graph_id: Some(graph_id),
                })]
            } else {
                vec![]
            };
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands,
            })
        }
        
        AgentRegistryCommand::DeauthorizeAgent { agent_id, graph_id } => {
            // Update both registries
            let mut agent_reg = agent_registry.write().await;
            let mut graph_reg = graph_registry.write().await;
            
            let token = RouterToken::new();
            agent_reg.deauthorize_agent_from_graph(
                &token,
                &agent_id,
                &graph_id,
                &mut *graph_reg,
            ).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentRegistryCommand::SetPrimeAgent { agent_id } => {
            let mut registry = agent_registry.write().await;
            let token = RouterToken::new();
            registry.set_prime_agent(&token, &agent_id).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
    }
}


/// Helper to check agent authorization for a graph
async fn check_agent_authorization(
    agent_registry: &Arc<RwLock<AgentRegistry>>,
    agent_id: Uuid,
    graph_id: Uuid,
) -> Result<()> {
    let registry = agent_registry.read().await;
    
    // Check if agent is authorized for this graph
    if let Some(agent_info) = registry.get_agent(&agent_id) {
        if agent_info.authorized_graphs.contains(&graph_id) {
            return Ok(());
        }
    }
    
    Err(ProcessorError(format!(
        "Agent {} is not authorized for graph {}", 
        agent_id, graph_id
    )).into())
}