/**
 * @module websocket
 * @description WebSocket server for real-time client communication
 * 
 * This module implements a WebSocket server that enables bidirectional communication
 * between the knowledge graph engine and external clients, supporting real-time
 * graph operations with high-throughput async execution.
 * 
 * ## Connection Management
 * 
 * Connection-based architecture with authentication:
 * - Unique UUID tracking for each connection
 * - Authentication required before command processing
 * - Heartbeat mechanism for connection health
 * - Automatic cleanup on disconnect
 * 
 * ## Async Command Processing
 * 
 * High-performance async architecture for scalable API traffic:
 * - Each WebSocket command spawns as an independent async task
 * - Commands execute concurrently without blocking each other
 * - Critical state commands (freeze/unfreeze) execute immediately
 * - Supports high-throughput scenarios with multiple concurrent operations
 * 
 * ## Command Protocol
 * 
 * JSON-based request/response system:
 * - **Client→Server**: 
 *   - `Auth`, `Heartbeat` - Connection management
 *   - `OpenGraph`, `CloseGraph` - Graph lifecycle (replaces SwitchGraph)
 *   - `CreateBlock`, `UpdateBlock`, `DeleteBlock` - Block operations (accept optional graph_id/graph_name)
 *   - `CreatePage`, `DeletePage` - Page operations (accept optional graph_id/graph_name)
 *   - `CreateGraph`, `DeleteGraph` - Graph management
 *   - `FreezeOperations`, `UnfreezeOperations` - Test infrastructure
 * - **Server→Client**: `Success`, `Error`, `Heartbeat`
 * - Commands execute asynchronously with transaction wrapping
 * - Graph targeting via optional `graph_id` (UUID string) or `graph_name` fields
 * - Centralized graph resolution with smart defaults for CRUD operations
 * 
 * ## Transaction Integration
 * 
 * WebSocket commands integrate with the transaction system for ACID guarantees:
 * - Commands execute within transactions
 * - Automatic rollback on operation failures
 * - Content deduplication via hash checking
 * - Freeze mechanism supports deterministic testing scenarios
 */

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
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::AppState;

