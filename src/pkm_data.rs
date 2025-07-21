/**
 * @module pkm_data
 * @description Data structures for PKM (Personal Knowledge Management) entities
 * 
 * This module defines the core data structures used throughout the PKM Knowledge Graph
 * system. These structures represent the serialized format of Logseq blocks and pages
 * as they are transmitted from the JavaScript plugin to the Rust backend.
 * 
 * ## Core Types
 * 
 * - `PKMBlockData`: Represents a Logseq block (the fundamental unit of content)
 * - `PKMPageData`: Represents a Logseq page (a container for blocks)
 * - `PKMReference`: Extracted references from block/page content
 * 
 * ## Design Decisions
 * 
 * - All timestamps are transmitted as strings to handle various formats
 * - Properties are stored as raw JSON values for flexibility
 * - References are pre-extracted by JavaScript for performance
 * - Optional fields use Option<T> with serde(default) for robustness
 * 
 * ## Validation
 * 
 * Both PKMBlockData and PKMPageData implement a validate() method that ensures:
 * - Required fields are non-empty (id/name, timestamps)
 * - Data integrity before graph storage
 * 
 * The validation is intentionally lightweight, focusing on critical fields
 * while allowing flexibility for Logseq's evolving data model.
 */

use serde::{Deserialize, Serialize};

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
}

impl PKMBlockData {
    /// Validate the block data to ensure it meets our requirements
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        
        if self.id.is_empty() {
            errors.push("Block ID is empty".to_string());
        }
        
        if self.content.is_empty() {
            errors.push("Block content is empty".to_string());
        }
        
        if self.created.is_empty() {
            errors.push("Created timestamp is empty".to_string());
        }
        
        if self.updated.is_empty() {
            errors.push("Updated timestamp is empty".to_string());
        }
        
        if let Some(parent) = &self.parent {
            if parent.is_empty() {
                errors.push("Parent ID is empty".to_string());
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(", "))
        }
    }
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

impl PKMPageData {
    /// Validate the page data to ensure it meets our requirements
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        
        if self.name.is_empty() {
            errors.push("Page name is empty".to_string());
        }
        
        if self.created.is_empty() {
            errors.push("Created timestamp is empty".to_string());
        }
        
        if self.updated.is_empty() {
            errors.push("Updated timestamp is empty".to_string());
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(", "))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkm_block_data_validation() {
        let mut block = PKMBlockData {
            id: "test-id".to_string(),
            content: "Test content".to_string(),
            created: "2024-01-01".to_string(),
            updated: "2024-01-01".to_string(),
            parent: None,
            children: vec![],
            page: None,
            properties: serde_json::Value::Object(serde_json::Map::new()),
            references: vec![],
        };
        
        // Valid block should pass
        assert!(block.validate().is_ok());
        
        // Empty ID should fail
        block.id = "".to_string();
        assert!(block.validate().is_err());
    }

    #[test]
    fn test_pkm_page_data_validation() {
        let mut page = PKMPageData {
            name: "Test Page".to_string(),
            normalized_name: Some("test page".to_string()),
            created: "2024-01-01".to_string(),
            updated: "2024-01-01".to_string(),
            properties: serde_json::Value::Object(serde_json::Map::new()),
            blocks: vec![],
        };
        
        // Valid page should pass
        assert!(page.validate().is_ok());
        
        // Empty name should fail
        page.name = "".to_string();
        assert!(page.validate().is_err());
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
}