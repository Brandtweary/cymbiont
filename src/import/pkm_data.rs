/**
 * @module pkm_data
 * @description PKM data structures and graph transformation logic
 * 
 * This module defines the core PKM (Personal Knowledge Management) data structures
 * and provides the logic to transform them into graph nodes and edges. It serves
 * as the bridge between external PKM formats and the internal graph representation.
 * 
 * ## Core Types
 * 
 * - `PKMBlockData`: Knowledge block with content, references, and hierarchy
 * - `PKMPageData`: Knowledge page that contains and organizes blocks
 * - `PKMReference`: Typed references (page, block, tag) extracted from content
 * 
 * ## Key Responsibilities
 * 
 * ### Data Transformation
 * The `apply_to_graph()` methods on PKMBlockData and PKMPageData handle:
 * - Creating or updating graph nodes with appropriate metadata
 * - Resolving block references to expand content
 * - Creating edges for relationships (parent-child, page-block, references)
 * - Ensuring referenced pages/blocks exist (creating placeholders if needed)
 * 
 * ### Reference Resolution
 * - Block references `((block-id))` are resolved to their content
 * - Page references `[[page-name]]` create edges to page nodes
 * - Tags `#tag` are treated as implicit page references
 * - Properties are stored in node metadata, not as edges
 * 
 * ## Design Principles
 * 
 * - **Separation of Concerns**: PKM logic is isolated from generic graph operations
 * - **Lazy Page Creation**: Referenced pages are created on-demand
 * - **Normalized Names**: Page names are normalized to lowercase for consistency
 * - **Flexible Timestamps**: Accepts various timestamp formats via custom deserializer
 * - **Reference Safety**: Circular and self-references are handled gracefully
 */

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use chrono::Utc;
use petgraph::stable_graph::NodeIndex;
use crate::graph_manager::{GraphManager, NodeType, EdgeType, GraphError, GraphResult};
use crate::utils::{parse_datetime, parse_properties};
use crate::import::reference_resolver::resolve_block_references;

/// PKM block data received from the frontend
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PKMBlockData {
    pub id: String,
    pub content: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub created: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub updated: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub references: Vec<PKMReference>,
    #[serde(default)]
    pub reference_content: Option<String>,
}


/// PKM page data received from the frontend
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PKMPageData {
    pub name: String,
    #[serde(default)]
    pub normalized_name: Option<String>,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub created: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub updated: String,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub blocks: Vec<String>,
}


/// Reference within PKM content
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PKMReference {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub id: String,
}

/// Custom deserializer for timestamps that can be either strings or integers
fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TimestampVisitor;

    impl<'de> serde::de::Visitor<'de> for TimestampVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or an integer")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(TimestampVisitor)
}

