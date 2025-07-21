/**
 * @module graph_manager
 * @description Core knowledge graph storage engine using petgraph
 * 
 * This module is the heart of Cymbiont's knowledge graph implementation, providing
 * persistent graph storage using petgraph's StableGraph structure. All PKM data (pages
 * and blocks from Logseq) are stored as nodes in a directed graph with typed edges
 * representing various relationships.
 * 
 * ## Architecture Overview
 * 
 * The GraphManager maintains three critical data structures:
 * 1. `graph: StableGraph<NodeData, EdgeData>` - The actual graph structure
 * 2. `pkm_to_node: HashMap<String, NodeIndex>` - O(1) PKM ID → graph node lookup
 * 3. `last_full_sync: Option<i64>` - Unix timestamp for sync scheduling
 * 
 * ## Key Design Decisions
 * 
 * ### StableGraph vs Graph
 * Petgraph's StableGraph maintains consistent NodeIndex values even after node removals.
 * This is critical for our HashMap-based ID mapping system, preventing index invalidation.
 * 
 * ### Dual ID System
 * - **PKM ID**: Original Logseq identifier (block UUID or page name)
 * - **Internal ID**: UUID generated for graph serialization compatibility
 * This allows the graph to be self-contained while maintaining Logseq references.
 * 
 * ### Reference Resolution Strategy
 * When processing references to non-existent nodes, the system creates placeholder nodes:
 * - Missing pages: Created with name as content, empty properties
 * - Missing blocks: Created with empty content, PKM ID preserved
 * This ensures graph completeness and prevents broken references during incremental sync.
 * 
 * ### Auto-Save Mechanism
 * Dual-trigger persistence strategy optimizes for both data safety and performance:
 * - **Time-based**: Every 5 minutes (catches low-activity periods)
 * - **Operation-based**: Every 10 operations (catches high-activity bursts)
 * - **Batch control**: `disable_auto_save()` / `enable_auto_save()` for bulk operations
 * 
 * ## Node and Edge Types
 * 
 * ### NodeData Structure
 * - `id`: Internal UUID for serialization
 * - `pkm_id`: Original Logseq identifier
 * - `node_type`: Page or Block enum
 * - `content`: Page name or block text
 * - `properties`: HashMap of Logseq properties
 * - `created_at` / `updated_at`: Timestamps parsed from multiple formats
 * 
 * ### Edge Types and Semantics
 * - **PageRef**: `[[Page Name]]` - Block/page references another page
 * - **BlockRef**: `((block-id))` - Block references another block
 * - **Tag**: `#tag` - Creates implicit page node for tag
 * - **Property**: `key:: value` - Currently stored in node, not as edges
 * - **ParentChild**: Block hierarchy within pages
 * - **PageToBlock**: Page owns root-level blocks (no parent)
 * 
 * ## Key Functions
 * 
 * ### Graph Construction
 * - `new()`: Initialize manager with data directory, auto-loads existing graph
 * - `create_or_update_node_from_pkm_block()`: Primary block ingestion (handles references)
 * - `create_or_update_node_from_pkm_page()`: Page node creation/update
 * - `ensure_page_exists()`: Lazy page creation for references
 * 
 * ### Persistence
 * - `save_graph()`: Full graph serialization to JSON
 * - `load_graph()`: Deserialize from disk on startup
 * - `save_if_needed()`: Auto-save trigger logic
 * 
 * ### Sync Management
 * - `get_sync_status()`: Returns sync metadata and graph statistics
 * - `is_full_sync_needed()`: 2-hour threshold check
 * - `update_full_sync_timestamp()`: Mark successful sync completion
 * 
 * ### Internal Helpers
 * - `has_edge()`: Duplicate edge prevention
 * - `resolve_and_add_reference()`: Reference type dispatch
 * - `should_save()`: Auto-save threshold checks
 * 
 * ## Performance Characteristics
 * 
 * - **Node lookup**: O(1) via HashMap
 * - **Edge creation**: O(1) for direct insertion
 * - **Reference resolution**: O(1) for existing nodes, O(1) for placeholder creation
 * - **Full save**: O(V + E) where V = vertices, E = edges
 * - **Batch processing**: O(n) for n operations with single lock acquisition
 * 
 * ## Concurrency and Safety
 * 
 * The GraphManager is designed for single-threaded access within a Mutex:
 * - API handlers acquire the lock for each operation
 * - Batch operations hold the lock for the entire batch
 * - Auto-save can be disabled to prevent lock contention during bulk updates
 * 
 * ## Error Recovery
 * 
 * - **Missing references**: Create placeholders (never fail)
 * - **Save failures**: Log error, continue operation (data in memory)
 * - **Load failures**: Start with empty graph (non-fatal)
 * - **Malformed timestamps**: Fall back to current time
 * 
 * ## Current Limitations and Future Work
 * 
 * Currently this module is storage-only. Future enhancements will include:
 * - **Query API**: Neighbor traversal, path finding, subgraph extraction
 * - **Graph algorithms**: PageRank, similarity scoring, community detection
 * - **Incremental saves**: Track dirty nodes, save only changes
 * - **Write-ahead logging**: Crash recovery guarantees
 * - **Graph indexing**: Full-text search, property indexes
 * - **Compression**: Reduce JSON size for large graphs
 * 
 * ## Testing Strategy
 * 
 * The test suite validates:
 * - Node and edge creation/updates
 * - Reference resolution and placeholder creation
 * - Parent-child and page-block relationships
 * - Duplicate edge prevention
 * - Save/load persistence cycle
 * - Auto-save thresholds
 * - Complex graph traversal scenarios
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use petgraph::stable_graph::{StableGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use thiserror::Error;
use tracing::{info, warn, error, debug, trace};

use crate::pkm_data::{PKMBlockData, PKMPageData, PKMReference};
use crate::utils::{parse_datetime, parse_properties};

/// Type alias for our knowledge graph
type KnowledgeGraph = StableGraph<NodeData, EdgeData>;

/// Type alias for our node index mapping
type NodeMap = HashMap<String, NodeIndex>;

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
    
    /// The knowledge graph
    graph: KnowledgeGraph,
    
    /// Mapping from PKM IDs to graph node indices
    pkm_to_node: NodeMap,
    
    /// When the last incremental sync was performed (Unix timestamp in milliseconds)
    last_incremental_sync: Option<i64>,
    
    /// When the last full database sync was performed (Unix timestamp in milliseconds)
    last_full_sync: Option<i64>,
    
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
        
        // Create the data directory if it doesn't exist
        fs::create_dir_all(&data_dir)?;
        
        // Create the archived_nodes subdirectory
        let archive_dir = data_dir.join("archived_nodes");
        fs::create_dir_all(&archive_dir)?;
        
        let mut manager = Self {
            data_dir,
            graph: StableGraph::new(),
            pkm_to_node: HashMap::new(),
            last_incremental_sync: None,
            last_full_sync: None,
            last_save_time: Utc::now(),
            operations_since_save: 0,
            auto_save_enabled: true,
        };
        
        // Try to load existing graph
        let loaded_existing = match manager.load_graph() {
            Ok(_) => {
                // Graph stats are logged separately
                true
            },
            Err(e) => {
                error!("Error loading graph: {e:?}, starting with empty graph");
                false
            }
        };
        
        // Save initial state for new graphs
        if !loaded_existing {
            info!("🌐 Initializing new knowledge graph");
            if let Err(e) = manager.save_graph() {
                warn!("Failed to save initial graph state: {}", e);
            }
        }
        
        Ok(manager)
    }
    
    /// Load the graph from disk
    pub fn load_graph(&mut self) -> GraphResult<()> {
        let graph_path = self.data_dir.join("knowledge_graph.json");
        
        if !graph_path.exists() {
            return Ok(());
        }
        
        let mut file = File::open(graph_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        
        #[derive(Deserialize)]
        struct SerializedGraph {
            graph: KnowledgeGraph,
            pkm_to_node: NodeMap,
            last_incremental_sync: Option<i64>,
            last_full_sync: Option<i64>,
        }
        
        let serialized: SerializedGraph = serde_json::from_str(&contents)?;
        self.graph = serialized.graph;
        self.pkm_to_node = serialized.pkm_to_node;
        self.last_incremental_sync = serialized.last_incremental_sync;
        self.last_full_sync = serialized.last_full_sync;
        
        info!("📊 Loaded graph with {} nodes and {} edges", 
              self.graph.node_count(), self.graph.edge_count());
        
        Ok(())
    }
    
    /// Save the graph to disk
    pub fn save_graph(&mut self) -> GraphResult<()> {
        let graph_path = self.data_dir.join("knowledge_graph.json");
        
        #[derive(Serialize)]
        struct SerializedGraph<'a> {
            graph: &'a KnowledgeGraph,
            pkm_to_node: &'a NodeMap,
            last_incremental_sync: Option<i64>,
            last_full_sync: Option<i64>,
        }
        
        let serialized = SerializedGraph {
            graph: &self.graph,
            pkm_to_node: &self.pkm_to_node,
            last_incremental_sync: self.last_incremental_sync,
            last_full_sync: self.last_full_sync,
        };
        
        let json = serde_json::to_string_pretty(&serialized)?;
        let mut file = File::create(graph_path)?;
        file.write_all(json.as_bytes())?;
        
        // Reset save tracking
        self.last_save_time = Utc::now();
        self.operations_since_save = 0;
        
        Ok(())
    }
    
    /// Check if we should save based on time or operation count
    fn should_save(&self) -> bool {
        const SAVE_INTERVAL_MINUTES: i64 = 5;
        const SAVE_OPERATION_THRESHOLD: usize = 10;
        
        // Check time-based save (every 5 minutes)
        let minutes_since_save = (Utc::now() - self.last_save_time).num_minutes();
        if minutes_since_save >= SAVE_INTERVAL_MINUTES {
            debug!("⏱️ Time-based save triggered: {} minutes since last save", minutes_since_save);
            return true;
        }
        
        // Check operation-based save (every 10 operations)
        if self.operations_since_save >= SAVE_OPERATION_THRESHOLD {
            debug!("⏱️ Operation-based save triggered: {} operations since last save", self.operations_since_save);
            return true;
        }
        
        false
    }
    
    /// Save if needed based on time or operation count
    fn save_if_needed(&mut self) {
        if self.auto_save_enabled && self.should_save() {
            if let Err(e) = self.save_graph() {
                error!("Error during automatic save: {}", e);
            } else {
                debug!("💾 Automatic save completed");
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
    
    /// Get a node by its PKM ID
    #[allow(dead_code)]
    pub fn get_node_by_pkm_id(&self, pkm_id: &str) -> Option<&NodeData> {
        self.pkm_to_node.get(pkm_id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }
    
    /// Get the current sync status
    pub fn get_sync_status(&self, config: &crate::config::SyncConfig) -> serde_json::Value {
        let now = Utc::now().timestamp_millis();
        
        // Calculate hours since each sync type
        let hours_since_incremental = self.last_incremental_sync.map(|last_sync| {
            (now - last_sync) as f64 / (1000.0 * 60.0 * 60.0)
        });
        
        let hours_since_full = self.last_full_sync.map(|last_sync| {
            (now - last_sync) as f64 / (1000.0 * 60.0 * 60.0)
        });
        
        // Convert Unix timestamps to ISO strings for JavaScript consumption
        let last_incremental_sync_iso = self.last_incremental_sync.map(|timestamp| {
            DateTime::<Utc>::from_timestamp_millis(timestamp)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "invalid".to_string())
        });
        
        let last_full_sync_iso = self.last_full_sync.map(|timestamp| {
            DateTime::<Utc>::from_timestamp_millis(timestamp)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "invalid".to_string())
        });
        
        serde_json::json!({
            // Legacy fields for backwards compatibility
            "last_full_sync": self.last_incremental_sync,
            "last_full_sync_iso": last_incremental_sync_iso.clone(),
            "hours_since_sync": hours_since_incremental,
            "full_sync_needed": self.is_incremental_sync_needed(config.incremental_interval_hours),
            
            // New detailed fields
            "last_incremental_sync": self.last_incremental_sync,
            "last_incremental_sync_iso": last_incremental_sync_iso,
            "hours_since_incremental": hours_since_incremental,
            "incremental_sync_needed": self.is_incremental_sync_needed(config.incremental_interval_hours),
            
            "last_true_full_sync": self.last_full_sync,
            "last_true_full_sync_iso": last_full_sync_iso,
            "hours_since_full": hours_since_full,
            "true_full_sync_needed": self.is_true_full_sync_needed(config),
            
            // Configuration info
            "sync_config": {
                "incremental_interval_hours": config.incremental_interval_hours,
                "full_interval_hours": config.full_interval_hours,
                "enable_full_sync": config.enable_full_sync,
            },
            
            // Graph stats
            "node_count": self.graph.node_count(),
            "edge_count": self.graph.edge_count(),
        })
    }
    
    /// Check if an incremental sync is needed based on time since last sync
    pub fn is_incremental_sync_needed(&self, interval_hours: u64) -> bool {
        let now = Utc::now().timestamp_millis();
        
        self.last_incremental_sync.map_or_else(|| {
            info!("Incremental sync needed: No previous sync found");
            true
        }, |last_sync| {
            let hours_since_sync = (now - last_sync) / (1000 * 60 * 60);
            let sync_needed = hours_since_sync > interval_hours as i64;
            
            trace!("Last incremental sync: {last_sync}, Hours since sync: {hours_since_sync}, Incremental sync needed: {sync_needed}");
            
            sync_needed
        })
    }
    
    /// Check if a true full sync is needed (re-sync entire PKM)
    pub fn is_true_full_sync_needed(&self, config: &crate::config::SyncConfig) -> bool {
        // Full sync disabled by configuration
        if !config.enable_full_sync {
            return false;
        }
        
        let now = Utc::now().timestamp_millis();
        
        self.last_full_sync.map_or_else(|| {
            info!("True full sync needed: No previous full sync found");
            true
        }, |last_sync| {
            let hours_since_sync = (now - last_sync) / (1000 * 60 * 60);
            let sync_needed = hours_since_sync > config.full_interval_hours as i64;
            
            trace!("Last full sync: {last_sync}, Hours since sync: {hours_since_sync}, True full sync needed: {sync_needed}");
            
            sync_needed
        })
    }
    
    /// Update the last incremental sync timestamp
    pub fn update_incremental_sync_timestamp(&mut self) -> GraphResult<()> {
        let now = Utc::now().timestamp_millis();
        self.last_incremental_sync = Some(now);
        self.save_graph()
    }
    
    /// Update the last full sync timestamp
    pub fn update_full_sync_timestamp(&mut self) -> GraphResult<()> {
        let now = Utc::now().timestamp_millis();
        self.last_full_sync = Some(now);
        self.save_graph()
    }
    
    /// Verify PKM IDs and archive any nodes that no longer exist in the PKM
    pub fn verify_and_archive_missing_nodes(&mut self, page_ids: &[String], block_ids: &[String]) -> GraphResult<(usize, String)> {
        use std::collections::HashSet;
        
        // Convert ID lists to HashSets for O(1) lookup
        let page_set: HashSet<&str> = page_ids.iter().map(|s| s.as_str()).collect();
        let block_set: HashSet<&str> = block_ids.iter().map(|s| s.as_str()).collect();
        
        // Find nodes to archive
        let mut nodes_to_archive = Vec::new();
        let mut archived_pages = 0;
        let mut archived_blocks = 0;
        
        trace!("Checking for nodes to archive: {} pages and {} blocks in PKM", 
               page_ids.len(), block_ids.len());
        
        for (pkm_id, &node_idx) in &self.pkm_to_node {
            if let Some(node) = self.graph.node_weight(node_idx) {
                match node.node_type {
                    NodeType::Page => {
                        // Check both original and lowercase version
                        let normalized_id = node.pkm_id.to_lowercase();
                        if !page_set.contains(node.pkm_id.as_str()) && !page_set.contains(normalized_id.as_str()) {
                            trace!("🗄️ Archiving deleted page: {}", node.pkm_id);
                            nodes_to_archive.push((pkm_id.clone(), node_idx, node.clone()));
                            archived_pages += 1;
                        }
                    },
                    NodeType::Block => {
                        if !block_set.contains(node.pkm_id.as_str()) {
                            trace!("🗄️ Archiving deleted block: {}", node.pkm_id);
                            nodes_to_archive.push((pkm_id.clone(), node_idx, node.clone()));
                            archived_blocks += 1;
                        }
                    }
                }
            }
        }
        
        if nodes_to_archive.is_empty() {
            return Ok((0, "No nodes to archive".to_string()));
        }
        
        // Create archive data structure
        let archive_data = serde_json::json!({
            "timestamp": Utc::now().to_rfc3339(),
            "archived_pages": archived_pages,
            "archived_blocks": archived_blocks,
            "nodes": nodes_to_archive.iter().map(|(pkm_id, idx, node)| {
                let edges_out: Vec<_> = self.graph.edges(*idx)
                    .map(|edge| {
                        serde_json::json!({
                            "target": edge.target().index(),
                            "edge_type": edge.weight()
                        })
                    })
                    .collect();
                    
                let edges_in: Vec<_> = self.graph.edges_directed(*idx, petgraph::Direction::Incoming)
                    .map(|edge| {
                        serde_json::json!({
                            "source": edge.source().index(),
                            "edge_type": edge.weight()
                        })
                    })
                    .collect();
                
                serde_json::json!({
                    "pkm_id": pkm_id,
                    "node_index": idx.index(),
                    "node_data": node,
                    "edges_out": edges_out,
                    "edges_in": edges_in
                })
            }).collect::<Vec<_>>()
        });
        
        // Save archive file
        let archive_filename = format!("archive_{}.json", Utc::now().format("%Y%m%d_%H%M%S"));
        let archive_path = self.data_dir.join("archived_nodes").join(&archive_filename);
        
        trace!("📁 Creating archive file: {}", archive_path.display());
        std::fs::write(&archive_path, serde_json::to_string_pretty(&archive_data)?)?;
        
        // Remove nodes from graph
        for (pkm_id, node_idx, _) in &nodes_to_archive {
            self.graph.remove_node(*node_idx);
            self.pkm_to_node.remove(pkm_id);
        }
        
        // Save updated graph
        self.save_graph()?;
        
        let message = format!("Archived {} pages and {} blocks to {}", 
            archived_pages, archived_blocks, archive_filename);
        Ok((nodes_to_archive.len(), message))
    }
    
    /// Create or update a node from PKM block data
    pub fn create_or_update_node_from_pkm_block(&mut self, block_data: &PKMBlockData) -> GraphResult<NodeIndex> {
        let pkm_id = &block_data.id;
        
        // Generate internal ID (compatible with datastore)
        let internal_id = if let Some(&node_idx) = self.pkm_to_node.get(pkm_id) {
            // Node exists, get its internal ID
            self.graph.node_weight(node_idx)
                .map(|node| node.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        } else {
            // New node, generate ID
            uuid::Uuid::new_v4().to_string()
        };
        
        // Create node data
        let node_data = NodeData {
            id: internal_id,
            pkm_id: pkm_id.clone(),
            node_type: NodeType::Block,
            content: block_data.content.clone(),
            properties: parse_properties(&block_data.properties),
            created_at: parse_datetime(&block_data.created),
            updated_at: parse_datetime(&block_data.updated),
        };
        
        // Update or create node
        let node_idx = if let Some(&existing_idx) = self.pkm_to_node.get(pkm_id) {
            // Update existing node
            if let Some(node) = self.graph.node_weight_mut(existing_idx) {
                *node = node_data;
            }
            existing_idx
        } else {
            // Create new node
            let idx = self.graph.add_node(node_data);
            self.pkm_to_node.insert(pkm_id.clone(), idx);
            idx
        };
        
        // Process parent-child relationships
        if let Some(parent_id) = &block_data.parent {
            // Ensure parent exists (might be a block)
            if let Some(&parent_idx) = self.pkm_to_node.get(parent_id) {
                // Add parent-child edge if it doesn't exist
                if !self.has_edge(parent_idx, node_idx, &EdgeType::ParentChild) {
                    self.graph.add_edge(parent_idx, node_idx, EdgeData {
                        edge_type: EdgeType::ParentChild,
                        weight: 1.0,
                    });
                }
            }
        }
        
        // Process page relationship
        if let Some(page_name) = &block_data.page {
            // Ensure the page exists
            let page_idx = self.ensure_page_exists(page_name);
            
            // If this is a root block (no parent), add page-to-block edge
            if block_data.parent.is_none() {
                if !self.has_edge(page_idx, node_idx, &EdgeType::PageToBlock) {
                    self.graph.add_edge(page_idx, node_idx, EdgeData {
                        edge_type: EdgeType::PageToBlock,
                        weight: 1.0,
                    });
                }
            }
        }
        
        // Process references
        for reference in &block_data.references {
            self.resolve_and_add_reference(node_idx, reference)?;
        }
        
        // Increment operation counter and save if needed
        self.operations_since_save += 1;
        self.save_if_needed();
        
        Ok(node_idx)
    }
    
    /// Create or update a node from PKM page data
    pub fn create_or_update_node_from_pkm_page(&mut self, page_data: &PKMPageData) -> GraphResult<NodeIndex> {
        let page_name = &page_data.name;
        let normalized_name_owned = page_name.to_lowercase();
        let normalized_name = page_data.normalized_name.as_ref()
            .unwrap_or(&normalized_name_owned);
        
        // Check if node exists under original name or normalized name
        let existing_node = self.pkm_to_node.get(page_name)
            .or_else(|| self.pkm_to_node.get(normalized_name));
        
        // Generate internal ID (compatible with datastore)
        let internal_id = if let Some(&node_idx) = existing_node {
            // Node exists, get its internal ID
            self.graph.node_weight(node_idx)
                .map(|node| node.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        } else {
            // New node, generate ID
            uuid::Uuid::new_v4().to_string()
        };
        
        // Create node data
        let node_data = NodeData {
            id: internal_id,
            pkm_id: page_name.clone(),
            node_type: NodeType::Page,
            content: page_name.clone(),
            properties: parse_properties(&page_data.properties),
            created_at: parse_datetime(&page_data.created),
            updated_at: parse_datetime(&page_data.updated),
        };
        
        // Update or create node
        let node_idx = if let Some(&existing_idx) = existing_node {
            // Update existing node
            if let Some(node) = self.graph.node_weight_mut(existing_idx) {
                *node = node_data;
            }
            // Update mapping to use normalized name
            self.pkm_to_node.insert(normalized_name.to_string(), existing_idx);
            existing_idx
        } else {
            // Create new node
            let idx = self.graph.add_node(node_data);
            // Insert with normalized name for consistent lookups
            self.pkm_to_node.insert(normalized_name.to_string(), idx);
            idx
        };
        
        // Process root blocks
        for block_id in &page_data.blocks {
            if let Some(&block_idx) = self.pkm_to_node.get(block_id) {
                // Add page-to-block edge if it doesn't exist
                if !self.has_edge(node_idx, block_idx, &EdgeType::PageToBlock) {
                    self.graph.add_edge(node_idx, block_idx, EdgeData {
                        edge_type: EdgeType::PageToBlock,
                        weight: 1.0,
                    });
                }
            }
        }
        
        // Increment operation counter and save if needed
        self.operations_since_save += 1;
        self.save_if_needed();
        
        Ok(node_idx)
    }
    
    /// Ensure a page exists in our graph, creating it if necessary
    fn ensure_page_exists(&mut self, page_name: &str) -> NodeIndex {
        let normalized_name = page_name.to_lowercase();
        
        // Check both original and normalized names
        if let Some(&idx) = self.pkm_to_node.get(page_name)
            .or_else(|| self.pkm_to_node.get(&normalized_name)) {
            return idx;
        }
        
        // Page doesn't exist, create a placeholder
        let node_data = NodeData {
            id: uuid::Uuid::new_v4().to_string(),
            pkm_id: page_name.to_string(),
            node_type: NodeType::Page,
            content: page_name.to_string(),
            properties: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        let idx = self.graph.add_node(node_data);
        // Use normalized name for consistent lookups
        self.pkm_to_node.insert(normalized_name, idx);
        
        idx
    }
    
    /// Check if an edge of a specific type exists between two nodes
    fn has_edge(&self, source: NodeIndex, target: NodeIndex, edge_type: &EdgeType) -> bool {
        self.graph.edges_connecting(source, target)
            .any(|edge| edge.weight().edge_type == *edge_type)
    }
    
    /// Resolve a PKM reference to graph indices and add appropriate edges
    fn resolve_and_add_reference(&mut self, source_idx: NodeIndex, reference: &PKMReference) -> GraphResult<()> {
        match reference.r#type.as_str() {
            "page" => {
                // Ensure the referenced page exists
                let target_idx = self.ensure_page_exists(&reference.name);
                
                // Add page reference edge
                if !self.has_edge(source_idx, target_idx, &EdgeType::PageRef) {
                    self.graph.add_edge(source_idx, target_idx, EdgeData {
                        edge_type: EdgeType::PageRef,
                        weight: 1.0,
                    });
                }
            },
            "block" => {
                // Check if we know about this block
                let target_idx = if let Some(&idx) = self.pkm_to_node.get(&reference.id) {
                    idx
                } else {
                    // Block doesn't exist yet, create a placeholder
                    let node_data = NodeData {
                        id: uuid::Uuid::new_v4().to_string(),
                        pkm_id: reference.id.clone(),
                        node_type: NodeType::Block,
                        content: String::new(),
                        properties: HashMap::new(),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    };
                    
                    let idx = self.graph.add_node(node_data);
                    self.pkm_to_node.insert(reference.id.clone(), idx);
                    idx
                };
                
                // Add block reference edge
                if !self.has_edge(source_idx, target_idx, &EdgeType::BlockRef) {
                    self.graph.add_edge(source_idx, target_idx, EdgeData {
                        edge_type: EdgeType::BlockRef,
                        weight: 1.0,
                    });
                }
            },
            "tag" => {
                // Tags are treated as special pages
                let tag_name = reference.name.clone();
                let target_idx = self.ensure_page_exists(&tag_name);
                
                // Add tag edge
                if !self.has_edge(source_idx, target_idx, &EdgeType::Tag) {
                    self.graph.add_edge(source_idx, target_idx, EdgeData {
                        edge_type: EdgeType::Tag,
                        weight: 1.0,
                    });
                }
            },
            "property" => {
                // Properties could be handled in various ways
                // For now, we'll just store them in the node's properties map
                // and not create explicit edges
            },
            _ => {
                return Err(GraphError::ReferenceResolution(
                    format!("Unknown reference type: {}", reference.r#type)
                ));
            }
        }
        
        Ok(())
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
    
    /// Create test block data
    fn create_test_block(id: &str, content: &str) -> PKMBlockData {
        PKMBlockData {
            id: id.to_string(),
            content: content.to_string(),
            created: "2024-01-01T00:00:00Z".to_string(),
            updated: "2024-01-01T00:00:00Z".to_string(),
            parent: None,
            children: vec![],
            page: Some("TestPage".to_string()),
            properties: serde_json::json!({}),
            references: vec![],
        }
    }
    
    /// Create test page data
    fn create_test_page(name: &str) -> PKMPageData {
        PKMPageData {
            name: name.to_string(),
            normalized_name: Some(name.to_lowercase()),
            created: "2024-01-01T00:00:00Z".to_string(),
            updated: "2024-01-01T00:00:00Z".to_string(),
            properties: serde_json::json!({}),
            blocks: vec![],
        }
    }
    
    #[test]
    fn test_create_page_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        let page = create_test_page("TestPage");
        
        let node_idx = manager.create_or_update_node_from_pkm_page(&page).unwrap();
        
        // Verify node was created
        let node = manager.graph.node_weight(node_idx).unwrap();
        assert_eq!(node.pkm_id, "TestPage");
        assert_eq!(node.content, "TestPage");
        assert_eq!(node.node_type, NodeType::Page);
        
        // Verify mapping was created with normalized name
        assert_eq!(manager.pkm_to_node.get("testpage"), Some(&node_idx));
    }
    
    #[test]
    fn test_create_block_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        let block = create_test_block("block-123", "Test content");
        
        let node_idx = manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify node was created
        let node = manager.graph.node_weight(node_idx).unwrap();
        assert_eq!(node.pkm_id, "block-123");
        assert_eq!(node.content, "Test content");
        assert_eq!(node.node_type, NodeType::Block);
        
        // Verify page was auto-created
        assert!(manager.pkm_to_node.contains_key("testpage"));
    }
    
    #[test]
    fn test_parent_child_relationship() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create parent block
        let parent = create_test_block("parent-123", "Parent content");
        let parent_idx = manager.create_or_update_node_from_pkm_block(&parent).unwrap();
        
        // Create child block
        let mut child = create_test_block("child-456", "Child content");
        child.parent = Some("parent-123".to_string());
        let child_idx = manager.create_or_update_node_from_pkm_block(&child).unwrap();
        
        // Verify edge exists
        let edges: Vec<_> = manager.graph.edges_connecting(parent_idx, child_idx).collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight().edge_type, EdgeType::ParentChild);
    }
    
    #[test]
    fn test_page_to_block_relationship() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create page first
        let page = create_test_page("TestPage");
        let page_idx = manager.create_or_update_node_from_pkm_page(&page).unwrap();
        
        // Create block without parent (root block)
        let block = create_test_block("block-123", "Root block");
        let block_idx = manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify page-to-block edge exists
        let edges: Vec<_> = manager.graph.edges_connecting(page_idx, block_idx).collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight().edge_type, EdgeType::PageToBlock);
    }
    
    #[test]
    fn test_tag_reference() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let mut block = create_test_block("block-123", "Content with #philosophy");
        block.references = vec![
            PKMReference {
                r#type: "tag".to_string(),
                name: "philosophy".to_string(),
                id: String::new(),
            }
        ];
        
        let block_idx = manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify tag page was created
        assert!(manager.pkm_to_node.contains_key("philosophy"));
        let tag_idx = manager.pkm_to_node["philosophy"];
        
        // Verify tag edge exists
        let edges: Vec<_> = manager.graph.edges_connecting(block_idx, tag_idx).collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight().edge_type, EdgeType::Tag);
    }
    
    #[test]
    fn test_page_reference() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let mut block = create_test_block("block-123", "See [[Another Page]]");
        block.references = vec![
            PKMReference {
                r#type: "page".to_string(),
                name: "Another Page".to_string(),
                id: String::new(),
            }
        ];
        
        let block_idx = manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify referenced page was created (normalized to lowercase)
        assert!(manager.pkm_to_node.contains_key("another page"));
        let ref_page_idx = manager.pkm_to_node["another page"];
        
        // Verify page reference edge exists
        let edges: Vec<_> = manager.graph.edges_connecting(block_idx, ref_page_idx).collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight().edge_type, EdgeType::PageRef);
    }
    
    #[test]
    fn test_block_reference() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create target block first
        let target = create_test_block("target-789", "Target content");
        let target_idx = manager.create_or_update_node_from_pkm_block(&target).unwrap();
        
        // Create block with reference
        let mut source = create_test_block("source-123", "See ((target-789))");
        source.references = vec![
            PKMReference {
                r#type: "block".to_string(),
                name: String::new(),
                id: "target-789".to_string(),
            }
        ];
        
        let source_idx = manager.create_or_update_node_from_pkm_block(&source).unwrap();
        
        // Verify block reference edge exists
        let edges: Vec<_> = manager.graph.edges_connecting(source_idx, target_idx).collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight().edge_type, EdgeType::BlockRef);
    }
    
    #[test]
    fn test_properties_storage() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let mut block = create_test_block("block-123", "Task content");
        block.properties = serde_json::json!({
            "status": "in-progress",
            "priority": "high"
        });
        
        let node_idx = manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify properties were stored
        let node = manager.graph.node_weight(node_idx).unwrap();
        assert_eq!(node.properties.get("status"), Some(&"in-progress".to_string()));
        assert_eq!(node.properties.get("priority"), Some(&"high".to_string()));
    }
    
    #[test]
    fn test_update_existing_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create initial block
        let block1 = create_test_block("block-123", "Original content");
        let idx1 = manager.create_or_update_node_from_pkm_block(&block1).unwrap();
        
        // Update the same block
        let mut block2 = create_test_block("block-123", "Updated content");
        block2.updated = "2024-01-02T00:00:00Z".to_string();
        let idx2 = manager.create_or_update_node_from_pkm_block(&block2).unwrap();
        
        // Should be the same node index
        assert_eq!(idx1, idx2);
        
        // Content should be updated
        let node = manager.graph.node_weight(idx1).unwrap();
        assert_eq!(node.content, "Updated content");
    }
    
    #[test]
    fn test_save_and_load_graph() {
        let temp_dir = TempDir::new().unwrap();
        let node_count;
        let edge_count;
        
        // Create and populate graph
        {
            let mut manager = GraphManager::new(temp_dir.path()).unwrap();
            
            // Add some data
            let page = create_test_page("TestPage");
            manager.create_or_update_node_from_pkm_page(&page).unwrap();
            
            let block = create_test_block("block-123", "Test content");
            manager.create_or_update_node_from_pkm_block(&block).unwrap();
            
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
            assert!(manager.pkm_to_node.contains_key("testpage"));
            assert!(manager.pkm_to_node.contains_key("block-123"));
        }
    }
    
    #[test]
    fn test_operation_based_save() {
        let (mut manager, temp_dir) = create_test_manager();
        
        // Reset operations counter
        manager.operations_since_save = 0;
        
        // Add 9 blocks - should not trigger save
        for i in 0..9 {
            let block = create_test_block(&format!("block-{}", i), "Content");
            manager.create_or_update_node_from_pkm_block(&block).unwrap();
        }
        
        // Verify no save yet
        assert_eq!(manager.operations_since_save, 9);
        
        // Add 10th block - should trigger save
        let block = create_test_block("block-9", "Content");
        manager.create_or_update_node_from_pkm_block(&block).unwrap();
        
        // Verify save was triggered (counter reset)
        assert_eq!(manager.operations_since_save, 0);
        
        // Verify file exists
        assert!(temp_dir.path().join("knowledge_graph.json").exists());
    }
    
    #[test]
    fn test_no_duplicate_edges() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create blocks with same reference twice
        let mut block1 = create_test_block("block-1", "See [[Target]]");
        block1.references = vec![
            PKMReference {
                r#type: "page".to_string(),
                name: "Target".to_string(),
                id: String::new(),
            }
        ];
        
        let mut block2 = create_test_block("block-2", "Also see [[Target]]");
        block2.references = vec![
            PKMReference {
                r#type: "page".to_string(),
                name: "Target".to_string(),
                id: String::new(),
            }
        ];
        
        let idx1 = manager.create_or_update_node_from_pkm_block(&block1).unwrap();
        let idx2 = manager.create_or_update_node_from_pkm_block(&block2).unwrap();
        let target_idx = manager.pkm_to_node["target"];
        
        // Each block should have exactly one edge to Target
        let edges1: Vec<_> = manager.graph.edges_connecting(idx1, target_idx).collect();
        let edges2: Vec<_> = manager.graph.edges_connecting(idx2, target_idx).collect();
        assert_eq!(edges1.len(), 1);
        assert_eq!(edges2.len(), 1);
        
        // Update block1 with same reference - should not create duplicate edge
        manager.create_or_update_node_from_pkm_block(&block1).unwrap();
        let edges1_after: Vec<_> = manager.graph.edges_connecting(idx1, target_idx).collect();
        assert_eq!(edges1_after.len(), 1);
    }
    
    #[test]
    fn test_sync_status() {
        let (mut manager, _temp_dir) = create_test_manager();
        let config = crate::config::SyncConfig::default();
        
        // Initially no sync
        let status = manager.get_sync_status(&config);
        assert_eq!(status["incremental_sync_needed"], true);
        assert_eq!(status["true_full_sync_needed"], false); // Disabled by default
        assert_eq!(status["node_count"], 0);
        assert_eq!(status["edge_count"], 0);
        
        // Add some data
        let page = create_test_page("TestPage");
        manager.create_or_update_node_from_pkm_page(&page).unwrap();
        
        // Update incremental sync timestamp
        manager.update_incremental_sync_timestamp().unwrap();
        
        let status = manager.get_sync_status(&config);
        assert_eq!(status["incremental_sync_needed"], false);
        assert_eq!(status["node_count"], 1);
        assert!(status["last_incremental_sync"].as_i64().is_some());
        
        // Test with full sync enabled
        let mut full_config = config;
        full_config.enable_full_sync = true;
        let status = manager.get_sync_status(&full_config);
        assert_eq!(status["true_full_sync_needed"], true); // No previous full sync
        
        // Update full sync timestamp
        manager.update_full_sync_timestamp().unwrap();
        let status = manager.get_sync_status(&full_config);
        assert_eq!(status["true_full_sync_needed"], false);
    }
    
    #[test]
    fn test_real_graph_traversal() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Build a more complex graph structure
        // Page1
        //   ├─ Block1 (tags: #rust, #programming)
        //   │    └─ Block2 (references [[Page2]])
        //   └─ Block3 (references ((Block1)))
        
        // Create pages
        let page1 = create_test_page("Page1");
        let page1_idx = manager.create_or_update_node_from_pkm_page(&page1).unwrap();
        
        // Block1 with tags
        let mut block1 = create_test_block("block-1", "Learning about rust");
        block1.page = Some("Page1".to_string());
        block1.references = vec![
            PKMReference {
                r#type: "tag".to_string(),
                name: "rust".to_string(),
                id: String::new(),
            },
            PKMReference {
                r#type: "tag".to_string(),
                name: "programming".to_string(),
                id: String::new(),
            },
        ];
        let block1_idx = manager.create_or_update_node_from_pkm_block(&block1).unwrap();
        
        // Block2 as child of Block1
        let mut block2 = create_test_block("block-2", "See [[Page2]] for more");
        block2.parent = Some("block-1".to_string());
        block2.page = Some("Page1".to_string());
        block2.references = vec![
            PKMReference {
                r#type: "page".to_string(),
                name: "Page2".to_string(),
                id: String::new(),
            },
        ];
        let block2_idx = manager.create_or_update_node_from_pkm_block(&block2).unwrap();
        
        // Block3 referencing Block1
        let mut block3 = create_test_block("block-3", "As mentioned in ((block-1))");
        block3.page = Some("Page1".to_string());
        block3.references = vec![
            PKMReference {
                r#type: "block".to_string(),
                name: String::new(),
                id: "block-1".to_string(),
            },
        ];
        let block3_idx = manager.create_or_update_node_from_pkm_block(&block3).unwrap();
        
        // Now let's verify the ACTUAL graph structure using petgraph APIs
        
        // Check total counts
        assert_eq!(manager.graph.node_count(), 7); // Page1, Page2, rust, programming, block1, block2, block3
        assert_eq!(manager.graph.edge_count(), 7); // Various relationships
        
        // Use petgraph's neighbors() to check outgoing edges
        let page1_neighbors: Vec<_> = manager.graph.neighbors(page1_idx).collect();
        assert_eq!(page1_neighbors.len(), 2); // Should connect to block1 and block3
        assert!(page1_neighbors.contains(&block1_idx));
        assert!(page1_neighbors.contains(&block3_idx));
        
        // Check block1's outgoing edges
        let block1_neighbors: Vec<_> = manager.graph.neighbors(block1_idx).collect();
        assert_eq!(block1_neighbors.len(), 3); // block2, rust tag, programming tag
        
        // Verify we can traverse from block3 to block1 via block reference
        let block3_neighbors: Vec<_> = manager.graph.neighbors(block3_idx).collect();
        assert!(block3_neighbors.contains(&block1_idx));
        
        // Use petgraph's edges() to inspect edge types
        let block1_edges: Vec<_> = manager.graph.edges(block1_idx).collect();
        let tag_edges: Vec<_> = block1_edges.iter()
            .filter(|e| e.weight().edge_type == EdgeType::Tag)
            .collect();
        assert_eq!(tag_edges.len(), 2); // Two tag edges
        
        // Verify parent-child edge
        let parent_child_edges: Vec<_> = manager.graph.edges_connecting(block1_idx, block2_idx).collect();
        assert_eq!(parent_child_edges.len(), 1);
        assert_eq!(parent_child_edges[0].weight().edge_type, EdgeType::ParentChild);
        
    }
}