use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use chrono::Utc;
use regex::Regex;
use serde_json::json;
use tracing::{info, warn};
use super::pkm_data::{PKMBlockData, PKMPageData, PKMReference};
use super::{ImportError, Result};
use super::reference_resolver::resolve_block_references;

/// A Logseq block with its content and metadata
#[derive(Debug, Clone)]
struct LogseqBlock {
    id: Option<String>,
    content: String,
    properties: HashMap<String, String>,
    children: Vec<LogseqBlock>,
    indent_level: usize,
}

/// Import a Logseq graph from a directory
pub fn import_graph(logseq_dir: &Path) -> Result<(Vec<PKMPageData>, Vec<PKMBlockData>)> {
    if !logseq_dir.exists() {
        return Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Logseq directory not found: {:?}", logseq_dir),
        )));
    }

    let mut pages = Vec::new();
    let mut blocks = Vec::new();
    
    // Import pages
    let pages_dir = logseq_dir.join("pages");
    if pages_dir.exists() {
        info!("Importing pages from {:?}", pages_dir);
        import_directory(&pages_dir, &mut pages, &mut blocks, false)?;
    }
    
    // Import journals
    let journals_dir = logseq_dir.join("journals");
    if journals_dir.exists() {
        info!("Importing journals from {:?}", journals_dir);
        import_directory(&journals_dir, &mut pages, &mut blocks, true)?;
    }
    
    // Build block ID -> content map for reference resolution
    let mut block_map: HashMap<String, String> = HashMap::new();
    for block in &blocks {
        block_map.insert(block.id.clone(), block.content.clone());
    }
    
    // Resolve references in all blocks
    for block in &mut blocks {
        let mut visited = HashSet::new();
        let expanded_content = resolve_block_references(
            &block.content,
            &block_map,
            &mut visited,
            Some(&block.id),
        );
        block.reference_content = Some(expanded_content);
    }
    
    info!("Import complete: {} pages, {} blocks", pages.len(), blocks.len());
    Ok((pages, blocks))
}

/// Import all markdown files from a directory
fn import_directory(
    dir: &Path,
    pages: &mut Vec<PKMPageData>,
    blocks: &mut Vec<PKMBlockData>,
    _is_journal: bool,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            if let Err(e) = import_file(&path, pages, blocks, _is_journal) {
                warn!("Failed to import {:?}: {}", path, e);
            }
        }
    }
    Ok(())
}

/// Import a single Logseq markdown file
fn import_file(
    path: &Path,
    pages: &mut Vec<PKMPageData>,
    blocks: &mut Vec<PKMBlockData>,
    _is_journal: bool,
) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let page_name = path.file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ImportError::InvalidFormat("Invalid filename".to_string()))?;
    
    info!("Importing page: {}", page_name);
    
    // Parse the file into blocks
    let logseq_blocks = parse_logseq_file(&content)?;
    
    // Extract page properties from the first lines if they exist
    let mut page_properties = json!({});
    let mut page_updated = None;
    
    // Look for properties at the start of the file (before any blocks)
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('-') || line.starts_with('\t') {
            break;
        }
        if let Some((key, value)) = parse_property(line) {
            if key == "cymbiont-updated-ms" {
                page_updated = Some(value.clone());
            }
            page_properties[key] = json!(value);
        }
    }
    
    // Convert to PKM structures
    let now = Utc::now().timestamp_millis().to_string();
    let page_blocks = convert_blocks_to_pkm(
        &logseq_blocks,
        page_name,
        blocks,
        None,
    )?;
    
    // Create the page
    let page = PKMPageData {
        name: page_name.to_string(),
        normalized_name: Some(normalize_name(page_name)),
        created: page_updated.clone().unwrap_or_else(|| now.clone()),
        updated: page_updated.unwrap_or(now),
        properties: page_properties,
        blocks: page_blocks,
    };
    
    pages.push(page);
    Ok(())
}

