#![allow(dead_code)] // TODO: Remove when integrated with aichat-agent

/**
 * Knowledge Graph API Module
 * 
 * Provides the public API for knowledge graph operations with integrated transaction
 * logging and WebSocket synchronization. This module serves as the primary interface
 * for AI agents to interact with the knowledge graph while maintaining consistency.
 * 
 * Key features:
 * - Transaction-wrapped graph mutations
 * - Content deduplication via hash checking
 * - WebSocket synchronization for real-time updates
 */

use crate::{
    AppState,
    graph_manager::{GraphManager, GraphNodeIndex},
    import::pkm_data::{PKMBlockData, PKMPageData},
    import::logseq::extract_references,
    storage::Operation,
};
use std::sync::Arc;
use tracing::{warn, error, info};
use thiserror::Error;
use serde_json::json;

#[derive(Error, Debug)]
pub enum KgApiError {
    #[error("Graph error: {0}")]
    GraphError(String),
    
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

pub type Result<T> = std::result::Result<T, KgApiError>;

/// High-level API for knowledge graph operations
pub struct KgApi {
    app_state: Arc<AppState>,
}

impl KgApi {
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
            // Resolve references
            let reference_content = Some(graph_manager.resolve_references(&content, Some(&block_id)));
            
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
                reference_content,
            };
            
            // Add to graph
            graph_manager.create_or_update_node_from_pkm_block(&block_data)
                .map(|_node_idx| {
                    block_id.clone()
                })
                .map_err(|e| e.to_string())
        }).await
        .map_err(|e| KgApiError::GraphError(e.to_string()))
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
            if let Some(&node_idx) = graph_manager.get_node_index(&block_id) {
                // Get existing block data
                if let Some(node) = graph_manager.get_node(node_idx) {
                    // Resolve references for the new content
                    let reference_content = Some(graph_manager.resolve_references(&content, Some(&block_id)));
                    
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
                        reference_content,
                    };
                    
                    // Update the node
                    graph_manager.create_or_update_node_from_pkm_block(&updated_block)
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
                KgApiError::NodeNotFound(block_id)
            } else {
                KgApiError::GraphError(e.to_string())
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
            if let Some(&node_idx) = graph_manager.get_node_index(&block_id) {
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
                KgApiError::NodeNotFound(block_id)
            } else {
                KgApiError::GraphError(e.to_string())
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
            graph_manager.create_or_update_node_from_pkm_page(&page_data)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }).await;
        
        result.map_err(|e| KgApiError::GraphError(e.to_string()))
    }
    
    // Query operations (read-only, no transactions needed)
    
    /// Get a node by ID
    pub async fn get_node(&self, node_id: &str) -> Result<serde_json::Value> {
        // Get active graph
        let active_graph_id = self.app_state.get_active_graph_manager().await
            .ok_or_else(|| KgApiError::GraphError("No active graph".to_string()))?;
        
        let managers = self.app_state.graph_managers.read().await;
        let manager_lock = managers.get(&active_graph_id)
            .ok_or_else(|| KgApiError::GraphError("Graph manager not found".to_string()))?;
        let graph_manager = manager_lock.read().await;
        
        if let Some(&node_idx) = graph_manager.get_node_index(node_id) {
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
                Err(KgApiError::NodeNotFound(node_id.to_string()))
            }
        } else {
            Err(KgApiError::NodeNotFound(node_id.to_string()))
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
            .map_err(|e| KgApiError::GraphError(format!("Failed to switch graph: {}", e)))?;
        
        // Set as active graph
        self.app_state.set_active_graph(graph_id.clone()).await;
        
        // Update registry to track the switch and get graph info
        let graph_info = {
            let mut registry = self.app_state.graph_registry.lock()
                .map_err(|_| KgApiError::GraphError("Failed to lock registry".into()))?;
            registry.switch_graph(&graph_id)
                .map_err(|e| KgApiError::GraphError(format!("Failed to switch graph: {}", e)))?
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
                .map_err(|_| KgApiError::GraphError("Failed to lock registry".into()))?;
            
            registry.register_graph(None, name, description, &self.app_state.data_dir)
                .map_err(|e| KgApiError::GraphError(format!("Failed to register graph: {}", e)))?
        };
        
        // Save registry after creating new graph
        {
            let registry = self.app_state.graph_registry.lock()
                .map_err(|_| KgApiError::GraphError("Failed to lock registry".into()))?;
            
            registry.save()
                .map_err(|e| KgApiError::GraphError(format!("Failed to save registry: {}", e)))?;
        }
        
        // Create graph manager for the new graph
        self.app_state.get_or_create_graph_manager(&graph_info.id).await
            .map_err(|e| KgApiError::GraphError(format!("Failed to create graph manager: {}", e)))?;
        
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
            return Err(KgApiError::GraphError("Cannot delete the currently active graph".into()));
        }
        
        // Remove from registry (this also archives the data)
        {
            let mut registry = self.app_state.graph_registry.lock()
                .map_err(|_| KgApiError::GraphError("Failed to lock registry".into()))?;
            
            registry.remove_graph(&graph_id)
                .map_err(|e| KgApiError::GraphError(format!("Failed to remove graph: {}", e)))?;
            
            // Save registry
            registry.save()
                .map_err(|e| KgApiError::GraphError(format!("Failed to save registry: {}", e)))?;
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

// Helper trait to extend GraphManager
trait GraphManagerExt {
    fn get_node_index(&self, pkm_id: &str) -> Option<&GraphNodeIndex>;
    fn get_node(&self, idx: GraphNodeIndex) -> Option<&crate::graph_manager::NodeData>;
}

impl GraphManagerExt for GraphManager {
    fn get_node_index(&self, pkm_id: &str) -> Option<&GraphNodeIndex> {
        self.pkm_to_node.get(pkm_id)
    }
    
    fn get_node(&self, idx: GraphNodeIndex) -> Option<&crate::graph_manager::NodeData> {
        self.graph.node_weight(idx)
    }
}