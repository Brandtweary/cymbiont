/**
 * Graph Operations Module - Agent-Aware PKM Operations
 * 
 * Provides PKM-specific graph operations with runtime agent authorization.
 * All operations require an agent ID and verify authorization before execution.
 * 
 * ## Design Pattern: Single Trait API
 * 
 * The `GraphOps` trait provides all graph operations with agent awareness:
 * ```rust
 * use graph_operations::GraphOps;
 * 
 * // Operations require agent_id for authorization
 * app_state.add_block(agent_id, content, parent_id, page_name, properties, &graph_id).await?;
 * ```
 * 
 * This pattern provides several benefits:
 * - Runtime authorization checks for security
 * - Single source of truth for all graph operations
 * - Clear agent accountability for all changes
 * - Clean integration with transaction system
 * 
 * ## Core Operations
 * 
 * ### Block Operations
 * - `add_block()` - Create new PKM block with content, parent, and properties
 * - `update_block()` - Modify block content with automatic reference resolution
 * - `delete_block()` - Archive block node while preserving data
 * 
 * ### Page Operations  
 * - `create_page()` - Create new PKM page with normalized name handling
 * - `delete_page()` - Archive page node (handles normalized names)
 * 
 * ### Graph Management
 * - `create_graph()` - Initialize new knowledge graph with registry entry
 * - `delete_graph()` - Archive entire graph (can delete both open and closed)
 * - `open_graph()` - Load graph into RAM and trigger recovery
 * - `close_graph()` - Save graph and unload from RAM
 * - `list_graphs()` - Enumerate all registered graphs
 * - `list_open_graphs()` - List currently open graphs
 * 
 * ### Query Operations
 * - `get_node()` - Retrieve node by ID with PKM-aware formatting
 * - `query_graph_bfs()` - Breadth-first traversal (TODO)
 * 
 * ### Recovery Operations
 * - `replay_transaction()` - Replay a stored operation during crash recovery
 * 
 * ## Transaction Integration
 * 
 * All mutation operations are automatically wrapped in transactions:
 * 1. Operation parameters are stored in WAL before execution
 * 2. PKM transformations are applied within transaction boundary
 * 3. Success/failure updates transaction state
 * 4. Crash recovery replays operations with exact parameters
 * 
 * The Operation enum stores full API parameters:
 * ```rust
 * Operation::CreateBlock {
 *     content: String,
 *     parent_id: Option<String>,
 *     page_name: Option<String>,
 *     properties: Option<serde_json::Value>,
 * }
 * ```
 * 
 * ## Crash Recovery
 * 
 * Recovery happens at two points:
 * 1. **Startup** (main.rs): Runs `run_all_graphs_recovery()` for ALL graphs (both open and closed)
 * 2. **Graph Open**: Each `open_graph()` call triggers recovery for that specific graph
 * 
 * The recovery process:
 * - Startup: Iterates all graphs, temporarily opens closed ones for recovery
 * - Finds all Active transactions in each graph's WAL
 * - Calls `OperationExecutor::execute_operation()` for each pending transaction
 * - Updates transaction state based on result
 * - No PKM reconstruction needed - exact API replay
 * - Closed graphs are closed again after recovery
 * 
 * ## OperationExecutor Trait Implementation
 * 
 * Arc<AppState> implements the `OperationExecutor` trait from the storage layer.
 * This enables the transaction system to execute operations without knowing their
 * implementation details:
 * 
 * ```rust
 * // Storage layer defines the trait
 * pub trait OperationExecutor {
 *     async fn execute_operation(&self, operation: Operation) -> Result<(), String>;
 * }
 * 
 * // Graph operations module implements it
 * impl OperationExecutor for Arc<AppState> {
 *     async fn execute_operation(&self, operation: Operation) -> Result<(), String> {
 *         match operation {
 *             Operation::CreateBlock { .. } => self.add_block(...),
 *             // ... other operations
 *         }
 *     }
 * }
 * ```
 * 
 * ## Adding New Graph Operations
 * 
 * When adding new operations, follow these steps:
 * 
 * 1. **Define the Operation variant** in `storage/transaction_log.rs`:
 *    - Add new variant to `Operation` enum with agent_id and all parameters
 *    - Include all data needed to replay the operation during recovery
 * 
 * 2. **Add the trait method** to `GraphOps` trait in this file:
 *    - Include agent_id: Uuid as first parameter
 *    - Add graph_id: &Uuid parameter for graph targeting
 *    - Return appropriate Result<T> type
 * 
 * 3. **Implement the operation** in `impl GraphOps for Arc<AppState>`:
 *    - Start with authorization check using agent_registry
 *    - Create Operation enum with parameters for transaction log
 *    - Execute within with_graph_transaction() for ACID guarantees
 *    - Handle errors appropriately (GraphError vs NodeNotFound)
 * 
 * 4. **Add to OperationExecutor** implementation at bottom of this file:
 *    - Add match arm that calls the GraphOps method
 *    - Map operation parameters to method parameters
 *    - Convert errors to String for transaction system
 * 
 * 5. **Register in tool registry** (optional) in `agent/kg_tools.rs`:
 *    - Add tool registration in appropriate category
 *    - Parse parameters from JSON args
 *    - Call GraphOps method with agent_id and parsed params
 * 
 * 6. **Add WebSocket command** (optional) in `server/websocket.rs`:
 *    - Define command variant in Command enum
 *    - Add handler that extracts current_agent_id
 *    - Call GraphOps method and return success/error response
 * 
 * ## Error Handling
 * 
 * Operations return `Result<T, GraphOperationError>` with two error variants:
 * - `GraphError` - General graph operation failures
 * - `NodeNotFound` - Specific node lookup failures
 */

