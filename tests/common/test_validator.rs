//! JSON-Based Test Validation System
//!
//! Direct validation against JSON persistence files for the single-agent architecture.
//!
//! ## Architecture
//!
//! The test validator reads JSON files directly from the data directory and validates
//! that expected operations resulted in correct state changes. This is simpler than
//! and matches the JSON-based persistence model.
//!
//! ## Usage Example
//!
//! ```rust
//! let mut validator = TestValidator::new(&test_env.data_dir);
//!
//! // Set up expectations
//! validator.expect_create_page("test-page", None);
//! validator.expect_create_block("block-id", "content", Some("test-page"));
//! validator.expect_user_message(MessagePattern::Exact("Hello"));
//!
//! // Run operations...
//!
//! // Validate all expectations were met
//! validator.validate_all();
//! ```

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;


// ===== JSON File Reader =====

/// Read JSON files from the data directory
struct JsonReader;

impl JsonReader {

    /// Read the graph registry file
    fn read_graph_registry(data_dir: &Path) -> Result<Value, String> {
        let registry_path = data_dir.join("graph_registry.json");
        let content = fs::read_to_string(&registry_path)
            .map_err(|e| format!("Failed to read graph_registry.json: {e}"))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse graph_registry.json: {e}"))
    }

    /// Read a specific graph's data
    fn read_graph_data(data_dir: &Path, graph_id: &str) -> Result<Value, String> {
        let graph_path = data_dir
            .join("graphs")
            .join(graph_id)
            .join("knowledge_graph.json");
        let content = fs::read_to_string(&graph_path)
            .map_err(|e| format!("Failed to read graph {graph_id}: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse graph {graph_id}: {e}"))
    }
}

// ===== Expected Operations =====

/// Expected graph operation for validation
#[derive(Debug, Clone)]
pub struct ExpectedGraphOp {
    pub graph_id: Option<String>, // Which graph this operation belongs to (None = auto-detect single graph)
    pub op_type: GraphOpType,
    pub content: Option<String>,
    pub page_name: Option<String>,
    pub block_id: Option<String>,
    pub properties: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphOpType {
    CreatePage,
    CreateBlock,
    UpdateBlock,
    DeleteBlock,
    DeletePage,
}


/// Expected graph registry entry
#[derive(Debug, Clone)]
pub struct ExpectedGraph {
    pub name: String,
    pub is_open: bool,
}


// ===== Main Test Validator =====

/// Main test validator for JSON-based persistence
pub struct TestValidator {
    data_dir: PathBuf,

    // Graph expectations
    expected_graph_ops: Vec<ExpectedGraphOp>,
    deleted_nodes: HashSet<String>,


