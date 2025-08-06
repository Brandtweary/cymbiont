//! Graph Validation Helpers
//! 
//! Automated test validation for knowledge graph state after operations.
//! The GraphValidationFixture tracks expected transformations and validates
//! the final graph state, eliminating manual assertions and reducing test brittleness.
//! 
//! ## Usage Example
//! 
//! ```rust
//! use crate::common::GraphValidationFixture;
//! 
//! pub fn test_websocket_operations() {
//!     let mut fixture = GraphValidationFixture::new();
//!     
//!     // Set up expectations for imported dummy graph
//!     fixture.expect_dummy_graph();
//!     
//!     // Test create block
//!     let cmd = json!({"type": "create_block", "content": "Test content", "page_name": "test-page"});
//!     let response = send_command(&mut ws, cmd);
//!     let block_id = expect_success(response).unwrap()["block_id"].as_str().unwrap();
//!     fixture.expect_create_block(block_id, "Test content", Some("test-page"));
//!     
//!     // Test update block
//!     let cmd = json!({"type": "update_block", "block_id": block_id, "content": "Updated content"});
//!     send_command(&mut ws, cmd);
//!     fixture.expect_update_block(block_id, "Updated content");
//!     
//!     // Test delete block  
//!     let cmd = json!({"type": "delete_block", "block_id": "some-block-id"});
//!     send_command(&mut ws, cmd);
//!     fixture.expect_delete("some-block-id");
//!     
//!     // Test custom edges (e.g., parent-child relationships)
//!     fixture.expect_edge("parent-id", "child-id", "ParentChild");
//!     
//!     // Validate everything at once
//!     fixture.validate_graph(&data_dir, &graph_id);
//! }
//! ```

use std::fs;
use std::path::Path;
use std::collections::{HashMap, HashSet};
use serde_json::Value;

/// Validate that the graph registry has the correct schema and contains the expected graph
pub fn validate_registry_schema(data_dir: &Path, graph_id: &str) {
    let registry_path = data_dir.join("graph_registry.json");
    
    // Load and parse registry
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read graph registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse graph registry");
    
    // Validate top-level structure
    assert!(registry["graphs"].is_object(), "Registry must have 'graphs' object");
    assert!(registry["open_graphs"].is_array(), "Registry must have 'open_graphs' array");
    
    // Validate the tested graph exists
    let graphs = registry["graphs"].as_object().unwrap();
    assert!(graphs.contains_key(graph_id), 
        "Graph {} not found in registry", graph_id);
    
    // Validate graph entry schema
    let graph_info = &graphs[graph_id];
    assert_eq!(graph_info["id"].as_str(), Some(graph_id), 
        "Graph ID mismatch in registry");
    assert!(graph_info["name"].is_string(), 
        "Graph must have 'name' field");
    assert!(graph_info["kg_path"].is_string(), 
        "Graph must have 'kg_path' field");
    assert!(graph_info["created"].is_string(), 
        "Graph must have 'created' field");
    assert!(graph_info["last_accessed"].is_string(), 
        "Graph must have 'last_accessed' field");
    
    // Validate open_graphs references valid graphs
    let open_graphs = registry["open_graphs"].as_array().unwrap();
    for open_id in open_graphs {
        let id_str = open_id.as_str()
            .expect("open_graphs must contain strings");
        assert!(graphs.contains_key(id_str), 
            "open_graphs references non-existent graph: {}", id_str);
    }
}

/// Represents the expected state of a graph for validation
pub struct GraphValidator {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    pkm_to_node: HashMap<String, usize>,
}

/// Expected node configuration for validation
#[derive(Debug, Clone)]
pub struct ExpectedNode {
    pub node_type: &'static str,
    pub pkm_id: String,
    pub content: Option<String>,
    pub properties: Option<Value>,
}

/// Expected edge configuration for validation
#[derive(Debug, Clone)]
pub struct ExpectedEdge {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: &'static str,
}

