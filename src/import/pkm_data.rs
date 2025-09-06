//! # PKM Data Structures and Helper Functions
//!
//! This module defines the core PKM (Personal Knowledge Management) data structures
//! and provides helper functions for PKM-specific graph operations. It encapsulates
//! all PKM domain logic including block reference resolution, page normalization,
//! and relationship management.
//!
//! ## Core Data Types
//!
//! ### PKMBlockData
//! Represents a single knowledge block with rich metadata:
//! - **Content**: The actual text content with markdown formatting
//! - **Hierarchy**: Parent-child relationships within the knowledge structure
//! - **References**: Extracted links to pages, blocks, and tags
//! - **Properties**: Structured metadata (status, priority, dates, etc.)
//! - **Timestamps**: Creation and modification tracking with flexible deserialization
//!
//! ### PKMPageData
//! Represents a knowledge page that organizes and contains blocks:
//! - **Naming**: Both original and normalized names for consistent referencing
//! - **Block Organization**: Lists of root-level blocks belonging to the page
//! - **Metadata**: Page-level properties and timestamps
//!
//! ### PKMReference
//! Typed references extracted from content during parsing:
//! - **Page References**: `[[Page Name]]` - Links to other pages
//! - **Block References**: `((block-id))` - Direct block citations
//! - **Tag References**: `#tag` - Categorical classifications
//!
//! ## Graph Transformation Architecture
//!
//! ### Helper Functions
//! - **Block Operations**: Create and update blocks with automatic reference resolution
//! - **Page Operations**: Create, update, and normalize page names consistently
//! - **Reference Resolution**: Expand `((block-id))` patterns to actual content
//! - **Relationship Management**: Setup parent-child and page-block edges
//!
//! ## Design Principles
//!
//! ### Separation of Concerns
//! - PKM logic is isolated from graph operations
//! - Helper functions work directly with GraphManager, not AppState
//! - GraphOps handles transactions and command routing
//!
//! ### Reference Resolution
//! - Block references `((block-id))` are expanded to actual content
//! - Circular reference protection prevents infinite loops
//! - Self-references are detected and preserved as-is
//! - Missing references gracefully degrade to original syntax
//!
//! ## Key Functions
//!
//! ### Block Helpers
//! - `update_block_with_resolution()`: Update block content and references
//! - `setup_block_relationships()`: Create parent-child and page-block edges
//!
//! ### Page Helpers
//! - `ensure_page_exists()`: Find or create a page by name
//! - `create_or_update_page()`: Smart page creation with property updates
//! - `find_page_for_deletion()`: Locate page by original or normalized name
//!
//! ### Reference Resolution
//! - `resolve_block_references()`: Core resolution with circular protection
//! - `build_block_map()`: Generate block ID to content mapping

use crate::error::*;
use crate::graph::graph_manager::{EdgeType, GraphManager, NodeType};
use once_cell::sync::Lazy;
use petgraph::stable_graph::NodeIndex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// Regex for matching block references like ((block-id))
static BLOCK_REF_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\(\(([a-zA-Z0-9-]+)\)\)").unwrap());

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
fn deserialize_timestamp<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TimestampVisitor;

    impl<'de> serde::de::Visitor<'de> for TimestampVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or an integer")
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(TimestampVisitor)
}

// ============================================================================
// Core Reference Resolution Functions
// ============================================================================

/// Resolve block references in content by replacing ((block-id)) with referenced block content
///
/// # Arguments
/// * `content` - The content containing block references
/// * `block_map` - Map from block ID to block content
/// * `visited` - Set of already visited block IDs to prevent circular references
/// * `current_block_id` - The ID of the current block (to prevent self-references)
///
/// # Returns
/// The content with all block references expanded
pub fn resolve_block_references(
    content: &str,
    block_map: &HashMap<String, String>,
    visited: &mut HashSet<String>,
    current_block_id: Option<&str>,
) -> String {
    // Add current block to visited set to prevent self-references
    if let Some(id) = current_block_id {
        visited.insert(id.to_string());
    }

    let result = BLOCK_REF_RE
        .replace_all(content, |caps: &regex::Captures| {
            let block_id = &caps[1];

            // Check for circular references (including self-reference)
            if visited.contains(block_id) {
                return caps[0].to_string(); // Keep original reference
            }

            if let Some(referenced_content) = block_map.get(block_id) {
                // Mark this block as visited before recursing
                visited.insert(block_id.to_string());

                // Recursively resolve any references in the referenced content
                let expanded = resolve_block_references(
                    referenced_content,
                    block_map,
                    visited,
                    Some(block_id),
                );

                // Remove from visited after processing (allows the same block to be referenced multiple times in different contexts)
                visited.remove(block_id);

                expanded
            } else {
                // Keep the original reference if we can't find the block
                caps[0].to_string()
            }
        })
        .to_string();

    // Remove current block from visited set after processing
    if let Some(id) = current_block_id {
        visited.remove(id);
    }

    result
}

