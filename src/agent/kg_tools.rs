//! Knowledge Graph Tools for LLM Agents
//!
//! This module provides a simple, efficient tool system for agents to interact
//! with the knowledge graph. Tools are registered in a static HashMap for fast
//! dispatch without ownership complexity.
//!
//! ## Design Philosophy
//!
//! - **Static Registry**: Tools are function pointers in a static HashMap
//! - **No Ownership**: Tools receive AppState as a parameter, don't own it
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
//! // Execute a tool
//! let result = kg_tools::execute_tool(
//!     app_state,
//!     agent_id,
//!     "add_block",
//!     json!({"content": "Hello", "graph_id": "..."})
//! ).await?;
//! ```

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::{json, Value};
use uuid::Uuid;
use crate::app_state::AppState;
use crate::graph::graph_operations::GraphOps;
use crate::lock::AsyncRwLockExt;
use crate::error::*;
use crate::agent::schemas::ToolDefinition;
use crate::agent::agent::{Agent, ToolResult};
use crate::agent::llm::ToolCall;



/// Type alias for async tool functions
/// Takes Arc<AppState> reference, mutable agent reference, and args - returns a future
type ToolFn = for<'a> fn(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
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
    
    // Agent graph management (for agents to manage their own context)
    tools.insert("set_default_graph", set_default_graph as ToolFn);
    tools.insert("get_default_graph", get_default_graph as ToolFn);
    tools.insert("list_my_graphs", list_my_graphs as ToolFn);
    
    tools
});

/// Execute a tool by name
pub async fn execute_tool(
    app_state: &Arc<AppState>,
    agent: &mut Agent,
    tool_name: &str,
    args: Value,
) -> Result<Value> {
    let tool = TOOLS.get(tool_name)
        .ok_or_else(|| AgentError::tool(format!("Tool not found: {}", tool_name)))?;
    
    tool(app_state, agent, args).await
}

/// Phase 3: Execute tools without holding agent locks
/// 
/// Static function that executes tools by getting agent data as needed
/// without maintaining locks during tool execution
pub async fn execute_tools_stateless(
    app_state: &Arc<AppState>,
    agent_id: Uuid,
    tool_calls: Vec<ToolCall>,
) -> Result<Vec<ToolResult>> {
    use crate::lock::AsyncRwLockExt;
    
    tracing::debug!("Phase 3: Executing {} tools for agent {}", tool_calls.len(), agent_id);
    
    let mut results = Vec::new();
    
    for tool_call in tool_calls {
        tracing::debug!("Phase 3: Executing tool '{}' for agent {}", tool_call.name, agent_id);
        
        // Get agent briefly to execute tool
        let result = {
            let agents = app_state.agents.read_or_panic("execute tools stateless").await;
            match agents.get(&agent_id) {
                Some(agent_arc) => {
                    // Lock agent only during tool execution, not for registry operations
                    let mut agent = agent_arc.write_or_panic("execute tool").await;
                    agent.execute_tool(app_state, &tool_call.name, tool_call.arguments.clone()).await?
                }
                None => {
                    return Err(AgentError::tool(format!("Agent {} not found", agent_id)).into());
                }
            }
        };
        
        results.push(ToolResult {
            name: tool_call.name.clone(),
            arguments: tool_call.arguments,
            result,
        });
        
        tracing::debug!("Phase 3: Tool '{}' completed for agent {}", tool_call.name, agent_id);
    }
    
    Ok(results)
}

/// Get tool schemas for the LLM
pub fn get_tool_schemas() -> Vec<ToolDefinition> {
    crate::agent::schemas::all_tool_definitions()
}