impl GraphValidator {
    /// Load a graph from disk and prepare it for validation
    pub fn load(data_dir: &Path, graph_id: &str) -> Self {
        let graph_path = data_dir.join("graphs")
            .join(graph_id)
            .join("knowledge_graph.json");
        
        let graph_content = fs::read_to_string(&graph_path)
            .expect("Failed to read knowledge graph");
        
        let graph: Value = serde_json::from_str(&graph_content)
            .expect("Failed to parse knowledge graph");
        
        let nodes = graph["graph"]["nodes"].as_array()
            .expect("No nodes in graph")
            .to_vec();
        
        let edges = graph["graph"]["edges"].as_array()
            .expect("No edges in graph")
            .to_vec();
        
        // Extract pkm_to_node mapping
        let pkm_to_node = graph["pkm_to_node"].as_object()
            .expect("No pkm_to_node in graph")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_u64().expect("Invalid node index") as usize))
            .collect();
        
        Self {
            nodes,
            edges,
            pkm_to_node,
        }
    }
    
    /// Assert that a node exists with the expected properties
    pub fn assert_node_exists(&self, expected: &ExpectedNode) {
        let node = self.find_node(&expected.pkm_id, expected.node_type)
            .unwrap_or_else(|| panic!(
                "Node not found - type: {}, id: {}", 
                expected.node_type, expected.pkm_id
            ));
        
        // Verify content if specified
        if let Some(expected_content) = &expected.content {
            assert_eq!(
                node["content"].as_str(),
                Some(expected_content.as_str()),
                "Content mismatch for node {}",
                expected.pkm_id
            );
        }
        
        // Verify properties if specified
        if let Some(expected_props) = &expected.properties {
            if let Some(props_obj) = expected_props.as_object() {
                for (key, value) in props_obj {
                    assert_eq!(
                        &node["properties"][key],
                        value,
                        "Property '{}' mismatch for node {}",
                        key, expected.pkm_id
                    );
                }
            }
        }
    }
    
    
    /// Assert that an edge exists between two nodes
    pub fn assert_edge_exists(&self, expected: &ExpectedEdge) {
        let source_index = self.get_node_index(&expected.source_id)
            .unwrap_or_else(|| panic!("Source node not found: {}", expected.source_id));
        
        let target_index = self.get_node_index(&expected.target_id)
            .unwrap_or_else(|| panic!("Target node not found: {}", expected.target_id));
        
        let edge = self.find_edge(source_index, target_index)
            .unwrap_or_else(|| panic!(
                "Edge not found from {} to {}", 
                expected.source_id, expected.target_id
            ));
        
        assert_eq!(
            edge[2]["edge_type"].as_str(),
            Some(expected.edge_type),
            "Edge type mismatch for {} -> {}",
            expected.source_id, expected.target_id
        );
    }
    
    /// Find a node by ID and type
    fn find_node(&self, pkm_id: &str, node_type: &str) -> Option<&Value> {
        self.nodes.iter().find(|n| {
            n["node_type"].as_str() == Some(node_type) && 
            n["pkm_id"].as_str() == Some(pkm_id)
        })
    }
    
    /// Get the index of a node by ID
    fn get_node_index(&self, pkm_id: &str) -> Option<usize> {
        self.pkm_to_node.get(pkm_id).copied()
    }
    
    /// Find an edge between two node indices
    fn find_edge(&self, source_idx: usize, target_idx: usize) -> Option<&Value> {
        self.edges.iter().find(|e| {
            if let (Some(source), Some(target)) = (e[0].as_u64(), e[1].as_u64()) {
                source as usize == source_idx && target as usize == target_idx
            } else {
                false
            }
        })
    }
    
}

/// Test fixture for tracking expected graph transformations and validating final state
pub struct GraphValidationFixture {
    /// Expected nodes after all operations (keyed by pkm_id)
    expected_nodes: HashMap<String, ExpectedNode>,
    /// Expected edges after all operations
    pub expected_edges: Vec<ExpectedEdge>,
    /// Node IDs that should NOT exist in the final graph
    deleted_nodes: HashSet<String>,
}

impl GraphValidationFixture {
    pub fn new() -> Self {
        Self {
            expected_nodes: HashMap::new(),
            expected_edges: Vec::new(),
            deleted_nodes: HashSet::new(),
        }
    }
    
    /// Record that a page will be created
    pub fn expect_create_page(&mut self, name: &str, properties: Option<Value>) -> &mut Self {
        self.expected_nodes.insert(name.to_string(), ExpectedNode {
            node_type: "Page",
            pkm_id: name.to_string(),
            content: None,
            properties,
        });
        // Remove from deleted set if it was there
        self.deleted_nodes.remove(name);
        self
    }
    
    /// Record that a block will be created (requires block_id from response)
    pub fn expect_create_block(
        &mut self, 
        block_id: &str, 
        content: &str, 
        page_name: Option<&str>
    ) -> &mut Self {
        self.expected_nodes.insert(block_id.to_string(), ExpectedNode {
            node_type: "Block",
            pkm_id: block_id.to_string(),
            content: Some(content.to_string()),
            properties: None,
        });
        
        // Add edge if page was specified
        if let Some(page) = page_name {
            self.expected_edges.push(ExpectedEdge {
                source_id: page.to_string(),
                target_id: block_id.to_string(),
                edge_type: "PageToBlock",
            });
        }
        
        // Remove from deleted set if it was there
        self.deleted_nodes.remove(block_id);
        self
    }
    
    /// Record that a block's content will be updated
    pub fn expect_update_block(&mut self, block_id: &str, new_content: &str) -> &mut Self {
        if let Some(node) = self.expected_nodes.get_mut(block_id) {
            node.content = Some(new_content.to_string());
        } else {
            // If we're updating a block that wasn't created in this test,
            // we still need to expect it exists with the new content
            self.expected_nodes.insert(block_id.to_string(), ExpectedNode {
                node_type: "Block",
                pkm_id: block_id.to_string(),
                content: Some(new_content.to_string()),
                properties: None,
            });
        }
        self
    }
    
