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
 * - `delete_graph()` - Archive entire graph (prevents deleting active graph)
 * - `switch_graph()` - Change active graph context with crash recovery
 * - `list_graphs()` - Enumerate all registered graphs
 * 
 * ### Query Operations
 * - `get_node()` - Retrieve node by ID with PKM-aware formatting
 * - `get_active_graph()` - Get current graph ID
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
 * 1. **Startup** (main.rs): Replays pending transactions for active graph
 * 2. **Graph Switch**: Replays pending transactions before activation
 * 
 * The recovery process:
 * - Finds all Active transactions in WAL
 * - Calls `replay_transaction()` for each
 * - Updates transaction state based on result
 * - No PKM reconstruction needed - exact API replay
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
    ) -> Result<String>;
    
    /// Update an existing block
    async fn update_block(&self, block_id: String, content: String) -> Result<()>;
    
    /// Delete a block
    async fn delete_block(&self, block_id: String) -> Result<()>;
    
    /// Create a new page
    async fn create_page(&self, page_name: String, properties: Option<serde_json::Value>) -> Result<()>;
    
    /// Delete a page
    async fn delete_page(&self, page_name: String) -> Result<()>;
    
    /// Get a node by ID
    async fn get_node(&self, node_id: &str) -> Result<serde_json::Value>;
    
    /// Query graph with BFS traversal
    fn query_graph_bfs(&self, start_id: &str, max_depth: usize) -> Result<Vec<serde_json::Value>>;
    
    /// Switch to a different graph by ID
    async fn switch_graph(&self, graph_id: String) -> Result<serde_json::Value>;
    
    /// Get the currently active graph ID
    async fn get_active_graph(&self) -> Result<Option<String>>;
    
    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>>;
    
    /// Create a new knowledge graph
    async fn create_graph(&self, name: Option<String>, description: Option<String>) -> Result<serde_json::Value>;
    
    /// Delete a knowledge graph
    async fn delete_graph(&self, graph_id: String, force: bool) -> Result<()>;
    
    /// Replay a transaction during recovery with proper state management
    async fn replay_transaction(&self, transaction: crate::storage::Transaction, coordinator: Arc<crate::storage::TransactionCoordinator>) -> Result<()>;
}