/// Parse a Logseq file into a tree of blocks
fn parse_logseq_file(content: &str) -> Result<Vec<LogseqBlock>> {
    if content.trim().is_empty() {
        return Err(ImportError::Parse("Empty file content".to_string()));
    }
    
    let mut all_blocks: Vec<LogseqBlock> = Vec::new();
    let mut current_block: Option<LogseqBlock> = None;
    let mut in_properties = false;
    
    // First pass: parse all blocks without hierarchy
    for line in content.lines() {
        // Skip page-level properties at the start
        if !line.starts_with('-') && !line.starts_with('\t') && !line.starts_with(' ') {
            if parse_property(line).is_some() {
                continue;
            }
        }
        
        // Check if this is a block line
        if let Some(indent_level) = get_indent_level(line) {
            // Save the previous block if any
            if let Some(block) = current_block.take() {
                all_blocks.push(block);
            }
            
            // Start a new block
            let content = line.trim_start().trim_start_matches('-').trim();
            current_block = Some(LogseqBlock {
                id: None,
                content: content.to_string(),
                properties: HashMap::new(),
                children: Vec::new(),
                indent_level,
            });
            in_properties = true;
        } else if in_properties && current_block.is_some() {
            // Check if this is a property line for the current block
            let trimmed = line.trim();
            if let Some((key, value)) = parse_property(trimmed) {
                if let Some(ref mut block) = current_block {
                    if key == "id" {
                        block.id = Some(value.clone());
                    }
                    block.properties.insert(key, value);
                }
            } else if !trimmed.is_empty() {
                // This is continuation of block content
                in_properties = false;
                if let Some(ref mut block) = current_block {
                    block.content.push('\n');
                    block.content.push_str(trimmed);
                }
            }
        } else if let Some(ref mut block) = current_block {
            // Continuation of block content
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                block.content.push('\n');
                block.content.push_str(trimmed);
            }
        }
    }
    
    // Add the last block
    if let Some(block) = current_block {
        all_blocks.push(block);
    }
    
    // Second pass: build hierarchy
    build_block_hierarchy(all_blocks)
}

/// Build hierarchy from a flat list of blocks based on indentation
fn build_block_hierarchy(blocks: Vec<LogseqBlock>) -> Result<Vec<LogseqBlock>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }
    
    // Convert to a format we can work with
    let mut nodes: Vec<Option<LogseqBlock>> = blocks.into_iter().map(Some).collect();
    let mut parent_indices: Vec<Option<usize>> = vec![None; nodes.len()];
    
    // First pass: determine parent relationships
    let mut stack: Vec<usize> = Vec::new();
    
    for i in 0..nodes.len() {
        let current_indent = nodes[i].as_ref().unwrap().indent_level;
        
        // Pop from stack until we find a potential parent
        while let Some(&parent_idx) = stack.last() {
            let parent_indent = nodes[parent_idx].as_ref().unwrap().indent_level;
            if parent_indent < current_indent {
                // Found a parent
                parent_indices[i] = Some(parent_idx);
                break;
            }
            stack.pop();
        }
        
        // Push current block index to stack
        stack.push(i);
    }
    
    // Second pass: build the tree by moving children into their parents
    // Process in reverse order so we move children before parents
    for i in (0..nodes.len()).rev() {
        if let Some(parent_idx) = parent_indices[i] {
            if let Some(child) = nodes[i].take() {
                if let Some(ref mut parent) = nodes[parent_idx] {
                    parent.children.insert(0, child); // Insert at beginning to maintain order
                }
            }
        }
    }
    
    // Collect remaining root blocks
    let root_blocks: Vec<LogseqBlock> = nodes.into_iter()
        .filter_map(|node| node)
        .collect();
    
    Ok(root_blocks)
}

/// Get the indentation level of a line (number of tabs or spaces / 2)
fn get_indent_level(line: &str) -> Option<usize> {
    if !line.trim_start().starts_with('-') {
        return None;
    }
    
    let mut level = 0;
    for ch in line.chars() {
        match ch {
            '\t' => level += 2,
            ' ' => level += 1, // Count spaces, we'll divide by 2 later
            '-' => break,
            _ => return None,
        }
    }
    
    // Convert spaces to tab-equivalent (2 spaces = 1 tab)
    Some(level / 2)
}

/// Parse a property line (key:: value)
fn parse_property(line: &str) -> Option<(String, String)> {
    let re = Regex::new(r"^([a-zA-Z0-9_-]+)::\s*(.+)$").unwrap();
    re.captures(line).map(|caps| {
        (caps[1].to_string(), caps[2].to_string())
    })
}

