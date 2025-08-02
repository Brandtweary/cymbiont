#![allow(dead_code)] // TODO: Remove when integrated with aichat-agent

/**
 * Graph Operations Module
 * 
 * Provides the public API for PKM-oriented knowledge graph operations. This module
 * orchestrates the transformation of PKM data structures (blocks, pages) into graph
 * nodes and edges, handling all the domain-specific logic while delegating generic
 * graph operations to the underlying graph_manager.
 * 
 * ## Purpose
 * 
 * This module serves as the primary interface for PKM operations on the knowledge graph.
 * It bridges the gap between external interfaces (WebSocket, AI agent tools) and the
 * generic graph engine, handling PKM-specific concerns like reference resolution,
 * page normalization, and block hierarchies.
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
 * - `switch_graph()` - Change active graph context
 * - `list_graphs()` - Enumerate all registered graphs
 * 
 * ### Query Operations
 * - `get_node()` - Retrieve node by ID with PKM-aware formatting
 * - `get_active_graph()` - Get current graph ID
 * - `query_graph_bfs()` - Breadth-first traversal (TODO)
 * 
 * ## PKM-Specific Logic
 * 
 * This module handles PKM concerns including:
 * - Creating PKMBlockData/PKMPageData structures
 * - Extracting references from content
 * - Normalizing page names
 * - Managing block parent-child relationships
 * - Coordinating with the PKM data layer's `apply_to_graph()` methods
 * 
 * ## Transaction Integration
 * 
 * All mutation operations are automatically wrapped in transactions via
 * `AppState::with_active_graph_transaction()`. This ensures:
 * - Atomic operations with rollback on failure
 * - Write-ahead logging for crash recovery
 * - Content deduplication via hash checking
 * 
 * ## Error Handling
 * 
 * Operations return `Result<T, GraphOperationError>` with two error variants:
 * - `GraphError` - General graph operation failures
 * - `NodeNotFound` - Specific node lookup failures
 * 
 * ## Usage Note
 * 
 * This module is specifically designed for PKM operations. If you need direct,
 * domain-agnostic graph manipulation, use the graph_manager functions directly
 * instead of going through this layer.
 */

use crate::{
    AppState,
    import::pkm_data::{PKMBlockData, PKMPageData},
    import::logseq::extract_references,
    storage::Operation,
};
use std::sync::Arc;
use tracing::{warn, error, info};
use thiserror::Error;
use serde_json::json;

