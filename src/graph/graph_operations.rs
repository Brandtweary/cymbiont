//! Graph Operations Module - PKM Operations
//!
//! Provides PKM-specific graph operations.
//! Operations are routed through the CQRS command system.
//!
//! ## Design Pattern: Single Trait API
//!
//! The `GraphOps` trait provides all graph operations:
//! ```rust
//! use graph_operations::GraphOps;
//!
//! // All mutations go through CommandQueue
//! let response = app_state.command_queue.execute(
//!     Command::Graph(GraphCommand::CreateBlock {
//!         graph_id, content, parent_id, page_name, properties
//!     })
//! ).await?;
//! ```
//!
//! This pattern provides several benefits:
//! - Centralized command handling through CQRS
//! - Single source of truth for all graph operations
//! - Consistent command routing and validation
//! - Clean integration with transaction system
//!
//! PKM-specific logic (block reference resolution, page normalization) is delegated to
//! helper functions in `pkm_data.rs` to maintain separation of concerns.
//!
//! ## Core Operations
//!
//! ### Block Operations
//! - `add_block()` - Create new PKM block with content, parent, and properties
//! - `update_block()` - Modify block content with automatic reference resolution
//! - `delete_block()` - Archive block node while preserving data
//!
//! ### Page Operations  
//! - `create_page()` - Create new PKM page with normalized name handling
//! - `delete_page()` - Archive page node (handles normalized names)
//!
//! ### Graph Management
//! - `create_graph()` - Initialize new knowledge graph with registry entry
//! - `delete_graph()` - Archive entire graph (can delete both open and closed)
//! - `open_graph()` - Load graph into RAM and trigger recovery
//! - `close_graph()` - Save graph and unload from RAM
//! - `list_graphs()` - Enumerate all registered graphs
//! - `list_open_graphs()` - List currently open graphs
//!
//! ### Query Operations
//! - `get_node()` - Retrieve node by ID with PKM-aware formatting
//! - `query_graph_bfs()` - Breadth-first traversal (TODO)
//!
//! ### Recovery Operations
//! - `replay_transaction()` - Replay a stored operation during crash recovery
//!
//! ## Transaction Integration
//!
//! All mutation operations are automatically wrapped in transactions:
//! 1. Operation parameters are stored in WAL before execution
//! 2. PKM transformations are applied within transaction boundary
//! 3. Success/failure updates transaction state
//! 4. Crash recovery replays operations with exact parameters
//!
//! The Operation enum stores full API parameters:
//! ```rust
//! Operation::Graph(GraphOperation::CreateBlock {
//!     graph_id: Uuid,
//!     content: String,
//!     parent_id: Option<String>,
//!     page_name: Option<String>,
//!     properties: Option<serde_json::Value>,
//! })
//! ```
//!
//! ## Crash Recovery
//!
//! Recovery happens at two points:
//! 1. **Startup** (main.rs): Runs `run_all_graphs_recovery()` for ALL graphs (both open and closed)
//! 2. **Graph Open**: Each `open_graph()` call triggers recovery for that specific graph
//!
//! The recovery process:
//! - Startup: Iterates all graphs, temporarily opens closed ones for recovery
//! - Finds all Active transactions in each graph's WAL
//! - RecoveryContext replays each pending operation
//! - Updates transaction state based on result
//! - No PKM reconstruction needed - exact API replay
//! - Closed graphs are closed again after recovery
//!
//! ## Adding New Graph Operations
//!
//! When adding new operations, follow these steps:
//!
//! 1. **Define the Command variant** in `cqrs/commands.rs`:
//!    - Add new variant to `GraphCommand` enum with all necessary parameters
//!    - Include all data needed to replay the command during recovery
//!
//! 2. **Add the trait method** to `GraphOps` trait in this file:
//!    - Add graph_id: &Uuid parameter for graph targeting
//!    - Return appropriate Result<T> type
//!    - Route to `command_queue.execute()` internally
//!
//! 3. **Implement command handling** in `cqrs/router.rs`:
//!    - Add match arm in `apply_graph_command()` method
//!    - Call helper function with RouterToken for validation
//!    - All mutations automatically logged to WAL
//!
//! 4. **Create helper function** in this file:
//!    - Takes RouterToken as first parameter (enforces CQRS routing)
//!    - Implements the actual graph operation logic
//!    - Handles PKM-specific concerns (reference resolution, etc.)
//!
//! 5. **Register in tool registry** (optional) in `agent/kg_tools.rs`:
//!    - Add tool registration in appropriate category
//!    - Parse parameters from JSON args
//!    - Call GraphOps method with parsed params
//!
//! 6. **Add WebSocket command** (optional) in `server/websocket.rs`:
//!    - Define command variant in Command enum
//!    - Add handler that processes the command
//!    - Call GraphOps method and return success/error response
//!
//! ## Error Handling
//!
//! Operations return `Result<T, GraphOperationError>` with two error variants:
//! - `GraphError` - General graph operation failures
//! - `NodeNotFound` - Specific node lookup failures

