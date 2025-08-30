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

use std::path::Path;
use std::sync::Arc;
use tracing::{info, error};
use crate::app_state::AppState;
use crate::graph::graph_operations::GraphOps;
use super::logseq;
use crate::error::*;
use crate::lock::AsyncRwLockExt;
use serde_json;

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
    info!("✅ Successfully parsed {} pages and {} blocks", pages.len(), blocks.len());
    
    // Get or create a graph for this import
    let graph_name = custom_name.unwrap_or_else(|| {
        logseq_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("imported-graph")
            .to_string()
    });
    
    // Use the centralized create_graph function which handles:
    // 1. Graph registration
    // 2. Prime agent authorization
    // 3. Graph manager creation
    let graph_info = app_state.create_graph(
        Some(graph_name.clone()),
        Some(format!("Imported from: {}", logseq_path.display()))
    ).await?;
    
    let graph_id = uuid::Uuid::parse_str(
        graph_info["id"].as_str()
            .ok_or_else(|| ImportError::validation("Graph ID not found in response"))?
    ).map_err(|e| ImportError::validation(format!("Invalid graph ID: {}", e)))?;
    
    info!("📊 Using graph: {} ({})", graph_name, graph_id);
    
    // Get the prime agent ID for import authorization
    let prime_agent_id = {
        let agent_registry = app_state.agent_registry.read_or_panic("get prime agent for import").await;
        agent_registry.get_prime_agent_id()
            .ok_or_else(|| ImportError::validation("Prime agent not found".to_string()))?
    };
    
    info!("🔑 Using Prime Agent {} for import authorization", prime_agent_id);
    
    // Import the data using GraphOps (which logs to WAL)
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
        
        match app_state.create_page(
            prime_agent_id,
            page.name.clone(),
            properties,
            &graph_id,
            false, // don't skip WAL
        ).await {
            Ok(_) => page_count += 1,
            Err(e) => {
                let err_msg = format!("Failed to import page {}: {}", page.name, e);
                error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }
    
    // Import blocks after pages (so parent pages exist)
    for block in blocks {
        // Convert properties for GraphOps
        let properties = if block.properties.is_null() || block.properties == serde_json::json!({}) {
            None
        } else {
            Some(block.properties.clone())
        };
        
        match app_state.add_block(
            prime_agent_id,
            block.content.clone(),
            block.parent.clone(),     // Parent block ID
            block.page.clone(),       // Page name (will create page if needed)
            properties,
            &graph_id,
            false, // don't skip WAL
        ).await {
            Ok(_) => block_count += 1,
            Err(e) => {
                let err_msg = format!("Failed to import block {}: {}", block.id, e);
                error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }
    
    info!("✅ Imported {} pages and {} blocks", page_count, block_count);
    
    // Return the import result
    Ok(ImportResult {
        graph_id: graph_id.to_string(),
        graph_name,
        pages_imported: page_count,
        blocks_imported: block_count,
        errors,
    })
}