#[derive(Error, Debug)]
pub enum GraphOperationError {
    #[error("Graph error: {0}")]
    GraphError(String),
    
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

pub type Result<T> = std::result::Result<T, GraphOperationError>;

/// High-level operations for knowledge graph management
pub struct GraphOperations {
    app_state: Arc<AppState>,
}

impl GraphOperations {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    /// Add a new block to the knowledge graph and sync via WebSocket
    pub async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Result<String> {
        // Generate a proper UUID for this block
        let block_id = uuid::Uuid::new_v4().to_string();
        
        // Create the operation
        let operation = Operation::CreateNode {
            node_type: "block".to_string(),
            content: content.clone(),
            temp_id: None,
        };
        
        // Execute with transaction
        self.app_state.with_active_graph_transaction(operation, |graph_manager| {
            // Create the block data
            let block_data = PKMBlockData {
                id: block_id.clone(),
                content: content.clone(),
                properties: properties.unwrap_or(json!({})),
                parent: parent_id.clone(),
                page: Some(page_name.clone().unwrap_or_else(|| "untitled".to_string())),
                references: extract_references(&content),
                children: vec![], // New blocks have no children initially
                created: chrono::Utc::now().to_rfc3339(),
                updated: chrono::Utc::now().to_rfc3339(),
                reference_content: None, // Let apply_to_graph handle resolution
            };
            
            // Add to graph
            block_data.apply_to_graph(graph_manager)
                .map(|_node_idx| {
                    block_id.clone()
                })
                .map_err(|e| e.to_string())
        }).await
        .map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    /// Update an existing block in both graph and via WebSocket
    pub async fn update_block(
        &self,
        block_id: String,
        content: String,
    ) -> Result<()> {
        // Create transaction
        let operation = Operation::UpdateNode {
            node_id: block_id.clone(),
            content: content.clone(),
        };
        
        // Execute with transaction
        self.app_state.with_active_graph_transaction(operation, |graph_manager| {
            // Find the node
            if let Some(node_idx) = graph_manager.find_node(&block_id) {
                // Get existing block data
                if let Some(node) = graph_manager.get_node(node_idx) {
                    // Create updated block data
                    let updated_block = PKMBlockData {
                        id: block_id.clone(),
                        content: content.clone(),
                        properties: serde_json::to_value(&node.properties).unwrap_or(json!({})),
                        parent: None, // Preserve existing parent
                        page: Some("".to_string()), // Preserve existing page
                        references: extract_references(&content),
                        children: vec![], // Preserve existing children
                        created: node.created_at.to_rfc3339(),
                        updated: chrono::Utc::now().to_rfc3339(),
                        reference_content: None, // Let apply_to_graph handle resolution
                    };
                    
                    // Update the node
                    updated_block.apply_to_graph(graph_manager)
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
    pub async fn delete_block(&self, block_id: String) -> Result<()> {
        // Create transaction
        let operation = Operation::DeleteNode {
            node_id: block_id.clone(),
        };
        
        // Execute with transaction
        self.app_state.with_active_graph_transaction(operation, |graph_manager| {
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
    pub async fn create_page(
        &self,
        page_name: String,
        properties: Option<serde_json::Value>,
    ) -> Result<()> {
        
        // Create transaction
        let operation = Operation::CreateNode {
            node_type: "page".to_string(),
            content: page_name.clone(),
            temp_id: None,
        };
        
        // Execute with transaction
        let result = self.app_state.with_active_graph_transaction(operation, |graph_manager| {
            // Create page data
            let page_data = PKMPageData {
                name: page_name.clone(),
                normalized_name: Some(page_name.to_lowercase()),
                properties: properties.clone().unwrap_or(json!({})),
                created: chrono::Utc::now().to_rfc3339(),
                updated: chrono::Utc::now().to_rfc3339(),
                blocks: vec![],
            };
            
            // Add to graph
            page_data.apply_to_graph(graph_manager)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }).await;
        
        result.map_err(|e| GraphOperationError::GraphError(e.to_string()))
    }
    
    /// Delete a page from both graph and via WebSocket
    pub async fn delete_page(&self, page_name: String) -> Result<()> {
        // Create transaction
        let operation = Operation::DeleteNode {
            node_id: page_name.clone(),
        };
        
        // Execute with transaction
        self.app_state.with_active_graph_transaction(operation, |graph_manager| {
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
    
    // Query operations (read-only, no transactions needed)
    
    /// Get a node by ID
    pub async fn get_node(&self, node_id: &str) -> Result<serde_json::Value> {
        // Get active graph
        let active_graph_id = self.app_state.get_active_graph_manager().await
            .ok_or_else(|| GraphOperationError::GraphError("No active graph".to_string()))?;
        
        let managers = self.app_state.graph_managers.read().await;
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
    pub fn query_graph_bfs(
        &self,
        _start_id: &str,
        _max_depth: usize,
    ) -> Result<Vec<serde_json::Value>> {
        // TODO: Implement BFS traversal in graph_manager
        // For now, return empty result
        warn!("BFS traversal not yet implemented");
        Ok(vec![])
    }
    
    // Graph management operations
    
    /// Switch to a different graph by ID
    pub async fn switch_graph(&self, graph_id: String) -> Result<serde_json::Value> {
        // Ensure the graph exists or create it
        self.app_state.get_or_create_graph_manager(&graph_id).await
            .map_err(|e| GraphOperationError::GraphError(format!("Failed to switch graph: {}", e)))?;
        
        // Set as active graph
        self.app_state.set_active_graph(graph_id.clone()).await;
        
        // Update registry to track the switch and get graph info
        let graph_info = {
            let mut registry = self.app_state.graph_registry.lock()
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
    pub async fn get_active_graph(&self) -> Result<Option<String>> {
        Ok(self.app_state.get_active_graph_manager().await)
    }
    
    /// List all available graphs
    pub async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        if let Ok(registry) = self.app_state.graph_registry.lock() {
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
    pub async fn create_graph(
        &self, 
        name: Option<String>, 
        description: Option<String>
    ) -> Result<serde_json::Value> {
        // Register the graph in the registry
        let graph_info = {
            let mut registry = self.app_state.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
            registry.register_graph(None, name, description, &self.app_state.data_dir)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to register graph: {}", e)))?
        };
        
        // Save registry after creating new graph
        {
            let registry = self.app_state.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
            registry.save()
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Create graph manager for the new graph
        self.app_state.get_or_create_graph_manager(&graph_info.id).await
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
    pub async fn delete_graph(&self, graph_id: String) -> Result<()> {
        // Check if this is the active graph
        let active_graph = self.app_state.get_active_graph_manager().await;
        if active_graph.as_ref() == Some(&graph_id) {
            return Err(GraphOperationError::GraphError("Cannot delete the currently active graph".into()));
        }
        
        // Remove from registry (this also archives the data)
        {
            let mut registry = self.app_state.graph_registry.lock()
                .map_err(|_| GraphOperationError::GraphError("Failed to lock registry".into()))?;
            
            registry.remove_graph(&graph_id)
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to remove graph: {}", e)))?;
            
            // Save registry
            registry.save()
                .map_err(|e| GraphOperationError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Remove from managers
        {
            let mut managers = self.app_state.graph_managers.write().await;
            managers.remove(&graph_id);
        }
        
        // Remove from transaction coordinators
        {
            let mut coordinators = self.app_state.transaction_coordinators.write().await;
            coordinators.remove(&graph_id);
        }
        
        Ok(())
    }
}