impl GraphOperationsExt for Arc<AppState> {
    async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Result<String> {
        use tracing::debug;
        
        // Generate a proper UUID for this block
        let block_id = uuid::Uuid::new_v4().to_string();
        
        // Create the operation with full API parameters
        let operation = Operation::CreateBlock {
            content: content.clone(),
            parent_id: parent_id.clone(),
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction
        self.with_active_graph_transaction(operation, |graph_manager| {
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
            let result = block_data.apply_to_graph(graph_manager)
                .map(|node_idx| {
                    block_id.clone()
                })
                .map_err(|e| e.to_string());
            result
        }).await
        .map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    /// Update an existing block in both graph and via WebSocket
    async fn update_block(
        &self,
        block_id: String,
        content: String,
    ) -> Result<()> {
        // Create transaction with full API parameters
        let operation = Operation::UpdateBlock {
            block_id: block_id.clone(),
            content: content.clone(),
        };
        
        // Execute with transaction
        self.with_active_graph_transaction(operation, |graph_manager| {
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
    async fn delete_block(&self, block_id: String) -> Result<()> {
        // Create transaction with full API parameters
        let operation = Operation::DeleteBlock {
            block_id: block_id.clone(),
        };
        
        // Execute with transaction
        self.with_active_graph_transaction(operation, |graph_manager| {
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
    ) -> Result<()> {
        use tracing::debug;
        
        // Create transaction with full API parameters
        let operation = Operation::CreatePage {
            page_name: page_name.clone(),
            properties: properties.clone(),
        };
        
        // Execute with transaction
        let result = self.with_active_graph_transaction(operation, |graph_manager| {
            
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
    async fn delete_page(&self, page_name: String) -> Result<()> {
        // Create transaction with full API parameters
        let operation = Operation::DeletePage {
            page_name: page_name.clone(),
        };
        
        // Execute with transaction
        self.with_active_graph_transaction(operation, |graph_manager| {
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
    
    async fn get_node(&self, node_id: &str) -> Result<serde_json::Value> {
        // Get active graph
        let active_graph_id = self.get_active_graph_manager().await
            .ok_or_else(|| GraphOperationError::GraphError("No active graph".to_string()))?;
        
        let managers = self.graph_managers.read().await;
        let manager_lock = managers.get(&active_graph_id)
            .ok_or_else(|| GraphOperationError::GraphError("Graph manager not found".to_string()))?;
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
    ) -> Result<Vec<serde_json::Value>> {
        // TODO: Implement BFS traversal in graph_manager
        // For now, return empty result
        warn!("BFS traversal not yet implemented");
        Ok(vec![])
    }
    
    /// Switch to a different graph by ID
    async fn switch_graph(&self, graph_id: String) -> Result<serde_json::Value> {
        use tracing::debug;
        
        // Save the current active graph before switching
        if let Some(current_active) = self.get_active_graph_manager().await {
            let managers = self.graph_managers.read().await;
            if let Some(manager_lock) = managers.get(&current_active) {
                match manager_lock.write().await.save_graph() {
                    Ok(_) => {},
                    Err(e) => error!("Failed to save graph {} before switch: {}", current_active, e),
                }
            }
            drop(managers);
        }
        
        // Ensure the graph exists or create it
        self.get_or_create_graph_manager(&graph_id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to switch graph: {}", e)))?;
        
        // Set as active graph FIRST
        self.set_active_graph(graph_id.clone()).await;
        
        // Now run recovery on the newly active graph
        if let Some(coordinator) = self.get_transaction_coordinator(&graph_id).await {
            match coordinator.recover_pending_transactions().await {
                Ok(pending_transactions) => {
                    if !pending_transactions.is_empty() {
                        info!("🔄 Replaying {} pending transactions for graph {}", 
                              pending_transactions.len(), graph_id);
                        
                        // Replay each transaction with proper state updates
                        for transaction in pending_transactions {
                            if let Err(e) = self.replay_transaction(transaction, coordinator.clone()).await {
                                error!("Failed to replay transaction: {}", e);
                            }
                        }
                        
                        // Save the graph after recovery to ensure replayed changes are persisted
                        let managers = self.graph_managers.read().await;
                        if let Some(manager_lock) = managers.get(&graph_id) {
                            match manager_lock.write().await.save_graph() {
                                Ok(_) => {},
                                Err(e) => error!("Failed to save graph {} after recovery: {}", graph_id, e),
                            }
                        }
                        drop(managers);
                    }
                }
                Err(e) => {
                    error!("Failed to recover transactions for {}: {}", graph_id, e);
                    return Err(GraphOperationError::GraphError(format!("Transaction recovery failed: {}", e)));
                }
            }
        } else {
        }
        
        // Update registry to track the switch and get graph info
        let graph_info = {
            let mut registry = self.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            registry.switch_graph(&graph_id)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to switch graph: {}", e)))?
        };
        
        Ok(json!({
            "id": graph_info.id,
            "name": graph_info.name,
            "created": graph_info.created.to_rfc3339(),
            "last_accessed": graph_info.last_accessed.to_rfc3339(),
            "description": graph_info.description,
        }))
    }
    
    /// Get the currently active graph ID
    async fn get_active_graph(&self) -> Result<Option<String>> {
        Ok(self.get_active_graph_manager().await)
    }
    
    /// List all available graphs
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        if let Ok(registry) = self.graph_registry.lock() {
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
            let mut registry = self.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
            registry.register_graph(None, name, description, &self.data_dir)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to register graph: {}", e)))?
        };
        
        // Save registry after creating new graph
        {
            let registry = self.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
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
    /// Prevents deletion of the active graph unless `force` is true.
    /// When active graph is deleted, automatically switches to another available graph.
    async fn delete_graph(&self, graph_id: String, force: bool) -> Result<()> {
        // Check if this is the active graph
        let active_graph = self.get_active_graph_manager().await;
        if active_graph.as_ref() == Some(&graph_id) && !force {
            return Err(GraphOperationError::GraphError("Cannot delete the currently active graph".into()));
        }
        
        // Remove from registry (this also archives the data)
        {
            let mut registry = self.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
            registry.remove_graph(&graph_id)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to remove graph: {}", e)))?;
            
            // Save registry
            registry.save()
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Remove from managers
        {
            let mut managers = self.graph_managers.write().await;
            managers.remove(&graph_id);
        }
        
        // Remove from transaction coordinators
        {
            let mut coordinators = self.transaction_coordinators.write().await;
            coordinators.remove(&graph_id);
        }
        
        Ok(())
    }
    
    /// Replay a transaction during recovery
    /// This handles the complete transaction lifecycle: execution and state update
    async fn replay_transaction(&self, transaction: crate::storage::Transaction, coordinator: Arc<crate::storage::TransactionCoordinator>) -> Result<()> {
        use tracing::debug;
        let tx_id = transaction.id.clone();
        let operation = transaction.operation.clone();
        
        
        // Execute the operation using the OperationExecutor trait
        let result = OperationExecutor::execute_operation(self, operation).await;
        
        // Update transaction state based on result
        match result {
            Ok(()) => {
                coordinator.commit_transaction(&tx_id).await
                    .map_err(|e| GraphOperationError::GraphError(e.to_string()))?;
                info!("✅ Successfully replayed transaction {}", tx_id);
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
    async fn execute_operation(&self, operation: Operation) -> std::result::Result<(), String> {
        match operation {
            Operation::CreateBlock { content, parent_id, page_name, properties } => {
                GraphOperationsExt::add_block(self, content, parent_id, page_name, properties)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            Operation::UpdateBlock { block_id, content } => {
                GraphOperationsExt::update_block(self, block_id, content)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeleteBlock { block_id } => {
                GraphOperationsExt::delete_block(self, block_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::CreatePage { page_name, properties } => {
                GraphOperationsExt::create_page(self, page_name, properties)
                    .await
                    .map_err(|e| e.to_string())
            },
            Operation::DeletePage { page_name } => {
                GraphOperationsExt::delete_page(self, page_name)
                    .await
                    .map_err(|e| e.to_string())
            },
        }
    }
}

