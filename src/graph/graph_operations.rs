//! Graph Operations Module - Agent-Aware PKM Operations
//! 
//! Provides PKM-specific graph operations with runtime agent authorization.
//! All operations require an agent ID and verify authorization before execution.
//! 
//! ## Design Pattern: Single Trait API
//! 
//! The `GraphOps` trait provides all graph operations with agent awareness:
//! ```rust
//! use graph_operations::GraphOps;
//! 
//! // Operations require agent_id for authorization
//! app_state.add_block(agent_id, content, parent_id, page_name, properties, &graph_id, skip_wal).await?;
//! ```
//! 
//! This pattern provides several benefits:
//! - Runtime authorization checks for security
//! - Single source of truth for all graph operations
//! - Clear agent accountability for all changes
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
//!     agent_id: Uuid,
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
//! 1. **Define the Operation variant** in `storage/wal.rs`:
//!    - Add new variant to `Operation` enum with agent_id and all parameters
//!    - Include all data needed to replay the operation during recovery
//! 
//! 2. **Add the trait method** to `GraphOps` trait in this file:
//!    - Include agent_id: Uuid as first parameter
//!    - Add graph_id: &Uuid parameter for graph targeting
//!    - Return appropriate Result<T> type
//! 
//! 3. **Implement the operation** in `impl GraphOps for Arc<AppState>`:
//!    - Call `check_authorization()` to verify agent access
//!    - Create Operation enum (or None if skip_wal)
//!    - Call `coordinator.begin(operation)` to start transaction
//!    - Perform the actual graph modifications
//!    - Call `tx.commit()` to finalize the transaction
//! 
//! 4. **Add to RecoveryContext** in `storage/recovery.rs`:
//!    - Add match arm in execute_operation() method
//!    - Map operation parameters to appropriate recovery logic
//!    - Handle recovery-specific considerations (skip_wal, etc.)
//! 
//! 5. **Register in tool registry** (optional) in `agent/kg_tools.rs`:
//!    - Add tool registration in appropriate category
//!    - Parse parameters from JSON args
//!    - Call GraphOps method with agent_id and parsed params
//! 
//! 6. **Add WebSocket command** (optional) in `server/websocket.rs`:
//!    - Define command variant in Command enum
//!    - Add handler that extracts current_agent_id
//!    - Call GraphOps method and return success/error response
//! 
//! ## Error Handling
//! 
//! Operations return `Result<T, GraphOperationError>` with two error variants:
//! - `GraphError` - General graph operation failures
//! - `NodeNotFound` - Specific node lookup failures

use crate::{
    AppState,
    agent::agent_registry::AgentRegistry,
    graph::graph_registry::GraphRegistry,
    storage::{ 
        wal::{
            Operation, GraphOperation
        }
    },
    import::pkm_data,
};
use std::sync::Arc;
use tracing::{warn, info};
use serde_json::json;
use uuid::Uuid;
use crate::error::*;
use crate::lock::AsyncRwLockExt;


/// Helper to verify agent authorization for a graph operation.
/// Returns an error if the agent is not authorized.
fn verify_authorization(
    agent_registry: &AgentRegistry,
    agent_id: &Uuid,
    graph_id: &Uuid,
) -> Result<()> {
    if !agent_registry.is_agent_authorized(agent_id, graph_id) {
        Err(GraphError::invalid_state(format!(
            "Agent {} is not authorized for graph {} - authorization required for all graph operations",
            agent_id, graph_id
        )).into())
    } else {
        Ok(())
    }
}

/// Helper to check authorization for a graph operation.
/// Encapsulates the full registry read/verify/drop pattern.
async fn check_authorization(
    agent_registry: &Arc<tokio::sync::RwLock<AgentRegistry>>,
    agent_id: &Uuid,
    graph_id: &Uuid,
) -> Result<()> {
    let registry = agent_registry.read_or_panic("check authorization").await;
    verify_authorization(&registry, agent_id, graph_id)?;
    Ok(())
}