/// Build a map of block ID -> content from a graph manager
pub fn build_block_map(graph_manager: &GraphManager) -> HashMap<String, String> {
    let mut block_map = HashMap::new();

    for idx in graph_manager.graph.node_indices() {
        if let Some(node) = graph_manager.graph.node_weight(idx) {
            if matches!(node.node_type, NodeType::Block) {
                block_map.insert(node.id.clone(), node.content.clone());
            }
        }
    }

    block_map
}

// ============================================================================
// Block Helper Functions
// ============================================================================

/// Create a block node with reference resolution and optional block ID
pub fn create_block_with_resolution_and_id(
    manager: &mut GraphManager,
    block_id: Option<String>,
    content: String,
    properties: Option<&serde_json::Value>,
) -> Result<(String, Option<String>)> {
    let block_id = block_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = chrono::Utc::now();

    // Parse properties
    let props = properties
        .map(|p| crate::utils::parse_properties(p))
        .unwrap_or_default();

    // Resolve references if content contains them
    let reference_content = if content.contains("((") {
        let block_map = build_block_map(manager);
        let mut visited = HashSet::new();
        Some(resolve_block_references(
            &content,
            &block_map,
            &mut visited,
            Some(&block_id),
        ))
    } else {
        None
    };

    // Create the node
    manager
        .create_or_update_node(
            block_id.clone(),
            NodeType::Block,
            content,
            reference_content.clone(),
            props,
            now,
            now,
        )
        .map_err(|e| GraphError::lifecycle(e.to_string()))?;

    Ok((block_id, reference_content))
}

/// Update a block with reference resolution
pub fn update_block_with_resolution(
    manager: &mut GraphManager,
    block_id: &str,
    new_content: String,
) -> Result<Option<String>> {
    let node_idx = manager
        .find_node(block_id)
        .ok_or_else(|| GraphError::not_found(format!("Block not found: {}", block_id)))?;

    let existing_node = manager
        .get_node(node_idx)
        .ok_or_else(|| GraphError::not_found(format!("Block not found: {}", block_id)))?
        .clone();

    // Resolve references if content changed and contains them
    let reference_content = if new_content != existing_node.content && new_content.contains("((") {
        let block_map = build_block_map(manager);
        let mut visited = HashSet::new();
        Some(resolve_block_references(
            &new_content,
            &block_map,
            &mut visited,
            Some(block_id),
        ))
    } else {
        existing_node.reference_content
    };

    manager
        .create_or_update_node(
            existing_node.id,
            existing_node.node_type,
            new_content,
            reference_content.clone(),
            existing_node.properties,
            existing_node.created_at,
            chrono::Utc::now(),
        )
        .map_err(|e| GraphError::lifecycle(e.to_string()))?;

    Ok(reference_content)
}

/// Setup block relationships (parent and page)
pub fn setup_block_relationships(
    manager: &mut GraphManager,
    block_id: &str,
    parent_id: Option<&str>,
    page_name: Option<&str>,
) -> Result<()> {
    // Handle parent-child relationship
    if let Some(parent) = parent_id {
        if let Some(parent_idx) = manager.find_node(parent) {
            if let Some(child_idx) = manager.find_node(block_id) {
                manager.add_edge(parent_idx, child_idx, EdgeType::ParentChild, 1.0);
            }
        }
    }

    // Handle page relationship
    if let Some(page) = page_name {
        let page_idx = ensure_page_exists(manager, page)?;
        if let Some(block_idx) = manager.find_node(block_id) {
            manager.add_edge(page_idx, block_idx, EdgeType::PageToBlock, 1.0);
        }
    }

    Ok(())
}

// ============================================================================
// Page Helper Functions
// ============================================================================

/// Ensure a page exists, creating if necessary
pub fn ensure_page_exists(manager: &mut GraphManager, page_name: &str) -> Result<NodeIndex> {
    let normalized_name = page_name.to_lowercase();

    // Check both original and normalized names
    if let Some(idx) = manager
        .find_node(page_name)
        .or_else(|| manager.find_node(&normalized_name))
    {
        return Ok(idx);
    }

    // Create the page if it doesn't exist
    let idx = manager.create_node(
        normalized_name,
        NodeType::Page,
        page_name.to_string(),
        None,
        HashMap::new(),
        chrono::Utc::now(),
        chrono::Utc::now(),
    );
    Ok(idx)
}