    /// Record that a node will be deleted
    pub fn expect_delete(&mut self, node_id: &str) -> &mut Self {
        // Remove from expected nodes
        self.expected_nodes.remove(node_id);
        
        // Add to deleted set
        self.deleted_nodes.insert(node_id.to_string());
        
        // Remove any edges involving this node
        self.expected_edges.retain(|e| {
            e.source_id != node_id && e.target_id != node_id
        });
        
        self
    }
    
    /// Add an expected edge between two nodes
    pub fn expect_edge(&mut self, source_id: &str, target_id: &str, edge_type: &'static str) -> &mut Self {
        self.expected_edges.push(ExpectedEdge {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            edge_type,
        });
        self
    }
    
    /// Validate that the actual graph matches all expectations
    pub fn validate_graph(&self, data_dir: &Path, graph_id: &str) {
        // First validate the registry structure
        validate_registry_schema(data_dir, graph_id);
        
        // Then validate the graph contents
        let validator = GraphValidator::load(data_dir, graph_id);
        
        // Check all expected nodes exist with correct properties
        for (_, expected) in &self.expected_nodes {
            validator.assert_node_exists(expected);
        }
        
        // Check all deleted nodes don't exist
        for node_id in &self.deleted_nodes {
            // We need to determine the node type - try both Block and Page
            // In a real scenario, we might track the type when deleting
            let block_exists = validator.find_node(node_id, "Block").is_some();
            let page_exists = validator.find_node(node_id, "Page").is_some();
            
            assert!(
                !block_exists && !page_exists,
                "Deleted node should not exist: {}",
                node_id
            );
        }
        
        // Check all expected edges exist
        for edge in &self.expected_edges {
            validator.assert_edge_exists(edge);
        }
    }
}

/// Helper to import initial test data and set up expectations
impl GraphValidationFixture {
    /// Add expectations for the dummy graph that's imported in tests
    pub fn expect_dummy_graph(&mut self) -> &mut Self {
        // Main pages from the dummy graph
        self.expect_create_page("cyberorganism-test-1", None)
            .expect_create_page("cyberorganism-test-2", None)
            .expect_create_page("contents", None)
            .expect_create_page("test-websocket", None);
        
        // Add a few key blocks to verify structure
        // Block with ID 67f9a190-b504-46ca-b1d9-cfe1a80f1633
        self.expect_create_block(
            "67f9a190-b504-46ca-b1d9-cfe1a80f1633",
            "## Introduction to Knowledge Graphs",
            Some("cyberorganism-test-1")
        );
        
        // Block with ID 67f9a190-985b-4dbf-90e4-c2abffb2ab51 (used in update test)
        self.expect_create_block(
            "67f9a190-985b-4dbf-90e4-c2abffb2ab51",
            "## Types of Knowledge Graphs",
            Some("cyberorganism-test-1")
        );
        
        // A regular content block
        self.expect_create_block(
            "67fbd626-8e4a-485f-ad03-fd1ce5539ebb", // This one gets deleted in tests
            "Knowledge graphs represent information as a network of entities, relationships, and attributes.",
            None // Child blocks may not have direct page connections
        );
        
        self
    }
    
    /// Validate that the graph contains expected nodes by content (for crash recovery tests)
    /// This is useful when we don't know the block IDs ahead of time
    pub fn validate_graph_with_content_checks(
        &self, 
        data_dir: &Path, 
        graph_id: &str,
        expected_blocks: &[(&str, Option<&str>)] // (content, page_name)
    ) {
        let validator = GraphValidator::load(data_dir, graph_id);
        
        // First validate all nodes we know about
        self.validate_graph(data_dir, graph_id);
        
        // Then check for blocks by content
        for (content, page_name) in expected_blocks {
            let block = validator.nodes.iter()
                .find(|n| {
                    n["node_type"].as_str() == Some("Block") && 
                    n["content"].as_str() == Some(content)
                })
                .unwrap_or_else(|| panic!("Block with content '{}' not found", content));
            
            // If page_name was specified, verify the edge exists
            if let Some(page) = page_name {
                let block_id = block["pkm_id"].as_str().unwrap();
                let edge_exists = validator.edges.iter().any(|e| {
                    if let (Some(source_idx), Some(target_idx)) = (e[0].as_u64(), e[1].as_u64()) {
                        let source_node = &validator.nodes[source_idx as usize];
                        let target_node = &validator.nodes[target_idx as usize];
                        
                        source_node["pkm_id"].as_str() == Some(page) &&
                        target_node["pkm_id"].as_str() == Some(block_id) &&
                        e[2]["edge_type"].as_str() == Some("PageToBlock")
                    } else {
                        false
                    }
                });
                
                assert!(edge_exists, "Block '{}' should be connected to page '{}'", content, page);
            }
        }
    }
}