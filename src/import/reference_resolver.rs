/**
 * @module reference_resolver
 * @description Block reference resolution for knowledge graph import
 * 
 * This module handles the expansion of block references during import, converting
 * references like ((block-id)) into the actual content of the referenced block.
 * This is critical for maintaining the semantic richness of knowledge graphs
 * where blocks frequently reference other blocks.
 * 
 * ## Reference Format
 * 
 * Block references follow the pattern: `((block-id))`
 * - `block-id`: UUID or alphanumeric identifier for the target block
 * - References can be nested (references within referenced content)
 * - Multiple references per block are supported
 * 
 * ## Circular Reference Protection
 * 
 * The resolver implements sophisticated circular reference detection:
 * - Maintains a visited set during traversal
 * - Prevents self-references (block referencing itself)
 * - Detects cycles in reference chains
 * - Leaves unresolvable references unchanged
 * 
 * ## Performance Considerations
 * 
 * - Uses regex with lazy static compilation for efficiency
 * - Recursively processes references to handle nested cases
 * - Tracks visited blocks to prevent infinite loops
 * - Optimized for import-time processing (not runtime queries)
 */

use std::collections::{HashMap, HashSet};
use regex::Regex;
use once_cell::sync::Lazy;
use crate::graph_manager::{GraphManager, NodeType};

static BLOCK_REF_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\(\(([a-zA-Z0-9-]+)\)\)").unwrap()
});

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
    
    let result = BLOCK_REF_RE.replace_all(content, |caps: &regex::Captures| {
        let block_id = &caps[1];
        
        // Check for circular references (including self-reference)
        if visited.contains(block_id) {
            return caps[0].to_string(); // Keep original reference
        }
        
        if let Some(referenced_content) = block_map.get(block_id) {
            // Mark this block as visited before recursing
            visited.insert(block_id.to_string());
            
            // Recursively resolve any references in the referenced content
            let expanded = resolve_block_references(referenced_content, block_map, visited, Some(block_id));
            
            // Remove from visited after processing (allows the same block to be referenced multiple times in different contexts)
            visited.remove(block_id);
            
            expanded
        } else {
            // Keep the original reference if we can't find the block
            caps[0].to_string()
        }
    }).to_string();
    
    // Remove current block from visited set after processing
    if let Some(id) = current_block_id {
        visited.remove(id);
    }
    
    result
}

/// Build a map of block ID -> content from a graph manager
/// This is used for reference resolution during graph operations
pub fn build_block_map_from_graph(graph_manager: &GraphManager) -> HashMap<String, String> {
    let mut block_map = HashMap::new();
    
    for idx in graph_manager.graph.node_indices() {
        if let Some(node) = graph_manager.graph.node_weight(idx) {
            if matches!(node.node_type, NodeType::Block) {
                block_map.insert(node.pkm_id.clone(), node.content.clone());
            }
        }
    }
    
    block_map
}

/// Resolve references in content using blocks from the graph manager
/// This is a convenience function that builds the block map and resolves references
pub fn resolve_references_in_graph(
    content: &str,
    current_block_id: &str,
    graph_manager: &GraphManager,
) -> String {
    let block_map = build_block_map_from_graph(graph_manager);
    let mut visited = HashSet::new();
    resolve_block_references(content, &block_map, &mut visited, Some(current_block_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resolve_simple_block_reference() {
        let mut block_map = HashMap::new();
        block_map.insert("block-123".to_string(), "This is the referenced content".to_string());
        
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
        block_map.insert("block-a".to_string(), "Block A references ((block-b))".to_string());
        block_map.insert("block-b".to_string(), "Block B references ((block-c))".to_string());
        block_map.insert("block-c".to_string(), "Block C content".to_string());
        
        let content = "Start with ((block-a))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "Start with Block A references Block B references Block C content");
    }
    
    #[test]
    fn test_resolve_circular_references() {
        let mut block_map = HashMap::new();
        block_map.insert("block-a".to_string(), "A references ((block-b))".to_string());
        block_map.insert("block-b".to_string(), "B references ((block-a))".to_string());
        
        let content = "Start: ((block-a))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        // Should expand both but stop at the circular reference
        assert_eq!(result, "Start: A references B references ((block-a))");
    }
    
    #[test]
    fn test_resolve_self_reference() {
        let mut block_map = HashMap::new();
        block_map.insert("self-ref".to_string(), "This block references itself: ((self-ref))".to_string());
        
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
}