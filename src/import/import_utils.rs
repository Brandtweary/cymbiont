/**
 * @module import_utils
 * @description High-level import coordination for knowledge graphs
 * 
 * This module provides the main entry point for importing external knowledge graphs
 * into Cymbiont. It coordinates the entire import process from source parsing to
 * graph creation, registry management, and error collection.
 * 
 * ## Key Functions
 * 
 * - `import_logseq_graph()`: Complete Logseq import workflow with error handling
 * 
 * ## Import Process
 * 
 * 1. **Source Parsing**: Delegate to format-specific parsers (logseq.rs)
 * 2. **Graph Registration**: Create or update graph in registry
 * 3. **Data Import**: Process pages and blocks with reference resolution
 * 4. **Error Collection**: Aggregate non-fatal errors for reporting
 * 5. **Result Reporting**: Return comprehensive import statistics
 * 
 * ## Error Handling
 * 
 * The import process is designed to be resilient:
 * - Parse errors for individual files are collected but don't stop the import
 * - Reference resolution failures are reported but don't break the graph
 * - Only fatal errors (registry issues, I/O failures) abort the import
 * 
 * This approach ensures maximum data recovery from potentially corrupted sources.
 */

use std::path::Path;
use std::error::Error;
use tracing::{info, error};
use crate::app_state::AppState;
use super::logseq;

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
    app_state: &AppState,
    logseq_path: &Path,
    custom_name: Option<String>,
) -> Result<ImportResult, Box<dyn Error + Send + Sync>> {
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
    
    // Use the resolved data directory from app state
    let data_dir = &app_state.data_dir;
    
    // Register the graph with the registry and get its ID
    let graph_id = {
        // Debug assertion to fail fast if another thread holds the write lock
        debug_assert!(
            app_state.graph_registry.try_write().is_ok(),
            "Registry write lock unavailable - another thread may be holding it"
        );
        
        let mut registry = app_state.graph_registry.write()
            .map_err(|e| format!("Failed to write graph registry: {}", e))?;
        
        let graph_info = registry.register_graph(
            None,  // Let registry generate ID
            Some(graph_name.clone()),
            Some(format!("Imported from: {}", logseq_path.display())),
            data_dir
        )?;
        
        // Save the registry after creating the graph
        registry.save()?;
        
        graph_info.id
    };
    
    info!("📊 Using graph: {} ({})", graph_name, graph_id);
    
    // Create the graph manager if it doesn't exist
    app_state.get_or_create_graph_manager(&graph_id).await?;
    
    // Import the data
    let (page_count, block_count, errors) = {
        let resources = app_state.graph_resources.read().await;
        
        let graph_resources = resources.get(&graph_id)
            .ok_or_else(|| format!("Graph not found for ID: {}", graph_id))?;
        
        let mut graph_manager = graph_resources.manager.write().await;
        
        // Disable auto-save during bulk import for performance
        graph_manager.disable_auto_save();
        
        let mut errors = Vec::new();
        
        let mut page_count = 0;
        for page in pages {
            match page.apply_to_graph(&mut *graph_manager) {
                Ok(_) => page_count += 1,
                Err(e) => {
                    let err_msg = format!("Failed to import page {}: {}", page.name, e);
                    error!("{}", err_msg);
                    errors.push(err_msg);
                }
            }
        }
        
        let mut block_count = 0;
        for block in blocks {
            match block.apply_to_graph(&mut *graph_manager) {
                Ok(_) => block_count += 1,
                Err(e) => {
                    let err_msg = format!("Failed to import block {}: {}", block.id, e);
                    error!("{}", err_msg);
                    errors.push(err_msg);
                }
            }
        }
        
        // Re-enable auto-save and force save
        graph_manager.enable_auto_save();
        
        graph_manager.save_graph()
            .map_err(|e| format!("Failed to save imported graph: {}", e))?;
        
        // Return the collected data
        (page_count, block_count, errors)
    };
    
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