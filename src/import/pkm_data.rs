// TODO: This module was gutted during the import refactor. Consider what needs to be kept:
// - Keep PKM data structures as pure data types
// - Remove transformation logic that's been moved elsewhere
// - Decide on the future of reference resolution functions
#![allow(dead_code)]

//! # PKM Data Structures and Graph Transformation
//!
//! This module defines the core PKM (Personal Knowledge Management) data structures
//! and provides the logic to transform them into graph nodes and edges. It serves
//! as the bridge between external PKM formats and the internal graph representation,
//! handling the complex task of converting hierarchical, reference-rich knowledge
//! structures into a navigable graph format.
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
//! ### Smart Node Management
//! The `apply_to_graph()` methods implement sophisticated node lifecycle management:
//! - **Existence Checking**: Efficiently determines if nodes already exist
//! - **Update vs Create**: Preserves existing nodes while updating content
//! - **UUID Generation**: Creates consistent internal identifiers
//! - **Metadata Preservation**: Maintains properties and timestamps across updates
//!
//! ### Edge Creation Strategy
//! Creates typed edges to represent different relationship semantics:
//! - **ParentChild**: Hierarchical block relationships
//! - **PageToBlock**: Page ownership of blocks
//! - **PageRef**: Explicit page references from content
//! - **BlockRef**: Direct block-to-block citations
//! - **Tag**: Categorical relationships via hashtags
//!
//! ### Reference Resolution Pipeline
//! - **Content Expansion**: Block references are resolved to actual content
//! - **Circular Detection**: Prevents infinite loops in reference chains
//! - **Placeholder Creation**: Ensures referenced entities exist in the graph
//! - **Lazy Loading**: Creates referenced pages and blocks on-demand
//!
//! ## Design Principles and Patterns
//!
//! ### Separation of Concerns
//! PKM logic is completely isolated from generic graph operations, allowing:
//! - **Domain Expertise**: PKM-specific business logic remains centralized
//! - **Graph Flexibility**: Underlying graph engine can evolve independently
//! - **Testing Simplicity**: PKM transformations can be tested in isolation
//!
//! ### Defensive Programming
//! - **Flexible Timestamps**: Custom deserializer handles strings, integers, and ISO formats
//! - **Graceful Degradation**: Missing references become placeholders rather than errors
//! - **Data Validation**: Content and structure validation with meaningful error messages
//! - **Safe Defaults**: Optional fields have sensible default values
//!
//! ### Performance Optimization
//! - **Batch Operations**: Groups related graph operations for efficiency
//! - **Content Mapping**: Pre-builds block maps for fast reference resolution
//! - **Incremental Updates**: Only modifies changed content during updates
//! - **Smart Normalization**: Caches normalized names to avoid repeated computation
//!
//! ## Factory Methods and Convenience APIs
//!
//! ### Creation Helpers
//! - `PKMBlockData::new_block()`: Creates blocks with automatic UUID generation
//! - `PKMPageData::new_page()`: Initializes pages with normalized naming
//! - Smart defaults for timestamps, properties, and references
//!
//! ### Update Operations
//! - `update_block_content()`: Handles the complex find-update-resolve-save cycle
//! - `create_or_update_page()`: Intelligently manages page existence and updates
//! - Reference re-resolution when content changes
//!
//! ## Integration with Import Pipeline
//!
//! This module fits into the broader import system architecture:
//! 1. **Input Processing**: Receives parsed data from format-specific modules (Logseq, etc.)
//! 2. **Data Transformation**: Converts external formats to PKM structures
//! 3. **Graph Application**: Uses GraphManager to persist nodes and edges
//! 4. **Reference Resolution**: Coordinates with reference_resolver for content expansion
//! 5. **Error Propagation**: Reports transformation errors through centralized error system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use petgraph::stable_graph::NodeIndex;
use uuid::Uuid;
use crate::graph::graph_manager::{GraphManager, NodeType, EdgeType};
use crate::graph::graph_operations::GraphOps;
// use crate::utils::parse_properties; // Removed: unused after refactor
use crate::import::logseq::extract_references;
use crate::error::*;
use crate::AppState;
use serde_json::json;

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