/// Create or update a page with properties
pub fn create_or_update_page(
    manager: &mut GraphManager,
    page_name: &str,
    properties: Option<&serde_json::Value>,
) -> Result<()> {
    let normalized_name = page_name.to_lowercase();
    let now = chrono::Utc::now();

    // Check if page already exists
    if let Some(node_idx) = manager
        .find_node(page_name)
        .or_else(|| manager.find_node(&normalized_name))
    {
        // Page exists - just update properties if provided
        if let Some(props) = properties {
            if let Some(existing_node) = manager.get_node(node_idx) {
                // Update only properties and timestamp
                let mut node_data = existing_node.clone();
                node_data.properties = crate::utils::parse_properties(props);
                node_data.updated_at = now;

                manager
                    .create_or_update_node(
                        node_data.id,
                        node_data.node_type,
                        node_data.content,
                        node_data.reference_content,
                        node_data.properties,
                        node_data.created_at,
                        node_data.updated_at,
                    )
                    .map_err(|e| GraphError::lifecycle(e.to_string()))?;
            }
        }
    } else {
        // Page doesn't exist - create it
        let default_props = json!({});
        let props = properties.unwrap_or(&default_props);

        manager
            .create_or_update_node(
                normalized_name,
                NodeType::Page,
                page_name.to_string(),
                None, // Pages don't have reference content
                crate::utils::parse_properties(props),
                now,
                now,
            )
            .map_err(|e| GraphError::lifecycle(e.to_string()))?;
    }

    Ok(())
}

/// Find a page for deletion by original or normalized name
pub fn find_page_for_deletion(
    manager: &GraphManager,
    page_name: &str,
) -> Result<(String, NodeIndex)> {
    let normalized_name = page_name.to_lowercase();

    // Try both the original name and normalized name
    let node_idx = manager
        .find_node(page_name)
        .or_else(|| manager.find_node(&normalized_name))
        .ok_or_else(|| GraphError::not_found(format!("Page not found: {}", page_name)))?;

    Ok((normalized_name, node_idx))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_simple_block_reference() {
        let mut block_map = HashMap::new();
        block_map.insert(
            "block-123".to_string(),
            "This is the referenced content".to_string(),
        );

        let content = "See ((block-123)) for details";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);

        assert_eq!(result, "See This is the referenced content for details");
    }

    #[test]
    fn test_resolve_multiple_block_references() {
        let mut block_map = HashMap::new();
        block_map.insert("block-1".to_string(), "first block".to_string());
        block_map.insert("block-2".to_string(), "second block".to_string());

        let content = "Both ((block-1)) and ((block-2)) are important";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);

        assert_eq!(result, "Both first block and second block are important");
    }

    #[test]
    fn test_resolve_nested_block_references() {
        let mut block_map = HashMap::new();
        block_map.insert(
            "block-a".to_string(),
            "Block A references ((block-b))".to_string(),
        );
        block_map.insert(
            "block-b".to_string(),
            "Block B references ((block-c))".to_string(),
        );
        block_map.insert("block-c".to_string(), "Block C content".to_string());

        let content = "Start with ((block-a))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);

        assert_eq!(
            result,
            "Start with Block A references Block B references Block C content"
        );
    }

    #[test]
    fn test_resolve_circular_references() {
        let mut block_map = HashMap::new();
        block_map.insert(
            "block-a".to_string(),
            "A references ((block-b))".to_string(),
        );
        block_map.insert(
            "block-b".to_string(),
            "B references ((block-a))".to_string(),
        );

        let content = "Start: ((block-a))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);

        // Should expand both but stop at the circular reference
        assert_eq!(result, "Start: A references B references ((block-a))");
    }

    #[test]
    fn test_resolve_self_reference() {
        let mut block_map = HashMap::new();
        block_map.insert(
            "self-ref".to_string(),
            "This block references itself: ((self-ref))".to_string(),
        );

        let content = "((self-ref))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, Some("self-ref"));

        // Should not expand self-reference
        assert_eq!(result, "((self-ref))");
    }

    #[test]
    fn test_resolve_missing_block_reference() {
        let block_map = HashMap::new();

        let content = "Reference to ((missing-block)) should remain";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);

        assert_eq!(result, "Reference to ((missing-block)) should remain");
    }

    // Note: Tests for create_block_with_resolution and update_block_with_resolution
    // require a real GraphManager with filesystem access, so they belong in integration
    // tests rather than unit tests. The core reference resolution logic is tested above
    // with pure functions that don't require filesystem access.
}
