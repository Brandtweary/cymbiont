#![allow(dead_code)] // TODO: Remove when integrated with aichat-agent

/**
 * Knowledge Graph API Module
 * 
 * Provides the public API for knowledge graph operations with integrated transaction
 * logging and automatic PKM synchronization via WebSocket. This module serves as the
 * primary interface for AI agents and other consumers to interact with the knowledge
 * graph while maintaining consistency between the in-memory graph and Logseq.
 * 
 * Key responsibilities:
 * - Transaction-wrapped graph mutations
 * - Content deduplication via hash checking
 * - Automatic WebSocket sync to Logseq
 * - Saga coordination for multi-step operations
 * - Clean public API hiding internal complexity
 */

use crate::{
    AppState,
    graph_manager::{GraphManager, GraphNodeIndex},
    pkm_data::{PKMBlockData, PKMPageData},
    transaction_log::Operation,
    transaction::TransactionError,
    saga::SagaError,
    websocket::{Command, broadcast_command},
};
use std::sync::Arc;
use std::collections::HashMap;
use tracing::{warn, error, debug};
use thiserror::Error;
use serde_json::json;

#[derive(Error, Debug)]
pub enum KgApiError {
    #[error("Transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    
    #[error("Saga error: {0}")]
    SagaError(#[from] SagaError),
    
    #[error("Graph error: {0}")]
    GraphError(String),
    
    #[error("WebSocket error: {0}")]
    WebSocketError(String),
    
    #[error("Content already being processed")]
    DuplicateContent,
    
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
    
    /// Add a new block to the knowledge graph and sync to Logseq
    pub async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Result<String> {
        // Check for duplicate content
        let content_hash = compute_content_hash(&content);
        if self.app_state.transaction_coordinator.is_content_pending(&content_hash).await {
            return Err(KgApiError::DuplicateContent);
        }
        
        // Start a saga for the block creation workflow
        let (saga_id, temp_id) = self.app_state.workflow_sagas
            .create_block_workflow(content.clone(), "block".to_string())
            .await?;
        
        // Generate correlation ID for this operation
        let correlation_id = format!("block-{}", uuid::Uuid::new_v4());
        
        // Store correlation_id -> saga_id mapping
        {
            let mut correlation_map = self.app_state.correlation_to_saga.write().await;
            correlation_map.insert(correlation_id.clone(), saga_id.clone());
        }
        
        // Create the block data
        let block_data = PKMBlockData {
            id: temp_id.clone(),
            content: content.clone(),
            properties: properties.unwrap_or(json!({})),
            parent: parent_id.clone(),
            page: Some(page_name.clone().unwrap_or_else(|| "untitled".to_string())),
            references: vec![], // TODO: Extract references from content
            children: vec![], // New blocks have no children initially
            created: chrono::Utc::now().to_rfc3339(),
            updated: chrono::Utc::now().to_rfc3339(),
        };
        
        // Add to graph (transaction already created by saga)
        let node_idx = {
            let mut graph_manager = self.app_state.graph_manager.lock().unwrap();
            graph_manager.create_or_update_node_from_pkm_block(&block_data)
                .map_err(|e| KgApiError::GraphError(e.to_string()))?
        };
        
        debug!("Added block to graph with temp_id: {} at index: {:?}", temp_id, node_idx);
        
        // Send WebSocket command to create in Logseq
        let command = Command::CreateBlock {
            content,
            parent_id,
            page_name,
            correlation_id: Some(correlation_id),
            temp_id: Some(temp_id.clone()),
        };
        
        broadcast_command(&self.app_state, command).await
            .map_err(|e| KgApiError::WebSocketError(e.to_string()))?;
        
        // TODO: Wait for acknowledgment and update with real UUID
        // For now, return the temp_id
        Ok(temp_id)
    }
    
    /// Update an existing block in both graph and Logseq
    pub async fn update_block(
        &self,
        block_id: String,
        content: String,
    ) -> Result<()> {
        // Check for duplicate content
        let content_hash = compute_content_hash(&content);
        if self.app_state.transaction_coordinator.is_content_pending(&content_hash).await {
            return Err(KgApiError::DuplicateContent);
        }
        
        // Create transaction
        let operation = Operation::UpdateNode {
            node_id: block_id.clone(),
            content: content.clone(),
        };
        
        let tx_id = self.app_state.transaction_coordinator
            .begin_transaction(operation)
            .await?;
        
        // Update in graph
        let update_result = {
            let mut graph_manager = self.app_state.graph_manager.lock().unwrap();
            
            // Find the node
            if let Some(&node_idx) = graph_manager.get_node_index(&block_id) {
                // Get existing block data
                if let Some(node) = graph_manager.get_node(node_idx) {
                    // Create updated block data
                    let updated_block = PKMBlockData {
                        id: block_id.clone(),
                        content: content.clone(),
                        properties: serde_json::to_value(&node.properties).unwrap_or(json!({})),
                        parent: None, // Preserve existing parent
                        page: Some("".to_string()), // Preserve existing page
                        references: vec![], // TODO: Extract references
                        children: vec![], // Preserve existing children
                        created: node.created_at.to_rfc3339(),
                        updated: chrono::Utc::now().to_rfc3339(),
                    };
                    
                    // Update the node
                    graph_manager.create_or_update_node_from_pkm_block(&updated_block)
                        .map(|_| ())
                        .map_err(|e| KgApiError::GraphError(e.to_string()))
                } else {
                    Err(KgApiError::NodeNotFound(block_id.clone()))
                }
            } else {
                Err(KgApiError::NodeNotFound(block_id.clone()))
            }
        };
        
        match update_result {
            Ok(()) => {
                // Commit transaction
                self.app_state.transaction_coordinator
                    .commit_transaction(&tx_id)
                    .await?;
                
                // Send WebSocket command
                let command = Command::UpdateBlock {
                    block_id,
                    content,
                    correlation_id: None, // TODO: Add correlation tracking for updates
                };
                
                broadcast_command(&self.app_state, command).await
                    .map_err(|e| KgApiError::WebSocketError(e.to_string()))?;
                
                Ok(())
            }
            Err(e) => {
                // Abort transaction
                self.app_state.transaction_coordinator
                    .abort_transaction(&tx_id, &e.to_string())
                    .await?;
                Err(e)
            }
        }
    }
    
    /// Delete a block from both graph and Logseq
    pub async fn delete_block(&self, block_id: String) -> Result<()> {
        // Create transaction
        let operation = Operation::DeleteNode {
            node_id: block_id.clone(),
        };
        
        let tx_id = self.app_state.transaction_coordinator
            .begin_transaction(operation)
            .await?;
        
        // Delete from graph
        let delete_result = {
            let mut graph_manager = self.app_state.graph_manager.lock().unwrap();
            
            if let Some(&node_idx) = graph_manager.get_node_index(&block_id) {
                // Archive the node
                graph_manager.archive_nodes(vec![(block_id.clone(), node_idx)])
                    .map(|_| ())
                    .map_err(|e| KgApiError::GraphError(e.to_string()))
            } else {
                Err(KgApiError::NodeNotFound(block_id.clone()))
            }
        };
        
        match delete_result {
            Ok(()) => {
                // Commit transaction
                self.app_state.transaction_coordinator
                    .commit_transaction(&tx_id)
                    .await?;
                
                // Send WebSocket command
                let command = Command::DeleteBlock { 
                    block_id,
                    correlation_id: None, // TODO: Add correlation tracking for deletes
                };
                
                broadcast_command(&self.app_state, command).await
                    .map_err(|e| KgApiError::WebSocketError(e.to_string()))?;
                
                Ok(())
            }
            Err(e) => {
                // Abort transaction
                self.app_state.transaction_coordinator
                    .abort_transaction(&tx_id, &e.to_string())
                    .await?;
                Err(e)
            }
        }
    }
    
    /// Create a new page in both graph and Logseq
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
        
        let tx_id = self.app_state.transaction_coordinator
            .begin_transaction(operation)
            .await?;
        
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
        let create_result = {
            let mut graph_manager = self.app_state.graph_manager.lock().unwrap();
            graph_manager.create_or_update_node_from_pkm_page(&page_data)
                .map(|_| ())
                .map_err(|e| KgApiError::GraphError(e.to_string()))
        };
        
        match create_result {
            Ok(()) => {
                // Commit transaction
                self.app_state.transaction_coordinator
                    .commit_transaction(&tx_id)
                    .await?;
                
                // Send WebSocket command
                let command = Command::CreatePage {
                    name: page_name,
                    properties: properties.map(|v| {
                        // Convert serde_json::Value to HashMap<String, String>
                        if let serde_json::Value::Object(map) = v {
                            map.into_iter()
                                .filter_map(|(k, v)| {
                                    if let serde_json::Value::String(s) = v {
                                        Some((k, s))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        } else {
                            HashMap::new()
                        }
                    }),
                    correlation_id: None, // TODO: Add correlation tracking for pages
                };
                
                broadcast_command(&self.app_state, command).await
                    .map_err(|e| KgApiError::WebSocketError(e.to_string()))?;
                
                Ok(())
            }
            Err(e) => {
                // Abort transaction
                self.app_state.transaction_coordinator
                    .abort_transaction(&tx_id, &e.to_string())
                    .await?;
                Err(e)
            }
        }
    }
    
    // Query operations (read-only, no transactions needed)
    
    /// Get a node by ID
    pub fn get_node(&self, node_id: &str) -> Result<serde_json::Value> {
        let graph_manager = self.app_state.graph_manager.lock().unwrap();
        
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

fn compute_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}