//! EDN (Extensible Data Notation) Configuration Manipulation
//!
//! This module provides utilities for safely manipulating Logseq's config.edn files,
//! specifically for managing Cymbiont-specific properties. It handles the complexities
//! of EDN format parsing and modification using regex-based approaches.
//!
//! ## Overview
//!
//! Logseq stores its configuration in EDN format, a Clojure-inspired data notation.
//! Cymbiont needs to add two properties to ensure proper functionality:
//! - `:block-hidden-properties` - A set containing `:cymbiont-updated-ms` to hide timestamps
//! - `:cymbiont/graph-id` - A UUID string for multi-graph identification
//!
//! ## Design Decisions
//!
//! We use regex-based manipulation rather than a full EDN parser because:
//! 1. We only need to modify two specific properties
//! 2. Full EDN parsing would require additional dependencies
//! 3. Regex patterns are sufficient for our targeted modifications
//! 4. We preserve all formatting and comments in the user's config
//!
//! ## Key Functions
//!
//! - `update_block_hidden_properties()` - Adds a property to the `:block-hidden-properties` set
//! - `update_graph_id()` - Adds or updates the `:cymbiont/graph-id` property
//! - `validate_config_properties()` - Checks if required properties are present
//! - `update_config_file()` - Main entry point that validates and updates a config file
//!
//! ## Regex Patterns
//!
//! The module uses multiline regex mode (`(?m)`) to ensure we only match actual
//! property lines, not comments. Key patterns:
//! - `^(\s*):block-hidden-properties\s*#\{([^}]*)\}` - Matches property set
//! - `^(\s*):cymbiont/graph-id\s+"([^"]+)"` - Matches graph ID
//!
//! ## Error Handling
//!
//! All functions return `Result<T, EdnError>` where EdnError can be:
//! - IO errors from file operations
//! - Regex compilation errors (unlikely with our static patterns)
//! - Validation errors for malformed configs
//!
//! ## Testing
//!
//! The module includes comprehensive tests covering:
//! - Basic property addition
//! - Idempotency (not modifying when property exists)
//! - Comment line handling
//! - Indentation preservation
//! - Error cases for malformed configs

use std::fs;
use std::path::Path;
use regex::Regex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EdnError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),
    
    #[error("Config validation failed: {0}")]
    ValidationFailed(String),
}

pub type Result<T> = std::result::Result<T, EdnError>;

/// Result of config validation
#[derive(Debug)]
pub struct ConfigValidation {
    pub has_hidden_property: bool,
    pub has_graph_id: bool,
    pub graph_id: Option<String>,
}

/// Update the :block-hidden-properties set to include the specified property
pub fn update_block_hidden_properties(content: &str, property: &str) -> Result<String> {
    // Use regex to find actual :block-hidden-properties lines (not comments)
    let re = Regex::new(r"(?m)^(\s*):block-hidden-properties\s*#\{([^}]*)\}")?;
    
    // Check if there's already an actual property line
    if let Some(captures) = re.captures(content) {
        let existing_props = captures.get(2).map_or("", |m| m.as_str().trim());
        
        // Check if property is already there
        if existing_props.contains(property) {
            return Ok(content.to_string());
        }
        
        // Add to existing properties
        let new_props = if existing_props.is_empty() {
            property.to_string()
        } else {
            format!("{} {}", existing_props, property)
        };
        
        // Replace the existing line
        let indent = captures.get(1).map_or("", |m| m.as_str());
        let new_line = format!("{}:block-hidden-properties #{{{}}}", indent, new_props);
        let updated_content = content.replace(&captures[0], &new_line);
        
        Ok(updated_content)
    } else {
        // No existing property line found, add it after the comment section
        if let Some(comment_pos) = content.find(";; :block-hidden-properties #{:public :icon}") {
            // Find the end of this comment line
            if let Some(newline_pos) = content[comment_pos..].find('\n') {
                let insert_pos = comment_pos + newline_pos + 1;
                let before = &content[..insert_pos];
                let after = &content[insert_pos..];
                
                // Insert the new property line
                let updated_content = format!("{}:block-hidden-properties #{{{}}}\n{}", before, property, after);
                Ok(updated_content)
            } else {
                Err(EdnError::ValidationFailed(
                    "Could not find newline after block-hidden-properties comment".to_string()
                ))
            }
        } else {
            Err(EdnError::ValidationFailed(
                "Could not find block-hidden-properties comment section to insert after".to_string()
            ))
        }
    }
}

