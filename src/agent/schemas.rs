//! Tool Schema Definitions for LLM Function Calling
//!
//! This module defines the schema structures used to describe tools to LLM providers.
//! These schemas follow the format used by Ollama and other function-calling capable
//! models, enabling agents to understand what tools are available and how to use them.
//!
//! ## Schema Structure
//!
//! Each tool is described by:
//! - **Name**: Unique identifier for the tool
//! - **Description**: Human-readable explanation of what the tool does
//! - **Parameters**: JSON Schema describing the expected parameters
//!
//! The parameter schema includes:
//! - **Properties**: Map of parameter names to their types and descriptions
//! - **Required**: List of mandatory parameters
//! - **Type**: Always "object" for tool parameters
//!
//! ## Usage
//!
//! These schemas are generated for each tool in the ToolRegistry and sent to the
//! LLM provider when requesting completions. The LLM uses these schemas to understand
//! which tools to call and how to format the parameters correctly.
//!
//! ## Compatibility
//!
//! The schema format is compatible with:
//! - Ollama's tool calling interface
//! - OpenAI's function calling format
//! - Anthropic's tool use specification
//!
//! This ensures agents can work with multiple LLM providers without schema translation.
//!
//! ## Schema Generation
//!
//! Each tool has a dedicated schema function that returns a ToolDefinition with
//! complete parameter specifications. The schemas include type information,
//! descriptions, and required field indicators that help LLMs understand proper
//! tool usage and parameter formatting.
//!
//! ## Validation Benefits
//!
//! By providing explicit schemas, the system enables:
//! - Parameter validation before tool execution
//! - Better LLM understanding of tool capabilities
//! - Automatic error detection for malformed calls
//! - Consistent parameter naming across all tools
//! - Self-documenting API surface for agent developers
//!
//! ## Tool Categories
//!
//! The schemas cover the complete Cymbiont knowledge graph API:
//! - Block lifecycle operations (create, update, delete)
//! - Page management (create, delete with properties)
//! - Graph administration (create, delete, open, close, list)
//! - Query operations (node retrieval, breadth-first search)
//! 
//! All schemas follow consistent patterns for UUID parameters, optional fields,
//! and error response formats, enabling predictable agent behavior.

#![allow(dead_code)] // TODO: Remove when agent integration is complete

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tool definition format compatible with Ollama and other LLM APIs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: ParameterSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: HashMap<String, PropertySchema>,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySchema {
    #[serde(rename = "type")]
    pub property_type: String,
    pub description: String,
}

/// Helper macro to create property schemas more concisely
macro_rules! prop {
    ($type:expr, $desc:expr) => {
        PropertySchema {
            property_type: $type.to_string(),
            description: $desc.to_string(),
        }
    };
}

/// Helper to create a tool definition
fn tool(name: &str, description: &str, properties: Vec<(&str, PropertySchema)>, required: Vec<&str>) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: ParameterSchema {
            schema_type: "object".to_string(),
            properties: properties.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            required: required.into_iter().map(|s| s.to_string()).collect(),
        },
    }
}

// Block Operations

pub fn add_block_schema() -> ToolDefinition {
    tool(
        "add_block",
        "Create a new block in the knowledge graph with content, optional parent, page, and properties",
        vec![
            ("content", prop!("string", "The content/text of the block to create")),
            ("graph_id", prop!("string", "UUID of the knowledge graph to add the block to")),
            ("parent_id", prop!("string", "Optional UUID of the parent block")),
            ("page_name", prop!("string", "Optional name of the page to add the block to")),
            ("properties", prop!("object", "Optional JSON object with additional properties for the block")),
        ],
        vec!["content", "graph_id"],
    )
}

pub fn update_block_schema() -> ToolDefinition {
    tool(
        "update_block",
        "Update the content of an existing block in the knowledge graph",
        vec![
            ("block_id", prop!("string", "UUID of the block to update")),
            ("content", prop!("string", "New content for the block")),
            ("graph_id", prop!("string", "UUID of the knowledge graph containing the block")),
        ],
        vec!["block_id", "content", "graph_id"],
    )
}