/// Agent-aware graph operations that automatically handle authorization.
/// These methods verify agent authorization at runtime before performing operations.
/// This is the single source of truth for all graph operations.
pub trait GraphOps {
    /// Add a new block with agent authorization
    async fn add_block(
        &self,
        agent_id: Uuid,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<String>;

    /// Update block with agent authorization
    async fn update_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        content: String,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()>;

    /// Delete block with agent authorization
    async fn delete_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()>;

    /// Create page with agent authorization
    async fn create_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()>;

    /// Delete page with agent authorization
    async fn delete_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()>;
    
    /// Get a node by ID with agent authorization
    async fn get_node(&self, agent_id: Uuid, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value>;
    
    /// Query graph with BFS traversal with agent authorization
    async fn query_graph_bfs(&self, agent_id: Uuid, start_id: &str, max_depth: usize, graph_id: &Uuid) -> Result<Vec<serde_json::Value>>;
    
    /// Open a graph (load into RAM and trigger recovery)
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value>;
    
    /// Close a graph (save and unload from RAM)
    async fn close_graph(&self, graph_id: Uuid) -> Result<()>;
    
    /// List all open graphs
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>>;
    
    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>>;
    
    /// Create a new knowledge graph
    async fn create_graph(&self, name: Option<String>, description: Option<String>) -> Result<serde_json::Value>;
    
    /// Delete a knowledge graph
    async fn delete_graph(&self, graph_id: &Uuid) -> Result<()>;
}

// Agent-aware graph operations implementation with runtime authorization
impl GraphOps for Arc<AppState> {
    async fn add_block(
        &self,
        agent_id: Uuid,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<String> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Create the WAL operation only if not skipping WAL
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Graph(GraphOperation::CreateBlock {
                graph_id: *graph_id,
                agent_id,
                content: content.clone(),
                parent_id: parent_id.clone(),
                page_name: page_name.clone(),
                properties: properties.clone(),
            }))
        };
        
        let coordinator = &self.transaction_coordinator;
        let tx = coordinator.begin(operation).await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read_or_panic("add block - read managers").await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(graph_id.to_string()))?;
        let mut manager = manager_lock.write_or_panic("add block - write manager").await;
        
        // Create block with reference resolution
        let (block_id, _reference_content) = pkm_data::create_block_with_resolution(
            &mut manager,
            content,
            properties.as_ref(),
        )?;
        
        // Setup relationships
        pkm_data::setup_block_relationships(
            &mut manager,
            &block_id,
            parent_id.as_deref(),
            page_name.as_deref(),
        )?;
        