/// WebSocket connection state
pub struct WsConnection {
    pub id: String,
    pub sender: tokio::sync::mpsc::UnboundedSender<Message>,
    pub authenticated: bool,
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
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
    FreezeOperations,
    UnfreezeOperations,
    GetFreezeState,
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
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let connection_id = Uuid::new_v4().to_string();
    info!("🔌 New WebSocket connection: {}", connection_id);

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    
    // Create shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let ws_connection = WsConnection {
        id: connection_id.clone(),
        sender: tx.clone(),
        authenticated: false,
        shutdown_tx: shutdown_tx.clone(),
    };

    // Add connection to state
    if let Some(ref connections) = state.ws_connections {
        connections.write().await.insert(connection_id.clone(), ws_connection);
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
            }
        });
    }

    // Cleanup on disconnect
    info!("🔌 WebSocket disconnected: {}", connection_id);
    
    // Send shutdown signal
    let _ = shutdown_tx.send(true);
    
    // Remove from connections map
    if let Some(ref connections) = state.ws_connections {
        connections.write().await.remove(&connection_id);
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match msg {
        Message::Text(text) => {
            match serde_json::from_str::<Command>(&text) {
                Ok(command) => {
                    handle_command(command, connection_id, state).await?;
                }
                Err(e) => {
                    return Err(Box::new(e));
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


/// Helper to resolve graph ID from optional graph_id and graph_name
async fn resolve_graph_for_command(
    state: &Arc<AppState>,
    graph_id: Option<&str>,
    graph_name: Option<&str>,
    allow_smart_default: bool,
) -> Result<uuid::Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let registry = state.graph_registry.read()
        .map_err(|e| format!("Failed to read registry: {}", e))?;
    
    let graph_uuid = if let Some(id_str) = graph_id {
        Some(uuid::Uuid::parse_str(id_str)
            .map_err(|_| format!("Invalid UUID: {}", id_str))?)
    } else {
        None
    };
    
    Ok(registry.resolve_graph_target(
        graph_uuid.as_ref(),
        graph_name.as_deref(),
        allow_smart_default,
    )?)
}

/// Handle command execution
async fn handle_command(
    command: Command,
    connection_id: &str,
    state: &Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    
    // Check authentication for non-auth commands
    if !matches!(command, 
        Command::Auth { .. } | 
        Command::Heartbeat | 
        Command::Test { .. }
    ) {
        if !is_authenticated(connection_id, state).await {
            warn!("Rejecting command from unauthenticated connection {}: {:?}", connection_id, command);
            send_error_response(connection_id, state, "Not authenticated").await?;
            return Ok(());
        }
    }

    match command {
        Command::Auth { token } => {
            // Validate token against configured auth token
            use crate::server::auth::validate_token;
            
            if !validate_token(state, &token).await {
                warn!("🔐 WebSocket authentication failed for {}: invalid token", connection_id);
                send_error_response(connection_id, state, "Invalid authentication token").await?;
                return Ok(());
            }
            
            // Set authenticated (atomic operation)
            match set_authenticated(connection_id, state).await {
                Ok(is_first) => {
                    // Send success response (no lock held)
                    send_success_response(connection_id, state, None).await?;
                    info!("🔐 WebSocket authenticated: {}", connection_id);
                    
                    // Signal that WebSocket is ready if this is the first authenticated connection
                    if is_first {
                        if let Ok(mut tx_guard) = state.ws_ready_tx.lock() {
                            if let Some(tx) = tx_guard.take() {
                                let _ = tx.send(());
                                info!("📡 WebSocket ready signal sent");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to authenticate connection {}: {}", connection_id, e);
                    send_error_response(connection_id, state, "Authentication failed").await?;
                }
            }
        }
        Command::Heartbeat => {
            // Client sent a heartbeat/pong - just acknowledge receipt, don't respond
            // This prevents infinite heartbeat loops
        }
        Command::CreateBlock { content, parent_id, page_name, temp_id: _, graph_id, graph_name } => {
            // Call kg_api to create the block
            info!("📝 CreateBlock command received via WebSocket");
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            match state.add_block(content, parent_id, page_name, None, &resolved_graph_id).await {
                Ok(block_id) => {
                    let data = serde_json::json!({ "block_id": block_id });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to create block: {}", e)).await?;
                }
            }
        }
        Command::UpdateBlock { block_id, content, graph_id, graph_name } => {
            // Call kg_api to update the block
            info!("✏️ UpdateBlock command received via WebSocket: {}", block_id);
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            match state.update_block(block_id.clone(), content, &resolved_graph_id).await {
                Ok(()) => {
                    let data = serde_json::json!({ "block_id": block_id });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to update block: {}", e)).await?;
                }
            }
        }
        Command::DeleteBlock { block_id, graph_id, graph_name } => {
            // Call kg_api to delete the block
            info!("🗑️ DeleteBlock command received via WebSocket: {}", block_id);
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            match state.delete_block(block_id.clone(), &resolved_graph_id).await {
                Ok(()) => {
                    let data = serde_json::json!({ "block_id": block_id });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to delete block: {}", e)).await?;
                }
            }
        }
        Command::CreatePage { name, properties, graph_id, graph_name } => {
            // Call kg_api to create the page
            info!("📄 CreatePage command received via WebSocket: {}", name);
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Convert HashMap<String, String> to serde_json::Value
            let properties_json = properties.map(|props| {
                serde_json::Value::Object(
                    props.into_iter()
                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                        .collect()
                )
            });
            
            match state.create_page(name.clone(), properties_json, &resolved_graph_id).await {
                Ok(()) => {
                    let data = serde_json::json!({ "page_name": name });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to create page: {}", e)).await?;
                }
            }
        }
        Command::Test { message } => {
            // Test command - just echo back the message with some stats
            info!("📡 Test command received from client: {}", message);
            
            let (total, authenticated) = get_connection_stats(state).await;
            let response_data = serde_json::json!({
                "echo": message,
                "connection_id": connection_id,
                "total_connections": total,
                "authenticated_connections": authenticated,
            });
            
            send_success_response(connection_id, state, Some(response_data)).await?;
        }
        Command::OpenGraph { graph_id, graph_name } => {
            // Open a graph
            info!("📂 OpenGraph command received via WebSocket");
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to open
            ).await?;
            
            match state.open_graph(resolved_graph_id).await {
                Ok(graph_info) => {
                    send_success_response(connection_id, state, Some(graph_info)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to open graph: {}", e)).await?;
                }
            }
        }
        Command::CloseGraph { graph_id, graph_name } => {
            // Close a graph
            info!("📁 CloseGraph command received via WebSocket");
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to close
            ).await?;
            
            match state.close_graph(resolved_graph_id).await {
                Ok(()) => {
                    send_success_response(connection_id, state, None).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to close graph: {}", e)).await?;
                }
            }
        }
        Command::CreateGraph { name, description } => {
            // Create a new graph
            info!("📊 CreateGraph command received via WebSocket");
            
            use crate::graph_operations::GraphOperationsExt;
            
            match state.create_graph(name, description).await {
                Ok(graph_info) => {
                    send_success_response(connection_id, state, Some(graph_info)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to create graph: {}", e)).await?;
                }
            }
        }
        Command::DeleteGraph { graph_id, graph_name } => {
            // Delete a graph
            info!("🗑️ DeleteGraph command received via WebSocket");
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to delete
            ).await?;
            
            match state.delete_graph(&resolved_graph_id).await {
                Ok(()) => {
                    let data = serde_json::json!({ "deleted_graph_id": resolved_graph_id.to_string() });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to delete graph: {}", e)).await?;
                }
            }
        }
        Command::DeletePage { page_name, graph_id, graph_name } => {
            // Delete a page
            info!("🗑️ DeletePage command received via WebSocket: {}", page_name);
            
            use crate::graph_operations::GraphOperationsExt;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            match state.delete_page(page_name.clone(), &resolved_graph_id).await {
                Ok(()) => {
                    let data = serde_json::json!({ "page_name": page_name });
                    send_success_response(connection_id, state, Some(data)).await?;
                }
                Err(e) => {
                    send_error_response(connection_id, state, &format!("Failed to delete page: {}", e)).await?;
                }
            }
        }
        Command::FreezeOperations => {
            // Freeze all graph operations
            info!("❄️ FreezeOperations command received via WebSocket");
            
            let mut freeze_state = state.operation_freeze.write().await;
            *freeze_state = true;
            
            send_success_response(connection_id, state, None).await?;
        }
        Command::UnfreezeOperations => {
            // Unfreeze all graph operations
            info!("🔥 UnfreezeOperations command received via WebSocket");
            
            let mut freeze_state = state.operation_freeze.write().await;
            *freeze_state = false;
            
            send_success_response(connection_id, state, None).await?;
        }
        Command::GetFreezeState => {
            // Get current freeze state
            info!("🌡️ GetFreezeState command received via WebSocket");
            
            let freeze_state = state.operation_freeze.read().await;
            let data = serde_json::json!({ "frozen": *freeze_state });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
    }
    
    Ok(())
}

/// Check if a connection is authenticated (safe, read-only)
async fn is_authenticated(
    connection_id: &str,
    state: &Arc<AppState>,
) -> bool {
    if let Some(ref connections) = state.ws_connections {
        let conns = connections.read().await;
        if let Some(conn) = conns.get(connection_id) {
            return conn.authenticated;
        }
    }
    false
}

/// Set a connection as authenticated (safe, atomic write)
async fn set_authenticated(
    connection_id: &str,
    state: &Arc<AppState>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(ref connections) = state.ws_connections {
        let mut conns = connections.write().await;
        if let Some(conn) = conns.get_mut(connection_id) {
            conn.authenticated = true;
            // Return true if this is the first authenticated connection
            Ok(conns.values().filter(|c| c.authenticated).count() == 1)
        } else {
            Err("Connection not found".into())
        }
    } else {
        Err("No connection tracking".into())
    }
}

/// Get connection stats (safe, read-only)
async fn get_connection_stats(
    state: &Arc<AppState>,
) -> (usize, usize) {
    if let Some(ref connections) = state.ws_connections {
        let conns = connections.read().await;
        let total = conns.len();
        let authenticated = conns.values().filter(|c| c.authenticated).count();
        (total, authenticated)
    } else {
        (0, 0)
    }
}

/// Send response to a specific connection (safe, no lock held during send)
async fn send_response(
    connection_id: &str,
    state: &Arc<AppState>,
    response: Response,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get the sender without holding lock
    let sender = if let Some(ref connections) = state.ws_connections {
        let conns = connections.read().await;
        if let Some(conn) = conns.get(connection_id) {
            conn.sender.clone()
        } else {
            return Ok(()); // Connection gone, silently succeed
        }
    } else {
        return Ok(());
    };
    
    // Now send without holding any lock
    let msg = serde_json::to_string(&response)?;
    sender.send(Message::Text(msg))?;
    Ok(())
}

/// Send success response
async fn send_success_response(
    connection_id: &str,
    state: &Arc<AppState>,
    data: Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let response = Response::Success {
        data,
    };
    send_response(connection_id, state, response).await
}

/// Send error response
async fn send_error_response(
    connection_id: &str,
    state: &Arc<AppState>,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let response = Response::Error {
        message: message.to_string(),
    };
    send_response(connection_id, state, response).await
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
    }

}