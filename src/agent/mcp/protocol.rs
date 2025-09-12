//! MCP Protocol Types (JSON-RPC 2.0)
//!
//! This module defines the JSON-RPC 2.0 message types used by the MCP protocol.
//! All communication follows the JSON-RPC 2.0 specification with MCP-specific
//! method names and parameters.
//!
//! ## Interfacing with MCP
//!
//! To communicate with the MCP server:
//!
//! 1. **Start the server**: `cargo run -- --mcp`
//! 2. **Send JSON-RPC messages** to stdin (one per line)
//! 3. **Read JSON-RPC responses** from stdout (one per line)
//!
//! Example session:
//! ```json
//! → {"jsonrpc":"2.0","method":"initialize","params":{},"id":1}
//! ← {"jsonrpc":"2.0","result":{"protocolVersion":"2024-11-05",...},"id":1}
//! → {"jsonrpc":"2.0","method":"initialized"}
//! → {"jsonrpc":"2.0","method":"tools/list","id":2}
//! ← {"jsonrpc":"2.0","result":{"tools":[...]},"id":2}
//! → {"jsonrpc":"2.0","method":"tools/call","params":{"name":"list_graphs","arguments":{}},"id":3}
//! ← {"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"..."}]},"id":3}
//! ```
//!
//! ## Message Types
//!
//! - **Request**: Client-to-server method invocation (requires `id`)
//! - **Response**: Server-to-client method result or error (echoes `id`)
//! - **Notification**: One-way message (no `id`, no response expected)
//! - **Error**: Standardized error structure within Response
//!
//! ## MCP Methods
//!
//! - `initialize`: Handshake and capability negotiation
//! - `initialized`: Client notification after initialization (notification)
//! - `tools/list`: List available tools with JSON schemas
//! - `tools/call`: Execute a specific tool with arguments
//! - `prompts/list`: List available prompts (returns empty array)
//! - `resources/list`: List available resources (returns empty array)
//!
//! ## Error Codes
//!
//! Standard JSON-RPC 2.0 error codes:
//! - `-32700`: Parse error (malformed JSON)
//! - `-32600`: Invalid request (missing required fields)
//! - `-32601`: Method not found (unknown method name)
//! - `-32602`: Invalid params (wrong parameter types/structure)
//! - `-32603`: Internal error (tool execution failure)
//!
//! All logs are sent to stderr to keep stdout clean for JSON-RPC communication.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 Notification (no id field)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// MCP-specific constants
pub mod methods {
    /// Initialization handshake
    pub const INITIALIZE: &str = "initialize";
    /// Client notification after initialization
    pub const INITIALIZED: &str = "initialized";
    /// List available tools
    pub const TOOLS_LIST: &str = "tools/list";
    /// Execute a tool
    pub const TOOLS_CALL: &str = "tools/call";
    /// List available prompts
    pub const PROMPTS_LIST: &str = "prompts/list";
    /// List available resources
    pub const RESOURCES_LIST: &str = "resources/list";
}

/// Standard JSON-RPC 2.0 error codes
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    // TODO: Will be used when implementing --agent mode (Phase 6: Process Orchestration)
    #[allow(dead_code)]
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

impl Request {
    /// Create a new request
    // TODO: Will be used when implementing --agent mode (Phase 6: Process Orchestration)
    #[allow(dead_code)]
    pub fn new(method: impl Into<String>, params: Option<Value>, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
            id,
        }
    }
}

impl Response {
    /// Create a successful response
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(error: Error, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

impl Error {
    /// Create a new error
    pub fn new(code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    /// Create a parse error
    pub fn parse_error() -> Self {
        Self::new(error_codes::PARSE_ERROR, "Parse error", None)
    }

    /// Create an invalid request error
    // TODO: Will be used when implementing --agent mode (Phase 6: Process Orchestration)
    #[allow(dead_code)]
    pub fn invalid_request() -> Self {
        Self::new(error_codes::INVALID_REQUEST, "Invalid request", None)
    }

    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", method),
            None,
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_PARAMS, message, None)
    }

    /// Create an internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(error_codes::INTERNAL_ERROR, message, None)
    }
}

impl Notification {
    /// Create a new notification
    // TODO: Will be used when implementing --agent mode (Phase 6: Process Orchestration)
    #[allow(dead_code)]
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}