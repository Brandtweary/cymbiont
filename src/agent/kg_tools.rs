//! Knowledge Graph Tools for LLM Agents
//!
//! This module provides a simple, efficient tool system for agents to interact
//! with the knowledge graph through the CQRS architecture. Tools are registered
//! in a static HashMap for fast dispatch without ownership complexity.
//!
//! ## Design Philosophy
//!
//! - **Static Registry**: Tools are function pointers in a static HashMap
//! - **CQRS Integration**: All mutations route through CommandQueue
//! - **No Direct State Access**: Tools use GraphOps trait which submits commands
//! - **Simple Functions**: Each tool is a focused 10-20 line function
//! - **Pure Data Schemas**: Tool definitions are just data for the LLM
//!
//! ## Tool Categories
//!
//! ### Block Operations
//! - `add_block`: Create new blocks with content and relationships
//! - `update_block`: Modify existing block content
//! - `delete_block`: Archive blocks from the graph
//!
//! ### Page Operations
//! - `create_page`: Create new pages with optional properties
//! - `delete_page`: Archive pages and their blocks
//!
//! ### Query Operations
//! - `get_node`: Retrieve node information by ID
//! - `query_graph_bfs`: Breadth-first search traversal
//!
//! ### Graph Management
//! - `list_graphs`: Enumerate all registered graphs
//! - `list_open_graphs`: List currently loaded graphs
//! - `open_graph`: Load a graph into memory
//! - `close_graph`: Save and unload a graph
//! - `create_graph`: Create a new knowledge graph
//! - `delete_graph`: Archive a graph
//!
//! ## Usage
//!
//! ```rust
//! // Get tool schemas for LLM
//! let tools = kg_tools::get_tool_schemas();
//!
//! // Execute a tool (mutations go through CQRS)
//! let result = kg_tools::execute_tool(
//!     app_state,
//!     "add_block",
//!     json!({"content": "Hello", "graph_id": "..."})
//! ).await?;
//! ```
//!
//! ## CQRS Architecture
//!
//! Tools don't directly modify state. Instead:
//! 1. Tool functions call GraphOps methods on AppState
//! 2. GraphOps methods submit commands to CommandQueue
//! 3. CommandProcessor executes commands sequentially
//! 4. State changes are logged to command WAL for recovery
//!
//! This ensures all mutations are audited, recoverable, and deadlock-free.

use crate::agent::schemas::ToolDefinition;
use crate::app_state::AppState;
use crate::error::*;
use crate::graph::graph_operations::GraphOps;
use crate::utils::AsyncRwLockExt;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

/// Type alias for async tool functions
/// Takes Arc<AppState> reference and args - returns a future
type ToolFn = for<'a> fn(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// Static registry of tools - no ownership needed
static TOOLS: Lazy<HashMap<&'static str, ToolFn>> = Lazy::new(|| {
    let mut tools = HashMap::new();

    // Block operations
    tools.insert("add_block", add_block as ToolFn);
    tools.insert("update_block", update_block as ToolFn);
    tools.insert("delete_block", delete_block as ToolFn);

    // Page operations
    tools.insert("create_page", create_page as ToolFn);
    tools.insert("delete_page", delete_page as ToolFn);

    // Query operations
    tools.insert("get_node", get_node as ToolFn);
    tools.insert("query_graph_bfs", query_graph_bfs as ToolFn);

    // Graph management
    tools.insert("list_graphs", list_graphs as ToolFn);
    tools.insert("list_open_graphs", list_open_graphs as ToolFn);
    tools.insert("open_graph", open_graph as ToolFn);
    tools.insert("close_graph", close_graph as ToolFn);
    tools.insert("create_graph", create_graph as ToolFn);
    tools.insert("delete_graph", delete_graph as ToolFn);

    tools
});

/// Execute a tool by name
pub async fn execute_tool(
    app_state: &Arc<AppState>,
    tool_name: &str,
    args: Value,
) -> Result<Value> {
    let tool = TOOLS
        .get(tool_name)
        .ok_or_else(|| AgentError::tool(format!("Tool not found: {}", tool_name)))?;

    tool(app_state, args).await
}

/// Get tool schemas for the LLM
pub fn get_tool_schemas() -> Vec<ToolDefinition> {
    crate::agent::schemas::all_tool_definitions()
}