/// Helper function to parse graph target (ID or name) from args
/// 
/// Supports both graph_id (UUID) and graph_name (string) parameters.
/// Falls back to the agent's default graph or smart default if no graph is specified.
/// Uses the GraphRegistry to resolve names to IDs.
async fn parse_graph_target(app_state: &Arc<AppState>, agent: &Agent, args: &Value, use_smart_default: bool) -> Result<Uuid> {
    // Try to get graph_id first
    let graph_id = args.get("graph_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    
    // Try to get graph_name
    let graph_name = args.get("graph_name")
        .and_then(|v| v.as_str());
    
    // If neither provided, choose fallback behavior
    if graph_id.is_none() && graph_name.is_none() {
        if use_smart_default {
            // Use smart default (for set_default_graph)
            let registry = app_state.graph_registry.read_or_panic("parse graph target - smart default").await;
            return registry.resolve_graph_target(None, None, true)
                .map_err(|e| AgentError::tool(format!("Failed to resolve graph target: {}", e)).into());
        } else {
            // Use agent's default graph (for regular tools)
            if let Some(default_id) = agent.default_graph_id {
                return Ok(default_id);
            }
            return Err(AgentError::tool("No graph specified and no default graph set for agent").into());
        }
    }
    
    // Use the registry to resolve the target
    let registry = app_state.graph_registry.read_or_panic("parse graph target").await;
    registry.resolve_graph_target(
        graph_id.as_ref(),
        graph_name,
        false  // Explicit graph provided, no fallback needed
    ).map_err(|e| AgentError::tool(format!("Failed to resolve graph target: {}", e)).into())
}

// Block Operations

fn add_block<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let content = args.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: content"))?
            .to_string();
        
        let parent_id = args.get("parent_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let page_name = args.get("page_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let properties = args.get("properties").cloned();
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.add_block(agent.id, content, parent_id, page_name, properties, &graph_id, false).await {
            Ok(block_id) => Ok(json!({
                "success": true,
                "block_id": block_id
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn update_block<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let block_id = args.get("block_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: block_id"))?
            .to_string();
        
        let content = args.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: content"))?
            .to_string();
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.update_block(agent.id, block_id, content, &graph_id, false).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn delete_block<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let block_id = args.get("block_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: block_id"))?
            .to_string();
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.delete_block(agent.id, block_id, &graph_id, false).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

// Page Operations

fn create_page<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let page_name = args.get("page_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: page_name"))?
            .to_string();
        
        let properties = args.get("properties").cloned();
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.create_page(agent.id, page_name, properties, &graph_id, false).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn delete_page<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let page_name = args.get("page_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: page_name"))?
            .to_string();
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.delete_page(agent.id, page_name, &graph_id, false).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

// Query Operations

fn get_node<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let node_id = args.get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: node_id"))?;
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.get_node(agent.id, node_id, &graph_id).await {
            Ok(node_data) => Ok(node_data),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn query_graph_bfs<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let start_id = args.get("start_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::tool("Missing required parameter: start_id"))?;
        
        let max_depth = args.get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;
        
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        // This is now an async operation
        match app_state.query_graph_bfs(agent.id, start_id, max_depth, &graph_id).await {
            Ok(nodes) => Ok(json!({
                "success": true,
                "nodes": nodes
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

// Graph Management Operations

fn list_graphs<'a>(
    app_state: &'a Arc<AppState>,
    _agent: &'a mut Agent,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        match app_state.list_graphs().await {
            Ok(graphs) => Ok(json!({
                "success": true,
                "graphs": graphs
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn list_open_graphs<'a>(
    app_state: &'a Arc<AppState>,
    _agent: &'a mut Agent,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        match app_state.list_open_graphs().await {
            Ok(graph_ids) => Ok(json!({
                "success": true,
                "graph_ids": graph_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn open_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.open_graph(graph_id).await {
            Ok(graph_info) => Ok(json!({
                "success": true,
                "graph_id": graph_info["id"],
                "name": graph_info["name"]
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn close_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.close_graph(graph_id).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn create_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let name = args.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let description = args.get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        match app_state.create_graph(name, description).await {
            Ok(graph_info) => {
                // If this agent has no default graph, set it to the newly created one
                // (This commonly happens for the prime agent creating its first graph)
                let graph_id = graph_info["id"].as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .unwrap();
                
                if agent.get_default_graph_id().is_none() {
                    tracing::debug!("create_graph: Setting default graph for agent {}", agent.id);
                    agent.set_default_graph_id(Some(graph_id), false).await?;
                    tracing::debug!("create_graph: Default graph set successfully");
                    tracing::info!("Set default graph for agent {} to new graph {}", agent.id, graph_id);
                }
                
                Ok(json!({
                    "success": true,
                    "graph_id": graph_info["id"],
                    "name": graph_info["name"]
                }))
            },
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

fn delete_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        let graph_id = parse_graph_target(app_state, agent, &args, false).await?;
        
        match app_state.delete_graph(&graph_id).await {
            Ok(()) => Ok(json!({
                "success": true
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": e.to_string()
            }))
        }
    })
}

// Agent Graph Management Operations

fn set_default_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        
        let graph_id = parse_graph_target(app_state, agent, &args, true).await?;
        
        // Check if agent is authorized for this graph
        let is_authorized = {
            let agent_registry = app_state.agent_registry.read_or_panic("set default graph").await;
            agent_registry.is_agent_authorized(&agent.id, &graph_id)
        };
        
        if !is_authorized {
            return Ok(json!({
                "success": false,
                "error": format!("Agent is not authorized for graph {}", graph_id)
            }));
        }
        
        // Update the agent's default graph directly (we already have mutable access)
        agent.set_default_graph_id(Some(graph_id), false).await?;
        
        Ok(json!({
            "success": true,
            "graph_id": graph_id.to_string()
        }))
    })
}

fn get_default_graph<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        // Get default graph directly from agent reference
        if let Some(default_id) = agent.get_default_graph_id() {
            // Get graph name if available
            let graph_registry = app_state.graph_registry.read_or_panic("get default graph").await;
            let graph_name = graph_registry.get_graph(&default_id)
                .map(|info| info.name.clone());
            drop(graph_registry);
            
            Ok(json!({
                "success": true,
                "graph_id": default_id.to_string(),
                "graph_name": graph_name
            }))
        } else {
            Ok(json!({
                "success": true,
                "graph_id": null,
                "graph_name": null
            }))
        }
    })
}

fn list_my_graphs<'a>(
    app_state: &'a Arc<AppState>,
    agent: &'a mut Agent,
    _args: Value,
) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        // Get authorized graphs and build list (drop sync locks before await)
        let graphs = {
            let agent_registry = app_state.agent_registry.read_or_panic("list my graphs").await;
            let graph_registry = app_state.graph_registry.read_or_panic("list my graphs").await;
            
            // Get authorized graph IDs for this agent
            let authorized_graphs = agent_registry
                .get_agent(&agent.id)
                .map(|info| info.authorized_graphs.clone())
                .unwrap_or_default();
            
            // Build list with graph info
            let mut graphs = Vec::new();
            for graph_id in authorized_graphs {
                if let Some(graph_info) = graph_registry.get_graph(&graph_id) {
                    graphs.push(json!({
                        "id": graph_id.to_string(),
                        "name": graph_info.name,
                        "is_open": graph_registry.is_graph_open(&graph_id)
                    }));
                }
            }
            graphs
        };
        
        // Get current default directly from agent reference
        let default_graph_id = agent.get_default_graph_id();
        
        Ok(json!({
            "success": true,
            "graphs": graphs,
            "default_graph_id": default_graph_id.map(|id| id.to_string())
        }))
    })
}

#[cfg(test)]
mod tests {
    // Note: parse_graph_target tests would require a full AppState setup
    // Integration tests cover this functionality
}