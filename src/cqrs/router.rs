//! Command Router - Enforcing CQRS at Compile Time
//!
//! The router module serves two critical purposes: it routes commands to their
//! handlers, and it enforces the CQRS architecture through the `RouterToken` pattern.
//! This is where commands are transformed into actual state changes, with compile-time
//! guarantees that all mutations flow through the command processor.
//!
//! ## The `RouterToken` Pattern
//!
//! `RouterToken` is a zero-sized type that can ONLY be created within this module.
//! All business logic methods require a `RouterToken` as their first parameter,
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
//! ### Why `RouterToken`?
//!
//! Without `RouterToken`, developers might accidentally:
//! - Call business logic directly, bypassing the command queue
//! - Create race conditions by modifying state concurrently
//!
//! `RouterToken` makes these mistakes impossible at compile time. If you don't
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
//! Router creates `RouterToken`
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
//! - Getting references from `HashMap`s
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
//! All errors are converted to `CommandResult` with appropriate messages.
//!
//! ## Design Benefits
//!
//! ### Separation of Concerns
//! - Router: Command dispatch and resource coordination
//! - Business logic: Domain operations (with `RouterToken`)
//! - Processor: State ownership and command execution
//!
//! ### Type Safety
//! `RouterToken` ensures all mutations go through CQRS at compile time,
//! not just by convention or code review.
//!
//! ### Maintainability
//! Adding new commands is straightforward:
//! 1. Define command in `commands.rs`
//! 2. Add match arm here
//! 3. Implement handler (requiring `RouterToken`)
//!
//! The compiler guides you through the process.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::commands::{
    CommandResult, GraphCommand, GraphRegistryCommand, RegistryCommand,
};
use crate::error::{ProcessorError, Result, StorageError};
use crate::graph::graph_manager::GraphManager;
use crate::graph::graph_operations;
use crate::graph::graph_registry::GraphRegistry;

/// Type alias for the graph managers collection
type GraphManagersMap = Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>;

/// Private token that proves a call came through the CQRS router.
/// This type can ONLY be constructed within the router module,
/// preventing any code outside the CQRS system from calling
/// business logic methods directly.
#[derive(Debug)]
pub struct RouterToken(());

impl RouterToken {
    /// Private constructor - only callable within this module
    const fn new() -> Self {
        Self(())
    }
}

// ===== Helper Functions =====

/// Get a graph manager from the managers map
async fn get_graph_manager(
    managers: &GraphManagersMap,
    graph_id: &Uuid,
) -> Result<Arc<RwLock<GraphManager>>> {
    let managers_guard = managers.read().await;
    managers_guard
        .get(graph_id)
        .ok_or_else(|| ProcessorError::NotFound("Graph".to_string()).into())
        .map(Arc::clone)
}

// ===== Graph Command Handlers =====

#[allow(clippy::too_many_arguments)]
async fn handle_create_block(
    managers: &GraphManagersMap,
    graph_id: Uuid,
    block_id: Option<String>,
    content: String,
    parent_id: Option<String>,
    page_name: Option<String>,
    properties: Option<serde_json::Value>,
    reference_content: Option<String>,
) -> Result<CommandResult> {
    let manager = get_graph_manager(managers, &graph_id).await?;
    let token = RouterToken::new();

    let block_id = {
        let mut graph_manager = manager.write().await;
        graph_operations::execute_create_block(
            &token,
            &mut graph_manager,
            block_id,
            content,
            parent_id.as_deref(),
            page_name.as_deref(),
            properties.as_ref(),
            reference_content,
        )
    };

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "block_id": block_id })),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_update_block(
    managers: &GraphManagersMap,
    graph_id: Uuid,
    block_id: String,
    content: String,
) -> Result<CommandResult> {
    let manager = get_graph_manager(managers, &graph_id).await?;
    let token = RouterToken::new();

    {
        let mut graph_manager = manager.write().await;
        graph_operations::execute_update_block(
            &token,
            &mut graph_manager,
            &block_id,
            content,
            graph_id,
        )?;
    }

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "block_id": block_id })),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_delete_block(
    managers: &GraphManagersMap,
    graph_id: Uuid,
    block_id: String,
) -> Result<CommandResult> {
    let manager = get_graph_manager(managers, &graph_id).await?;
    let token = RouterToken::new();

    {
        let mut graph_manager = manager.write().await;
        graph_operations::execute_delete_block(&token, &mut graph_manager, &block_id, graph_id)?;
    }

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "block_id": block_id })),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_create_page(
    managers: &GraphManagersMap,
    graph_id: Uuid,
    page_name: String,
    properties: Option<serde_json::Value>,
) -> Result<CommandResult> {
    let manager = get_graph_manager(managers, &graph_id).await?;
    let token = RouterToken::new();

    {
        let mut graph_manager = manager.write().await;
        graph_operations::execute_create_page(
            &token,
            &mut graph_manager,
            &page_name,
            properties.as_ref(),
        );
    }

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "page_name": page_name })),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_delete_page(
    managers: &GraphManagersMap,
    graph_id: Uuid,
    page_name: String,
) -> Result<CommandResult> {
    let manager = get_graph_manager(managers, &graph_id).await?;
    let token = RouterToken::new();

    {
        let mut graph_manager = manager.write().await;
        graph_operations::execute_delete_page(&token, &mut graph_manager, &page_name, graph_id)?;
    }

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "page_name": page_name })),
        error: None,
        child_commands: vec![],
    })
}


