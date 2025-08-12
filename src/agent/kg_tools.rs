use std::collections::HashMap;
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;
use serde_json::{json, Value};
use uuid::Uuid;
use crate::app_state::AppState;
use crate::graph_operations::GraphOperationsExt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),
    
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

type Result<T> = std::result::Result<T, ToolError>;

/// Type alias for async tool functions
pub type ToolFunction = Box<dyn Fn(Value, Arc<AppState>) -> Pin<Box<dyn Future<Output = Result<Value>> + Send>> + Send + Sync>;

/// Registry for knowledge graph tools that can be called by LLMs
pub struct ToolRegistry {
    tools: HashMap<String, ToolFunction>,
    app_state: Arc<AppState>,
}

impl ToolRegistry {
    /// Create a new tool registry with all KG operations
    pub fn new(app_state: Arc<AppState>) -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            app_state: app_state.clone(),
        };
        
        registry.register_block_operations();
        registry.register_page_operations();
        registry.register_query_operations();
        registry.register_graph_management();
        
        registry
    }
    
    /// Execute a tool by name with the given arguments
    pub async fn execute(&self, tool_name: &str, args: Value) -> Result<Value> {
        let tool = self.tools.get(tool_name)
            .ok_or_else(|| ToolError::ToolNotFound(tool_name.to_string()))?;
        
        tool(args, self.app_state.clone()).await
    }
    
    /// Get list of available tool names
    pub fn list_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
    
    /// Register a tool function
    fn register(&mut self, name: &str, func: ToolFunction) {
        self.tools.insert(name.to_string(), func);
    }
    
    fn register_block_operations(&mut self) {
        // add_block
        self.register("add_block", Box::new(|args, state| {
            Box::pin(async move {
                let content = args.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("content".to_string()))?
                    .to_string();
                
                let parent_id = args.get("parent_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                let page_name = args.get("page_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                let properties = args.get("properties").cloned();
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.add_block(content, parent_id, page_name, properties, &graph_id).await {
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
        }));
        
        // update_block
        self.register("update_block", Box::new(|args, state| {
            Box::pin(async move {
                let block_id = args.get("block_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("block_id".to_string()))?
                    .to_string();
                
                let content = args.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("content".to_string()))?
                    .to_string();
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.update_block(block_id, content, &graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // delete_block
        self.register("delete_block", Box::new(|args, state| {
            Box::pin(async move {
                let block_id = args.get("block_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("block_id".to_string()))?
                    .to_string();
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.delete_block(block_id, &graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
    }
    
    fn register_page_operations(&mut self) {
        // create_page
        self.register("create_page", Box::new(|args, state| {
            Box::pin(async move {
                let page_name = args.get("page_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("page_name".to_string()))?
                    .to_string();
                
                let properties = args.get("properties").cloned();
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.create_page(page_name, properties, &graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // delete_page
        self.register("delete_page", Box::new(|args, state| {
            Box::pin(async move {
                let page_name = args.get("page_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("page_name".to_string()))?
                    .to_string();
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.delete_page(page_name, &graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
    }
    
    fn register_query_operations(&mut self) {
        // get_node
        self.register("get_node", Box::new(|args, state| {
            Box::pin(async move {
                let node_id = args.get("node_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("node_id".to_string()))?;
                
                let graph_id = parse_graph_id(&args)?;
                
                match state.get_node(node_id, &graph_id).await {
                    Ok(node_data) => Ok(node_data),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // query_graph_bfs
        self.register("query_graph_bfs", Box::new(|args, state| {
            Box::pin(async move {
                let start_id = args.get("start_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::MissingParameter("start_id".to_string()))?;
                
                let max_depth = args.get("max_depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as usize;
                
                let graph_id = parse_graph_id(&args)?;
                
                // This is actually a sync operation
                match state.query_graph_bfs(start_id, max_depth, &graph_id) {
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
        }));
    }
    
    fn register_graph_management(&mut self) {
        // list_graphs
        self.register("list_graphs", Box::new(|_args, state| {
            Box::pin(async move {
                match state.list_graphs().await {
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
        }));
        
        // list_open_graphs
        self.register("list_open_graphs", Box::new(|_args, state| {
            Box::pin(async move {
                match state.list_open_graphs().await {
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
        }));
        
        // open_graph
        self.register("open_graph", Box::new(|args, state| {
            Box::pin(async move {
                let graph_id = parse_graph_id(&args)?;
                
                match state.open_graph(graph_id).await {
                    Ok(graph_info) => Ok(graph_info),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // close_graph
        self.register("close_graph", Box::new(|args, state| {
            Box::pin(async move {
                let graph_id = parse_graph_id(&args)?;
                
                match state.close_graph(graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // create_graph
        self.register("create_graph", Box::new(|args, state| {
            Box::pin(async move {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                let description = args.get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                match state.create_graph(name, description).await {
                    Ok(graph_info) => Ok(graph_info),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
        
        // delete_graph
        self.register("delete_graph", Box::new(|args, state| {
            Box::pin(async move {
                let graph_id = parse_graph_id(&args)?;
                
                match state.delete_graph(&graph_id).await {
                    Ok(()) => Ok(json!({
                        "success": true
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }));
    }
}

/// Helper function to parse graph_id from args
fn parse_graph_id(args: &Value) -> Result<Uuid> {
    let graph_id_str = args.get("graph_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::MissingParameter("graph_id".to_string()))?;
    
    Uuid::parse_str(graph_id_str)
        .map_err(|e| ToolError::InvalidParameter(format!("Invalid graph_id format: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_graph_id() {
        // Valid UUID
        let args = json!({"graph_id": "550e8400-e29b-41d4-a716-446655440000"});
        assert!(parse_graph_id(&args).is_ok());
        
        // Missing graph_id
        let args = json!({});
        assert!(parse_graph_id(&args).is_err());
        
        // Invalid UUID format
        let args = json!({"graph_id": "not-a-uuid"});
        assert!(parse_graph_id(&args).is_err());
    }
}