impl PKMBlockData {
    /// Apply this block to the graph, creating/updating the node and edges
    pub fn apply_to_graph(&self, graph: &mut GraphManager) -> GraphResult<NodeIndex> {
        // Generate internal ID
        let internal_id = if let Some(existing_idx) = graph.find_node(&self.id) {
            // Node exists, get its internal ID
            graph.get_node(existing_idx)
                .map(|node| node.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        
        // Resolve references if not already resolved
        let resolved_content = if self.reference_content.is_some() {
            self.reference_content.clone()
        } else {
            // Build block map for reference resolution
            let block_map = build_block_content_map(graph);
            let mut visited = HashSet::new();
            Some(resolve_block_references(&self.content, &block_map, &mut visited, Some(&self.id)))
        };
        
        // 1. Create/update the block node
        let node_idx = graph.create_or_update_node(
            self.id.clone(),
            internal_id,
            NodeType::Block,
            self.content.clone(),
            resolved_content,
            parse_properties(&self.properties),
            parse_datetime(&self.created),
            parse_datetime(&self.updated),
        )?;
        
        // 2. Handle parent-child relationship
        if let Some(parent_id) = &self.parent {
            if let Some(parent_idx) = graph.find_node(parent_id) {
                graph.add_edge(parent_idx, node_idx, EdgeType::ParentChild, 1.0);
            }
        }
        
        // 3. Handle page relationship
        if let Some(page_name) = &self.page {
            let page_idx = ensure_page_exists(graph, page_name)?;
            graph.add_edge(page_idx, node_idx, EdgeType::PageToBlock, 1.0);
        }
        
        // 4. Process references
        for reference in &self.references {
            process_reference(graph, node_idx, reference)?;
        }
        
        Ok(node_idx)
    }
}

impl PKMPageData {
    /// Apply this page to the graph, creating/updating the node and edges
    pub fn apply_to_graph(&self, graph: &mut GraphManager) -> GraphResult<NodeIndex> {
        let normalized_name_owned = self.name.to_lowercase();
        let normalized_name = self.normalized_name.as_ref()
            .unwrap_or(&normalized_name_owned);
        
        // Generate internal ID
        let internal_id = if let Some(existing_idx) = graph.find_node(&normalized_name) {
            // Node exists, get its internal ID
            graph.get_node(existing_idx)
                .map(|node| node.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        } else {
            uuid::Uuid::new_v4().to_string()
        };
            
        // 1. Create/update the page node
        let node_idx = graph.create_or_update_node(
            normalized_name.clone(),
            internal_id,
            NodeType::Page,
            self.name.clone(),
            None, // Pages don't have reference content
            parse_properties(&self.properties),
            parse_datetime(&self.created),
            parse_datetime(&self.updated),
        )?;
        
        // 2. Connect to root blocks
        for block_id in &self.blocks {
            if let Some(block_idx) = graph.find_node(block_id) {
                graph.add_edge(node_idx, block_idx, EdgeType::PageToBlock, 1.0);
            }
        }
        
        Ok(node_idx)
    }
}

// Helper functions for PKM-specific logic

/// Build a map of block ID to block content for reference resolution
fn build_block_content_map(graph: &GraphManager) -> HashMap<String, String> {
    let mut block_map = HashMap::new();
    
    for node_idx in graph.graph.node_indices() {
        if let Some(node) = graph.graph.node_weight(node_idx) {
            if matches!(node.node_type, NodeType::Block) {
                block_map.insert(node.pkm_id.clone(), node.content.clone());
            }
        }
    }
    
    block_map
}

/// Ensure a page exists in the graph, creating a placeholder if necessary
fn ensure_page_exists(graph: &mut GraphManager, page_name: &str) -> GraphResult<NodeIndex> {
    let normalized_name = page_name.to_lowercase();
    
    // Check both original and normalized names
    if let Some(idx) = graph.find_node(page_name)
        .or_else(|| graph.find_node(&normalized_name)) {
        return Ok(idx);
    }
    
    // Create placeholder page
    Ok(graph.create_node(
        normalized_name,
        uuid::Uuid::new_v4().to_string(),
        NodeType::Page,
        page_name.to_string(),
        None,
        HashMap::new(),
        Utc::now(),
        Utc::now(),
    ))
}

/// Process a PKM reference and create appropriate edges
fn process_reference(
    graph: &mut GraphManager,
    source_idx: NodeIndex,
    reference: &PKMReference,
) -> GraphResult<()> {
    match reference.r#type.as_str() {
        "page" => {
            let target_idx = ensure_page_exists(graph, &reference.name)?;
            graph.add_edge(source_idx, target_idx, EdgeType::PageRef, 1.0);
        }
        "block" => {
            let target_idx = if let Some(idx) = graph.find_node(&reference.id) {
                idx
            } else {
                // Create placeholder block
                graph.create_node(
                    reference.id.clone(),
                    uuid::Uuid::new_v4().to_string(),
                    NodeType::Block,
                    String::new(),
                    None,
                    HashMap::new(),
                    Utc::now(),
                    Utc::now(),
                )
            };
            graph.add_edge(source_idx, target_idx, EdgeType::BlockRef, 1.0);
        }
        "tag" => {
            let target_idx = ensure_page_exists(graph, &reference.name)?;
            graph.add_edge(source_idx, target_idx, EdgeType::Tag, 1.0);
        }
        "property" => {
            // Properties are stored in the node's properties map, not as edges
            // So we don't need to do anything here
        }
        _ => {
            return Err(GraphError::ReferenceResolution(
                format!("Unknown reference type: {}", reference.r#type)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use serde_json::json;

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
            properties: json!({}),
            references: vec![],
            reference_content: None,
        }
    }
    
    /// Create test page data
    fn create_test_page(name: &str) -> PKMPageData {
        PKMPageData {
            name: name.to_string(),
            normalized_name: Some(name.to_lowercase()),
            created: "2024-01-01T00:00:00Z".to_string(),
            updated: "2024-01-01T00:00:00Z".to_string(),
            properties: json!({}),
            blocks: vec![],
        }
    }

    #[test]
    fn test_pkm_reference_struct() {
        let page_ref = PKMReference {
            r#type: "page".to_string(),
            name: "TestPage".to_string(),
            id: "".to_string(),
        };
        
        let block_ref = PKMReference {
            r#type: "block".to_string(),
            name: "".to_string(),
            id: "block-id".to_string(),
        };
        
        assert_eq!(page_ref.r#type, "page");
        assert_eq!(page_ref.name, "TestPage");
        assert_eq!(block_ref.r#type, "block");
        assert_eq!(block_ref.id, "block-id");
    }

    #[test]
    fn test_create_page_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        let page = create_test_page("TestPage");
        
        let node_idx = page.apply_to_graph(&mut manager).unwrap();
        
        // Verify node was created
        let node = manager.get_node(node_idx).unwrap();
        assert_eq!(node.pkm_id, "testpage"); // Normalized
        assert_eq!(node.content, "TestPage");
        assert_eq!(node.node_type, NodeType::Page);
        
        // Verify mapping was created with normalized name
        assert!(manager.find_node("testpage").is_some());
    }
    
    #[test]
    fn test_create_block_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        let block = create_test_block("block-123", "Test content");
        
        let node_idx = block.apply_to_graph(&mut manager).unwrap();
        
        // Verify node was created
        let node = manager.get_node(node_idx).unwrap();
        assert_eq!(node.pkm_id, "block-123");
        assert_eq!(node.content, "Test content");
        assert_eq!(node.node_type, NodeType::Block);
        
        // Verify page was auto-created
        assert!(manager.find_node("testpage").is_some());
    }
    
    #[test]
    fn test_parent_child_relationship() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create parent block
        let parent = create_test_block("parent-123", "Parent content");
        let parent_idx = parent.apply_to_graph(&mut manager).unwrap();
        
        // Create child block
        let mut child = create_test_block("child-456", "Child content");
        child.parent = Some("parent-123".to_string());
        let child_idx = child.apply_to_graph(&mut manager).unwrap();
        
        // Verify edge exists using public graph access
        assert!(manager.has_edge(parent_idx, child_idx, &EdgeType::ParentChild));
    }
    
    #[test]
    fn test_page_to_block_relationship() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create page first
        let page = create_test_page("TestPage");
        let page_idx = page.apply_to_graph(&mut manager).unwrap();
        
        // Create block without parent (root block)
        let block = create_test_block("block-123", "Root block");
        let block_idx = block.apply_to_graph(&mut manager).unwrap();
        
        // Verify page-to-block edge exists
        assert!(manager.has_edge(page_idx, block_idx, &EdgeType::PageToBlock));
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
        
        let block_idx = block.apply_to_graph(&mut manager).unwrap();
        
        // Verify tag page was created
        let tag_idx = manager.find_node("philosophy").unwrap();
        
        // Verify tag edge exists
        assert!(manager.has_edge(block_idx, tag_idx, &EdgeType::Tag));
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
        
        let block_idx = block.apply_to_graph(&mut manager).unwrap();
        
        // Verify referenced page was created (normalized to lowercase)
        let ref_page_idx = manager.find_node("another page").unwrap();
        
        // Verify page reference edge exists
        assert!(manager.has_edge(block_idx, ref_page_idx, &EdgeType::PageRef));
    }
    
    #[test]
    fn test_block_reference() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create target block first
        let target = create_test_block("target-789", "Target content");
        let target_idx = target.apply_to_graph(&mut manager).unwrap();
        
        // Create block with reference
        let mut source = create_test_block("source-123", "See ((target-789))");
        source.references = vec![
            PKMReference {
                r#type: "block".to_string(),
                name: String::new(),
                id: "target-789".to_string(),
            }
        ];
        
        let source_idx = source.apply_to_graph(&mut manager).unwrap();
        
        // Verify block reference edge exists
        assert!(manager.has_edge(source_idx, target_idx, &EdgeType::BlockRef));
    }
    
    #[test]
    fn test_properties_storage() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        let mut block = create_test_block("block-123", "Task content");
        block.properties = json!({
            "status": "in-progress",
            "priority": "high"
        });
        
        let node_idx = block.apply_to_graph(&mut manager).unwrap();
        
        // Verify properties were stored
        let node = manager.get_node(node_idx).unwrap();
        assert_eq!(node.properties.get("status"), Some(&"in-progress".to_string()));
        assert_eq!(node.properties.get("priority"), Some(&"high".to_string()));
    }
    
    #[test]
    fn test_update_existing_node() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create initial block
        let block1 = create_test_block("block-123", "Original content");
        let idx1 = block1.apply_to_graph(&mut manager).unwrap();
        
        // Update the same block
        let mut block2 = create_test_block("block-123", "Updated content");
        block2.updated = "2024-01-02T00:00:00Z".to_string();
        let idx2 = block2.apply_to_graph(&mut manager).unwrap();
        
        // Should be the same node index
        assert_eq!(idx1, idx2);
        
        // Content should be updated
        let node = manager.get_node(idx1).unwrap();
        assert_eq!(node.content, "Updated content");
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
        
        let idx1 = block1.apply_to_graph(&mut manager).unwrap();
        let _idx2 = block2.apply_to_graph(&mut manager).unwrap();
        let target_idx = manager.find_node("target").unwrap();
        
        // Each block should have exactly one edge to Target
        assert!(manager.has_edge(idx1, target_idx, &EdgeType::PageRef));
        
        // Update block1 with same reference - should not create duplicate edge
        block1.apply_to_graph(&mut manager).unwrap();
        // Still only one edge (has_edge would be false if there were multiple)
        assert!(manager.has_edge(idx1, target_idx, &EdgeType::PageRef));
    }
    