        // Commit the transaction
        tx.commit().await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        Ok(block_id)
    }
    
    async fn update_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        content: String,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Create the WAL operation only if not skipping WAL
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Graph(GraphOperation::UpdateBlock {
                graph_id: *graph_id,
                agent_id,
                block_id: block_id.clone(),
                content: content.clone(),
            }))
        };
        
        let coordinator = &self.transaction_coordinator;
        let tx = coordinator.begin(operation).await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read_or_panic("update block - read managers").await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(graph_id.to_string()))?;
        let mut manager = manager_lock.write_or_panic("update block - write manager").await;
        
        // Update block with reference resolution
        pkm_data::update_block_with_resolution(
            &mut manager,
            &block_id,
            content,
        )?;
        
        // Commit the transaction
        tx.commit().await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        Ok(())
    }
    
    async fn delete_block(&self, agent_id: Uuid, block_id: String, graph_id: &Uuid, skip_wal: bool) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Create the WAL operation only if not skipping WAL
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Graph(GraphOperation::DeleteBlock {
                graph_id: *graph_id,
                agent_id,
                block_id: block_id.clone(),
            }))
        };
        
        // Clone for error handling
        let block_id_for_error = block_id.clone();
        
        let coordinator = &self.transaction_coordinator;
        let tx = coordinator.begin(operation).await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read_or_panic("delete block - read managers").await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(graph_id.to_string()))?;
        let mut manager = manager_lock.write_or_panic("delete block - write manager").await;
        
        if let Some(node_idx) = manager.find_node(&block_id) {
            // Archive the node
            manager.delete_nodes(vec![(block_id.clone(), node_idx)])
                .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        } else {
            return Err(GraphError::node_not_found(block_id_for_error, *graph_id).into());
        }
        
        // Commit the transaction
        tx.commit().await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        Ok(())
    }
    
    async fn create_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
        skip_wal: bool,
    ) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Create the WAL operation only if not skipping WAL
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Graph(GraphOperation::CreatePage {
                graph_id: *graph_id,
                agent_id,
                page_name: page_name.clone(),
                properties: properties.clone(),
            }))
        };
        
        let coordinator = &self.transaction_coordinator;
        let tx = coordinator.begin(operation).await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read_or_panic("create page - read managers").await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(graph_id.to_string()))?;
        let mut manager = manager_lock.write_or_panic("create page - write manager").await;
        
        // Create or update page with properties
        pkm_data::create_or_update_page(
            &mut manager,
            &page_name,
            properties.as_ref(),
        )?;
        
        // Commit the transaction
        tx.commit().await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        Ok(())
    }
    
    async fn delete_page(&self, agent_id: Uuid, page_name: String, graph_id: &Uuid, skip_wal: bool) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Create the WAL operation only if not skipping WAL
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Graph(GraphOperation::DeletePage {
                graph_id: *graph_id,
                agent_id,
                page_name: page_name.clone(),
            }))
        };
        
        // Clone for error handling (move closure captures the original)
        let page_name_for_error = page_name.clone();
        
        let coordinator = &self.transaction_coordinator;
        let tx = coordinator.begin(operation).await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read_or_panic("delete page - read managers").await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(graph_id.to_string()))?;
        let mut manager = manager_lock.write_or_panic("delete page - write manager").await;
        
        // Find page by original or normalized name
        let (normalized_name, node_idx) = pkm_data::find_page_for_deletion(
            &manager,
            &page_name,
        ).map_err(|_| GraphError::node_not_found(page_name_for_error, *graph_id))?;
        
        // Archive the node
        manager.delete_nodes(vec![(normalized_name, node_idx)])
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        // Commit the transaction
        tx.commit().await
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
        
        Ok(())
    }
    
    async fn get_node(&self, agent_id: Uuid, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // Get managers directly from AppState
        let managers = self.graph_managers.read().await;
        let manager_lock = managers.get(graph_id)
            .ok_or_else(|| GraphError::not_found(format!("graph {}", graph_id)))?;
        let graph_manager = manager_lock.read().await;
        
        if let Some(node_idx) = graph_manager.find_node(node_id) {
            if let Some(node) = graph_manager.get_node(node_idx) {
                Ok(json!({
                    "id": node.pkm_id,
                    "type": format!("{:?}", node.node_type),
                    "content": node.content,
                    "properties": node.properties,
                    "created_at": node.created_at.to_rfc3339(),
                    "updated_at": node.updated_at.to_rfc3339(),
                }))
            } else {
                Err(GraphError::node_not_found(node_id, *graph_id).into())
            }
        } else {
            Err(GraphError::node_not_found(node_id, *graph_id).into())
        }
    }
    
    /// Query graph with BFS traversal
    async fn query_graph_bfs(
        &self,
        agent_id: Uuid,
        _start_id: &str,
        _max_depth: usize,
        graph_id: &Uuid,
    ) -> Result<Vec<serde_json::Value>> {
        check_authorization(&self.agent_registry, &agent_id, graph_id).await?;
        
        // TODO: Implement BFS traversal in graph_manager
        // For now, return empty result
        warn!("BFS traversal not yet implemented");
        Ok(vec![])
    }
    
    /// Open a graph (load into RAM and trigger recovery)
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value> {
        info!("📂 Opening graph: {}", graph_id);
        
        // Open the graph through registry
        {
            let mut registry = self.graph_registry.write_or_panic("open graph").await;
            registry.open_graph(&graph_id, false).await
                .map_err(|e| GraphError::lifecycle(format!("Failed to open graph: {}", e)))?;
        }
        
        // Get graph info from registry
        let graph_info = {
            let registry = self.graph_registry.read_or_panic("get graph info").await;
            registry.get_graph(&graph_id)
                .ok_or_else(|| GraphError::not_found(format!("graph {}", graph_id)))?
                .clone()
        };
        
        Ok(json!({
            "id": graph_info.id,
            "name": graph_info.name,
            "created": graph_info.created.to_rfc3339(),
            "last_accessed": graph_info.last_accessed.to_rfc3339(),
            "description": graph_info.description,
        }))
    }
    
    /// Close a graph (save and unload from RAM)
    async fn close_graph(&self, graph_id: Uuid) -> Result<()> {
        // Close the graph through registry
        let mut registry = self.graph_registry.write_or_panic("close graph").await;
        registry.close_graph(&graph_id, false).await
            .map_err(|e| GraphError::lifecycle(format!("Failed to close graph: {}", e)))?;
        
        Ok(())
    }
    
    /// List all open graphs
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>> {
        let registry = self.graph_registry.read_or_panic("list open graphs").await;
        Ok(registry.get_open_graphs())
    }
    
    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        let registry = self.graph_registry.read_or_panic("list graphs").await;
        let graphs = registry.get_all_graphs();
        Ok(graphs.into_iter().map(|info| {
            json!({
                "id": info.id,
                "name": info.name,
                "created": info.created.to_rfc3339(),
                "last_accessed": info.last_accessed.to_rfc3339(),
                "description": info.description,
            })
        }).collect())
    }
    
    /// Create a new knowledge graph with automatic prime agent authorization
    async fn create_graph(
        &self, 
        name: Option<String>, 
        description: Option<String>
    ) -> Result<serde_json::Value> {
        tracing::debug!("create_graph: Starting graph creation");
        // Use the complete workflow method
        let graph_info = GraphRegistry::create_graph_complete(
            self.graph_registry.clone(),
            name,
            description
        ).await
            .map_err(|e| GraphError::lifecycle(format!("Failed to create graph: {}", e)))?;
        tracing::debug!("create_graph: Graph created successfully");
        
        Ok(json!({
            "id": graph_info.id,
            "name": graph_info.name,
            "created": graph_info.created.to_rfc3339(),
            "last_accessed": graph_info.last_accessed.to_rfc3339(),
            "description": graph_info.description,
        }))
    }
    
    /// Delete a knowledge graph
    /// 
    /// Archives the graph to `{data_dir}/archived_graphs/` with timestamp.
    /// Can delete both open and closed graphs.
    async fn delete_graph(&self, graph_id: &Uuid) -> Result<()> {
        // Delete through registry
        let mut registry = self.graph_registry.write_or_panic("delete graph").await;
        registry.remove_graph(graph_id, false).await
            .map_err(|e| GraphError::lifecycle(format!("Failed to delete graph: {}", e)))?;
        
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Operation, wal::{Transaction, TransactionState}};
    
    #[test]
    fn test_error_types() {
        // Test GraphError creation
        let graph_err = GraphError::lifecycle("Test error");
        assert_eq!(graph_err.to_string(), "Graph lifecycle error: Test error");
        
        // Test NodeNotFound creation
        let graph_id = uuid::Uuid::new_v4();
        let node_err = GraphError::node_not_found("block-123", graph_id);
        assert_eq!(node_err.to_string(), format!("Node not found: block-123 in graph {}", graph_id));
    }
    
    #[test]
    fn test_operation_variants() {
        // Test that all GraphOperation variants can be created through Operation enum
        let agent_id = uuid::Uuid::new_v4();
        let graph_id = uuid::Uuid::new_v4();
        
        let create_block = Operation::Graph(GraphOperation::CreateBlock {
            graph_id,
            agent_id,
            content: "Test content".to_string(),
            parent_id: Some("parent-123".to_string()),
            page_name: Some("TestPage".to_string()),
            properties: Some(json!({"key": "value"})),
        });
        assert!(matches!(create_block, Operation::Graph(GraphOperation::CreateBlock { .. })));
        
        let update_block = Operation::Graph(GraphOperation::UpdateBlock {
            graph_id,
            agent_id,
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
        });
        assert!(matches!(update_block, Operation::Graph(GraphOperation::UpdateBlock { .. })));
        
        let delete_block = Operation::Graph(GraphOperation::DeleteBlock {
            graph_id,
            agent_id,
            block_id: "block-123".to_string(),
        });
        assert!(matches!(delete_block, Operation::Graph(GraphOperation::DeleteBlock { .. })));
        
        let create_page = Operation::Graph(GraphOperation::CreatePage {
            graph_id,
            agent_id,
            page_name: "NewPage".to_string(),
            properties: Some(json!({"type": "journal"})),
        });
        assert!(matches!(create_page, Operation::Graph(GraphOperation::CreatePage { .. })));
        
        let delete_page = Operation::Graph(GraphOperation::DeletePage {
            graph_id,
            agent_id,
            page_name: "OldPage".to_string(),
        });
        assert!(matches!(delete_page, Operation::Graph(GraphOperation::DeletePage { .. })));
    }
    
    #[test]
    fn test_transaction_creation() {
        // Test that transactions are created properly for operations
        let agent_id = uuid::Uuid::new_v4();
        let graph_id = uuid::Uuid::new_v4();
        let operation = Operation::Graph(GraphOperation::CreateBlock {
            graph_id,
            agent_id,
            content: "Test block".to_string(),
            parent_id: None,
            page_name: Some("TestPage".to_string()),
            properties: None,
        });
        
        let transaction = Transaction::new(operation.clone());
        
        // Verify transaction fields
        assert!(!transaction.id.is_empty());
        assert!(matches!(transaction.operation, Operation::Graph(GraphOperation::CreateBlock { .. })));
        assert_eq!(transaction.state, TransactionState::Active);
        assert!(transaction.content_hash.is_some()); // CreateBlock should have content hash
        assert!(transaction.error_message.is_none());
        
        // Test operation without content hash
        let delete_op = Operation::Graph(GraphOperation::DeleteBlock {
            graph_id,
            agent_id,
            block_id: "block-456".to_string(),
        });
        let delete_tx = Transaction::new(delete_op);
        assert!(delete_tx.content_hash.is_none()); // DeleteBlock should not have content hash
    }
    
    
    #[test]
    fn test_result_type_conversions() {
        // Test that our Result type works with error conversions
        fn returns_graph_error() -> Result<String> {
            Err(GraphError::lifecycle("Something went wrong").into())
        }
        
        fn returns_node_not_found() -> Result<String> {
            let graph_id = uuid::Uuid::new_v4();
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
    
    #[test]
    fn test_json_serialization_for_operations() {
        // Test that operations can be serialized/deserialized for transaction log
        let agent_id = uuid::Uuid::new_v4();
        let graph_id = uuid::Uuid::new_v4();
        let op = Operation::Graph(GraphOperation::CreateBlock {
            graph_id,
            agent_id,
            content: "Test content with «reference»".to_string(),
            parent_id: Some("parent-id".to_string()),
            page_name: Some("Daily Note".to_string()),
            properties: Some(json!({
                "tags": ["important", "review"],
                "priority": "high"
            })),
        });
        
        // Serialize
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("CreateBlock"));
        assert!(json.contains("Test content with «reference»"));
        
        // Deserialize
        let deserialized: Operation = serde_json::from_str(&json).unwrap();
        match deserialized {
            Operation::Graph(GraphOperation::CreateBlock { agent_id: _, content, parent_id, page_name, properties, .. }) => {
                assert_eq!(content, "Test content with «reference»");
                assert_eq!(parent_id, Some("parent-id".to_string()));
                assert_eq!(page_name, Some("Daily Note".to_string()));
                assert!(properties.is_some());
            }
            _ => panic!("Wrong operation type after deserialization"),
        }
    }
    
    #[test]
    fn test_authorization_error_formatting() {
        // Test that authorization error messages contain expected information
        let agent_id = uuid::Uuid::new_v4();
        let graph_id = uuid::Uuid::new_v4();
        
        let error = GraphError::invalid_state(format!(
            "Agent {} is not authorized for graph {} - authorization required for all graph operations", 
            agent_id, graph_id
        ));
        
        let error_message = error.to_string();
        assert!(error_message.contains("not authorized"));
        assert!(error_message.contains(&agent_id.to_string()));
        assert!(error_message.contains(&graph_id.to_string()));
        assert!(error_message.contains("authorization required"));
    }
}