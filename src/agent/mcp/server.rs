//! MCP Server Implementation
//!
//! This module implements the Model Context Protocol (MCP) server that exposes Cymbiont's 
//! knowledge graph operations via JSON-RPC 2.0 over stdio. The server acts as a bridge 
//! between AI agents (like Claude Code) and Cymbiont's tool system, enabling natural 
//! language interaction with knowledge graphs.
//!
//! ## Architecture
//!
//! The server runs as an async task that:
//! 1. Reads JSON-RPC requests from stdin line by line
//! 2. Routes requests to appropriate handlers based on method name
//! 3. Executes tools via the canonical `tools::execute_tool()` function
//! 4. Returns JSON-RPC responses to stdout
//!
//! The server maintains minimal state - just tracking initialization status. All actual
//! state management happens through the AppState and CQRS system.
//!
//! ## Protocol Flow
//!
//! 1. **Initialization**: Client sends `initialize` request, server returns capabilities
//! 2. **Tool Discovery**: Client calls `tools/list` to get available tools with schemas
//! 3. **Tool Execution**: Client calls `tools/call` with tool name and arguments
//! 4. **Notifications**: Client may send notifications (no response expected)
//!
//! ## Critical: Stdio Stream Separation
//!
//! **stdout is reserved exclusively for JSON-RPC messages.** Any non-protocol
//! output (logs, debug info, errors) MUST go to stderr. This is enforced by
//! configuring the tracing subscriber to use stderr via config.yaml. Violating
//! this rule will cause MCP clients to fail with parse errors.
//!
//! Configuration in config.yaml:
//! ```yaml
//! tracing:
//!   output: "stderr"  # Required for MCP mode
//! ```
//!
//! ## Tool Naming Convention
//!
//! MCP tools are prefixed with `cymbiont_` to avoid naming conflicts with client tools:
//! - MCP exposure: `cymbiont_add_block`  
//! - Internal name: `add_block`
//! - The prefix is automatically added when exposing tools and stripped when executing
//!
//! ## Error Handling
//!
//! All errors are returned as JSON-RPC error responses with appropriate error codes:
//! - Parse errors: -32700
//! - Invalid request: -32600
//! - Method not found: -32601
//! - Invalid params: -32602
//! - Internal error: -32603
//!
//! Internal errors are logged to stderr via tracing macros while protocol errors are 
//! returned as JSON-RPC error responses. Tool execution failures are wrapped in
//! internal error responses with descriptive messages.
//!
//! ## Supported Methods
//!
//! - `initialize`: MCP handshake, returns server capabilities
//! - `initialized`: Notification that client is ready (no response)
//! - `tools/list`: Returns all available tools with JSON schemas
//! - `tools/call`: Execute a specific tool with arguments
//!
//! ## Integration Points
//!
//! Currently, the MCP server can be started via `--mcp` flag and communicates over stdio.
//! Future integration (Phase 6) will involve:
//! - Parent process spawning MCP server with pipes instead of stdio
//! - Python subprocess (Claude Code SDK) connecting to those pipes
//! - Process supervision and restart logic
//!
//! For manual testing or custom integrations:
//! ```bash
//! # Start server
//! cargo run -- --mcp
//! 
//! # In another process, pipe JSON-RPC messages
//! echo '{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}' | cargo run -- --mcp
//! ```

