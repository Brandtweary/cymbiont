//! Graph persistence utilities for save/load operations

use std::path::Path;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, trace};

use crate::graph_manager::{KnowledgeGraph, NodeMap, NodeData, GraphError, GraphResult, NodeType};

/// Data returned when loading a graph from disk
#[derive(Debug)]
pub struct LoadedGraphData {
    pub graph: KnowledgeGraph,
    pub pkm_to_node: NodeMap,
}

/// Load a graph from the given directory
pub fn load_graph(data_dir: &Path) -> GraphResult<LoadedGraphData> {
    let graph_path = data_dir.join("knowledge_graph.json");
    
    if !graph_path.exists() {
        return Err(GraphError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "Graph file not found"
        )));
    }
    
    let mut file = File::open(graph_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    
    #[derive(Deserialize)]
    struct SerializedGraph {
        graph: KnowledgeGraph,
        pkm_to_node: NodeMap,
        #[serde(default)]
        version: u32,
    }
    
    let serialized: SerializedGraph = serde_json::from_str(&contents)?;
    
    if serialized.version > 1 {
        warn!("Graph file version {} is newer than supported version 1", serialized.version);
    }
    
    info!("📊 Loaded graph with {} nodes and {} edges", 
          serialized.graph.node_count(), serialized.graph.edge_count());
    
    Ok(LoadedGraphData {
        graph: serialized.graph,
        pkm_to_node: serialized.pkm_to_node,
    })
}

/// Save a graph to the given directory
pub fn save_graph(
    data_dir: &Path,
    graph: &KnowledgeGraph,
    pkm_to_node: &NodeMap,
) -> GraphResult<()> {
    let graph_path = data_dir.join("knowledge_graph.json");
    
    #[derive(Serialize)]
    struct SerializedGraph<'a> {
        graph: &'a KnowledgeGraph,
        pkm_to_node: &'a NodeMap,
        version: u32,
    }
    
    let serialized = SerializedGraph {
        graph,
        pkm_to_node,
        version: 1,
    };
    
    let json = serde_json::to_string_pretty(&serialized)?;
    let mut file = File::create(&graph_path)?;
    file.write_all(json.as_bytes())?;
    
    Ok(())
}

/// Archive nodes to a timestamped JSON file
pub fn archive_nodes(
    data_dir: &Path,
    graph_id: Option<&str>,
    nodes: &[(String, NodeData, Vec<serde_json::Value>, Vec<serde_json::Value>)],
) -> GraphResult<String> {
    if nodes.is_empty() {
        return Ok("No nodes to archive".to_string());
    }
    
    let archive_dir = data_dir.join("archived_nodes");
    fs::create_dir_all(&archive_dir)?;
    
    // Count node types
    let (archived_pages, archived_blocks) = nodes.iter()
        .fold((0, 0), |(pages, blocks), (_, node, _, _)| {
            match node.node_type {
                NodeType::Page => (pages + 1, blocks),
                NodeType::Block => (pages, blocks + 1),
            }
        });
    
    // Build archive JSON
    let archive_nodes: Vec<_> = nodes.iter()
        .map(|(pkm_id, node_data, edges_out, edges_in)| {
            serde_json::json!({
                "pkm_id": pkm_id,
                "node_data": node_data,
                "edges_out": edges_out,
                "edges_in": edges_in,
            })
        })
        .collect();
    
    let mut archive_data = serde_json::json!({
        "timestamp": Utc::now().to_rfc3339(),
        "archived_pages": archived_pages,
        "archived_blocks": archived_blocks,
        "nodes": archive_nodes,
    });
    
    if let Some(id) = graph_id {
        archive_data["graph_id"] = serde_json::json!(id);
    }
    
    // Save archive file
    let archive_filename = format!("archive_{}.json", Utc::now().format("%Y%m%d_%H%M%S"));
    let archive_path = archive_dir.join(&archive_filename);
    
    trace!("📁 Creating archive file: {}", archive_path.display());
    fs::write(&archive_path, serde_json::to_string_pretty(&archive_data)?)?;
    
    Ok(format!("Archived {} pages and {} blocks to {}", 
        archived_pages, archived_blocks, archive_filename))
}

/// Ensure required directories exist
pub fn ensure_directories(data_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    fs::create_dir_all(data_dir.join("archived_nodes"))?;
    Ok(())
}

/// Check if we should save based on time or operation count
pub fn should_save(last_save_time: DateTime<Utc>, operations_since_save: usize) -> bool {
    const SAVE_INTERVAL_MINUTES: i64 = 5;
    const SAVE_OPERATION_THRESHOLD: usize = 10;
    
    let minutes_since_save = (Utc::now() - last_save_time).num_minutes();
    if minutes_since_save >= SAVE_INTERVAL_MINUTES {
        return true;
    }
    
    if operations_since_save >= SAVE_OPERATION_THRESHOLD {
        return true;
    }
    
    false
}