/// Convert Logseq blocks to PKM blocks recursively
fn convert_blocks_to_pkm(
    logseq_blocks: &[LogseqBlock],
    page_name: &str,
    all_blocks: &mut Vec<PKMBlockData>,
    parent_id: Option<String>,
) -> Result<Vec<String>> {
    let mut block_ids = Vec::new();
    
    for logseq_block in logseq_blocks {
        // Skip empty blocks
        if logseq_block.content.trim().is_empty() {
            continue;
        }
        
        let block_id = logseq_block.id.clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        
        // Extract references from content
        let references = extract_references(&logseq_block.content);
        
        // Get timestamps from properties or use current time
        let now = Utc::now().timestamp_millis().to_string();
        let created = logseq_block.properties.get("created")
            .or_else(|| logseq_block.properties.get("cymbiont-updated-ms"))
            .cloned()
            .unwrap_or_else(|| now.clone());
        let updated = logseq_block.properties.get("updated")
            .or_else(|| logseq_block.properties.get("cymbiont-updated-ms"))
            .cloned()
            .unwrap_or(now);
        
        // Convert properties to JSON
        let mut properties = json!({});
        for (key, value) in &logseq_block.properties {
            if key != "id" && key != "created" && key != "updated" {
                properties[key] = json!(value);
            }
        }
        
        // Convert children recursively
        let children = convert_blocks_to_pkm(
            &logseq_block.children,
            page_name,
            all_blocks,
            Some(block_id.clone()),
        )?;
        
        let block = PKMBlockData {
            id: block_id.clone(),
            content: logseq_block.content.clone(),
            created,
            updated,
            parent: parent_id.clone(),
            children: children.clone(),
            page: Some(page_name.to_string()),
            properties,
            references,
            reference_content: None, // Will be populated in a second pass
        };
        
        all_blocks.push(block);
        block_ids.push(block_id);
    }
    
    Ok(block_ids)
}

/// Extract references from block content
pub fn extract_references(content: &str) -> Vec<PKMReference> {
    let mut references = Vec::new();
    
    // Extract page references [[page]]
    let page_ref_re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    for caps in page_ref_re.captures_iter(content) {
        references.push(PKMReference {
            r#type: "page".to_string(),
            name: caps[1].to_string(),
            id: String::new(),
        });
    }
    
    // Extract block references ((block-id))
    let block_ref_re = Regex::new(r"\(\(([a-zA-Z0-9-]+)\)\)").unwrap();
    for caps in block_ref_re.captures_iter(content) {
        references.push(PKMReference {
            r#type: "block".to_string(),
            name: String::new(),
            id: caps[1].to_string(),
        });
    }
    
    // Extract tags #tag
    let tag_re = Regex::new(r"#([a-zA-Z0-9_-]+)").unwrap();
    for caps in tag_re.captures_iter(content) {
        references.push(PKMReference {
            r#type: "tag".to_string(),
            name: caps[1].to_string(),
            id: String::new(),
        });
    }
    
    references
}