use crate::{
    AppState,
    import::pkm_data::{PKMBlockData, PKMPageData},
    storage::{Operation, OperationExecutor, AgentRegistry},
};
use std::sync::Arc;
use tracing::{warn, error, info};
use thiserror::Error;
use serde_json::json;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum GraphOperationError {
    #[error("Graph error: {0}")]
    GraphError(String),
    
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

pub type Result<T> = std::result::Result<T, GraphOperationError>;

/// Helper to verify agent authorization for a graph operation.
/// Returns an error if the agent is not authorized.
fn verify_authorization(
    agent_registry: &AgentRegistry,
    agent_id: &Uuid,
    graph_id: &Uuid,
) -> Result<()> {
    if !agent_registry.is_agent_authorized(agent_id, graph_id) {
        Err(GraphOperationError::GraphError(format!(
            "Agent {} is not authorized for graph {} - authorization required for all graph operations",
            agent_id, graph_id
        )))
    } else {
        Ok(())
    }
}

/// Helper to check authorization for a graph operation.
/// Encapsulates the full registry read/verify/drop pattern.
fn check_authorization(
    agent_registry: &Arc<std::sync::RwLock<AgentRegistry>>,
    agent_id: &Uuid,
    graph_id: &Uuid,
) -> Result<()> {
    let registry = agent_registry.read()
        .map_err(|_| GraphOperationError::GraphError("Failed to read agent registry".into()))?;
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
    ) -> Result<String>;

    /// Update block with agent authorization
    async fn update_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        content: String,
        graph_id: &Uuid,
    ) -> Result<()>;

    /// Delete block with agent authorization
    async fn delete_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        graph_id: &Uuid,
    ) -> Result<()>;

    /// Create page with agent authorization
    async fn create_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<()>;

    /// Delete page with agent authorization
    async fn delete_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        graph_id: &Uuid,
    ) -> Result<()>;
    
    /// Get a node by ID with agent authorization
    async fn get_node(&self, agent_id: Uuid, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value>;
    
    /// Query graph with BFS traversal with agent authorization
    fn query_graph_bfs(&self, agent_id: Uuid, start_id: &str, max_depth: usize, graph_id: &Uuid) -> Result<Vec<serde_json::Value>>;
    
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
    ) -> Result<String> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // Create the operation with full API parameters
        let operation = Operation::CreateBlock {
            agent_id,
            content: content.clone(),
            parent_id: parent_id.clone(),
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction on specific graph
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Create the block data using factory method
            let block_data = PKMBlockData::new_block(content, parent_id, page_name, properties);
            let block_id = block_data.id.clone();
            
            // Add to graph
            block_data.apply_to_graph(graph_manager)
                .map(|_| block_id)
                .map_err(|e| e.to_string())
        }).await
        .map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    async fn update_block(
        &self,
        agent_id: Uuid,
        block_id: String,
        content: String,
        graph_id: &Uuid,
    ) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // Create transaction with full API parameters
        let operation = Operation::UpdateBlock {
            agent_id,
            block_id: block_id.clone(),
            content: content.clone(),
        };
        
        // Execute with transaction on specific graph  
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Delegate to PKMBlockData helper for complex block update logic
            PKMBlockData::update_block_content(&block_id, content, graph_manager)
                .map_err(|e| e.to_string())
        }).await
        .map_err(|e| {
            if e.to_string().contains("Node not found") {
                GraphOperationError::NodeNotFound(block_id)
            } else {
                GraphOperationError::GraphError(e.to_string())
            }
        })
    }
    
    async fn delete_block(&self, agent_id: Uuid, block_id: String, graph_id: &Uuid) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // Create transaction with full API parameters
        let operation = Operation::DeleteBlock {
            agent_id,
            block_id: block_id.clone(),
        };
        
        // Execute with transaction on specific graph
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            if let Some(node_idx) = graph_manager.find_node(&block_id) {
                // Archive the node
                graph_manager.archive_nodes(vec![(block_id.clone(), node_idx)])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                Err(format!("Node not found: {}", block_id))
            }
        }).await
        .map_err(|e| {
            if e.to_string().contains("Node not found") {
                GraphOperationError::NodeNotFound(block_id)
            } else {
                GraphOperationError::GraphError(e.to_string())
            }
        })
    }
    
    async fn create_page(
        &self,
        agent_id: Uuid,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // Create transaction with full API parameters
        let operation = Operation::CreatePage {
            agent_id,
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction on specific graph
        let result = self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Delegate to PKMPageData helper for complex page creation logic
            PKMPageData::create_or_update_page(page_name, properties, graph_manager)
                .map_err(|e| e.to_string())
        }).await;
        
        result.map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    async fn delete_page(&self, agent_id: Uuid, page_name: String, graph_id: &Uuid) -> Result<()> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // Create transaction with full API parameters
        let operation = Operation::DeletePage {
            agent_id,
            page_name: page_name.clone(),
        };
        
        // Execute with transaction on specific graph
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Pages are stored with normalized names as keys
            let normalized_name = page_name.to_lowercase();
            
            // Try both the original name and normalized name
            let node_idx = graph_manager.find_node(&page_name)
                .or_else(|| graph_manager.find_node(&normalized_name));
                
            if let Some(node_idx) = node_idx {
                // Archive the node
                graph_manager.archive_nodes(vec![(normalized_name, node_idx)])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                Err(format!("Page not found: {}", page_name))
            }
        }).await
        .map_err(|e| {
            if e.to_string().contains("Page not found") {
                GraphOperationError::NodeNotFound(page_name)
            } else {
                GraphOperationError::GraphError(e.to_string())
            }
        })
    }
    
    async fn get_node(&self, agent_id: Uuid, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        let resources = self.graph_resources.read().await;
        let graph_resources = resources.get(graph_id)
            .ok_or_else(|| GraphOperationError::GraphError("Graph not found".to_string()))?;
        let graph_manager = graph_resources.manager.read().await;
        
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
                Err(GraphOperationError::NodeNotFound(node_id.to_string()))
            }
        } else {
            Err(GraphOperationError::NodeNotFound(node_id.to_string()))
        }
    }
    
    /// Query graph with BFS traversal
    fn query_graph_bfs(
        &self,
        agent_id: Uuid,
        _start_id: &str,
        _max_depth: usize,
        graph_id: &Uuid,
    ) -> Result<Vec<serde_json::Value>> {
        check_authorization(&self.agent_registry, &agent_id, graph_id)?;
        
        // TODO: Implement BFS traversal in graph_manager
        // For now, return empty result
        warn!("BFS traversal not yet implemented");
        Ok(vec![])
    }
    
    /// Open a graph (load into RAM and trigger recovery)
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value> {
        info!("📂 Opening graph: {}", graph_id);
        
        // Open the graph (handles loading and registry update)
        AppState::open_graph(self, &graph_id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to open graph: {}", e)))?;
        
        // Run recovery on the newly opened graph
        match self.run_graph_recovery(&graph_id).await {
            Ok(count) if count > 0 => {
                info!("✅ Successfully replayed {} transactions for graph {}", count, graph_id);
            }
            Err(e) => {
                error!("❌ Failed to recover transactions for {}: {}", graph_id, e);
                return Err(GraphOperationError::GraphError(format!("Transaction recovery failed: {}", e)));
            }
            _ => {} // No pending transactions
        }
        
        // Get graph info from registry
        let graph_info = {
            let registry = self.graph_registry.read()
                .map_err(|_| GraphOperationError::GraphError("Failed to read registry".into()))?;
            registry.get_graph(&graph_id)
                .ok_or_else(|| GraphOperationError::GraphError(format!("Graph '{}' not found", graph_id)))?
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
        // Close the graph (handles saving and registry update)
        AppState::close_graph(self, &graph_id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to close graph: {}", e)))?;
        
        Ok(())
    }
    
    /// List all open graphs
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>> {
        let registry = self.graph_registry.read()
            .map_err(|_| GraphOperationError::GraphError("Failed to read registry".into()))?;
        Ok(registry.get_open_graphs())
    }
    
    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        if let Ok(registry) = self.graph_registry.read() {
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
        } else {
            Ok(vec![])
        }
    }
    
    /// Create a new knowledge graph
    async fn create_graph(
        &self, 
        name: Option<String>, 
        description: Option<String>
    ) -> Result<serde_json::Value> {
        // Delegate to AppState which handles the complete workflow
        let graph_info = AppState::create_new_graph(self, name, description).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to create graph: {}", e)))?;
        
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
        // Delegate to AppState which handles the complete workflow
        AppState::delete_graph_completely(self, graph_id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to delete graph: {}", e)))?;
        
        Ok(())
    }
}


// Implement OperationExecutor trait for Arc<AppState>
// This allows the storage layer to execute operations during recovery
#[async_trait]
impl OperationExecutor for Arc<AppState> {
    async fn execute_operation(&self, graph_id: &Uuid, operation: Operation) -> std::result::Result<(), String> {
        match operation {
            Operation::CreateBlock { agent_id, content, parent_id, page_name, properties } => {
                GraphOps::add_block(self, agent_id, content, parent_id, page_name, properties, graph_id)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            Operation::UpdateBlock { agent_id, block_id, content } => {
                GraphOps::update_block(self, agent_id, block_id, content, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeleteBlock { agent_id, block_id } => {
                GraphOps::delete_block(self, agent_id, block_id, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::CreatePage { agent_id, page_name, properties } => {
                GraphOps::create_page(self, agent_id, page_name, properties, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeletePage { agent_id, page_name } => {
                GraphOps::delete_page(self, agent_id, page_name, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Operation, transaction_log::Transaction};
    use crate::storage::transaction_log::TransactionState;
    
    #[test]
    fn test_error_types() {
        // Test GraphError creation
        let graph_err = GraphOperationError::GraphError("Test error".to_string());
        assert_eq!(graph_err.to_string(), "Graph error: Test error");
        
        // Test NodeNotFound creation
        let node_err = GraphOperationError::NodeNotFound("block-123".to_string());
        assert_eq!(node_err.to_string(), "Node not found: block-123");
    }
    
    #[test]
    fn test_operation_variants() {
        // Test that all Operation variants can be created
        let agent_id = uuid::Uuid::new_v4();
        let create_block = Operation::CreateBlock {
            agent_id,
            content: "Test content".to_string(),
            parent_id: Some("parent-123".to_string()),
            page_name: Some("TestPage".to_string()),
            properties: Some(json!({"key": "value"})),
        };
        assert!(matches!(create_block, Operation::CreateBlock { .. }));
        
        let update_block = Operation::UpdateBlock {
            agent_id,
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
        };
        assert!(matches!(update_block, Operation::UpdateBlock { .. }));
        
        let delete_block = Operation::DeleteBlock {
            agent_id,
            block_id: "block-123".to_string(),
        };
        assert!(matches!(delete_block, Operation::DeleteBlock { .. }));
        
        let create_page = Operation::CreatePage {
            agent_id,
            page_name: "NewPage".to_string(),
            properties: Some(json!({"type": "journal"})),
        };
        assert!(matches!(create_page, Operation::CreatePage { .. }));
        
        let delete_page = Operation::DeletePage {
            agent_id,
            page_name: "OldPage".to_string(),
        };
        assert!(matches!(delete_page, Operation::DeletePage { .. }));
    }
    
    #[test]
    fn test_transaction_creation() {
        // Test that transactions are created properly for operations
        let agent_id = uuid::Uuid::new_v4();
        let operation = Operation::CreateBlock {
            agent_id,
            content: "Test block".to_string(),
            parent_id: None,
            page_name: Some("TestPage".to_string()),
            properties: None,
        };
        
        let transaction = Transaction::new(operation.clone());
        
        // Verify transaction fields
        assert!(!transaction.id.is_empty());
        assert!(matches!(transaction.operation, Operation::CreateBlock { .. }));
        assert_eq!(transaction.state, TransactionState::Active);
        assert!(transaction.content_hash.is_some()); // CreateBlock should have content hash
        assert!(transaction.error_message.is_none());
        
        // Test operation without content hash
        let delete_op = Operation::DeleteBlock {
            agent_id,
            block_id: "block-456".to_string(),
        };
        let delete_tx = Transaction::new(delete_op);
        assert!(delete_tx.content_hash.is_none()); // DeleteBlock should not have content hash
    }
    
    
    #[test]
    fn test_result_type_conversions() {
        // Test that our Result type works with error conversions
        fn returns_graph_error() -> Result<String> {
            Err(GraphOperationError::GraphError("Something went wrong".to_string()))
        }
        
        fn returns_node_not_found() -> Result<String> {
            Err(GraphOperationError::NodeNotFound("node-123".to_string()))
        }
        
        // Test error matching
        match returns_graph_error() {
            Err(GraphOperationError::GraphError(msg)) => {
                assert_eq!(msg, "Something went wrong");
            }
            _ => panic!("Expected GraphError"),
        }
        
        match returns_node_not_found() {
            Err(GraphOperationError::NodeNotFound(id)) => {
                assert_eq!(id, "node-123");
            }
            _ => panic!("Expected NodeNotFound"),
        }
    }
    
    #[test]
    fn test_json_serialization_for_operations() {
        // Test that operations can be serialized/deserialized for transaction log
        let agent_id = uuid::Uuid::new_v4();
        let op = Operation::CreateBlock {
            agent_id,
            content: "Test content with «reference»".to_string(),
            parent_id: Some("parent-id".to_string()),
            page_name: Some("Daily Note".to_string()),
            properties: Some(json!({
                "tags": ["important", "review"],
                "priority": "high"
            })),
        };
        
        // Serialize
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("CreateBlock"));
        assert!(json.contains("Test content with «reference»"));
        
        // Deserialize
        let deserialized: Operation = serde_json::from_str(&json).unwrap();
        match deserialized {
            Operation::CreateBlock { agent_id: _, content, parent_id, page_name, properties } => {
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
        
        let error = GraphOperationError::GraphError(format!(
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