/// Helper function to parse graph target (ID or name) from args
///
/// Supports both graph_id (UUID) and graph_name (string) parameters.
/// Falls back to smart default if no graph is specified.
/// Uses the GraphRegistry to resolve names to IDs.
async fn parse_graph_target(
    app_state: &Arc<AppState>,
    args: &Value,
    use_smart_default: bool,
) -> Result<Uuid> {
    // Try to get graph_id first
    let graph_id = args
        .get("graph_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    // Try to get graph_name
    let graph_name = args.get("graph_name").and_then(|v| v.as_str());

    // If neither provided, choose fallback behavior
    if graph_id.is_none() && graph_name.is_none() {
        if use_smart_default {
            let registry = app_state
                .graph_registry
                .read_or_panic("parse graph target - smart default")
                .await;
            return registry
                .resolve_graph_target(None, None, true)
                .map_err(|e| {
                    AgentError::tool(format!("Failed to resolve graph target: {}", e)).into()
                });
        } else {
            // No default graph available
            return Err(
                AgentError::tool("No graph specified and no default graph available").into(),
            );
        }
    }

    // Use the registry to resolve the target
    let registry = app_state
        .graph_registry
        .read_or_panic("parse graph target")
        .await;
    registry
        .resolve_graph_target(
            graph_id.as_ref(),
            graph_name,
            false, // Explicit graph provided, no fallback needed
        )
        .map_err(|e| AgentError::tool(format!("Failed to resolve graph target: {}", e)).into())
}

// Block Operations

fn add_block<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: content"))?
            .to_string();

        let parent_id = args
            .get("parent_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let page_name = args
            .get("page_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let properties = args.get("properties").cloned();

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state
            .add_block(
                None, content, parent_id, page_name, properties, None, &graph_id,
            )
            .await
        {
            Ok(block_id) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'add_block' executed successfully",
                "block_id": block_id
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn update_block<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let block_id = args
            .get("block_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: block_id"))?
            .to_string();

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: content"))?
            .to_string();

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.update_block(block_id, content, &graph_id).await {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'update_block' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn delete_block<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let block_id = args
            .get("block_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: block_id"))?
            .to_string();

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.delete_block(block_id, &graph_id).await {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'delete_block' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

// Page Operations

fn create_page<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let page_name = args
            .get("page_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: page_name"))?
            .to_string();

        let properties = args.get("properties").cloned();

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state
            .create_page(page_name, properties, &graph_id)
            .await
        {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'create_page' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn delete_page<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let page_name = args
            .get("page_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: page_name"))?
            .to_string();

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.delete_page(page_name, &graph_id).await {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'delete_page' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

// Query Operations

fn get_node<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let node_id = args
            .get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: node_id"))?;

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.get_node(node_id, &graph_id).await {
            Ok(node_data) => {
                let mut result = node_data;
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("success".to_string(), json!(true));
                    obj.insert(
                        "message".to_string(),
                        json!("✓ Tool 'get_node' executed successfully"),
                    );
                }
                Ok(result)
            }
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn query_graph_bfs<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let start_id = args
            .get("start_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: start_id"))?;

        let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

        let graph_id = parse_graph_target(app_state, &args, true).await?;

        // This is now an async operation
        match app_state
            .query_graph_bfs(start_id, max_depth, &graph_id)
            .await
        {
            Ok(nodes) => Ok(json!({
                "success": true,
                "nodes": nodes
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

// Graph Management Operations

fn list_graphs<'a>(
    app_state: &'a Arc<AppState>,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        match app_state.list_graphs().await {
            Ok(graphs) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'list_graphs' executed successfully",
                "graphs": graphs
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn list_open_graphs<'a>(
    app_state: &'a Arc<AppState>,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        match app_state.list_open_graphs().await {
            Ok(graph_ids) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'list_open_graphs' executed successfully",
                "graph_ids": graph_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn open_graph<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.open_graph(graph_id).await {
            Ok(graph_info) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'open_graph' executed successfully",
                "graph_id": graph_info["id"],
                "name": graph_info["name"]
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn close_graph<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.close_graph(graph_id).await {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'close_graph' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn create_graph<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match app_state.create_graph(name, description).await {
            Ok(graph_info) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'create_graph' executed successfully",
                "graph_id": graph_info["id"],
                "name": graph_info["name"]
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

fn delete_graph<'a>(
    app_state: &'a Arc<AppState>,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, &args, true).await?;

        match app_state.delete_graph(&graph_id).await {
            Ok(()) => Ok(json!({
                "success": true,
                "message": "✓ Tool 'delete_graph' executed successfully"
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    })
}

#[cfg(test)]
mod tests {
    // Note: parse_graph_target tests would require a full AppState setup
    // Integration tests cover this functionality
}