pub fn delete_block_schema() -> ToolDefinition {
    tool(
        "delete_block",
        "Delete (archive) a block from the knowledge graph",
        vec![
            ("block_id", prop!("string", "UUID of the block to delete")),
            ("graph_id", prop!("string", "UUID of the knowledge graph containing the block")),
        ],
        vec!["block_id", "graph_id"],
    )
}

// Page Operations

pub fn create_page_schema() -> ToolDefinition {
    tool(
        "create_page",
        "Create a new page in the knowledge graph with optional properties",
        vec![
            ("page_name", prop!("string", "Name of the page to create")),
            ("graph_id", prop!("string", "UUID of the knowledge graph to add the page to")),
            ("properties", prop!("object", "Optional JSON object with additional properties for the page")),
        ],
        vec!["page_name", "graph_id"],
    )
}

pub fn delete_page_schema() -> ToolDefinition {
    tool(
        "delete_page",
        "Delete (archive) a page from the knowledge graph",
        vec![
            ("page_name", prop!("string", "Name of the page to delete")),
            ("graph_id", prop!("string", "UUID of the knowledge graph containing the page")),
        ],
        vec!["page_name", "graph_id"],
    )
}

// Query Operations

pub fn get_node_schema() -> ToolDefinition {
    tool(
        "get_node",
        "Retrieve a node by its ID from the knowledge graph",
        vec![
            ("node_id", prop!("string", "UUID of the node to retrieve")),
            ("graph_id", prop!("string", "UUID of the knowledge graph containing the node")),
        ],
        vec!["node_id", "graph_id"],
    )
}

pub fn query_graph_bfs_schema() -> ToolDefinition {
    tool(
        "query_graph_bfs",
        "Perform breadth-first search traversal from a starting node",
        vec![
            ("start_id", prop!("string", "UUID of the starting node for traversal")),
            ("max_depth", prop!("number", "Maximum depth to traverse (default: 3)")),
            ("graph_id", prop!("string", "UUID of the knowledge graph to traverse")),
        ],
        vec!["start_id", "graph_id"],
    )
}

// Graph Management Operations

pub fn list_graphs_schema() -> ToolDefinition {
    tool(
        "list_graphs",
        "List all registered knowledge graphs",
        vec![],
        vec![],
    )
}

pub fn list_open_graphs_schema() -> ToolDefinition {
    tool(
        "list_open_graphs",
        "List all currently open (loaded) knowledge graphs",
        vec![],
        vec![],
    )
}

pub fn open_graph_schema() -> ToolDefinition {
    tool(
        "open_graph",
        "Load a knowledge graph into memory and trigger recovery if needed",
        vec![
            ("graph_id", prop!("string", "UUID of the knowledge graph to open")),
        ],
        vec!["graph_id"],
    )
}

pub fn close_graph_schema() -> ToolDefinition {
    tool(
        "close_graph",
        "Save a knowledge graph and unload it from memory",
        vec![
            ("graph_id", prop!("string", "UUID of the knowledge graph to close")),
        ],
        vec!["graph_id"],
    )
}

pub fn create_graph_schema() -> ToolDefinition {
    tool(
        "create_graph",
        "Create a new knowledge graph with optional name and description",
        vec![
            ("name", prop!("string", "Optional name for the new graph")),
            ("description", prop!("string", "Optional description of the graph's purpose")),
        ],
        vec![],
    )
}

pub fn delete_graph_schema() -> ToolDefinition {
    tool(
        "delete_graph",
        "Archive a knowledge graph (can delete both open and closed graphs)",
        vec![
            ("graph_id", prop!("string", "UUID of the knowledge graph to archive")),
        ],
        vec!["graph_id"],
    )
}

/// Get all tool definitions as a vector
pub fn all_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        add_block_schema(),
        update_block_schema(),
        delete_block_schema(),
        create_page_schema(),
        delete_page_schema(),
        get_node_schema(),
        query_graph_bfs_schema(),
        list_graphs_schema(),
        list_open_graphs_schema(),
        open_graph_schema(),
        close_graph_schema(),
        create_graph_schema(),
        delete_graph_schema(),
    ]
}