use crate::agent::tools;
use crate::app_state::AppState;
use crate::error::{MCPError, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

use super::protocol::{error_codes, methods, Error, Request, Response};

/// MCP Server
pub struct MCPServer {
    app_state: Arc<AppState>,
    initialized: bool,
}

impl MCPServer {
    /// Create a new MCP server
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            initialized: false,
        }
    }

    /// Run the MCP server on stdio
    pub async fn run_stdio(&mut self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        
        // The server runs until the process shuts down (via duration or Ctrl+C)
        // We never exit just because stdin is empty - we keep waiting for input
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    // Parse JSON-RPC request
                    match serde_json::from_str::<Request>(&line) {
                        Ok(request) => {
                            // Check if this is a notification (no id field)
                            let is_notification = request.id.is_none();
                            
                            let response = self.handle_request(request).await;
                            
                            // Only send response if this wasn't a notification
                            if !is_notification {
                                let response_str = serde_json::to_string(&response)
                                    .map_err(|e| MCPError::Serialization(e))?;
                                stdout.write_all(response_str.as_bytes()).await
                                    .map_err(|e| MCPError::IO(e))?;
                                stdout.write_all(b"\n").await
                                    .map_err(|e| MCPError::IO(e))?;
                                stdout.flush().await
                                    .map_err(|e| MCPError::IO(e))?;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse JSON-RPC request: {}", e);
                            let response = Response::error(Error::parse_error(), None);
                            let response_str = serde_json::to_string(&response)
                                .map_err(|e| MCPError::Serialization(e))?;
                            stdout.write_all(response_str.as_bytes()).await
                                .map_err(|e| MCPError::IO(e))?;
                            stdout.write_all(b"\n").await
                                .map_err(|e| MCPError::IO(e))?;
                            stdout.flush().await
                                .map_err(|e| MCPError::IO(e))?;
                        }
                    }
                }
                Ok(None) => {
                    // stdin returned None - this can happen with TTY or when no input is available
                    // We don't treat this as disconnection - just keep waiting
                    // Small sleep to avoid busy-waiting
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(_e) => {
                    // Actual IO error - this is worth logging but we still continue
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Handle a JSON-RPC request
    async fn handle_request(&mut self, request: Request) -> Response {
        match request.method.as_str() {
            methods::INITIALIZE => self.handle_initialize(request),
            methods::INITIALIZED => {
                self.handle_initialized(request);
                // No response for notifications
                return Response::success(json!(null), None);
            }
            methods::TOOLS_LIST => self.handle_tools_list(request).await,
            methods::TOOLS_CALL => self.handle_tools_call(request).await,
            methods::PROMPTS_LIST => self.handle_prompts_list(request),
            methods::RESOURCES_LIST => self.handle_resources_list(request),
            _ => Response::error(Error::method_not_found(&request.method), request.id),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, request: Request) -> Response {
        self.initialized = true;
        
        // Return server capabilities
        let result = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {},
                "logging": {}
            },
            "serverInfo": {
                "name": "cymbiont",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        
        Response::success(result, request.id)
    }

    /// Handle initialized notification
    fn handle_initialized(&mut self, _request: Request) {
        info!("MCP client initialized");
        // This is a notification, no response needed
    }

    /// Handle tools/list request
    async fn handle_tools_list(&self, request: Request) -> Response {
        if !self.initialized {
            return Response::error(
                Error::new(
                    error_codes::INTERNAL_ERROR,
                    "Server not initialized",
                    None,
                ),
                request.id,
            );
        }

        // Get tool schemas from the canonical tools module
        let tool_schemas = tools::get_tool_schemas();
        
        // Convert to MCP format with cymbiont_ prefix
        let tools: Vec<Value> = tool_schemas
            .into_iter()
            .map(|schema| {
                json!({
                    "name": format!("cymbiont_{}", schema.name),
                    "description": schema.description,
                    "inputSchema": {
                        "type": "object",
                        "properties": schema.parameters.properties,
                        "required": schema.parameters.required
                    }
                })
            })
            .collect();

        let result = json!({
            "tools": tools
        });

        Response::success(result, request.id)
    }

    /// Handle tools/call request
    async fn handle_tools_call(&self, request: Request) -> Response {
        if !self.initialized {
            return Response::error(
                Error::new(
                    error_codes::INTERNAL_ERROR,
                    "Server not initialized",
                    None,
                ),
                request.id,
            );
        }

        // Extract tool name and arguments
        let params = match request.params {
            Some(Value::Object(params)) => params,
            _ => {
                return Response::error(
                    Error::invalid_params("Expected object with 'name' and 'arguments'"),
                    request.id,
                );
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(name) => name,
            None => {
                return Response::error(
                    Error::invalid_params("Missing 'name' field"),
                    request.id,
                );
            }
        };

        // Remove cymbiont_ prefix if present
        let internal_name = tool_name.strip_prefix("cymbiont_").unwrap_or(tool_name);

        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        // Execute tool via the canonical tools module
        match tools::execute_tool(&self.app_state, internal_name, args).await {
            Ok(result) => {
                let response = json!({
                    "content": [{
                        "type": "text",
                        "text": result.to_string()
                    }]
                });
                Response::success(response, request.id)
            }
            Err(e) => {
                error!("Tool execution failed for '{}': {}", tool_name, e);
                Response::error(
                    Error::internal_error(format!("Tool execution failed: {}", e)),
                    request.id,
                )
            }
        }
    }

    /// Handle prompts/list request
    fn handle_prompts_list(&self, request: Request) -> Response {
        if !self.initialized {
            return Response::error(
                Error::new(
                    error_codes::INTERNAL_ERROR,
                    "Server not initialized",
                    None,
                ),
                request.id,
            );
        }
        
        // We don't have any prompts, return empty array
        Response::success(json!({ "prompts": [] }), request.id)
    }

    /// Handle resources/list request
    fn handle_resources_list(&self, request: Request) -> Response {
        if !self.initialized {
            return Response::error(
                Error::new(
                    error_codes::INTERNAL_ERROR,
                    "Server not initialized",
                    None,
                ),
                request.id,
            );
        }
        
        // We don't have any resources, return empty array
        Response::success(json!({ "resources": [] }), request.id)
    }
}

/// Run the MCP server on stdio (convenience function)
pub async fn run_mcp_server(app_state: Arc<AppState>) -> Result<()> {
    let mut server = MCPServer::new(app_state);
    server.run_stdio().await
}