use crate::error::*;
use crate::{
    cqrs::{Command, GraphCommand, GraphRegistryCommand, RegistryCommand},
    AppState,
};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Graph operations that route through the CQRS command system.
/// These methods provide the main API for graph operations.
/// This is the single source of truth for all graph operations.
pub trait GraphOps {
    /// Add a new block
    async fn add_block(
        &self,
        block_id: Option<String>,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        reference_content: Option<String>,
        graph_id: &Uuid,
    ) -> Result<String>;

    /// Update block
    async fn update_block(&self, block_id: String, content: String, graph_id: &Uuid) -> Result<()>;

    /// Delete block
    async fn delete_block(&self, block_id: String, graph_id: &Uuid) -> Result<()>;

    /// Create page
    async fn create_page(
        &self,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<()>;

    /// Delete page
    async fn delete_page(&self, page_name: String, graph_id: &Uuid) -> Result<()>;

    /// Get a node by ID
    async fn get_node(&self, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value>;

    /// Query graph with BFS traversal
    async fn query_graph_bfs(
        &self,
        start_id: &str,
        max_depth: usize,
        graph_id: &Uuid,
    ) -> Result<Vec<serde_json::Value>>;

    /// Open a graph (load into RAM and trigger recovery)
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value>;

    /// Close a graph (save and unload from RAM)
    async fn close_graph(&self, graph_id: Uuid) -> Result<()>;

    /// List all open graphs
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>>;

    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>>;

    /// Create a new knowledge graph
    async fn create_graph(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<serde_json::Value>;

    /// Delete a knowledge graph
    async fn delete_graph(&self, graph_id: &Uuid) -> Result<()>;
}

// Graph operations implementation that routes through CQRS
impl GraphOps for Arc<AppState> {
    async fn add_block(
        &self,
        block_id: Option<String>,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        reference_content: Option<String>,
        graph_id: &Uuid,
    ) -> Result<String> {
        // Submit command to CQRS system
        let command = Command::Graph(GraphCommand::CreateBlock {
            graph_id: *graph_id,
            block_id,
            content,
            parent_id,
            page_name,
            properties,
            reference_content,
        });

        let result = self.command_queue.execute(command).await?;

        // Extract block_id from result
        if result.success {
            if let Some(data) = result.data {
                if let Some(block_id) = data.get("block_id").and_then(|v| v.as_str()) {
                    return Ok(block_id.to_string());
                }
            }
        }
        Err(GraphError::invalid_state("Failed to create block").into())
    }

    async fn update_block(&self, block_id: String, content: String, graph_id: &Uuid) -> Result<()> {
        // Submit command to CQRS system
        let command = Command::Graph(GraphCommand::UpdateBlock {
            graph_id: *graph_id,
            block_id,
            content,
        });

        self.command_queue.execute(command).await?;
        Ok(())
    }

    async fn delete_block(&self, block_id: String, graph_id: &Uuid) -> Result<()> {
        // Submit command to CQRS system
        let command = Command::Graph(GraphCommand::DeleteBlock {
            graph_id: *graph_id,
            block_id,
        });

        self.command_queue.execute(command).await?;
        Ok(())
    }

    async fn create_page(
        &self,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<()> {
        // Submit command to CQRS system
        let command = Command::Graph(GraphCommand::CreatePage {
            graph_id: *graph_id,
            page_name,
            properties,
        });

        self.command_queue.execute(command).await?;
        Ok(())
    }

    async fn delete_page(&self, page_name: String, graph_id: &Uuid) -> Result<()> {
        // Submit command to CQRS system
        let command = Command::Graph(GraphCommand::DeletePage {
            graph_id: *graph_id,
            page_name,
        });

        self.command_queue.execute(command).await?;
        Ok(())
    }

    async fn get_node(&self, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value> {
        // Direct read from graph manager
        let managers = self.graph_managers.read().await;
        let manager_arc = managers
            .get(graph_id)
            .ok_or_else(|| GraphError::not_found(*graph_id))?;
        let manager = manager_arc.read().await;

        // Find and return the node
        let node_idx = manager
            .find_node(node_id)
            .ok_or_else(|| GraphError::node_not_found(node_id, *graph_id))?;
        let node_data = manager
            .get_node(node_idx)
            .ok_or_else(|| GraphError::node_not_found(node_id, *graph_id))?;

        Ok(serde_json::to_value(node_data)?)
    }

    /// Query graph with BFS traversal
    #[allow(unused_variables)]
    async fn query_graph_bfs(
        &self,
        start_id: &str,
        max_depth: usize,
        graph_id: &Uuid,
    ) -> Result<Vec<serde_json::Value>> {
        // TODO: Implement BFS traversal in graph_manager
        // For now, return empty result
        Ok(vec![])
    }

    /// Open a graph (load into RAM and trigger recovery)
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value> {
        info!("📂 Opening graph: {}", graph_id);

        // Submit command to CQRS system
        let command = Command::Registry(RegistryCommand::Graph(GraphRegistryCommand::OpenGraph {
            graph_id,
        }));

        let result = self.command_queue.execute(command).await?;

        if result.success {
            result.data.ok_or_else(|| {
                GraphError::invalid_state("No data returned from OpenGraph command").into()
            })
        } else {
            Err(GraphError::invalid_state(format!("OpenGraph failed: {:?}", result.error)).into())
        }
    }

    /// Close a graph (save and unload from RAM)
    async fn close_graph(&self, graph_id: Uuid) -> Result<()> {
        // Submit command to CQRS system
        let command = Command::Registry(RegistryCommand::Graph(GraphRegistryCommand::CloseGraph {
            graph_id,
        }));

        self.command_queue.execute(command).await?;
        Ok(())
    }

    /// List all open graphs
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>> {
        // Direct read from graph managers
        let managers = self.graph_managers.read().await;
        Ok(managers.keys().copied().collect())
    }

    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        // Direct read from graph registry
        let registry = self.graph_registry.read().await;
        let graphs: Vec<serde_json::Value> = registry
            .get_all_graphs()
            .into_iter()
            .map(|info| serde_json::to_value(info).unwrap())
            .collect();
        Ok(graphs)
    }

    /// Create a new knowledge graph
    async fn create_graph(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<serde_json::Value> {
        // Submit command to CQRS system
        let command =
            Command::Registry(RegistryCommand::Graph(GraphRegistryCommand::CreateGraph {
                name,
                description,
            }));

        let result = self.command_queue.execute(command).await?;

        if result.success {
            result.data.ok_or_else(|| {
                GraphError::invalid_state("No data returned from CreateGraph command").into()
            })
        } else {
            Err(GraphError::invalid_state(format!("CreateGraph failed: {:?}", result.error)).into())
        }
    }

    /// Delete a knowledge graph
    ///
    /// Archives the graph to `{data_dir}/archived_graphs/` with timestamp.
    /// Can delete both open and closed graphs.
    async fn delete_graph(&self, graph_id: &Uuid) -> Result<()> {
        // Submit command to CQRS system
        let command =
            Command::Registry(RegistryCommand::Graph(GraphRegistryCommand::RemoveGraph {
                graph_id: *graph_id,
            }));

        self.command_queue.execute(command).await?;
        Ok(())
    }
}

// ================================================================================
// Helper Functions - Business Logic for Graph Operations
// ================================================================================
// These functions contain the actual implementation logic that the router delegates to.
// They work directly with GraphManager and handle PKM-specific operations.

use crate::cqrs::router::RouterToken;
use crate::graph::graph_manager::GraphManager;
use crate::import::pkm_data;

/// Execute the create block operation
/// This contains the business logic extracted from the old add_block implementation
/// REQUIRES RouterToken to ensure this is only called through CQRS
pub async fn execute_create_block(
    _token: &RouterToken,
    graph_manager: &mut GraphManager,
    block_id: Option<String>,
    content: String,
    parent_id: Option<String>,
    page_name: Option<String>,
    properties: Option<serde_json::Value>,
    reference_content: Option<String>,
) -> Result<String> {
    // Use provided block_id if available, otherwise generate new one
    let final_block_id = block_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    // Use provided reference_content if available, otherwise resolve on-the-fly
    let (block_id, _reference_content) = if let Some(ref_content) = reference_content {
        // Use pre-resolved content from import
        let now = chrono::Utc::now();
        let props = properties
            .as_ref()
            .map(|p| crate::utils::parse_properties(p))
            .unwrap_or_default();

        graph_manager
            .create_or_update_node(
                final_block_id.clone(),
                crate::graph::graph_manager::NodeType::Block,
                content.clone(),
                Some(ref_content.clone()),
                props,
                now,
                now,
            )
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;

        (final_block_id, Some(ref_content))
    } else {
        // Resolve on-the-fly as before
        pkm_data::create_block_with_resolution_and_id(
            graph_manager,
            Some(final_block_id),
            content,
            properties.as_ref(),
        )?
    };

    // Setup relationships
    pkm_data::setup_block_relationships(
        graph_manager,
        &block_id,
        parent_id.as_deref(),
        page_name.as_deref(),
    )?;

    Ok(block_id)
}

/// Execute the update block operation
/// This contains the business logic extracted from the old update_block implementation
/// REQUIRES RouterToken to ensure this is only called through CQRS
pub async fn execute_update_block(
    _token: &RouterToken,
    graph_manager: &mut GraphManager,
    block_id: &str,
    content: String,
    _graph_id: Uuid,
) -> Result<()> {
    // Update block with reference resolution
    pkm_data::update_block_with_resolution(graph_manager, block_id, content)?;

    Ok(())
}

/// Execute the delete block operation
/// This contains the business logic extracted from the old delete_block implementation
/// REQUIRES RouterToken to ensure this is only called through CQRS
pub async fn execute_delete_block(
    _token: &RouterToken,
    graph_manager: &mut GraphManager,
    block_id: &str,
    graph_id: Uuid,
) -> Result<()> {
    if let Some(node_idx) = graph_manager.find_node(block_id) {
        // Archive the node
        graph_manager
            .delete_nodes(vec![(block_id.to_string(), node_idx)])
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
    } else {
        return Err(GraphError::node_not_found(block_id, graph_id).into());
    }

    Ok(())
}

/// Execute the create page operation
/// This contains the business logic extracted from the old create_page implementation
/// REQUIRES RouterToken to ensure this is only called through CQRS
pub async fn execute_create_page(
    _token: &RouterToken,
    graph_manager: &mut GraphManager,
    page_name: String,
    properties: Option<serde_json::Value>,
) -> Result<()> {
    // Create or update page with properties
    pkm_data::create_or_update_page(graph_manager, &page_name, properties.as_ref())?;

    Ok(())
}

/// Execute the delete page operation
/// This contains the business logic extracted from the old delete_page implementation
/// REQUIRES RouterToken to ensure this is only called through CQRS
pub async fn execute_delete_page(
    _token: &RouterToken,
    graph_manager: &mut GraphManager,
    page_name: &str,
    graph_id: Uuid,
) -> Result<()> {
    // Find page by original or normalized name
    let (normalized_name, node_idx) = pkm_data::find_page_for_deletion(graph_manager, page_name)
        .map_err(|_| GraphError::node_not_found(page_name, graph_id))?;

    // Archive the node
    graph_manager
        .delete_nodes(vec![(normalized_name, node_idx)])
        .map_err(|e| GraphError::lifecycle(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cqrs::{Command, GraphCommand};
    use serde_json::json;

    #[test]
    fn test_error_types() {
        // Test GraphError creation
        let graph_err = GraphError::lifecycle("Test error");
        assert_eq!(graph_err.to_string(), "Graph lifecycle error: Test error");

        // Test NodeNotFound creation
        let graph_id = Uuid::new_v4();
        let node_err = GraphError::node_not_found("block-123", graph_id);
        assert_eq!(
            node_err.to_string(),
            format!("Node not found: block-123 in graph {}", graph_id)
        );
    }

    #[test]
    fn test_command_variants() {
        // Test that all GraphCommand variants can be created through Command enum
        let graph_id = Uuid::new_v4();

        let create_block = Command::Graph(GraphCommand::CreateBlock {
            graph_id,
            block_id: None,
            content: "Test content".to_string(),
            parent_id: Some("parent-123".to_string()),
            page_name: Some("TestPage".to_string()),
            properties: Some(json!({"key": "value"})),
            reference_content: None,
        });
        assert!(matches!(
            create_block,
            Command::Graph(GraphCommand::CreateBlock { .. })
        ));

        let update_block = Command::Graph(GraphCommand::UpdateBlock {
            graph_id,
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
        });
        assert!(matches!(
            update_block,
            Command::Graph(GraphCommand::UpdateBlock { .. })
        ));

        let delete_block = Command::Graph(GraphCommand::DeleteBlock {
            graph_id,
            block_id: "block-123".to_string(),
        });
        assert!(matches!(
            delete_block,
            Command::Graph(GraphCommand::DeleteBlock { .. })
        ));

        let create_page = Command::Graph(GraphCommand::CreatePage {
            graph_id,
            page_name: "NewPage".to_string(),
            properties: Some(json!({"type": "journal"})),
        });
        assert!(matches!(
            create_page,
            Command::Graph(GraphCommand::CreatePage { .. })
        ));

        let delete_page = Command::Graph(GraphCommand::DeletePage {
            graph_id,
            page_name: "OldPage".to_string(),
        });
        assert!(matches!(
            delete_page,
            Command::Graph(GraphCommand::DeletePage { .. })
        ));
    }

    #[test]
    fn test_result_type_conversions() {
        // Test that our Result type works with error conversions
        fn returns_graph_error() -> Result<String> {
            Err(GraphError::lifecycle("Something went wrong").into())
        }

        fn returns_node_not_found() -> Result<String> {
            let graph_id = Uuid::new_v4();
            Err(GraphError::node_not_found("node-123", graph_id).into())
        }

        // Test error matching
        match returns_graph_error() {
            Err(CymbiontError::Graph(GraphError::Lifecycle { message })) => {
                assert_eq!(message, "Something went wrong");
            }
            _ => panic!("Expected GraphError"),
        }

        match returns_node_not_found() {
            Err(CymbiontError::Graph(GraphError::NodeNotFound { node_id, .. })) => {
                assert_eq!(node_id, "node-123");
            }
            _ => panic!("Expected NodeNotFound"),
        }
    }
}
