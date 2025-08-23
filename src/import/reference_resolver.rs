//! # Block Reference Resolution Engine
//!
//! This module provides sophisticated block reference resolution for knowledge graph import,
//! handling the critical task of expanding block references like `((block-id))` into their
//! actual content. This transformation is essential for maintaining the semantic richness
//! of knowledge graphs where blocks frequently reference and build upon other blocks,
//! creating complex webs of interconnected knowledge.
//!
//! ## Reference Format and Semantics
//!
//! ### Block Reference Syntax
//! Block references follow the pattern: `((block-id))`
//! - **Block ID**: UUID or alphanumeric identifier for the target block
//! - **Nested References**: References can exist within referenced content, creating chains
//! - **Multiple References**: Single blocks can contain multiple references to different blocks
//! - **Mixed Content**: References can be embedded within regular text and markdown
//!
//! ### Reference Resolution Strategy
//! When a block reference `((target-123))` is encountered:
//! 1. **Lookup**: Find the target block's content using the block ID
//! 2. **Expansion**: Replace the reference with the target block's actual content
//! 3. **Recursion**: Process any references within the expanded content
//! 4. **Safety**: Apply circular reference detection and loop prevention
//!
//! ## Circular Reference Protection System
//!
//! The resolver implements a comprehensive multi-layered protection system:
//!
//! ### Visited Set Tracking
//! - Maintains a `HashSet<String>` of currently visited block IDs during traversal
//! - Prevents infinite loops by detecting when a block is referenced while being processed
//! - Automatically adds/removes blocks from the visited set during recursion
//!
//! ### Self-Reference Prevention
//! - Explicitly prevents blocks from referencing themselves
//! - Uses the `current_block_id` parameter to identify self-references
//! - Preserves original reference syntax for self-references rather than expanding
//!
//! ### Cycle Detection Algorithm
//! - Detects complex cycles like A→B→C→A through visited set membership testing
//! - Stops expansion at the point where a cycle is detected
//! - Maintains partial expansion up to the cycle point for maximum information preservation
//!
//! ## Performance Optimization Techniques
//!
//! ### Lazy Static Regex Compilation
//! - Uses `once_cell::sync::Lazy` to compile regex patterns only once
//! - Avoids repeated regex compilation overhead during batch processing
//! - Pattern: `r"\(\(([a-zA-Z0-9-]+)\)\)"` optimized for common block ID formats
//!
//! ### Efficient Content Mapping
//! - Pre-builds comprehensive block ID → content mappings for fast lookups
//! - Separates mapping construction from resolution to optimize batch operations
//! - Provides specialized functions for building maps from GraphManager instances
//!
//! ### Recursive Processing with Backtracking
//! - Processes references recursively to handle arbitrary nesting depths
//! - Uses backtracking to properly manage the visited set across recursive calls
//! - Ensures each block can be referenced multiple times in different contexts
//!
//! ## Integration with Import Pipeline
//!
//! ### Primary Use Cases
//! 1. **Import-Time Resolution**: Expands references during initial graph construction
//! 2. **Update Resolution**: Re-resolves references when block content changes
//! 3. **Graph Queries**: Provides expanded content for search and analysis operations
//!
//! ### API Design Patterns
//! - **Standalone Resolution**: `resolve_block_references()` for direct reference processing
//! - **Graph Integration**: `resolve_references_in_graph()` for GraphManager integration
//! - **Mapping Utilities**: `build_block_map_from_graph()` for efficient batch operations
//!
//! ### Error Handling Philosophy
//! - **Graceful Degradation**: Missing references are preserved in original form
//! - **No Exceptions**: Never throws errors for unresolvable references
//! - **Information Preservation**: Maintains maximum information even with partial failures
//!
//! This module serves as the foundation for all reference resolution operations
//! across the import system, ensuring consistent and reliable expansion of block
//! references while maintaining performance and data integrity.

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