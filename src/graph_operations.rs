#![allow(dead_code)] // TODO: Remove when integrated with aichat-agent

/**
 * Graph Operations Module - PKM Operations Extension Trait
 * 
 * Provides PKM-specific graph operations as extension methods on Arc<AppState>.
 * This design eliminates the confusion of a separate service object while keeping
 * domain-specific logic cleanly separated from core state management.
 * 
 * ## Design Pattern: Extension Trait
 * 
 * The `GraphOperationsExt` trait extends Arc<AppState> with PKM operations:
 * ```rust
 * use graph_operations::GraphOperationsExt;
 * 
 * // Operations appear directly on AppState
 * app_state.add_block(content, parent_id, page_name, properties).await?;
 * ```
 * 
 * This pattern provides several benefits:
 * - No artificial service object creation
 * - Clear that these are PKM-specific extensions
 * - Natural integration with AppState's coordination role
 * - Stateless operations with no overhead
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
 * - Calls `replay_transaction()` for each pending transaction
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
 * When adding new operations:
 * 1. Add variant to `Operation` enum in storage/transaction_log.rs
 * 2. Add case to `OperationExecutor` implementation below
 * 3. Implement the actual operation in `GraphOperationsExt`
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
    import::logseq::extract_references,
    storage::{Operation, OperationExecutor},
};
use std::sync::Arc;
use std::collections::HashMap;
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

/// Extension trait for PKM-specific graph operations on AppState
pub trait GraphOperationsExt {
    /// Add a new block to the knowledge graph
    async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<String>;
    
    /// Update an existing block
    async fn update_block(&self, block_id: String, content: String, graph_id: &Uuid) -> Result<()>;
    
    /// Delete a block
    async fn delete_block(&self, block_id: String, graph_id: &Uuid) -> Result<()>;
    
    /// Create a new page
    async fn create_page(&self, page_name: String, properties: Option<serde_json::Value>, graph_id: &Uuid) -> Result<()>;
    
    /// Delete a page
    async fn delete_page(&self, page_name: String, graph_id: &Uuid) -> Result<()>;
    
    /// Get a node by ID
    async fn get_node(&self, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value>;
    
    /// Query graph with BFS traversal
    fn query_graph_bfs(&self, start_id: &str, max_depth: usize, graph_id: &Uuid) -> Result<Vec<serde_json::Value>>;
    
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
    
    /// Replay a transaction during recovery with proper state management
    async fn replay_transaction(&self, graph_id: &Uuid, transaction: crate::storage::Transaction, coordinator: Arc<crate::storage::TransactionCoordinator>) -> Result<()>;
}

impl GraphOperationsExt for Arc<AppState> {
    async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<String> {
        
        // Generate a proper UUID for this block
        let block_id = uuid::Uuid::new_v4().to_string();
        
        // Create the operation with full API parameters
        let operation = Operation::CreateBlock {
            content: content.clone(),
            parent_id: parent_id.clone(),
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction on specific graph
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Create the block data
            let block_data = PKMBlockData {
                id: block_id.clone(),
                content: content.clone(),
                properties: properties.unwrap_or(json!({})),
                parent: parent_id.clone(),
                page: page_name.clone(),
                references: extract_references(&content),
                children: vec![], // New blocks have no children initially
                created: chrono::Utc::now().to_rfc3339(),
                updated: chrono::Utc::now().to_rfc3339(),
                reference_content: None, // Let apply_to_graph handle resolution
            };
            
            // Add to graph
            block_data.apply_to_graph(graph_manager)
                .map(|_| block_id.clone())
                .map_err(|e| e.to_string())
        }).await
        .map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    /// Update an existing block in both graph and via WebSocket
    async fn update_block(
        &self,
        block_id: String,
        content: String,
        graph_id: &Uuid,
    ) -> Result<()> {
        
        // Create transaction with full API parameters
        let operation = Operation::UpdateBlock {
            block_id: block_id.clone(),
            content: content.clone(),
        };
        
        // Execute with transaction on specific graph
        self.with_graph_transaction(graph_id, operation, |graph_manager| {
            // Find the node
            if let Some(node_idx) = graph_manager.find_node(&block_id) {
                // Get existing node data to preserve all fields
                if let Some(existing_node) = graph_manager.get_node(node_idx) {
                    // Clone existing data and update only what we need
                    let mut node_data = existing_node.clone();
                    
                    // Update content and timestamp
                    node_data.content = content.clone();
                    node_data.updated_at = chrono::Utc::now();
                    
                    // Resolve references if content changed
                    if existing_node.content != content {
                        // Build block map for reference resolution
                        let mut block_map = HashMap::new();
                        for idx in graph_manager.graph.node_indices() {
                            if let Some(node) = graph_manager.graph.node_weight(idx) {
                                if matches!(node.node_type, crate::graph_manager::NodeType::Block) {
                                    block_map.insert(node.pkm_id.clone(), node.content.clone());
                                }
                            }
                        }
                        
                        // Resolve references in the new content
                        let mut visited = std::collections::HashSet::new();
                        node_data.reference_content = Some(
                            crate::import::reference_resolver::resolve_block_references(
                                &content, 
                                &block_map, 
                                &mut visited, 
                                Some(&block_id)
                            )
                        );
                    }
                    
                    // Use create_or_update_node to update the node directly
                    graph_manager.create_or_update_node(
                        node_data.pkm_id,
                        node_data.id,
                        node_data.node_type,
                        node_data.content,
                        node_data.reference_content,
                        node_data.properties,
                        node_data.created_at,
                        node_data.updated_at,
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                } else {
                    Err(format!("Node not found: {}", block_id))
                }
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
    
    /// Delete a block from both graph and via WebSocket
    async fn delete_block(&self, block_id: String, graph_id: &Uuid) -> Result<()> {
        
        // Create transaction with full API parameters
        let operation = Operation::DeleteBlock {
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
    
    /// Create a new page in both graph and via WebSocket
    async fn create_page(
        &self,
        page_name: String,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<()> {
        
        // Create transaction with full API parameters
        let operation = Operation::CreatePage {
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction on specific graph
        let result = self.with_graph_transaction(graph_id, operation, |graph_manager| {
            
            let normalized_name = page_name.to_lowercase();
            
            // Check if page already exists
            if let Some(node_idx) = graph_manager.find_node(&page_name)
                .or_else(|| graph_manager.find_node(&normalized_name)) {
                
                // Page exists - just update properties if provided
                if let Some(existing_node) = graph_manager.get_node(node_idx) {
                    if properties.is_some() {
                        // Update only properties and timestamp
                        let mut node_data = existing_node.clone();
                        node_data.properties = crate::utils::parse_properties(&properties.unwrap_or(json!({})));
                        node_data.updated_at = chrono::Utc::now();
                        
                        graph_manager.create_or_update_node(
                            node_data.pkm_id,
                            node_data.id,
                            node_data.node_type,
                            node_data.content,
                            node_data.reference_content,
                            node_data.properties,
                            node_data.created_at,
                            node_data.updated_at,
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                    } else {
                        // Page exists and no properties to update
                        Ok(())
                    }
                } else {
                    Err("Failed to get existing page node".to_string())
                }
            } else {
                // Page doesn't exist - create it
                let page_data = PKMPageData {
                    name: page_name.clone(),
                    normalized_name: Some(normalized_name),
                    properties: properties.clone().unwrap_or(json!({})),
                    created: chrono::Utc::now().to_rfc3339(),
                    updated: chrono::Utc::now().to_rfc3339(),
                    blocks: vec![],
                };
                
                // Add to graph
                page_data.apply_to_graph(graph_manager)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }).await;
        
        result.map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    /// Delete a page from both graph and via WebSocket
    async fn delete_page(&self, page_name: String, graph_id: &Uuid) -> Result<()> {
        
        // Create transaction with full API parameters
        let operation = Operation::DeletePage {
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
    
    async fn get_node(&self, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value> {
        
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
        _start_id: &str,
        _max_depth: usize,
        _graph_id: &Uuid,
    ) -> Result<Vec<serde_json::Value>> {
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
        if let Some(coordinator) = self.get_transaction_coordinator(&graph_id).await {
            match coordinator.recover_pending_transactions().await {
                Ok(pending_transactions) => {
                    if !pending_transactions.is_empty() {
                        info!("🔄 Replaying {} pending transactions for graph {}", 
                              pending_transactions.len(), graph_id);
                        
                        // Replay each transaction with proper state updates
                        for transaction in pending_transactions {
                            if let Err(e) = self.replay_transaction(&graph_id, transaction, coordinator.clone()).await {
                                error!("Failed to replay transaction: {}", e);
                            }
                        }
                        
                        // Save the graph after recovery to ensure replayed changes are persisted
                        let resources = self.graph_resources.read().await;
                        if let Some(graph_resources) = resources.get(&graph_id) {
                            match graph_resources.manager.write().await.save_graph() {
                                Ok(_) => info!("💾 Saved graph {} after recovery", graph_id),
                                Err(e) => error!("Failed to save graph {} after recovery: {}", graph_id, e),
                            }
                        }
                        drop(resources);
                    }
                }
                Err(e) => {
                    error!("❌ Failed to recover transactions for {}: {}", graph_id, e);
                    return Err(GraphOperationError::GraphError(format!("Transaction recovery failed: {}", e)));
                }
            }
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
        // Register the graph in the registry
        let graph_info = {
            // Debug assertion to fail fast if another thread holds the write lock
            debug_assert!(
                self.graph_registry.try_write().is_ok(),
                "Registry write lock unavailable - another thread may be holding it"
            );
            
            let mut registry = self.graph_registry.write()
                .map_err(|_| GraphOperationError::GraphError("Failed to write registry".into()))?;
            
            registry.register_graph(None, name, description, &self.data_dir)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to register graph: {}", e)))?
        };
        
        // Save registry after creating new graph
        {
            let registry = self.graph_registry.read()
                .map_err(|_| GraphOperationError::GraphError("Failed to read registry".into()))?;
            
            registry.save()
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Create graph manager for the new graph
        self.get_or_create_graph_manager(&graph_info.id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to create graph manager: {}", e)))?;
        
        info!("Created new knowledge graph: {} ({})", graph_info.name, graph_info.id);
        
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
        
        // Remove from registry (this also archives the data)
        {
            // Debug assertion to fail fast if another thread holds the write lock
            debug_assert!(
                self.graph_registry.try_write().is_ok(),
                "Registry write lock unavailable - another thread may be holding it"
            );
            
            let mut registry = self.graph_registry.write()
                .map_err(|_| GraphOperationError::GraphError("Failed to write registry".into()))?;
            
            registry.remove_graph(graph_id)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to remove graph: {}", e)))?;
            
            // Save registry
            registry.save()
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Remove from resources (manager + coordinator bundled together)
        {
            let mut resources = self.graph_resources.write().await;
            resources.remove(graph_id);
        }
        
        Ok(())
    }
    
    /// Replay a transaction during recovery
    /// This handles the complete transaction lifecycle: execution and state update
    async fn replay_transaction(&self, graph_id: &Uuid, transaction: crate::storage::Transaction, coordinator: Arc<crate::storage::TransactionCoordinator>) -> Result<()> {
        let tx_id = transaction.id.clone();
        let operation = transaction.operation.clone();
        
        // Execute the operation using the OperationExecutor trait with graph context
        let result = OperationExecutor::execute_operation(self, graph_id, operation).await;
        
        // Update transaction state based on result
        match result {
            Ok(()) => {
                coordinator.commit_transaction(&tx_id).await
                    .map_err(|e| GraphOperationError::GraphError(e.to_string()))?;
            }
            Err(e) => {
                coordinator.abort_transaction(&tx_id, &e).await
                    .map_err(|e| GraphOperationError::GraphError(e.to_string()))?;
                error!("❌ Failed to replay transaction {}: {}", tx_id, e);
            }
        }
        
        Ok(())
    }
}

// Implement OperationExecutor trait for Arc<AppState>
// This allows the storage layer to execute operations without knowing about GraphOperationsExt
#[async_trait]
impl OperationExecutor for Arc<AppState> {
    async fn execute_operation(&self, graph_id: &Uuid, operation: Operation) -> std::result::Result<(), String> {
        match operation {
            Operation::CreateBlock { content, parent_id, page_name, properties } => {
                GraphOperationsExt::add_block(self, content, parent_id, page_name, properties, graph_id)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            Operation::UpdateBlock { block_id, content } => {
                GraphOperationsExt::update_block(self, block_id, content, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeleteBlock { block_id } => {
                GraphOperationsExt::delete_block(self, block_id, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::CreatePage { page_name, properties } => {
                GraphOperationsExt::create_page(self, page_name, properties, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeletePage { page_name } => {
                GraphOperationsExt::delete_page(self, page_name, graph_id)
                    .await
                    .map_err(|e| e.to_string())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Operation, Transaction};
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
        let create_block = Operation::CreateBlock {
            content: "Test content".to_string(),
            parent_id: Some("parent-123".to_string()),
            page_name: Some("TestPage".to_string()),
            properties: Some(json!({"key": "value"})),
        };
        assert!(matches!(create_block, Operation::CreateBlock { .. }));
        
        let update_block = Operation::UpdateBlock {
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
        };
        assert!(matches!(update_block, Operation::UpdateBlock { .. }));
        
        let delete_block = Operation::DeleteBlock {
            block_id: "block-123".to_string(),
        };
        assert!(matches!(delete_block, Operation::DeleteBlock { .. }));
        
        let create_page = Operation::CreatePage {
            page_name: "NewPage".to_string(),
            properties: Some(json!({"type": "journal"})),
        };
        assert!(matches!(create_page, Operation::CreatePage { .. }));
        
        let delete_page = Operation::DeletePage {
            page_name: "OldPage".to_string(),
        };
        assert!(matches!(delete_page, Operation::DeletePage { .. }));
    }
    
    #[test]
    fn test_transaction_creation() {
        // Test that transactions are created properly for operations
        let operation = Operation::CreateBlock {
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
        let op = Operation::CreateBlock {
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
            Operation::CreateBlock { content, parent_id, page_name, properties } => {
                assert_eq!(content, "Test content with «reference»");
                assert_eq!(parent_id, Some("parent-id".to_string()));
                assert_eq!(page_name, Some("Daily Note".to_string()));
                assert!(properties.is_some());
            }
            _ => panic!("Wrong operation type after deserialization"),
        }
    }
}