/// Normalize a page name (lowercase, replace spaces with underscores)
fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "_")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::reference_resolver::resolve_block_references;
    
    #[test]
    fn test_parse_property() {
        assert_eq!(
            parse_property("id:: 12345"),
            Some(("id".to_string(), "12345".to_string()))
        );
        assert_eq!(
            parse_property("cymbiont-updated-ms:: 1752719785318"),
            Some(("cymbiont-updated-ms".to_string(), "1752719785318".to_string()))
        );
        assert_eq!(parse_property("not a property"), None);
    }
    
    #[test]
    fn test_get_indent_level() {
        assert_eq!(get_indent_level("- Block"), Some(0));
        assert_eq!(get_indent_level("\t- Block"), Some(1));
        assert_eq!(get_indent_level("\t\t- Block"), Some(2));
        assert_eq!(get_indent_level("  - Block"), Some(1)); // 2 spaces = level 1
        assert_eq!(get_indent_level("    - Block"), Some(2)); // 4 spaces = 2 levels
        assert_eq!(get_indent_level("Not a block"), None);
    }
    
    #[test]
    fn test_extract_references() {
        let content = "This is a [[page reference]] and a ((12345)) block ref with #tag";
        let refs = extract_references(content);
        
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].r#type, "page");
        assert_eq!(refs[0].name, "page reference");
        assert_eq!(refs[1].r#type, "block");
        assert_eq!(refs[1].id, "12345");
        assert_eq!(refs[2].r#type, "tag");
        assert_eq!(refs[2].name, "tag");
    }
    
    #[test]
    fn test_parse_simple_block() {
        let content = "- This is a simple block";
        let blocks = parse_logseq_file(content).unwrap();
        
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "This is a simple block");
        assert_eq!(blocks[0].indent_level, 0);
        assert!(blocks[0].id.is_none());
        assert!(blocks[0].children.is_empty());
    }
    
    #[test]
    fn test_parse_block_with_id() {
        let content = r#"- This is a block
  id:: 12345-67890"#;
        let blocks = parse_logseq_file(content).unwrap();
        
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "This is a block");
        assert_eq!(blocks[0].id, Some("12345-67890".to_string()));
        assert_eq!(blocks[0].properties.get("id"), Some(&"12345-67890".to_string()));
    }
    
    #[test]
    fn test_parse_nested_blocks() {
        let content = "- Parent block\n\t- Child block 1\n\t- Child block 2\n\t\t- Grandchild block";
        let blocks = parse_logseq_file(content).unwrap();
        
        
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "Parent block");
        assert_eq!(blocks[0].children.len(), 2);
        
        assert_eq!(blocks[0].children[0].content, "Child block 1");
        assert_eq!(blocks[0].children[1].content, "Child block 2");
        assert_eq!(blocks[0].children[1].children.len(), 1);
        assert_eq!(blocks[0].children[1].children[0].content, "Grandchild block");
    }
    
    #[test]
    fn test_parse_block_with_multiple_properties() {
        let content = r#"- Block with properties
  id:: test-id
  created:: 2024-01-01
  tags:: #rust #testing"#;
        let blocks = parse_logseq_file(content).unwrap();
        
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].properties.len(), 3);
        assert_eq!(blocks[0].properties.get("id"), Some(&"test-id".to_string()));
        assert_eq!(blocks[0].properties.get("created"), Some(&"2024-01-01".to_string()));
        assert_eq!(blocks[0].properties.get("tags"), Some(&"#rust #testing".to_string()));
    }
    
    #[test]
    fn test_parse_multiline_block() {
        let content = r#"- First line of block
  This is the second line
  And a third line"#;
        let blocks = parse_logseq_file(content).unwrap();
        
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "First line of block\nThis is the second line\nAnd a third line");
    }
    
    #[test]
    fn test_parse_empty_blocks_filtered() {
        let content = "- Non-empty block\n- \n\t- Child of empty";
        let blocks = parse_logseq_file(content).unwrap();
        
        // The empty block should be parsed but filtered out during conversion
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].content, "Non-empty block");
        assert_eq!(blocks[1].content, "");
        assert_eq!(blocks[1].children.len(), 1);
        assert_eq!(blocks[1].children[0].content, "Child of empty");
    }
    
    #[test]
    fn test_convert_blocks_filters_empty() {
        let logseq_blocks = vec![
            LogseqBlock {
                id: None,
                content: "Good block".to_string(),
                properties: HashMap::new(),
                children: vec![],
                indent_level: 0,
            },
            LogseqBlock {
                id: None,
                content: "  ".to_string(), // Empty after trim
                properties: HashMap::new(),
                children: vec![],
                indent_level: 0,
            },
        ];
        
        let mut all_blocks = Vec::new();
        let block_ids = convert_blocks_to_pkm(&logseq_blocks, "test-page", &mut all_blocks, None).unwrap();
        
        assert_eq!(block_ids.len(), 1); // Only one block ID returned
        assert_eq!(all_blocks.len(), 1); // Only one block created
        assert_eq!(all_blocks[0].content, "Good block");
    }
    
    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("Test Page"), "test_page");
        assert_eq!(normalize_name("UPPERCASE"), "uppercase");
        assert_eq!(normalize_name("Multiple  Spaces"), "multiple__spaces");
    }
    
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
        
        // Test from a third block's perspective
        let content = "Start: ((block-a))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        // Should expand both but stop at the circular reference
        assert_eq!(result, "Start: A references B references ((block-a))");
        
        // Test when we're inside block-a itself
        let content2 = "((block-a))";
        let mut visited2 = HashSet::new();
        let result2 = resolve_block_references(content2, &block_map, &mut visited2, Some("block-a"));
        
        // Should not expand self-reference
        assert_eq!(result2, "((block-a))");
    }
    
    #[test]
    fn test_resolve_missing_block_reference() {
        let block_map = HashMap::new();
        
        let content = "Reference to ((missing-block)) should remain";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "Reference to ((missing-block)) should remain");
    }
    
    #[test]
    fn test_resolve_empty_block_reference() {
        let mut block_map = HashMap::new();
        block_map.insert("empty-block".to_string(), "".to_string());
        
        let content = "Empty: ((empty-block)) here";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "Empty:  here");
    }
    
    #[test]
    fn test_resolve_complex_content() {
        let mut block_map = HashMap::new();
        block_map.insert("block-1".to_string(), "**bold text**".to_string());
        block_map.insert("block-2".to_string(), "[[Page Link]]".to_string());
        
        let content = "Mix of ((block-1)) and ((block-2)) with #tag";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "Mix of **bold text** and [[Page Link]] with #tag");
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
    fn test_resolve_empty_block_reference_syntax() {
        let block_map = HashMap::new();
        
        let content = "Empty ref: (()) should remain";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        // Empty references should remain unchanged
        assert_eq!(result, "Empty ref: (()) should remain");
    }
    
    #[test]
    fn test_resolve_block_ref_at_boundaries() {
        let mut block_map = HashMap::new();
        block_map.insert("start".to_string(), "START".to_string());
        block_map.insert("end".to_string(), "END".to_string());
        
        // At start
        let content = "((start)) of line";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        assert_eq!(result, "START of line");
        
        // At end
        let content2 = "End of line ((end))";
        let mut visited2 = HashSet::new();
        let result2 = resolve_block_references(content2, &block_map, &mut visited2, None);
        assert_eq!(result2, "End of line END");
        
        // Entire content
        let content3 = "((start))";
        let mut visited3 = HashSet::new();
        let result3 = resolve_block_references(content3, &block_map, &mut visited3, None);
        assert_eq!(result3, "START");
    }
    
    #[test]
    fn test_resolve_multiple_refs_same_block() {
        let mut block_map = HashMap::new();
        block_map.insert("repeat".to_string(), "REPEAT".to_string());
        
        let content = "((repeat)) and ((repeat)) again";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "REPEAT and REPEAT again");
    }
    
    #[test]
    fn test_resolve_whitespace_in_ref() {
        let block_map = HashMap::new();
        
        // Extra spaces should not match our regex
        let content = "(( block-123 ))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        // Should remain unchanged
        assert_eq!(result, "(( block-123 ))");
    }
    
    #[test]
    fn test_resolve_deeply_nested() {
        let mut block_map = HashMap::new();
        block_map.insert("level1".to_string(), "L1->((level2))".to_string());
        block_map.insert("level2".to_string(), "L2->((level3))".to_string());
        block_map.insert("level3".to_string(), "L3->((level4))".to_string());
        block_map.insert("level4".to_string(), "L4->((level5))".to_string());
        block_map.insert("level5".to_string(), "L5-END".to_string());
        
        let content = "Start: ((level1))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        assert_eq!(result, "Start: L1->L2->L3->L4->L5-END");
    }
    
    #[test]
    fn test_resolve_mixed_circular_noncircular() {
        let mut block_map = HashMap::new();
        block_map.insert("normal".to_string(), "Normal content".to_string());
        block_map.insert("circ-a".to_string(), "A->((circ-b))".to_string());
        block_map.insert("circ-b".to_string(), "B->((circ-a))".to_string());
        block_map.insert("mixed".to_string(), "Has ((normal)) and ((circ-a))".to_string());
        
        let content = "((mixed))";
        let mut visited = HashSet::new();
        let result = resolve_block_references(content, &block_map, &mut visited, None);
        
        // Should expand normal and circ-a, but stop at circular reference
        assert_eq!(result, "Has Normal content and A->B->((circ-a))");
    }
    
    #[test]
    fn test_parse_page_properties() {
        let content = r#"title:: My Page
created:: 2024-01-01

- First block"#;
        let blocks = parse_logseq_file(content).unwrap();
        
        // Page properties are skipped by the block parser
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "First block");
    }
}