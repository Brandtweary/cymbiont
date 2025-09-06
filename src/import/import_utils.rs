//! @module import_utils
//! @description High-level import coordination for knowledge graphs
//!
//! This module provides the main entry point for importing external knowledge graphs
//! into Cymbiont. It coordinates the entire import process from source parsing to
//! graph creation, registry management, and error collection.
//!
//! ## Key Functions
//!
//! - `import_logseq_graph()`: Complete Logseq import workflow with error handling
//!
//! ## Import Process
//!
//! 1. **Source Parsing**: Delegate to format-specific parsers (logseq.rs)
//! 2. **Graph Registration**: Create or update graph in registry
//! 3. **Data Import**: Process pages and blocks with reference resolution
//! 4. **Error Collection**: Aggregate non-fatal errors for reporting
//! 5. **Result Reporting**: Return comprehensive import statistics
//!
//! ## Error Handling
//!
//! The import process is designed to be resilient:
//! - Parse errors for individual files are collected but don't stop the import
//! - Reference resolution failures are reported but don't break the graph
//! - Only fatal errors (registry issues, I/O failures) abort the import
//!
//! This approach ensures maximum data recovery from potentially corrupted sources.
//!
//! ## ID Mapping Strategy
//!
//! During import, Cymbiont generates new UUIDs for all imported entities rather than
//! using the source system's IDs. This ensures:
//! - **Uniqueness**: No conflicts with existing data
//! - **Consistency**: All IDs follow the same format
//! - **Traceability**: Original IDs preserved in properties for debugging
//!
//! The import process maintains an ID mapping table to correctly resolve references
//! between blocks and pages. Block references in content (e.g., `((block-id))`) are
//! automatically updated to use the new UUIDs.
//!
//! ## Import Statistics
//!
//! The `ImportResult` structure provides detailed metrics:
//! - `graph_id`: The UUID of the created/updated graph
//! - `graph_name`: Human-readable name of the graph
//! - `pages_imported`: Count of successfully imported pages
//! - `blocks_imported`: Count of successfully imported blocks
//! - `errors`: Collection of non-fatal errors encountered
//!
//! ## Future Extensions
//!
//! The module is designed to be extensible for additional import sources:
//! - Obsidian markdown vaults
//! - Roam Research JSON exports
//! - Notion API integration
//! - Generic markdown directories

use super::logseq;
use crate::app_state::AppState;
use crate::error::*;
use crate::graph::graph_operations::GraphOps;
use serde_json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

/// Result of a Logseq import operation
#[derive(Debug)]
pub struct ImportResult {
    pub graph_id: String,
    pub graph_name: String,
    pub pages_imported: usize,
    pub blocks_imported: usize,
    pub errors: Vec<String>,
}

/// Import a Logseq graph into Cymbiont
pub async fn import_logseq_graph(
    app_state: &Arc<AppState>,
    logseq_path: &Path,
    custom_name: Option<String>,
) -> Result<ImportResult> {
    info!("📥 Importing Logseq graph from: {:?}", logseq_path);

    // Parse the Logseq graph
    let (pages, blocks) = logseq::import_graph(logseq_path)?;
    info!(
        "✅ Successfully parsed {} pages and {} blocks",
        pages.len(),
        blocks.len()
    );

    // Get or create a graph for this import
    let graph_name = custom_name.unwrap_or_else(|| {
        logseq_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("imported-graph")
            .to_string()
    });

    // Use the centralized create_graph function which handles:
    // 1. Graph registration
    // 3. Graph manager creation
    let graph_info = app_state
        .create_graph(
            Some(graph_name.clone()),
            Some(format!("Imported from: {}", logseq_path.display())),
        )
        .await?;

    let graph_id = Uuid::parse_str(
        graph_info["id"]
            .as_str()
            .ok_or_else(|| ImportError::validation("Graph ID not found in response"))?,
    )
    .map_err(|e| ImportError::validation(format!("Invalid graph ID: {}", e)))?;

    info!("📊 Using graph: {} ({})", graph_name, graph_id);

    // Import the data using GraphOps
    let mut errors = Vec::new();
    let mut page_count = 0;
    let mut block_count = 0;

    // Import pages first
    for page in pages {
        // Convert properties for GraphOps
        let properties = if page.properties.is_null() || page.properties == serde_json::json!({}) {
            None
        } else {
            Some(page.properties.clone())
        };

        match app_state
            .create_page(page.name.clone(), properties, &graph_id)
            .await
        {
            Ok(_) => page_count += 1,
            Err(e) => {
                let err_msg = format!("Failed to import page {}: {}", page.name, e);
                error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }

    // Build a mapping from original IDs to new UUIDs
    let mut id_mapping: HashMap<String, String> = HashMap::new();

    // First pass: generate new UUIDs for all blocks
    for block in &blocks {
        let new_id = Uuid::new_v4().to_string();
        id_mapping.insert(block.id.clone(), new_id);
    }

    // Import blocks after pages (so parent pages exist)
    for block in blocks {
        // Generate our own UUID instead of using Logseq's
        let our_block_id = id_mapping
            .get(&block.id)
            .expect("Block ID should exist in mapping")
            .clone();

        // Update parent reference to use new UUID if it exists
        let parent_id = block
            .parent
            .as_ref()
            .and_then(|pid| id_mapping.get(pid))
            .cloned();

        // Convert properties for GraphOps
        let properties = if block.properties.is_null() || block.properties == serde_json::json!({})
        {
            None
        } else {
            Some(block.properties.clone())
        };

        // Update reference_content to use new IDs
        let updated_ref_content = if let Some(ref_content) = block.reference_content {
            let mut content = ref_content;
            // Replace all old block IDs with new ones in reference content
            for (old_id, new_id) in &id_mapping {
                content = content.replace(&format!("(({}))", old_id), &format!("(({}))", new_id));
            }
            Some(content)
        } else {
            None
        };

        // Update block content to use new IDs in block references
        let mut updated_content = block.content.clone();
        for (old_id, new_id) in &id_mapping {
            updated_content =
                updated_content.replace(&format!("(({}))", old_id), &format!("(({}))", new_id));
        }

        match app_state
            .add_block(
                Some(our_block_id.clone()), // Use our generated UUID
                updated_content,            // Content with updated block references
                parent_id,                  // Use mapped parent ID
                block.page.clone(),         // Page name (will create page if needed)
                properties,
                updated_ref_content, // Reference content with updated IDs
                &graph_id,
            )
            .await
        {
            Ok(_) => block_count += 1,
            Err(e) => {
                let err_msg = format!("Failed to import block {}: {}", block.id, e);
                error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }

    info!(
        "✅ Imported {} pages and {} blocks",
        page_count, block_count
    );

    // Return the import result
    Ok(ImportResult {
        graph_id: graph_id.to_string(),
        graph_name,
        pages_imported: page_count,
        blocks_imported: block_count,
        errors,
    })
}