/// Add or update the :cymbiont/graph-id property
pub fn update_graph_id(content: &str, graph_id: &str) -> Result<String> {
    // Check if the property already exists
    let re = Regex::new(r#"(?m)^(\s*):cymbiont/graph-id\s+"([^"]+)""#)?;
    
    if let Some(captures) = re.captures(content) {
        // Property exists, check if it matches
        let existing_id = captures.get(2).map_or("", |m| m.as_str());
        if existing_id == graph_id {
            return Ok(content.to_string());
        }
        
        // Replace with new ID
        let indent = captures.get(1).map_or("", |m| m.as_str());
        let new_line = format!("{}:cymbiont/graph-id \"{}\"", indent, graph_id);
        let updated_content = content.replace(&captures[0], &new_line);
        Ok(updated_content)
    } else {
        // Property doesn't exist, add it at the end before the closing brace
        if let Some(last_brace_pos) = content.rfind('}') {
            // Find the last newline before the closing brace
            let before_brace = &content[..last_brace_pos];
            if let Some(_last_newline_pos) = before_brace.rfind('\n') {
                let indent = " ";  // Use single space for consistency with other properties
                let new_property = format!("\n{}:cymbiont/graph-id \"{}\"", indent, graph_id);
                
                // Insert before the closing brace
                let mut updated_content = String::with_capacity(content.len() + new_property.len());
                updated_content.push_str(&content[..last_brace_pos]);
                updated_content.push_str(&new_property);
                updated_content.push_str(&content[last_brace_pos..]);
                
                Ok(updated_content)
            } else {
                Err(EdnError::ValidationFailed(
                    "Could not find proper position to insert graph ID".to_string()
                ))
            }
        } else {
            Err(EdnError::ValidationFailed(
                "Could not find closing brace in config".to_string()
            ))
        }
    }
}

/// Validate that the config has both required properties
pub fn validate_config_properties(content: &str) -> ConfigValidation {
    // Check for hidden property
    let hidden_re = Regex::new(r"(?m)^(\s*):block-hidden-properties\s*#\{([^}]*)\}").unwrap();
    let has_hidden_property = if let Some(captures) = hidden_re.captures(content) {
        let props = captures.get(2).map_or("", |m| m.as_str());
        props.contains(":cymbiont-updated-ms")
    } else {
        false
    };
    
    // Check for graph ID
    let id_re = Regex::new(r#"(?m)^(\s*):cymbiont/graph-id\s+"([^"]+)""#).unwrap();
    let (has_graph_id, graph_id) = if let Some(captures) = id_re.captures(content) {
        let id = captures.get(2).map_or("", |m| m.as_str());
        (true, Some(id.to_string()))
    } else {
        (false, None)
    };
    
    ConfigValidation {
        has_hidden_property,
        has_graph_id,
        graph_id,
    }
}

/// Update a config file with both required properties
pub fn update_config_file(path: &Path, graph_id: &str) -> Result<()> {
    use tracing::debug;
    
    debug!("update_config_file called for path: {:?}, graph_id: {}", path, graph_id);
    
    // Read the current content
    let content = fs::read_to_string(path)?;
    debug!("Read {} bytes from config.edn", content.len());
    
    // Validate current state
    let validation = validate_config_properties(&content);
    debug!("Validation result - has_hidden_property: {}, has_graph_id: {}, graph_id: {:?}", 
         validation.has_hidden_property, validation.has_graph_id, validation.graph_id);
    
    // Update if needed
    let mut updated_content = content;
    let mut updated = false;
    
    if !validation.has_hidden_property {
        debug!("Attempting to update block-hidden-properties");
        updated_content = update_block_hidden_properties(&updated_content, ":cymbiont-updated-ms")?;
        updated = true;
    }
    
    if !validation.has_graph_id || validation.graph_id.as_deref() != Some(graph_id) {
        debug!("Attempting to update graph ID");
        updated_content = update_graph_id(&updated_content, graph_id)?;
        updated = true;
    }
    
    // Write back if changes were made
    if updated {
        debug!("Writing updated config back to {:?}", path);
        fs::write(path, updated_content)?;
    } else {
        debug!("No updates needed for config.edn");
    }
    
    Ok(())
}

/// Update a graph's config.edn to hide cymbiont properties before launching Logseq
/// 
/// This is a legacy function that updates only the block-hidden-properties.
/// It's preserved here as it contains finicky regex logic that works correctly.
/// Currently disabled in favor of runtime validation via the /config/validate endpoint.
#[allow(dead_code)]
pub fn update_config_for_prelaunch(graph_path: &str) -> Result<()> {
    use std::path::PathBuf;
    use tracing::{info, warn};
    
    let config_path = PathBuf::from(graph_path).join("logseq").join("config.edn");
    
    if !config_path.exists() {
        warn!("Config.edn not found at {:?} - skipping property hiding", config_path);
        return Ok(());
    }
    
    // Read the file
    let content = fs::read_to_string(&config_path)?;
    
    // Update block hidden properties
    match update_block_hidden_properties(&content, ":cymbiont-updated-ms") {
        Ok(updated_content) => {
            if updated_content != content {
                fs::write(&config_path, &updated_content)?;
                info!("✅ Updated config.edn to hide :cymbiont-updated-ms property");
            }
            Ok(())
        }
        Err(e) => {
            warn!("Failed to update config.edn: {}", e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_block_hidden_properties_empty() {
        let content = r#"{:meta/version 1
 ;; :block-hidden-properties #{:public :icon}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert!(result.contains(":block-hidden-properties #{:cymbiont-updated-ms}"));
    }

    #[test]
    fn test_update_block_hidden_properties_existing() {
        let content = r#"{:meta/version 1
 :block-hidden-properties #{:public :icon}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert!(result.contains(":block-hidden-properties #{:public :icon :cymbiont-updated-ms}"));
    }

    #[test]
    fn test_update_graph_id_new() {
        let content = r#"{:meta/version 1
 :preferred-workflow :now}"#;
        
        let result = update_graph_id(content, "test-uuid-123").unwrap();
        assert!(result.contains(":cymbiont/graph-id \"test-uuid-123\""));
    }

    #[test]
    fn test_validate_config_properties() {
        let content = r#"{:meta/version 1
 :block-hidden-properties #{:cymbiont-updated-ms}
 :cymbiont/graph-id "test-uuid-123"}"#;
        
        let validation = validate_config_properties(content);
        assert!(validation.has_hidden_property);
        assert!(validation.has_graph_id);
        assert_eq!(validation.graph_id, Some("test-uuid-123".to_string()));
    }

    #[test]
    fn test_update_block_hidden_properties_already_exists() {
        let content = r#"{:meta/version 1
 :block-hidden-properties #{:public :cymbiont-updated-ms :icon}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert_eq!(result, content); // Should not change
    }

    #[test]
    fn test_update_block_hidden_properties_comment_line() {
        let content = r#"{:meta/version 1
 ;; :block-hidden-properties #{:public :icon}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert!(result.contains(":block-hidden-properties #{:cymbiont-updated-ms}"));
        // The property should be added after the comment
        let lines: Vec<&str> = result.lines().collect();
        let comment_idx = lines.iter().position(|l| l.contains(";; :block-hidden-properties")).unwrap();
        let property_idx = lines.iter().position(|l| l.starts_with(":block-hidden-properties")).unwrap();
        assert!(property_idx == comment_idx + 1, "Property should be right after comment");
    }

    #[test]
    fn test_update_graph_id_existing_same() {
        let content = r#"{:meta/version 1
 :cymbiont/graph-id "test-uuid-123"}"#;
        
        let result = update_graph_id(content, "test-uuid-123").unwrap();
        assert_eq!(result, content); // Should not change
    }

    #[test]
    fn test_update_graph_id_existing_different() {
        let content = r#"{:meta/version 1
 :cymbiont/graph-id "old-uuid-456"}"#;
        
        let result = update_graph_id(content, "new-uuid-789").unwrap();
        assert!(result.contains(":cymbiont/graph-id \"new-uuid-789\""));
        assert!(!result.contains("old-uuid-456"));
    }

    #[test]
    fn test_update_block_hidden_properties_no_comment() {
        let content = r#"{:meta/version 1
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms");
        assert!(result.is_err());
        match result {
            Err(EdnError::ValidationFailed(msg)) => {
                assert!(msg.contains("Could not find block-hidden-properties comment"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }

    #[test]
    fn test_update_graph_id_no_closing_brace() {
        let content = r#"{:meta/version 1
 :preferred-workflow :now"#;
        
        let result = update_graph_id(content, "test-uuid");
        assert!(result.is_err());
        match result {
            Err(EdnError::ValidationFailed(msg)) => {
                assert!(msg.contains("Could not find closing brace"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }

    #[test]
    fn test_validate_missing_both_properties() {
        let content = r#"{:meta/version 1
 :preferred-workflow :now}"#;
        
        let validation = validate_config_properties(content);
        assert!(!validation.has_hidden_property);
        assert!(!validation.has_graph_id);
        assert_eq!(validation.graph_id, None);
    }

    #[test]
    fn test_update_block_hidden_properties_with_indentation() {
        let content = r#"{:meta/version 1
    :block-hidden-properties #{:public}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert!(result.contains("    :block-hidden-properties #{:public :cymbiont-updated-ms}"));
    }

    #[test]
    fn test_multiline_regex_matches_property_not_comment() {
        let content = r#"{:meta/version 1
 ;; This is a comment that mentions :block-hidden-properties #{:fake}
 :block-hidden-properties #{:real}
 :preferred-workflow :now}"#;
        
        let result = update_block_hidden_properties(content, ":cymbiont-updated-ms").unwrap();
        assert!(result.contains(":block-hidden-properties #{:real :cymbiont-updated-ms}"));
        // The comment line should remain unchanged
        assert!(result.contains(";; This is a comment that mentions :block-hidden-properties #{:fake}"));
    }
}