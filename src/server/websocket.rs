//! @module websocket
//! @description Core WebSocket protocol types and connection handling
//! 
//! This module defines the WebSocket protocol infrastructure for Cymbiont's real-time
//! communication layer. It handles connection lifecycle, message routing, and protocol
//! definitions while delegating command implementation to domain-specific handlers.
//! 
//! ## Architecture
//! 
//! The WebSocket server follows a layered architecture:
//! - **Protocol Layer** (this module): Connection handling, message parsing, command routing
//! - **Utility Layer** (websocket_utils): Shared helpers for auth, responses, resolution
//! - **Handler Layer** (websocket_commands/): Domain-specific command implementations
//! 
//! ## Connection Lifecycle
//! 
//! 1. **Upgrade**: HTTP connection upgraded to WebSocket at `/ws` endpoint
//! 2. **Authentication**: Client sends `Auth { token }` command to authenticate
//! 3. **Agent Selection**: Prime agent automatically set as current on auth success
//! 4. **Command Processing**: Commands routed to appropriate handlers
//! 5. **Cleanup**: Graceful disconnection with task cancellation
//! 
//! ## Protocol Design
//! 
//! ### Command Structure
//! All commands are JSON objects with a `type` field identifying the command.
//! Commands are routed to handlers based on their domain (agent/graph/misc).
//! 
//! ### Response Types
//! - `Success { data? }`: Command succeeded with optional data
//! - `Error { message }`: Command failed with descriptive error
//! - `Heartbeat`: Keep-alive pulse from server (30s intervals)
//! 
//! ### Concurrency Model
//! Each incoming message spawns as an independent async task, enabling
//! high-throughput concurrent command processing without blocking.
//! 
//! ## Security
//! 
//! - WebSocket upgrade is public (no auth required at upgrade time)
//! - Authentication happens post-connection via Auth command
//! - All commands except Auth/Test/Heartbeat require authentication
//! - Agent authorization enforced at GraphOps layer for all graph operations
//! 
//! ## Key Types
//! 
//! - `WsConnection`: Connection state including auth status and current agent
//! - `Command`: Comprehensive enum of all supported WebSocket commands
//! - `Response`: Success/Error/Heartbeat response types
//! 
//! ## Error Handling
//! 
//! Commands use idiomatic Rust error propagation with the `?` operator.
//! Handlers return errors that bubble up to handle_message() where they're
//! sent to clients as error responses. This provides clean, consistent error
//! handling without explicit error response calls in most handlers.
//! 
//! ## Integration Points
//! 
//! - **AppState**: Central state coordination
//! - **GraphOps**: Agent authorization for graph operations
//! - **Command Handlers**: Domain-specific implementations in websocket_commands/

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::time::interval;
use tracing::{error, warn};
use uuid::Uuid;

use crate::error::*;
use crate::lock::AsyncRwLockExt;
use crate::AppState;

/// WebSocket connection state
pub struct WsConnection {
    pub id: String,
    pub sender: tokio::sync::mpsc::UnboundedSender<Message>,
    pub authenticated: bool,
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub current_agent_id: Option<uuid::Uuid>,  // Current agent for this connection (defaults to prime agent)
}

/// Command protocol definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    CreateBlock {
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        temp_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_name: Option<String>,
    },
    UpdateBlock {
        block_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_name: Option<String>,
    },
    DeleteBlock {
        block_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_name: Option<String>,
    },
    CreatePage {
        name: String,
        properties: Option<HashMap<String, String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_name: Option<String>,
    },
    DeletePage {
        page_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        graph_name: Option<String>,
    },
    Heartbeat,
    Auth {
        token: String,
    },
    Test {
        message: String,
    },
    OpenGraph {
        graph_id: Option<String>,
        graph_name: Option<String>,
    },
    CloseGraph {
        graph_id: Option<String>,
        graph_name: Option<String>,
    },
    CreateGraph {
        name: Option<String>,
        description: Option<String>,
    },
    DeleteGraph {
        graph_id: Option<String>,
        graph_name: Option<String>,
    },
    ListGraphs,
    FreezeOperations,
    UnfreezeOperations,
    GetFreezeState,
    
    // Agent commands
    AgentChat {
        message: String,
        // TODO: Re-evaluate if echo field should be feature-gated or if it has legitimate user use cases
        echo: Option<String>,  // Test-only: force MockLLM to echo this response
    },
    AgentSelect {
        agent_id: Option<String>,    // Optional UUID
        agent_name: Option<String>,  // Optional name
        // If neither provided, defaults to prime agent
    },
    AgentList,
    AgentHistory {
        agent_id: Option<String>,    // Optional UUID
        agent_name: Option<String>,  // Optional name
        limit: Option<usize>,        // Optional, last N messages
        // If neither agent_id nor agent_name provided, uses current connection's agent or prime
    },
    AgentReset {
        agent_id: Option<String>,    // Optional UUID
        agent_name: Option<String>,  // Optional name
        // If neither provided, uses current connection's agent or prime
    },
    
    // Agent admin commands
    CreateAgent {
        name: String,
        description: Option<String>,
    },
    DeleteAgent {
        agent_id: Option<String>,
        agent_name: Option<String>,
    },
    ActivateAgent {
        agent_id: Option<String>,
        agent_name: Option<String>,
    },
    DeactivateAgent {
        agent_id: Option<String>,
        agent_name: Option<String>,
    },
    AuthorizeAgent {
        agent_id: Option<String>,
        agent_name: Option<String>,
        graph_id: Option<String>,
        graph_name: Option<String>,
    },
    DeauthorizeAgent {
        agent_id: Option<String>,
        agent_name: Option<String>,
        graph_id: Option<String>,
        graph_name: Option<String>,
    },
    AgentInfo {
        agent_id: Option<String>,
        agent_name: Option<String>,
    },
    
    // Command for CLI integration testing (only available in debug builds)
    #[cfg(debug_assertions)]
    TestCliCommand {
        command: String,
        params: serde_json::Value,
    },
}

