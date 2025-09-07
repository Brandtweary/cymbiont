//! Core knowledge graph storage engine using petgraph
//!
//! This module provides the foundational graph storage and manipulation capabilities
//! for Cymbiont's knowledge graph system. It implements a generic, high-performance
//! graph engine using petgraph's `StableGraph` structure, supporting arbitrary node
//! and edge types with automatic persistence.
//!
//! ## Architecture Overview
//!
//! The `GraphManager` maintains two critical data structures:
//! 1. `graph: StableGraph<NodeData, EdgeData>` - The actual graph structure
//! 2. `node_index: HashMap<String, NodeIndex>` - O(1) node ID → graph node lookup
//!
//! ## Key Design Decisions
//!
//! ### `StableGraph` Selection
//! Petgraph's `StableGraph` maintains consistent `NodeIndex` values even after node removals.
//! This is critical for our `HashMap`-based ID mapping system, preventing index invalidation
//! and ensuring reliable node references throughout the application lifetime.
//!
//! ### ID System
//! - **Node ID (id)**: Application-provided identifier (e.g., UUID for blocks, name for pages)
//!   This provides stable references for external operations while maintaining
//!   efficient graph traversal.
//!
//! ### Generic Design
//! `GraphManager` is domain-agnostic and accepts any node/edge data that conforms to
//! the `NodeData`/`EdgeData` structures. Domain-specific logic (e.g., PKM operations)
//! is implemented in separate modules that use `GraphManager`'s generic API.
//!
//! ### Persistence Strategy
//! Graph state is persisted through the CQRS command log system.
//! All mutations flow through `CommandQueue` and are logged before execution.
//! The graph structure is rebuilt from command replay on startup.
//!
//! ## Data Structures
//!
//! ### `NodeData`
//! - `id`: Node identifier provided by application
//! - `node_type`: Enum discriminator (Page or Block)
//! - `content`: Primary node content
//! - `reference_content`: Optional expanded/resolved content
//! - `properties`: Key-value metadata store
//! - `created_at` / `updated_at`: Audit timestamps
//!
//! ### `EdgeData`
//! - `edge_type`: Enum discriminator for edge semantics
//! - `weight`: Float weight for graph algorithms (default: 1.0)
//!
//! ### Edge Types
//! - **`PageRef`**: Reference to a page node
//! - **`BlockRef`**: Reference to a block node
//! - **`Tag`**: Tag association
//! - **`Property`**: Property reference (rarely used as edges)
//! - **`ParentChild`**: Hierarchical relationship
//! - **`PageToBlock`**: Page ownership relationship
//!
//! ## Core API
//!
//! ### Node Operations
//! - `create_node()`: Create new node with full attribute specification
//! - `create_or_update_node()`: Upsert operation with automatic update detection
//! - `find_node()`: O(1) lookup by external ID
//! - `get_node()`: Retrieve node data by graph index
//! - `archive_nodes()`: Soft delete with archival to timestamped JSON
//!
//! ### Edge Operations
//! - `add_edge()`: Create edge with automatic duplicate prevention
//! - `has_edge()`: Check edge existence by type
//!
//! ### Persistence Operations
//! - `new()`: Initialize empty graph manager
//! - Graph state rebuilt from command replay during recovery
//!
//! ## Performance Characteristics
//!
//! - **Node lookup**: O(1) via `HashMap` index
//! - **Node creation**: O(1) amortized
//! - **Edge creation**: O(1) with duplicate check
//! - **Full serialization**: O(V + E) where V = vertices, E = edges
//! - **Memory usage**: O(V + E) with efficient petgraph representation
//!
//! ## Concurrency Model
//!
//! `GraphManager` is owned exclusively by `CommandProcessor` in the CQRS architecture:
//! - All mutations require a `RouterToken` proving authorization
//! - Operations are executed sequentially by the `CommandProcessor`
//! - No external locking needed as `CommandProcessor` is single-threaded
//!
//! ## Error Handling
//!
//! - **Initialization failures**: Falls back to empty graph (non-fatal)
//! - **Command failures**: Operations rolled back, state unchanged
//! - **Archive failures**: Propagated to caller, graph state unchanged
//!
//! ## Persistence Format
//!
//! Graphs are serialized to JSON with the following structure:
//! ```json
//! {
//!   "version": 1,
//!   "graph": { /* petgraph serialization */ },
//!   "pkm_to_node": { /* ID mapping */ }
//! }
//! ```
//!
//! ## Integration Points
//!
//! - **CQRS Layer**: All mutations go through `CommandQueue` for durability
//! - **Domain Layer**: Called by domain-specific modules (e.g., `pkm_data`)
//! - **API Layer**: Wrapped by `graph_operations` for external access
//! - **Recovery Layer**: State rebuilt from command replay on startup

use chrono::{DateTime, Utc};
use petgraph::stable_graph::{NodeIndex, StableGraph};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::info;

use crate::error::{GraphError, Result};

/// Type alias for our knowledge graph
pub type KnowledgeGraph = StableGraph<NodeData, EdgeData>;

/// Type alias for our node index mapping
pub type NodeMap = HashMap<String, NodeIndex>;

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
    /// Node identifier (UUID for blocks, name for pages)
    pub id: String,

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
#[derive(Debug)]
pub struct GraphManager {
    /// The knowledge graph
    pub graph: KnowledgeGraph,

    /// Mapping from node IDs to graph node indices
    pub node_index: NodeMap,

