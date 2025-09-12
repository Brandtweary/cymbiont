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
//! These schemas are generated for each tool in the knowledge graph tools system and
//! sent to the LLM provider when requesting completions. The LLM uses these schemas to
//! understand which tools to call and how to format the parameters correctly.
//!
//! ## Compatibility
//!
//! The schema format is compatible with:
//! - Ollama's tool calling interface
//! - `OpenAI`'s function calling format
//! - Anthropic's tool use specification
//!
//! This ensures agents can work with multiple LLM providers without schema translation.
//!
//! ## Schema Generation
//!
//! Each tool has a dedicated schema function that returns a `ToolDefinition` with
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
fn tool(
    name: &str,
    description: &str,
    properties: Vec<(&str, PropertySchema)>,
    required: Vec<&str>,
) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: ParameterSchema {
            schema_type: "object".to_string(),
            properties: properties
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            required: required
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
        },
    }
}

// Block Operations

pub fn add_block_schema() -> ToolDefinition {
    tool(
        "add_block",
        "Create knowledge block. Returns block ID. Defaults to open graph if only one.",
        vec![
            ("content", prop!("string", "Block text content")),
            ("graph_id", prop!("string", "Optional UUID of the graph (uses agent's default if not specified)")),
            ("graph_name", prop!("string", "Optional name of the graph (alternative to graph_id)")),
            ("parent_id", prop!("string", "Parent block UUID for nesting")),
            ("page_name", prop!("string", "Page to add block to")),
            ("properties", prop!("object", "Custom metadata as JSON")),
        ],
        vec!["content"],
    )
}

pub fn update_block_schema() -> ToolDefinition {
    tool(
        "update_block",
        "Update block content. Returns success status.",
        vec![
            ("block_id", prop!("string", "Block UUID")),
            ("content", prop!("string", "New content")),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec!["block_id", "content"],
    )
}

pub fn delete_block_schema() -> ToolDefinition {
    tool(
        "delete_block",
        "Archive block. Preserves in archive/ for recovery.",
        vec![
            ("block_id", prop!("string", "Block UUID to archive")),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec!["block_id"],
    )
}

// Page Operations

pub fn create_page_schema() -> ToolDefinition {
    tool(
        "create_page",
        "Create page. Returns page ID. Pages organize blocks hierarchically.",
        vec![
            ("page_name", prop!("string", "Page name (case-insensitive)")),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
            (
                "properties",
                prop!(
                    "object",
                    "Custom metadata as JSON"
                ),
            ),
        ],
        vec!["page_name"],
    )
}

pub fn delete_page_schema() -> ToolDefinition {
    tool(
        "delete_page",
        "Archive page and its blocks. Preserves in archive/.",
        vec![
            ("page_name", prop!("string", "Page name to archive")),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec!["page_name"],
    )
}

// Query Operations

pub fn get_node_schema() -> ToolDefinition {
    tool(
        "get_node",
        "Fetch node by ID. Returns content, properties, and relationships.",
        vec![
            ("node_id", prop!("string", "Node UUID")),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec!["node_id"],
    )
}

pub fn query_graph_bfs_schema() -> ToolDefinition {
    tool(
        "query_graph_bfs",
        "BFS traversal from node. Returns connected nodes to max_depth (default: 3).",
        vec![
            (
                "start_id",
                prop!("string", "Starting node UUID"),
            ),
            (
                "max_depth",
                prop!("number", "Max traversal depth (default: 3)"),
            ),
            (
                "graph_id",
                prop!(
                    "string",
                    "Graph UUID (defaults to open graph)"
                ),
            ),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec!["start_id"],
    )
}

// Graph Management Operations

pub fn list_graphs_schema() -> ToolDefinition {
    tool(
        "list_graphs",
        "List all graphs. Returns [{id, name, created, last_accessed}].",
        vec![],
        vec![],
    )
}

pub fn list_open_graphs_schema() -> ToolDefinition {
    tool(
        "list_open_graphs",
        "List loaded graphs. Returns array of graph IDs in memory.",
        vec![],
        vec![],
    )
}

pub fn open_graph_schema() -> ToolDefinition {
    tool(
        "open_graph",
        "Load graph into memory. Triggers recovery if needed. Returns graph ID.",
        vec![
            ("graph_id", prop!("string", "Graph UUID")),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec![],
    )
}

pub fn close_graph_schema() -> ToolDefinition {
    tool(
        "close_graph",
        "Save and unload graph. Writes to disk, frees memory.",
        vec![
            ("graph_id", prop!("string", "Graph UUID")),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec![],
    )
}

pub fn create_graph_schema() -> ToolDefinition {
    tool(
        "create_graph",
        "Create empty graph. Returns new graph ID. Auto-opens after creation.",
        vec![
            ("name", prop!("string", "Graph name")),
            (
                "description",
                prop!("string", "Graph purpose/description"),
            ),
        ],
        vec![],
    )
}

pub fn delete_graph_schema() -> ToolDefinition {
    tool(
        "delete_graph",
        "Archive entire graph. Moves to archived_graphs/ with timestamp.",
        vec![
            ("graph_id", prop!("string", "Graph UUID")),
            (
                "graph_name",
                prop!(
                    "string",
                    "Graph name (alternative to ID)"
                ),
            ),
        ],
        vec![],
    )
}

// Import Operations

pub fn import_logseq_schema() -> ToolDefinition {
    tool(
        "import_logseq",
        "Import Logseq graph. Parses .md files, preserves hierarchy. Returns graph ID.",
        vec![
            ("path", prop!("string", "Logseq directory path")),
            (
                "graph_name",
                prop!(
                    "string",
                    "Custom name (defaults to dir name)"
                ),
            ),
        ],
        vec!["path"],
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
        import_logseq_schema(),
    ]
}