/// Response protocol definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success {
        data: Option<serde_json::Value>,
    },
    Error {
        message: String,
    },
    Heartbeat,
}

/// WebSocket upgrade handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle individual WebSocket connection
// TODO 🚦: Implement rate limiting per connection to prevent spam
// TODO 🔒: Add connection limits per IP address  
// TODO 📏: Enforce maximum message size limits
pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let connection_id = Uuid::new_v4().to_string();
    // Connection established - connection_id: {}

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    
    // Create shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let ws_connection = WsConnection {
        id: connection_id.clone(),
        sender: tx.clone(),
        authenticated: false,
        shutdown_tx: shutdown_tx.clone(),
        current_agent_id: None,  // Will be set to prime agent after authentication
    };

    // Add connection to state
    if let Some(ref connections) = state.ws_connections {
        connections.write_or_panic("websocket handler - insert connection").await.insert(connection_id.clone(), ws_connection);
    }

    // Spawn task to handle sending messages
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Spawn heartbeat task
    let heartbeat_tx = tx.clone();
    let mut heartbeat_shutdown_rx = shutdown_rx.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let heartbeat = Response::Heartbeat;
                    if let Ok(msg) = serde_json::to_string(&heartbeat) {
                        if heartbeat_tx.send(Message::Text(msg)).is_err() {
                            break;
                        }
                    }
                }
                _ = heartbeat_shutdown_rx.changed() => {
                    if *heartbeat_shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        let conn_id = connection_id.clone();
        let app_state = state.clone();
        
        // Spawn each message handling as a separate task to prevent blocking
        tokio::spawn(async move {
            if let Err(e) = handle_message(msg, &conn_id, &app_state).await {
                error!("Error handling message from {}: {:?}", conn_id, e);
                // Send error response back to client so they're not left hanging
                if let Err(send_err) = crate::server::websocket_utils::send_error_response(&conn_id, &app_state, &e.to_string()).await {
                    error!("Failed to send error response: {:?}", send_err);
                }
            }
        });
    }

    // Cleanup on disconnect
    // Connection closed - connection_id: {}
    
    // Send shutdown signal
    let _ = shutdown_tx.send(true);
    
    // Remove from connections map
    if let Some(ref connections) = state.ws_connections {
        connections.write_or_panic("websocket handler - remove connection").await.remove(&connection_id);
    }
    
    // Cancel tasks explicitly to prevent them from becoming zombie threads
    send_task.abort();
    heartbeat_task.abort();
    
    // Wait a moment for clean cancellation
    tokio::time::sleep(Duration::from_millis(10)).await;
}

