/**
 * @module graph_manager
 * @description Core knowledge graph storage engine using petgraph
 * 
 * This module provides the foundational graph storage and manipulation capabilities
 * for Cymbiont's knowledge graph system. It implements a generic, high-performance
 * graph engine using petgraph's StableGraph structure, supporting arbitrary node
 * and edge types with automatic persistence.
 * 
 * ## Architecture Overview
 * 
 * The GraphManager maintains two critical data structures:
 * 1. `graph: StableGraph<NodeData, EdgeData>` - The actual graph structure
 * 2. `pkm_to_node: HashMap<String, NodeIndex>` - O(1) external ID → graph node lookup
 * 
 * ## Key Design Decisions
 * 
 * ### StableGraph Selection
 * Petgraph's StableGraph maintains consistent NodeIndex values even after node removals.
 * This is critical for our HashMap-based ID mapping system, preventing index invalidation
 * and ensuring reliable node references throughout the application lifetime.
 * 
 * ### Dual ID System
 * - **External ID (pkm_id)**: Application-provided identifier (e.g., UUID, name)
 * - **Internal ID**: System-generated UUID for graph serialization
 * This separation allows the graph to be self-contained while maintaining stable
 * external references.
 * 
 * ### Generic Design
 * GraphManager is domain-agnostic and accepts any node/edge data that conforms to
 * the NodeData/EdgeData structures. Domain-specific logic (e.g., PKM operations)
 * is implemented in separate modules that use GraphManager's generic API.
 * 
 * ### Auto-Save Mechanism
 * Dual-trigger persistence strategy optimizes for both data safety and performance:
 * - **Time-based**: Every 5 minutes (protects against data loss during low activity)
 * - **Operation-based**: Every 10 operations (captures bursts of activity)
 * - **Batch control**: `disable_auto_save()` / `enable_auto_save()` for bulk imports
 * 
 * ## Data Structures
 * 
 * ### NodeData
 * - `id`: Internal UUID for serialization
 * - `pkm_id`: External identifier provided by application
 * - `node_type`: Enum discriminator (Page or Block)
 * - `content`: Primary node content
 * - `reference_content`: Optional expanded/resolved content
 * - `properties`: Key-value metadata store
 * - `created_at` / `updated_at`: Audit timestamps
 * 
 * ### EdgeData
 * - `edge_type`: Enum discriminator for edge semantics
 * - `weight`: Float weight for graph algorithms (default: 1.0)
 * 
 * ### Edge Types
 * - **PageRef**: Reference to a page node
 * - **BlockRef**: Reference to a block node
 * - **Tag**: Tag association
 * - **Property**: Property reference (rarely used as edges)
 * - **ParentChild**: Hierarchical relationship
 * - **PageToBlock**: Page ownership relationship
 * 
 * ## Core API
 * 
 * ### Node Operations
 * - `create_node()`: Create new node with full attribute specification
 * - `create_or_update_node()`: Upsert operation with automatic update detection
 * - `find_node()`: O(1) lookup by external ID
 * - `get_node()`: Retrieve node data by graph index
 * - `archive_nodes()`: Soft delete with archival to timestamped JSON
 * 
 * ### Edge Operations
 * - `add_edge()`: Create edge with automatic duplicate prevention
 * - `has_edge()`: Check edge existence by type
 * 
 * ### Persistence Operations
 * - `new()`: Initialize manager, auto-load existing graph from disk
 * - `save_graph()`: Explicit full serialization to JSON
 * - `save_if_needed()`: Conditional save based on configured thresholds
 * 
 * ### Utility Operations
 * - `disable_auto_save()` / `enable_auto_save()`: Batch operation support
 * 
 * ## Performance Characteristics
 * 
 * - **Node lookup**: O(1) via HashMap index
 * - **Node creation**: O(1) amortized
 * - **Edge creation**: O(1) with duplicate check
 * - **Full serialization**: O(V + E) where V = vertices, E = edges
 * - **Memory usage**: O(V + E) with efficient petgraph representation
 * 
 * ## Concurrency Model
 * 
 * GraphManager is designed for single-threaded access protected by external
 * synchronization (typically a Mutex in the application layer):
 * - All operations assume exclusive access
 * - Batch operations should hold the lock for the entire sequence
 * - Auto-save can be disabled during bulk operations to reduce I/O
 * 
 * ## Error Handling
 * 
 * - **Initialization failures**: Falls back to empty graph (non-fatal)
 * - **Save failures**: Logged but operations continue (data retained in memory)
 * - **Load failures**: Logged with graceful degradation to empty state
 * - **Archive failures**: Propagated to caller, graph state unchanged
 * 
 * ## Persistence Format
 * 
 * Graphs are serialized to JSON with the following structure:
 * ```json
 * {
 *   "version": 1,
 *   "graph": { /* petgraph serialization */ },
 *   "pkm_to_node": { /* ID mapping */ }
 * }
 * ```
 * 
 * ## Integration Points
 * 
 * - **Storage Layer**: Delegates to `graph_persistence` module for I/O
 * - **Domain Layer**: Called by domain-specific modules (e.g., `pkm_data`)
 * - **API Layer**: Wrapped by `graph_operations` for external access
 * - **Transaction Layer**: Participates in transaction coordination
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::io;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use petgraph::stable_graph::{StableGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use thiserror::Error;
use tracing::{info, warn, error};

use crate::storage::graph_persistence;

/// Type alias for our knowledge graph
pub type KnowledgeGraph = StableGraph<NodeData, EdgeData>;

/// Type alias for our node index mapping
pub type NodeMap = HashMap<String, NodeIndex>;

/// Errors that can occur when working with the graph manager
#[derive(Error, Debug)]
pub enum GraphError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Reference resolution error: {0}")]
    ReferenceResolution(String),
    
    #[error("Failed to parse datetime: {0}")]
    DateTimeParseError(#[from] chrono::ParseError),
}

/// Result type for graph operations
pub type GraphResult<T> = Result<T, GraphError>;

/// Type of node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeType {
    /// A PKM page
    Page,
    /// A PKM block
    Block,
}

/// Data stored in each graph node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    /// Our internal unique identifier
    pub id: String,
    
    /// Original PKM identifier (UUID for blocks, name for pages)
    pub pkm_id: String,
    
    /// Type of node (Page or Block)
    pub node_type: NodeType,
    
    /// Content of the node (block content or page name)
    pub content: String,
    
    /// Expanded content with block references resolved
    pub reference_content: Option<String>,
    
    /// Node properties (key-value pairs)
    pub properties: HashMap<String, String>,
    
    /// When the node was created
    pub created_at: DateTime<Utc>,
    
    /// When the node was last updated
    pub updated_at: DateTime<Utc>,
}

/// Type of edge between nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeType {
    /// Reference to a page: [[Page Name]]
    PageRef,
    
    /// Reference to a block: ((block-id))
    BlockRef,
    
    /// Tag reference: #tag
    Tag,
    
    /// Property reference: `key::` value
    Property,
    
    /// Parent-child relationship
    ParentChild,
    
    /// Page-to-root-block relationship
    PageToBlock,
}

/// Data stored in each graph edge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeData {
    /// Type of edge
    pub edge_type: EdgeType,
    
    /// Weight for ranking algorithms (default 1.0)
    pub weight: f32,
}

impl Default for EdgeData {
    fn default() -> Self {
        Self {
            edge_type: EdgeType::ParentChild,
            weight: 1.0,
        }
    }
}

/// Manages the knowledge graph
pub struct GraphManager {
    /// Base directory for storing data
    data_dir: PathBuf,
    
    /// Graph ID for multi-graph support
    graph_id: Option<String>,
    
    /// The knowledge graph
    pub graph: KnowledgeGraph,
    
    /// Mapping from PKM IDs to graph node indices
    pub pkm_to_node: NodeMap,
    
    
    /// When the graph was last saved (for time-based saves)
    last_save_time: DateTime<Utc>,
    
    /// Number of operations since last save (for operation-based saves)
    operations_since_save: usize,
    
    /// Whether to perform automatic saves during operations (disabled during batch processing)
    auto_save_enabled: bool,
}

impl GraphManager {
    /// Create a new graph manager with the given data directory
    pub fn new<P: AsRef<Path>>(data_dir: P) -> GraphResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        
        // Ensure directories exist
        graph_persistence::ensure_directories(&data_dir)?;
        
        // Extract graph ID from data directory path
        let graph_id = data_dir.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .filter(|s| *s == "graphs")
            .and_then(|_| data_dir.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
        
        let mut manager = Self {
            data_dir: data_dir.clone(),
            graph_id,
            graph: StableGraph::new(),
            pkm_to_node: HashMap::new(),
            last_save_time: Utc::now(),
            operations_since_save: 0,
            auto_save_enabled: true,
        };
        
        // Try to load existing graph
        match graph_persistence::load_graph(&data_dir) {
            Ok(data) => {
                manager.graph = data.graph;
                manager.pkm_to_node = data.pkm_to_node;
            }
            Err(e) => {
                error!("Error loading graph: {:?}, starting with empty graph", e);
                // Save initial state for new graphs
                info!("🌐 Initializing new knowledge graph");
                if let Err(e) = manager.save_graph() {
                    warn!("Failed to save initial graph state: {}", e);
                }
            }
        }
        
        Ok(manager)
    }
    
    
    /// Save the graph to disk
    pub fn save_graph(&mut self) -> GraphResult<()> {
        graph_persistence::save_graph(
            &self.data_dir,
            &self.graph,
            &self.pkm_to_node,
        )?;
        
        // Reset save tracking
        self.last_save_time = Utc::now();
        self.operations_since_save = 0;
        
        Ok(())
    }
    
    
    /// Save if needed based on time or operation count
    fn save_if_needed(&mut self) {
        if self.auto_save_enabled && graph_persistence::should_save(self.last_save_time, self.operations_since_save) {
            if let Err(e) = self.save_graph() {
                error!("Error during automatic save: {}", e);
            }
        }
    }
    
    /// Disable automatic saves (useful during batch processing)
    pub fn disable_auto_save(&mut self) {
        self.auto_save_enabled = false;
    }
    
    /// Enable automatic saves (default state)
    pub fn enable_auto_save(&mut self) {
        self.auto_save_enabled = true;
    }
    
    /// Find a node by its PKM ID
    pub fn find_node(&self, pkm_id: &str) -> Option<NodeIndex> {
        self.pkm_to_node.get(pkm_id).copied()
    }
    
    /// Create a new node
    pub fn create_node(
        &mut self,
        pkm_id: String,
        internal_id: String,
        node_type: NodeType,
        content: String,
        reference_content: Option<String>,
        properties: HashMap<String, String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> NodeIndex {
        let node_data = NodeData {
            id: internal_id,
            pkm_id: pkm_id.clone(),
            node_type,
            content,
            reference_content,
            properties,
            created_at,
            updated_at,
        };
        
        let idx = self.graph.add_node(node_data);
        self.pkm_to_node.insert(pkm_id, idx);
        
        // Increment operation counter
        self.operations_since_save += 1;
        self.save_if_needed();
        
        idx
    }
    
    /// Create or update a node
    pub fn create_or_update_node(
        &mut self,
        pkm_id: String,
        internal_id: String,
        node_type: NodeType,
        content: String,
        reference_content: Option<String>,
        properties: HashMap<String, String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> GraphResult<NodeIndex> {
        let node_data = NodeData {
            id: internal_id,
            pkm_id: pkm_id.clone(),
            node_type,
            content,
            reference_content,
            properties,
            created_at,
            updated_at,
        };
        
        let idx = if let Some(&existing_idx) = self.pkm_to_node.get(&pkm_id) {
            // Update existing node
            if let Some(node) = self.graph.node_weight_mut(existing_idx) {
                *node = node_data;
            }
            existing_idx
        } else {
            // Create new node
            let idx = self.graph.add_node(node_data);
            self.pkm_to_node.insert(pkm_id, idx);
            idx
        };
        
        // Increment operation counter
        self.operations_since_save += 1;
        self.save_if_needed();
        
        Ok(idx)
    }
    
    /// Add an edge between two nodes if it doesn't already exist
    /// Returns true if edge was added, false if it already existed
    pub fn add_edge(
        &mut self,
        source: NodeIndex,
        target: NodeIndex,
        edge_type: EdgeType,
        weight: f32,
    ) -> bool {
        if !self.has_edge(source, target, &edge_type) {
            self.graph.add_edge(source, target, EdgeData { edge_type, weight });
            true
        } else {
            false
        }
    }
    
    
    
    
    
    /// Archive nodes by their indices
    pub fn archive_nodes(&mut self, nodes: Vec<(String, NodeIndex)>) -> GraphResult<String> {
        if nodes.is_empty() {
            return Ok("No nodes to archive".to_string());
        }
        
        // Collect node data with edges for archiving
        let archive_data: Vec<_> = nodes.iter()
            .filter_map(|(pkm_id, node_idx)| {
                self.graph.node_weight(*node_idx).map(|node| {
                    let edges_out: Vec<_> = self.graph.edges(*node_idx)
                        .map(|edge| serde_json::json!({
                            "target": edge.target().index(),
                            "edge_type": edge.weight()
                        }))
                        .collect();
                    
                    let edges_in: Vec<_> = self.graph.edges_directed(*node_idx, petgraph::Direction::Incoming)
                        .map(|edge| serde_json::json!({
                            "source": edge.source().index(),
                            "edge_type": edge.weight()
                        }))
                        .collect();
                    
                    (pkm_id.clone(), node.clone(), edges_out, edges_in)
                })
            })
            .collect();
        
        // Archive through persistence utilities
        let result = graph_persistence::archive_nodes(
            &self.data_dir,
            self.graph_id.as_deref(),
            &archive_data,
        )?;
        
        // Remove nodes from graph
        for (pkm_id, node_idx) in &nodes {
            self.graph.remove_node(*node_idx);
            self.pkm_to_node.remove(pkm_id);
        }
        
        // Save updated graph
        self.save_graph()?;
        
        Ok(result)
    }
    
    
    
    
    /// Check if an edge of a specific type exists between two nodes
    pub fn has_edge(&self, source: NodeIndex, target: NodeIndex, edge_type: &EdgeType) -> bool {
        self.graph.edges_connecting(source, target)
            .any(|edge| edge.weight().edge_type == *edge_type)
    }
    
    /// Get a node by its graph index (for internal use)
    pub fn get_node(&self, idx: NodeIndex) -> Option<&NodeData> {
        self.graph.node_weight(idx)
    }
    
    
    
}

// Helper functions (from datastore)


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    /// Create a test GraphManager with a temporary directory
    fn create_test_manager() -> (GraphManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = GraphManager::new(temp_dir.path()).unwrap();
        (manager, temp_dir)
    }
    
    #[test]
    fn test_create_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let node_idx = manager.create_node(
            "test-node-1".to_string(),
            "internal-1".to_string(),
            NodeType::Block,
            "Test content".to_string(),
            Some("Resolved content".to_string()),
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );
        
        // Verify node was created
        let node = manager.get_node(node_idx).unwrap();
        assert_eq!(node.pkm_id, "test-node-1");
        assert_eq!(node.content, "Test content");
        assert_eq!(node.reference_content, Some("Resolved content".to_string()));
        assert_eq!(node.node_type, NodeType::Block);
        
        // Verify mapping was created
        assert_eq!(manager.find_node("test-node-1"), Some(node_idx));
    }
    
    #[test]
    fn test_create_or_update_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create initial node
        let idx1 = manager.create_or_update_node(
            "test-node-1".to_string(),
            "internal-1".to_string(),
            NodeType::Page,
            "Original content".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        ).unwrap();
        
        // Update the same node
        let idx2 = manager.create_or_update_node(
            "test-node-1".to_string(),
            "internal-2".to_string(),
            NodeType::Page,
            "Updated content".to_string(),
            Some("Updated resolved".to_string()),
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        ).unwrap();
        
        // Should be the same node index
        assert_eq!(idx1, idx2);
        
        // Content should be updated
        let node = manager.get_node(idx1).unwrap();
        assert_eq!(node.content, "Updated content");
        assert_eq!(node.reference_content, Some("Updated resolved".to_string()));
    }
    
    #[test]
    fn test_add_edge() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create two nodes
        let node1 = manager.create_node(
            "node-1".to_string(),
            "internal-1".to_string(),
            NodeType::Block,
            "Node 1".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );
        
        let node2 = manager.create_node(
            "node-2".to_string(),
            "internal-2".to_string(),
            NodeType::Block,
            "Node 2".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );
        
        // Add edge
        let added = manager.add_edge(node1, node2, EdgeType::BlockRef, 1.0);
        assert!(added);
        
        // Try to add duplicate - should return false
        let duplicate = manager.add_edge(node1, node2, EdgeType::BlockRef, 1.0);
        assert!(!duplicate);
        
        // Verify edge exists
        assert!(manager.has_edge(node1, node2, &EdgeType::BlockRef));
    }
    
    #[test]
    fn test_find_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let node_idx = manager.create_node(
            "find-me".to_string(),
            "internal-1".to_string(),
            NodeType::Page,
            "Content".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );
        
        // Should find the node
        assert_eq!(manager.find_node("find-me"), Some(node_idx));
        
        // Should not find non-existent node
        assert_eq!(manager.find_node("not-found"), None);
    }
    
    
    #[test]
    fn test_save_and_load_graph() {
        let temp_dir = TempDir::new().unwrap();
        let node_count;
        let edge_count;
        
        // Create and populate graph
        {
            let mut manager = GraphManager::new(temp_dir.path()).unwrap();
            
            // Add some nodes
            let node1 = manager.create_node(
                "node-1".to_string(),
                "internal-1".to_string(),
                NodeType::Page,
                "Page content".to_string(),
                None,
                HashMap::new(),
                Utc::now(),
                Utc::now(),
            );
            
            let node2 = manager.create_node(
                "node-2".to_string(),
                "internal-2".to_string(),
                NodeType::Block,
                "Block content".to_string(),
                None,
                HashMap::new(),
                Utc::now(),
                Utc::now(),
            );
            
            // Add an edge
            manager.add_edge(node1, node2, EdgeType::PageToBlock, 1.0);
            
            // Force save
            manager.save_graph().unwrap();
            
            node_count = manager.graph.node_count();
            edge_count = manager.graph.edge_count();
        }
        
        // Load graph in new manager
        {
            let manager = GraphManager::new(temp_dir.path()).unwrap();
            
            // Verify data was loaded
            assert_eq!(manager.graph.node_count(), node_count);
            assert_eq!(manager.graph.edge_count(), edge_count);
            assert!(manager.find_node("node-1").is_some());
            assert!(manager.find_node("node-2").is_some());
        }
    }
    
    #[test]
    fn test_operation_based_save() {
        let (mut manager, temp_dir) = create_test_manager();
        
        // Reset operations counter
        manager.operations_since_save = 0;
        
        // Add 9 nodes - should not trigger save
        for i in 0..9 {
            manager.create_node(
                format!("node-{}", i),
                format!("internal-{}", i),
                NodeType::Block,
                "Content".to_string(),
                None,
                HashMap::new(),
                Utc::now(),
                Utc::now(),
            );
        }
        
        // Verify no save yet
        assert_eq!(manager.operations_since_save, 9);
        
        // Add 10th node - should trigger save
        manager.create_node(
            "node-9".to_string(),
            "internal-9".to_string(),
            NodeType::Block,
            "Content".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );
        
        // Verify save was triggered (counter reset)
        assert_eq!(manager.operations_since_save, 0);
        
        // Verify file exists
        assert!(temp_dir.path().join("knowledge_graph.json").exists());
    }
    
}