/// Route a graph command to its handler
pub async fn route_graph_command(
    managers: &GraphManagersMap,
    command: GraphCommand,
) -> Result<CommandResult> {
    match command {
        GraphCommand::CreateBlock {
            graph_id,
            block_id,
            content,
            parent_id,
            page_name,
            properties,
            reference_content,
        } => {
            handle_create_block(
                managers,
                graph_id,
                block_id,
                content,
                parent_id,
                page_name,
                properties,
                reference_content,
            )
            .await
        }

        GraphCommand::UpdateBlock {
            graph_id,
            block_id,
            content,
        } => handle_update_block(managers, graph_id, block_id, content).await,

        GraphCommand::DeleteBlock { graph_id, block_id } => {
            handle_delete_block(managers, graph_id, block_id).await
        }

        GraphCommand::CreatePage {
            graph_id,
            page_name,
            properties,
        } => handle_create_page(managers, graph_id, page_name, properties).await,

        GraphCommand::DeletePage {
            graph_id,
            page_name,
        } => handle_delete_page(managers, graph_id, page_name).await,
    }
}


// ===== Graph Registry Command Handlers =====

async fn handle_create_graph(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    name: Option<String>,
    description: Option<String>,
    data_dir: &Path,
) -> Result<CommandResult> {
    let graph_id = Uuid::new_v4();
    let token = RouterToken::new();

    let graph_info = {
        let mut graph_reg = graph_registry.write().await;
        let mut managers = graph_managers.write().await;
        graph_reg.create_graph_complete(
            &token,
            graph_id,
            name,
            description,
            &mut managers,
            data_dir,
        )?
    };

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::to_value(graph_info).map_err(|e| ProcessorError::Serialization(e))?),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_register_graph(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    graph_id: Uuid,
    name: Option<String>,
    description: Option<String>,
    data_dir: &Path,
) -> Result<CommandResult> {
    let graph_dir = data_dir.join("graphs").join(graph_id.to_string());

    {
        let mut registry = graph_registry.write().await;
        registry.register_graph(Some(graph_id), name, description, &graph_dir);
    }

    let manager = Arc::new(RwLock::new(GraphManager::new(&graph_dir)?));
    {
        let mut managers = graph_managers.write().await;
        managers.insert(graph_id, manager);
    }

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::json!({ "graph_id": graph_id })),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_remove_graph(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    graph_id: Uuid,
) -> Result<CommandResult> {
    let token = RouterToken::new();
    {
        let mut registry = graph_registry.write().await;
        let mut managers = graph_managers.write().await;
        registry.remove_graph(&token, &graph_id, &mut managers)?;
    }

    Ok(CommandResult {
        success: true,
        data: None,
        error: None,
        child_commands: vec![],
    })
}

async fn handle_open_graph(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    graph_id: Uuid,
    data_dir: &Path,
) -> Result<CommandResult> {
    let token = RouterToken::new();
    let graph_info = {
        let mut registry = graph_registry.write().await;
        let mut managers = graph_managers.write().await;
        registry.open_graph_complete(&token, graph_id, &mut managers, data_dir)?;

        registry
            .get_graph(&graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone()
    };

    Ok(CommandResult {
        success: true,
        data: Some(serde_json::to_value(graph_info).map_err(|e| ProcessorError::Serialization(e))?),
        error: None,
        child_commands: vec![],
    })
}

async fn handle_close_graph(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    graph_id: Uuid,
) -> Result<CommandResult> {
    let token = RouterToken::new();
    {
        let mut registry = graph_registry.write().await;
        let mut managers = graph_managers.write().await;
        registry.close_graph(&token, &graph_id, &mut managers)?;
    }

    Ok(CommandResult {
        success: true,
        data: None,
        error: None,
        child_commands: vec![],
    })
}

/// Route a registry command to its handler
pub async fn route_registry_command(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    command: RegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        RegistryCommand::Graph(graph_cmd) => {
            route_graph_registry_command(graph_managers, graph_registry, graph_cmd, data_dir).await
        }
    }
}

/// Route a graph registry command
async fn route_graph_registry_command(
    graph_managers: &GraphManagersMap,
    graph_registry: &Arc<RwLock<GraphRegistry>>,
    command: GraphRegistryCommand,
    data_dir: &Path,
) -> Result<CommandResult> {
    match command {
        GraphRegistryCommand::CreateGraph { name, description } => {
            handle_create_graph(graph_managers, graph_registry, name, description, data_dir).await
        }
        GraphRegistryCommand::RegisterGraph {
            graph_id,
            name,
            description,
        } => {
            handle_register_graph(
                graph_managers,
                graph_registry,
                graph_id,
                name,
                description,
                data_dir,
            )
            .await
        }
        GraphRegistryCommand::RemoveGraph { graph_id } => {
            handle_remove_graph(graph_managers, graph_registry, graph_id).await
        }
        GraphRegistryCommand::OpenGraph { graph_id } => {
            handle_open_graph(graph_managers, graph_registry, graph_id, data_dir).await
        }
        GraphRegistryCommand::CloseGraph { graph_id } => {
            handle_close_graph(graph_managers, graph_registry, graph_id).await
        }
    }
}