    /// Data directory for this graph
    pub data_dir: PathBuf,

    /// Time of last save (for autosave)
    last_save: Instant,

    /// Number of operations since last save
    operations_since_save: usize,
}

impl GraphManager {
    /// Create a new empty graph manager
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        // Ensure directories exist
        fs::create_dir_all(data_dir.as_ref())
            .map_err(|e| GraphError::lifecycle(format!("Failed to create directories: {e}")))?;

        let manager = Self {
            graph: StableGraph::new(),
            node_index: HashMap::new(),
            data_dir: data_dir.as_ref().to_path_buf(),
            last_save: Instant::now(),
            operations_since_save: 0,
        };

        info!("🌐 Initialized empty knowledge graph");

        Ok(manager)
    }

    /// Save graph to JSON
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::json!({
            "version": 1,
            "graph": &self.graph,
            "node_index": &self.node_index,
        });

        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| GraphError::lifecycle(format!("Failed to serialize graph: {e}")))?;

        fs::write(path, json)
            .map_err(|e| GraphError::lifecycle(format!("Failed to write graph JSON: {e}")))?;

        Ok(())
    }

    /// Track an operation and trigger autosave if needed
    fn track_operation(&mut self) {
        self.operations_since_save += 1;
        let _ = self.check_autosave();
    }

    /// Check if autosave is needed and perform it
    ///
    /// Saves if either:
    /// - 5 minutes have passed since last save
    /// - 10 operations have been performed since last save
    fn check_autosave(&mut self) -> Result<()> {
        const AUTOSAVE_INTERVAL_SECS: u64 = 300; // 5 minutes
        const AUTOSAVE_OP_THRESHOLD: usize = 10;

        let should_save = self.operations_since_save >= AUTOSAVE_OP_THRESHOLD
            || self.last_save.elapsed().as_secs() >= AUTOSAVE_INTERVAL_SECS;

        if should_save {
            let save_path = self.data_dir.join("knowledge_graph.json");
            self.save(&save_path)?;

            self.last_save = Instant::now();
            self.operations_since_save = 0;

            info!("💾 Autosaved graph to {}", save_path.display());
        }

        Ok(())
    }

    /// Find a node by its PKM ID
    pub fn find_node(&self, node_id: &str) -> Option<NodeIndex> {
        self.node_index.get(node_id).copied()
    }

    /// Create a new node
    #[allow(clippy::too_many_arguments)]
    pub fn create_node(
        &mut self,
        node_id: String,
        node_type: NodeType,
        content: String,
        reference_content: Option<String>,
        properties: HashMap<String, String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> NodeIndex {
        let node_data = NodeData {
            id: node_id.clone(),
            node_type,
            content,
            reference_content,
            properties,
            created_at,
            updated_at,
        };

        let idx = self.graph.add_node(node_data);
        self.node_index.insert(node_id, idx);
        self.track_operation();
        idx
    }

    /// Create or update a node
    #[allow(clippy::too_many_arguments)]
    pub fn create_or_update_node(
        &mut self,
        node_id: String,
        node_type: NodeType,
        content: String,
        reference_content: Option<String>,
        properties: HashMap<String, String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> NodeIndex {
        let node_data = NodeData {
            id: node_id.clone(),
            node_type,
            content,
            reference_content,
            properties,
            created_at,
            updated_at,
        };

        let idx = if let Some(&existing_idx) = self.node_index.get(&node_id) {
            // Update existing node
            if let Some(node) = self.graph.node_weight_mut(existing_idx) {
                *node = node_data;
            }
            existing_idx
        } else {
            // Create new node
            let idx = self.graph.add_node(node_data);
            self.node_index.insert(node_id, idx);
            idx
        };

        self.track_operation();
        idx
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
        if self.has_edge(source, target, &edge_type) {
            false
        } else {
            self.graph
                .add_edge(source, target, EdgeData { edge_type, weight });
            self.track_operation();
            true
        }
    }

    /// Delete nodes from the graph
    ///
    /// Removes nodes and all their edges from the graph.
    pub fn delete_nodes(&mut self, nodes: Vec<(String, NodeIndex)>) -> usize {
        let mut deleted_count = 0;

        for (node_id, node_idx) in nodes {
            self.graph.remove_node(node_idx);
            self.node_index.remove(&node_id);
            deleted_count += 1;
        }

        if deleted_count > 0 {
            self.track_operation();
        }

        deleted_count
    }

    /// Check if an edge of a specific type exists between two nodes
    pub fn has_edge(&self, source: NodeIndex, target: NodeIndex, edge_type: &EdgeType) -> bool {
        self.graph
            .edges_connecting(source, target)
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

    /// Create a test `GraphManager` with a temporary directory
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
            NodeType::Block,
            "Test content".to_string(),
            Some("Resolved content".to_string()),
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );

        // Verify node was created
        let node = manager.get_node(node_idx).unwrap();
        assert_eq!(node.id, "test-node-1");
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
            NodeType::Page,
            "Original content".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );

        // Update the same node
        let idx2 = manager.create_or_update_node(
            "test-node-1".to_string(),
            NodeType::Page,
            "Updated content".to_string(),
            Some("Updated resolved".to_string()),
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );

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
            NodeType::Block,
            "Node 1".to_string(),
            None,
            HashMap::new(),
            Utc::now(),
            Utc::now(),
        );

        let node2 = manager.create_node(
            "node-2".to_string(),
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
}