    // Graph registry expectations
    expected_graphs: HashMap<Uuid, ExpectedGraph>,
    expected_open_graphs: HashSet<Uuid>,
    deleted_graphs: HashSet<Uuid>,
}

impl TestValidator {
    /// Create a new test validator for the given data directory
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            expected_graph_ops: Vec::new(),
            deleted_nodes: HashSet::new(),
            expected_graphs: HashMap::new(),
            expected_open_graphs: HashSet::new(),
            deleted_graphs: HashSet::new(),
        }
    }

    /// Consolidate layered operations to keep only the final state for each entity
    /// This handles cases where the same block/page is created, then updated, then deleted
    fn consolidate_operations(&mut self) {
        // Group operations by entity (block_id or page name)
        let mut entity_ops: HashMap<String, Vec<usize>> = HashMap::new();

        for (index, op) in self.expected_graph_ops.iter().enumerate() {
            // Get the entity identifier
            let entity_id = match op.op_type {
                GraphOpType::CreatePage | GraphOpType::DeletePage => {
                    op.content.clone() // Page name is in content
                }
                GraphOpType::CreateBlock | GraphOpType::UpdateBlock | GraphOpType::DeleteBlock => {
                    op.block_id.clone() // Block operations use block_id
                }
            };

            if let Some(id) = entity_id {
                entity_ops.entry(id).or_default().push(index);
            }
        }

        // For each entity with multiple operations, keep only the last one
        let mut indices_to_remove: HashSet<usize> = HashSet::new();
        for op_indices in entity_ops.values() {
            if op_indices.len() > 1 {
                // Mark all but the last operation for removal
                for &index in &op_indices[..op_indices.len() - 1] {
                    indices_to_remove.insert(index);
                }
            }
        }

        // Remove marked operations (in reverse order to maintain indices)
        let mut indices_vec: Vec<usize> = indices_to_remove.into_iter().collect();
        indices_vec.sort_by(|a, b| b.cmp(a)); // Sort descending
        for index in indices_vec {
            self.expected_graph_ops.remove(index);
        }
    }

    // ===== Graph Operation Expectations =====

    /// Record that a page will be created in a specific graph
    pub fn expect_create_page(
        &mut self,
        name: &str,
        properties: Option<Value>,
        graph_id: Option<&str>,
    ) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            graph_id: graph_id.map(std::string::ToString::to_string),
            op_type: GraphOpType::CreatePage,
            content: Some(name.to_string()),
            page_name: None,
            block_id: None,
            properties,
        });
        self.deleted_nodes.remove(name);
        self
    }

    /// Record that a block will be created in a specific graph
    pub fn expect_create_block(
        &mut self,
        block_id: &str,
        content: &str,
        page_name: Option<&str>,
        graph_id: Option<&str>,
    ) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            graph_id: graph_id.map(std::string::ToString::to_string),
            op_type: GraphOpType::CreateBlock,
            content: Some(content.to_string()),
            page_name: page_name.map(std::string::ToString::to_string),
            block_id: Some(block_id.to_string()),
            properties: None,
        });
        self.deleted_nodes.remove(block_id);
        self
    }

    /// Record that a block's content will be updated in a specific graph
    pub fn expect_update_block(
        &mut self,
        block_id: &str,
        new_content: &str,
        graph_id: Option<&str>,
    ) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            graph_id: graph_id.map(std::string::ToString::to_string),
            op_type: GraphOpType::UpdateBlock,
            content: Some(new_content.to_string()),
            page_name: None,
            block_id: Some(block_id.to_string()),
            properties: None,
        });
        self
    }

    /// Record that a block will be deleted in a specific graph
    pub fn expect_delete_block(&mut self, block_id: &str, graph_id: Option<&str>) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            graph_id: graph_id.map(std::string::ToString::to_string),
            op_type: GraphOpType::DeleteBlock,
            block_id: Some(block_id.to_string()),
            content: None,
            page_name: None,
            properties: None,
        });
        self.deleted_nodes.insert(block_id.to_string());
        self
    }

    /// Record that a page will be deleted in a specific graph
    pub fn expect_delete_page(&mut self, page_name: &str, graph_id: Option<&str>) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            graph_id: graph_id.map(std::string::ToString::to_string),
            op_type: GraphOpType::DeletePage,
            block_id: None,
            content: Some(page_name.to_string()),
            page_name: None,
            properties: None,
        });
        self.deleted_nodes.insert(page_name.to_string());
        self
    }

    /// Add expectations for the dummy graph that's imported in tests
    pub fn expect_dummy_graph(&mut self, graph_id: Option<&str>) -> &mut Self {
        self.expect_create_page("cyberorganism-test-1", None, graph_id)
            .expect_create_page("cyberorganism-test-2", None, graph_id)
            .expect_create_page("contents", None, graph_id)
            .expect_create_page("test-websocket", None, graph_id);
        self
    }


    // ===== Graph Registry Expectations =====

    /// Record that a graph will be created
    pub fn expect_graph_created(&mut self, id: Uuid, name: &str) -> &mut Self {
        self.expected_graphs.insert(
            id,
            ExpectedGraph {
                name: name.to_string(),
                is_open: false,
            },
        );
        self.deleted_graphs.remove(&id);
        self
    }

    /// Record that a graph will be opened
    pub fn expect_graph_open(&mut self, id: Uuid) -> &mut Self {
        if let Some(graph) = self.expected_graphs.get_mut(&id) {
            graph.is_open = true;
        }
        self.expected_open_graphs.insert(id);
        self
    }

    /// Record that a graph will be closed
    pub fn expect_graph_closed(&mut self, id: Uuid) -> &mut Self {
        if let Some(graph) = self.expected_graphs.get_mut(&id) {
            graph.is_open = false;
        }
        self.expected_open_graphs.remove(&id);
        self
    }

    /// Record that a graph will be deleted
    pub fn expect_graph_deleted(&mut self, id: Uuid) -> &mut Self {
        self.expected_graphs.remove(&id);
        self.expected_open_graphs.remove(&id);
        self.deleted_graphs.insert(id);
        self
    }

    // ===== Main Validation Methods =====

    /// Validate all expectations against JSON files
    pub fn validate_all(&mut self) -> Result<(), String> {
        // Consolidate layered operations before validation
        self.consolidate_operations();

        // Validate graph registry
        self.validate_registry()?;

        // Validate individual graphs
        self.validate_graphs()?;

        Ok(())
    }


    /// Validate `graph_registry.json`
    fn validate_registry(&self) -> Result<(), String> {
        let registry = JsonReader::read_graph_registry(&self.data_dir)?;

        // Validate schema
        let graphs = registry["graphs"]
            .as_object()
            .ok_or("Registry missing graphs object")?;
        let open_graphs = registry["open_graphs"]
            .as_array()
            .ok_or("Registry missing open_graphs array")?;

        // Validate expected graphs exist
        for (id, expected) in &self.expected_graphs {
            let id_str = id.to_string();
            let graph_info = graphs
                .get(&id_str)
                .ok_or_else(|| format!("Expected graph {id_str} not in registry"))?;

            let name = graph_info["name"]
                .as_str()
                .ok_or_else(|| format!("Graph {id_str} missing name"))?;
            if name != expected.name {
                return Err(format!(
                    "Graph {id_str} has name '{name}', expected '{}'",
                    expected.name
                ));
            }
        }

        // Validate deleted graphs don't exist
        for id in &self.deleted_graphs {
            let id_str = id.to_string();
            if graphs.contains_key(&id_str) {
                return Err(format!("Deleted graph {id_str} still in registry"));
            }
        }

        // Validate open graphs
        let open_set: HashSet<String> = open_graphs
            .iter()
            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
            .collect();

        for id in &self.expected_open_graphs {
            let id_str = id.to_string();
            if !open_set.contains(&id_str) {
                return Err(format!("Expected graph {id_str} to be open"));
            }
        }

        Ok(())
    }

    /// Validate individual graph JSON files and graph operations
    fn validate_graphs(&self) -> Result<(), String> {
        // Validate expected graphs exist with proper schema
        for id in self.expected_graphs.keys() {
            let id_str = id.to_string();

            // Try to read the graph file
            let graph_data = JsonReader::read_graph_data(&self.data_dir, &id_str)?;

            // Basic schema validation
            if graph_data["version"].as_u64() != Some(1) {
                return Err(format!("Graph {id_str} missing or invalid version"));
            }

            if graph_data["graph"].is_null() {
                return Err(format!("Graph {id_str} missing graph data"));
            }

            if graph_data["node_index"].is_null() {
                return Err(format!("Graph {id_str} missing node_index mapping"));
            }
        }

        // Now validate all graph operations
        self.validate_graph_operations()?;

        Ok(())
    }

    /// Validate all expected graph operations against their respective graphs
    fn validate_graph_operations(&self) -> Result<(), String> {
        if self.expected_graph_ops.is_empty() {
            return Ok(());
        }

        // Group operations by graph_id
        let mut ops_by_graph: HashMap<Option<String>, Vec<&ExpectedGraphOp>> = HashMap::new();
        for op in &self.expected_graph_ops {
            ops_by_graph
                .entry(op.graph_id.clone())
                .or_default()
                .push(op);
        }

        // Handle operations with explicit graph IDs
        for (graph_id, ops) in &ops_by_graph {
            if let Some(gid) = graph_id {
                self.validate_ops_for_graph(gid, ops)?;
            }
        }

        // Handle operations without explicit graph IDs (auto-detect single open graph)
        if let Some(ops) = ops_by_graph.get(&None) {
            let registry = JsonReader::read_graph_registry(&self.data_dir)?;
            let open_graphs = registry["open_graphs"]
                .as_array()
                .ok_or("Registry missing open_graphs array")?;

            if open_graphs.len() == 1 {
                let graph_id = open_graphs[0]
                    .as_str()
                    .ok_or("Invalid graph ID in open_graphs")?;
                self.validate_ops_for_graph(graph_id, ops)?;
            } else if open_graphs.is_empty() {
                return Err("No open graphs to validate operations against".to_string());
            } else {
                return Err(format!("Multiple open graphs ({}), cannot auto-detect graph for operations without explicit graph_id", open_graphs.len()));
            }
        }

        Ok(())
    }

    /// Helper to validate a `CreatePage` operation
    fn validate_create_page(
        &self,
        expected: &ExpectedGraphOp,
        nodes: &[Value],
        node_index: &serde_json::Map<String, Value>,
        graph_id: &str,
    ) -> Result<(), String> {
        let page_name = expected
            .content
            .as_ref()
            .ok_or("CreatePage expectation missing page name")?;

        let page_name_lower = page_name.to_lowercase();
        let found_node = node_index
            .iter()
            .find(|(key, _)| key.to_lowercase() == page_name_lower)
            .map(|(_, idx)| idx);

        if let Some(node_idx) = found_node {
            #[allow(clippy::cast_possible_truncation)]
            let node_idx = node_idx.as_u64().ok_or("Invalid node index")? as usize;
            let node = nodes
                .get(node_idx)
                .ok_or_else(|| format!("Node index {node_idx} out of bounds"))?;

            if node["node_type"].as_str() != Some("Page") {
                return Err(format!(
                    "Expected page '{}' in graph {} but found {}",
                    page_name, graph_id, node["node_type"]
                ));
            }

            if let Some(expected_props) = &expected.properties {
                let actual_props = &node["properties"];
                if actual_props != expected_props {
                    return Err(format!(
                        "Page '{page_name}' in graph {graph_id} properties mismatch"
                    ));
                }
            }
        } else if !self.deleted_nodes.contains(page_name) {
            return Err(format!(
                "Expected page '{page_name}' not found in graph {graph_id}"
            ));
        }
        Ok(())
    }

    /// Helper to validate a `CreateBlock` operation
    fn validate_create_block(
        &self,
        expected: &ExpectedGraphOp,
        nodes: &[Value],
        edges: &[Value],
        node_index: &serde_json::Map<String, Value>,
        graph_id: &str,
    ) -> Result<(), String> {
        let block_id = expected
            .block_id
            .as_ref()
            .ok_or("CreateBlock expectation missing block_id")?;
        let content = expected
            .content
            .as_ref()
            .ok_or("CreateBlock expectation missing content")?;

        if let Some(node_idx) = node_index.get(block_id) {
            #[allow(clippy::cast_possible_truncation)]
            let node_idx = node_idx.as_u64().ok_or("Invalid node index")? as usize;
            let node = nodes
                .get(node_idx)
                .ok_or_else(|| format!("Node index {node_idx} out of bounds"))?;

            if node["node_type"].as_str() != Some("Block") {
                return Err(format!(
                    "Expected block '{block_id}' in graph {graph_id} but found {}",
                    node["node_type"]
                ));
            }

            if node["content"].as_str() != Some(content) {
                return Err(format!("Block '{block_id}' in graph {graph_id} content mismatch: expected '{content}', got '{:?}'",
                    node["content"]));
            }

            if let Some(page_name) = &expected.page_name {
                let page_connected = edges.iter().any(|edge| {
                    if let (Some(source_idx), Some(target_idx)) =
                        (edge[0].as_u64(), edge[1].as_u64())
                    {
                        #[allow(clippy::cast_possible_truncation)]
                        let source_node = &nodes[source_idx as usize];
                        #[allow(clippy::cast_possible_truncation)]
                        let target_node = &nodes[target_idx as usize];

                        // Check both original and normalized (lowercase) page names for Logseq compatibility
                        let source_id = source_node["id"].as_str().unwrap_or("");
                        (source_id == page_name || source_id == page_name.to_lowercase())
                            && target_node["id"].as_str() == Some(block_id)
                            && edge[2]["edge_type"].as_str() == Some("PageToBlock")
                    } else {
                        false
                    }
                });

                if !page_connected {
                    return Err(format!(
                        "Block '{block_id}' in graph {graph_id} not connected to page '{page_name}'"
                    ));
                }
            }
        } else if !self.deleted_nodes.contains(block_id) {
            return Err(format!(
                "Expected block '{block_id}' not found in graph {graph_id}"
            ));
        }
        Ok(())
    }

    /// Helper to validate an `UpdateBlock` operation
    #[allow(clippy::unused_self)]
    fn validate_update_block(
        &self,
        expected: &ExpectedGraphOp,
        nodes: &[Value],
        node_index: &serde_json::Map<String, Value>,
        graph_id: &str,
    ) -> Result<(), String> {
        let block_id = expected
            .block_id
            .as_ref()
            .ok_or("UpdateBlock expectation missing block_id")?;
        let content = expected
            .content
            .as_ref()
            .ok_or("UpdateBlock expectation missing content")?;

        if let Some(node_idx) = node_index.get(block_id) {
            #[allow(clippy::cast_possible_truncation)]
            let node_idx = node_idx.as_u64().ok_or("Invalid node index")? as usize;
            let node = nodes
                .get(node_idx)
                .ok_or_else(|| format!("Node index {node_idx} out of bounds"))?;

            if node["content"].as_str() != Some(content) {
                return Err(format!(
                    "Block '{}' in graph {} not updated: expected '{}', got '{:?}'",
                    block_id, graph_id, content, node["content"]
                ));
            }
        } else {
            return Err(format!(
                "Updated block '{block_id}' not found in graph {graph_id}"
            ));
        }
        Ok(())
    }

    /// Validate a set of operations against a specific graph
    fn validate_ops_for_graph(
        &self,
        graph_id: &str,
        ops: &[&ExpectedGraphOp],
    ) -> Result<(), String> {
        let graph_data = JsonReader::read_graph_data(&self.data_dir, graph_id)?;

        let nodes = graph_data["graph"]["nodes"]
            .as_array()
            .ok_or_else(|| format!("Graph {graph_id} has no nodes"))?;

        let edges = graph_data["graph"]["edges"]
            .as_array()
            .ok_or_else(|| format!("Graph {graph_id} has no edges"))?;

        let node_index = graph_data["node_index"]
            .as_object()
            .ok_or_else(|| format!("Graph {graph_id} has no node_index mapping"))?;

        for expected in ops {
            match expected.op_type {
                GraphOpType::CreatePage => {
                    self.validate_create_page(expected, nodes, node_index, graph_id)?;
                }
                GraphOpType::CreateBlock => {
                    self.validate_create_block(expected, nodes, edges, node_index, graph_id)?;
                }
                GraphOpType::UpdateBlock => {
                    self.validate_update_block(expected, nodes, node_index, graph_id)?;
                }
                GraphOpType::DeleteBlock => {
                    let block_id = expected
                        .block_id
                        .as_ref()
                        .ok_or("DeleteBlock expectation missing block_id")?;

                    if node_index.contains_key(block_id) {
                        return Err(format!(
                            "Block '{block_id}' should be deleted but still exists in graph {graph_id}"
                        ));
                    }
                }
                GraphOpType::DeletePage => {
                    let page_name = expected
                        .content
                        .as_ref()
                        .ok_or("DeletePage expectation missing page name")?;

                    // Check both original and normalized (lowercase) names
                    let page_name_lower = page_name.to_lowercase();
                    if node_index.contains_key(page_name) || node_index.contains_key(&page_name_lower) {
                        return Err(format!(
                            "Page '{page_name}' should be deleted but still exists in graph {graph_id}"
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}
