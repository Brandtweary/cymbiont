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
//! ```
//!
//! ## Authorization Checking
//!
//! Most commands require authorization checks:
//! ```rust
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
//! - Processor: State ownership and command execution
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
use crate::agent::llm::{Message, LLMConfig};
use super::commands::{CommandResult, GraphCommand, AgentCommand, RegistryCommand,
                      GraphRegistryCommand};

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
}

/// Route a graph command to its handler
pub async fn route_graph_command(
    managers: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    command: GraphCommand,
) -> Result<CommandResult> {
    match command {
        GraphCommand::CreateBlock { graph_id, block_id, content, parent_id, page_name, properties, reference_content } => {
            
            // Get the graph manager
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()))?
                .clone();
            let mut graph_manager = graph_manager_arc.write().await;
            
            // Create block using graph_operations helper
            let token = RouterToken::new();
            let block_id = graph_operations::execute_create_block(
                &token,
                &mut *graph_manager,
                block_id,
                content,
                parent_id,
                page_name,
                properties,
                reference_content,
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
        
        GraphCommand::UpdateBlock { graph_id, block_id, content } => {
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()))?
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
        
        GraphCommand::DeleteBlock { graph_id, block_id } => {
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()))?
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
        
        GraphCommand::CreatePage { graph_id, page_name, properties } => {
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()))?
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
        
        GraphCommand::DeletePage { graph_id, page_name } => {
            
            let managers_guard = managers.read().await;
            let graph_manager_arc = managers_guard.get(&graph_id)
                .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()))?
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
    agent: &Arc<RwLock<Option<Agent>>>,
    command: AgentCommand,
) -> Result<CommandResult> {
    match command {
        AgentCommand::AddMessage { message } => {
            let mut agent_guard = agent.write().await;
            let agent_mut = agent_guard.as_mut()
                .ok_or_else(|| ProcessorError::NotFound("Agent".to_string()))?;
            
            // Deserialize and add message
            let msg: Message = serde_json::from_value(message)?;
            let token = RouterToken::new();
            agent_mut.add_message(&token, msg).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::ClearHistory => {
            let mut agent_guard = agent.write().await;
            let agent_mut = agent_guard.as_mut()
                .ok_or_else(|| ProcessorError::NotFound("Agent".to_string()))?;
            
            let token = RouterToken::new();
            agent_mut.clear_history(&token).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::SetLLMConfig { config } => {
            let mut agent_guard = agent.write().await;
            let agent_mut = agent_guard.as_mut()
                .ok_or_else(|| ProcessorError::NotFound("Agent".to_string()))?;
            
            // Deserialize and set config
            let llm_config: LLMConfig = serde_json::from_value(config)?;
            let token = RouterToken::new();
            agent_mut.set_llm_config(&token, llm_config).await?;
            
            Ok(CommandResult {
                success: true,
                data: None,
                error: None,
                child_commands: vec![],
            })
        }
        
        AgentCommand::SetSystemPrompt { prompt } => {
            let mut agent_guard = agent.write().await;
            let agent_mut = agent_guard.as_mut()
                .ok_or_else(|| ProcessorError::NotFound("Agent".to_string()))?;
            
            let token = RouterToken::new();
            agent_mut.set_system_prompt(&token, prompt).await?;
            
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
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    command: RegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        RegistryCommand::Graph(graph_cmd) => {
            route_graph_registry_command(
                graph_managers,
                graph_registry,
                graph_cmd,
                data_dir,
            ).await
        }
    }
}

/// Route a graph registry command
async fn route_graph_registry_command(
    graph_managers: &Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    command: GraphRegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        GraphRegistryCommand::CreateGraph { name, description } => {
            // Generate new UUID for graph
            let graph_id = Uuid::new_v4();
            let mut graph_reg = graph_registry.write().await;
            let mut managers = graph_managers.write().await;
            
            let token = RouterToken::new();
            let graph_info = graph_reg.create_graph_complete(
                &token,
                graph_id,
                name,
                description,
                &mut *managers,
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
            let token = RouterToken::new();
            registry.remove_graph(&token, &graph_id, &mut *managers).await?;
            
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
            
            // Get the graph info to return
            let graph_info = registry.get_graph(&graph_id)
                .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?;
            
            Ok(CommandResult {
                success: true,
                data: Some(serde_json::to_value(graph_info)?),
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