/// Handle individual WebSocket message
async fn handle_message(
    msg: Message,
    connection_id: &str,
    state: &Arc<AppState>,
) -> Result<()> {
    match msg {
        Message::Text(text) => {
            match serde_json::from_str::<Command>(&text) {
                Ok(command) => {
                    // Get current agent for authorization checks
                    let current_agent_id = if let Some(ref connections) = state.ws_connections {
                        let conns = connections.read_or_panic("heartbeat - read connections").await;
                        conns.get(connection_id).and_then(|conn| conn.current_agent_id)
                    } else {
                        None
                    };
                    
                    route_command(command, connection_id, state, current_agent_id).await?;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        Message::Close(_) => {
            // Connection closing
        }
        Message::Pong(_) => {
            // Pong received
        }
        _ => {}
    }
    Ok(())
}

/// Route command to appropriate handler
async fn route_command(
    command: Command,
    connection_id: &str,
    state: &Arc<AppState>,
    current_agent_id: Option<Uuid>,
) -> Result<()> {
    
    // Check authentication for non-auth commands
    #[cfg(debug_assertions)]
    let is_test_cli_command = matches!(command, Command::TestCliCommand { .. });
    #[cfg(not(debug_assertions))]
    let is_test_cli_command = false;
    
    if !matches!(command, 
        Command::Auth { .. } | 
        Command::Heartbeat | 
        Command::Test { .. }
    ) && !is_test_cli_command {
        if !crate::server::websocket_utils::is_authenticated(connection_id, state).await {
            warn!("Rejecting command from unauthenticated connection {}: {:?}", connection_id, command);
            crate::server::websocket_utils::send_error_response(connection_id, state, "Failed to execute command: not authenticated").await?;
            return Ok(());
        }
    }

    // Route to appropriate handler based on command type
    if is_agent_command(&command) {
        crate::server::websocket_commands::agent_commands::handle(command, connection_id, state).await
    } else if is_graph_command(&command) {
        crate::server::websocket_commands::graph_commands::handle(command, connection_id, state, current_agent_id).await
    } else {
        crate::server::websocket_commands::misc_commands::handle(command, connection_id, state).await
    }
}

/// Check if command is agent-related
fn is_agent_command(command: &Command) -> bool {
    matches!(command,
        Command::AgentChat { .. } |
        Command::AgentSelect { .. } |
        Command::AgentList |
        Command::AgentHistory { .. } |
        Command::AgentReset { .. } |
        Command::CreateAgent { .. } |
        Command::DeleteAgent { .. } |
        Command::ActivateAgent { .. } |
        Command::DeactivateAgent { .. } |
        Command::AuthorizeAgent { .. } |
        Command::DeauthorizeAgent { .. } |
        Command::AgentInfo { .. }
    )
}

/// Check if command is graph-related
fn is_graph_command(command: &Command) -> bool {
    matches!(command,
        Command::CreateBlock { .. } |
        Command::UpdateBlock { .. } |
        Command::DeleteBlock { .. } |
        Command::CreatePage { .. } |
        Command::DeletePage { .. } |
        Command::OpenGraph { .. } |
        Command::CloseGraph { .. } |
        Command::CreateGraph { .. } |
        Command::DeleteGraph { .. } |
        Command::ListGraphs
    )
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_command_serialization() {
        let cmd = Command::CreateBlock {
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("TestPage".to_string()),
            temp_id: Some("temp-456".to_string()),
            graph_id: None,
            graph_name: None,
        };
        
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"create_block\""));
        assert!(json.contains("\"content\":\"Test content\""));
        assert!(json.contains("\"page_name\":\"TestPage\""));
    }
    
    #[test]
    fn test_command_deserialization() {
        let json = r#"{
            "type": "auth",
            "token": "test-token-123"
        }"#;
        
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Auth { token } => assert_eq!(token, "test-token-123"),
            _ => panic!("Wrong command type"),
        }
    }
    
    #[test]
    fn test_response_serialization() {
        let response = Response::Success {
            data: Some(serde_json::json!({"test": "data"})),
        };
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        assert!(json.contains("\"test\":\"data\""));
    }
    
    #[test]
    fn test_heartbeat_command() {
        let json = r#"{"type": "heartbeat"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::Heartbeat));
    }
    
    #[test]
    fn test_graph_fields_serialization() {
        // Test with both graph_id and graph_name
        let cmd = Command::UpdateBlock {
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
            graph_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            graph_name: Some("My Graph".to_string()),
        };
        
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"graph_id\":\"550e8400-e29b-41d4-a716-446655440000\""));
        assert!(json.contains("\"graph_name\":\"My Graph\""));
        
        // Test with only graph_name (graph_id should be omitted)
        let cmd = Command::DeleteBlock {
            block_id: "block-456".to_string(),
            graph_id: None,
            graph_name: Some("Another Graph".to_string()),
        };
        
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(!json.contains("\"graph_id\""));
        assert!(json.contains("\"graph_name\":\"Another Graph\""));
    }
    
    #[test]
    fn test_graph_command_deserialization() {
        // Test OpenGraph command
        let json = r#"{
            "type": "open_graph",
            "graph_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::OpenGraph { graph_id, graph_name } => {
                assert_eq!(graph_id, Some("550e8400-e29b-41d4-a716-446655440000".to_string()));
                assert_eq!(graph_name, None);
            }
            _ => panic!("Wrong command type"),
        }
        
        // Test with graph_name
        let json = r#"{
            "type": "close_graph",
            "graph_name": "Test Graph"
        }"#;
        
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::CloseGraph { graph_id, graph_name } => {
                assert_eq!(graph_id, None);
                assert_eq!(graph_name, Some("Test Graph".to_string()));
            }
            _ => panic!("Wrong command type"),
        }
        
        // Test ListGraphs command
        let json = r#"{"type": "list_graphs"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::ListGraphs));
    }

}