impl PKMBlockData {
    /// Create a new block with content and optional metadata
    /// This is used when creating blocks via graph operations
    pub fn new_block(
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Self {
        let block_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        
        PKMBlockData {
            id: block_id,
            content: content.clone(),
            properties: properties.unwrap_or(json!({})),
            parent: parent_id,
            page: page_name,
            references: extract_references(&content),
            children: vec![],
            created: now.clone(),
            updated: now,
            reference_content: None, // Let apply_to_graph handle resolution
        }
    }
    
    /// Update existing block content with reference resolution
    /// Handles the find->clone->update->resolve->save pattern
    pub fn update_block_content(
        block_id: &str,
        new_content: String,
        graph_manager: &mut GraphManager,
    ) -> Result<()> {
        // Find the node
        if let Some(node_idx) = graph_manager.find_node(block_id) {
            // Get existing node data to preserve all fields
            if let Some(existing_node) = graph_manager.get_node(node_idx) {
                // Clone existing data and update only what we need
                let mut node_data = existing_node.clone();
                
                // Update content and timestamp
                node_data.content = new_content.clone();
                node_data.updated_at = chrono::Utc::now();
                
                // Resolve references if content changed
                if existing_node.content != new_content {
                    node_data.reference_content = Some(
                        crate::import::reference_resolver::resolve_references_in_graph(
                            &new_content,
                            block_id,
                            graph_manager
                        )
                    );
                }
                
                // Update the node in the graph
                graph_manager.create_or_update_node(
                    node_data.pkm_id,
                    node_data.id,
                    node_data.node_type,
                    node_data.content,
                    node_data.reference_content,
                    node_data.properties,
                    node_data.created_at,
                    node_data.updated_at,
                )?;
                
                Ok(())
            } else {
                Err(GraphError::not_found(format!("Node not found: {}", block_id)).into())
            }
        } else {
            Err(GraphError::not_found(format!("Node not found: {}", block_id)).into())
        }
    }
    /// Apply this block to the graph using GraphOps for proper WAL logging
    pub async fn apply_to_graph(
        &self,
        app_state: &Arc<AppState>,
        agent_id: Uuid,
        graph_id: &Uuid,
    ) -> Result<String> {
        // Convert properties to JSON (already a Value, just need to parse)
        let properties = if self.properties.is_null() || self.properties == json!({}) {
            None
        } else {
            Some(self.properties.clone())
        };
        
        // Create the block using GraphOps (which logs to WAL)
        // Note: add_block will handle page creation and parent-child relationships
        let block_id = app_state.add_block(
            agent_id,
            self.content.clone(),
            self.parent.clone(),     // Parent block ID
            self.page.clone(),       // Page name (will create page if needed)
            properties,
            graph_id,
            false, // don't skip WAL
        ).await?;
        
        // Note: References (tags, page refs, block refs) are handled by
        // the content parsing in add_block. The references field is metadata
        // from the import that we don't need to process separately.
        
        Ok(block_id)
    }
}

impl PKMPageData {
    /// Create a new page with name and optional properties
    /// This is used when creating pages via graph operations
    pub fn new_page(
        page_name: String,
        properties: Option<serde_json::Value>,
    ) -> Self {
        let normalized_name = page_name.to_lowercase();
        let now = chrono::Utc::now().to_rfc3339();
        
        PKMPageData {
            name: page_name,
            normalized_name: Some(normalized_name),
            properties: properties.unwrap_or(json!({})),
            created: now.clone(),
            updated: now,
            blocks: vec![],
        }
    }
    /// Apply this page to the graph using GraphOps for proper WAL logging
    pub async fn apply_to_graph(
        &self,
        app_state: &Arc<AppState>,
        agent_id: Uuid,
        graph_id: &Uuid,
    ) -> Result<()> {
        // Convert properties to JSON (already a Value, just need to parse)
        let properties = if self.properties.is_null() || self.properties == json!({}) {
            None
        } else {
            Some(self.properties.clone())
        };
        
        // Create the page using GraphOps (which logs to WAL)
        app_state.create_page(
            agent_id,
            self.name.clone(),
            properties,
            graph_id,
            false, // don't skip WAL
        ).await?;
        
        // Note: PageToBlock edges will be created when blocks are added
        // with this page as their page_name
        
        Ok(())
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
fn ensure_page_exists(graph: &mut GraphManager, page_name: &str) -> Result<NodeIndex> {
    
    let normalized_name = page_name.to_lowercase();
    
    // Check both original and normalized names
    if let Some(idx) = graph.find_node(page_name)
        .or_else(|| graph.find_node(&normalized_name)) {
        return Ok(idx);
    }
    
    // Create placeholder page
    let idx = graph.create_node(
        normalized_name,
        uuid::Uuid::new_v4().to_string(),
        NodeType::Page,
        page_name.to_string(),
        None,
        HashMap::new(),
        Utc::now(),
        Utc::now(),
    );
    Ok(idx)
}

/// Process a PKM reference and create appropriate edges
fn process_reference(
    graph: &mut GraphManager,
    source_idx: NodeIndex,
    reference: &PKMReference,
) -> Result<()> {
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
            return Err(ImportError::validation(
                format!("Unknown reference type: {}", reference.r#type)
            ).into());
        }
    }
    Ok(())
}