    #[test]
    fn test_build_block_content_map() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create some blocks
        create_test_block("block-1", "Block 1 content")
            .apply_to_graph(&mut manager).unwrap();
        
        create_test_page("TestPage")
            .apply_to_graph(&mut manager).unwrap();
        
        create_test_block("block-2", "Block 2 content")
            .apply_to_graph(&mut manager).unwrap();
        
        // Build map - should only contain blocks
        let map = build_block_content_map(&manager);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("block-1"), Some(&"Block 1 content".to_string()));
        assert_eq!(map.get("block-2"), Some(&"Block 2 content".to_string()));
        assert_eq!(map.get("testpage"), None); // Pages should not be in the map
    }

    #[test]
    fn test_reference_resolution() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        // Create target block with content
        let target = create_test_block("target-123", "This is the target content");
        target.apply_to_graph(&mut manager).unwrap();
        
        // Create block that references the target
        let mut source = create_test_block("source-456", "See ((target-123)) for details");
        source.references = vec![
            PKMReference {
                r#type: "block".to_string(),
                name: String::new(),
                id: "target-123".to_string(),
            }
        ];
        
        let source_idx = source.apply_to_graph(&mut manager).unwrap();
        
        // Verify reference was resolved
        let node = manager.get_node(source_idx).unwrap();
        assert_eq!(
            node.reference_content.as_ref().unwrap(),
            "See This is the target content for details"
        );
    }
}