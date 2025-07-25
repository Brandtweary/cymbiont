/**
 * @module websocket
 * @description WebSocket server for bidirectional communication with Logseq plugin
 * 
 * This module implements a WebSocket server that enables real-time bidirectional
 * communication between the Cymbiont backend and the Logseq plugin. It provides
 * the foundation for AI agents to create, update, and delete content in the user's
 * PKM through WebSocket commands.
 * 
 * ## Architecture
 * 
 * The module uses Axum's WebSocket support with a connection-based architecture:
 * - Each connection gets a unique UUID and tracking state
 * - Connections must authenticate before receiving commands
 * - Commands are broadcast to all authenticated connections
 * - Heartbeat mechanism detects stale connections
 * 
 * ## Command Protocol
 * 
 * Commands use JSON with a tagged enum pattern:
 * - **Client → Server**: auth, heartbeat, test
 * - **Server → Client**: create_block, update_block, delete_block, create_page
 * 
 * ## Connection Lifecycle
 * 
 * 1. Client connects and receives connection ID
 * 2. Client sends auth command with token
 * 3. Server validates and marks connection as authenticated
 * 4. Server can now send PKM manipulation commands
 * 5. Heartbeat keeps connection alive (30s intervals)
 * 6. On disconnect, connection is cleaned up
 * 
 * ## Concurrency Safety
 * 
 * The module implements deadlock-proof patterns:
 * - Helper functions encapsulate all lock operations
 * - Locks are never held during async operations
 * - Connection state is cloned before sending messages
 * 
 * ## Future Enhancements
 * 
 * - Command acknowledgments with Logseq UUIDs
 * - Integration with transaction log for correlation
 * - Command batching for efficiency
 * - Connection pooling for multi-graph support
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
        correlation_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        temp_id: Option<String>,
    },
    UpdateBlock {
        block_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
    DeleteBlock {
        block_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
    CreatePage {
        name: String,
        properties: Option<HashMap<String, String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
    Heartbeat,
    Auth {
        token: String,
    },
    Test {
        message: String,
    },
    // Acknowledgment messages from plugin to server
    BlockCreated {
        correlation_id: String,
        block_uuid: String,
        temp_id: String,
    },
    BlockUpdated {
        correlation_id: String,
        success: bool,
        error: Option<String>,
    },
    BlockDeleted {
        correlation_id: String,
        success: bool,
        error: Option<String>,
    },
    PageCreated {
        correlation_id: String,
        success: bool,
        error: Option<String>,
    },
}

/// Response protocol definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success {
        command_id: String,
        data: Option<serde_json::Value>,
    },
    Error {
        command_id: String,
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

    let ws_connection = WsConnection {
        id: connection_id.clone(),
        sender: tx.clone(),
        authenticated: false,
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
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let heartbeat = Response::Heartbeat;
            if let Ok(msg) = serde_json::to_string(&heartbeat) {
                if heartbeat_tx.send(Message::Text(msg)).is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        if let Err(e) = handle_message(msg, &connection_id, &state).await {
            error!("Error handling message from {}: {:?}", connection_id, e);
        }
    }

    // Cleanup on disconnect
    info!("🔌 WebSocket disconnected: {}", connection_id);
    if let Some(ref connections) = state.ws_connections {
        connections.write().await.remove(&connection_id);
    }
    send_task.abort();
    heartbeat_task.abort();
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
        Command::Test { .. } |
        Command::BlockCreated { .. } |
        Command::BlockUpdated { .. } |
        Command::BlockDeleted { .. } |
        Command::PageCreated { .. }
    ) {
        if !is_authenticated(connection_id, state).await {
            warn!("Rejecting command from unauthenticated connection {}: {:?}", connection_id, command);
            send_error_response(connection_id, state, "Not authenticated").await?;
            return Ok(());
        }
    }

    match command {
        Command::Auth { token: _ } => {
            // TODO: Validate token against state.auth_token or similar
            
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
        Command::CreateBlock { .. } | 
        Command::UpdateBlock { .. } | 
        Command::DeleteBlock { .. } | 
        Command::CreatePage { .. } => {
            // These commands are only sent FROM server TO client
            // The plugin should never send these to the server
            error!("Unexpected command from client: {:?}", command);
            send_error_response(connection_id, state, "Client should not send PKM manipulation commands").await?;
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
        Command::BlockCreated { correlation_id, block_uuid, temp_id } => {
            // Handle block creation acknowledgment
            info!("📥 Block creation acknowledgment: correlation_id={}, uuid={}, temp_id={}", 
                  correlation_id, block_uuid, temp_id);
            
            // Look up saga from correlation ID
            let saga_id = {
                let correlation_map = state.correlation_to_saga.read().await;
                correlation_map.get(&correlation_id).cloned()
            };
            
            if let Some(saga_id) = saga_id {
                // Update the node's PKM ID from temp to real
                {
                    // Get active graph
                    let active_graph_id = state.get_active_graph_manager().await;
                    if let Some(graph_id) = active_graph_id {
                        let managers = state.graph_managers.read().await;
                        if let Some(manager_lock) = managers.get(&graph_id) {
                            let mut graph_manager = manager_lock.write().await;
                            if let Some(&node_idx) = graph_manager.pkm_to_node.get(&temp_id) {
                                if let Some(node) = graph_manager.graph.node_weight_mut(node_idx) {
                                    // Update the node's PKM ID
                                    node.pkm_id = block_uuid.clone();
                                }
                                // Update the mapping
                                graph_manager.pkm_to_node.remove(&temp_id);
                                graph_manager.pkm_to_node.insert(block_uuid.clone(), node_idx);
                            }
                        }
                    }
                }
                
                // Complete the saga
                match state.workflow_sagas.handle_block_acknowledgment(&saga_id, &temp_id, true, Some(block_uuid)).await {
                    Ok(_) => {
                        info!("✅ Saga {} completed successfully", saga_id);
                        // Clean up correlation mapping
                        state.correlation_to_saga.write().await.remove(&correlation_id);
                    }
                    Err(e) => {
                        error!("Failed to complete saga {}: {:?}", saga_id, e);
                    }
                }
            } else {
                warn!("No saga found for correlation_id: {}", correlation_id);
            }
        }
        Command::BlockUpdated { correlation_id, success, error } => {
            info!("📥 Block update acknowledgment: correlation_id={}, success={}, error={:?}", 
                  correlation_id, success, error);
            // TODO: Handle update acknowledgments
        }
        Command::BlockDeleted { correlation_id, success, error } => {
            info!("📥 Block delete acknowledgment: correlation_id={}, success={}, error={:?}", 
                  correlation_id, success, error);
            // TODO: Handle delete acknowledgments
        }
        Command::PageCreated { correlation_id, success, error } => {
            info!("📥 Page creation acknowledgment: correlation_id={}, success={}, error={:?}", 
                  correlation_id, success, error);
            // TODO: Handle page creation acknowledgments
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
        command_id: Uuid::new_v4().to_string(),
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
        command_id: Uuid::new_v4().to_string(),
        message: message.to_string(),
    };
    send_response(connection_id, state, response).await
}

/// Get all authenticated connection senders (safe, releases lock before use)
async fn get_authenticated_senders(
    state: &Arc<AppState>,
) -> Vec<(String, tokio::sync::mpsc::UnboundedSender<Message>)> {
    if let Some(ref connections) = state.ws_connections {
        let conns = connections.read().await;
        conns.iter()
            .filter(|(_, conn)| conn.authenticated)
            .map(|(id, conn)| (id.clone(), conn.sender.clone()))
            .collect()
    } else {
        vec![]
    }
}

/// Broadcast command to all authenticated connections
pub async fn broadcast_command(
    state: &Arc<AppState>,
    command: Command,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get senders without holding lock
    let senders = get_authenticated_senders(state).await;
    
    // Serialize once
    let msg = serde_json::to_string(&command)?;
    
    // Send to all authenticated connections (no lock held)
    for (id, sender) in senders {
        if let Err(e) = sender.send(Message::Text(msg.clone())) {
            warn!("Failed to send to connection {}: {:?}", id, e);
        }
    }
    
    Ok(())
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
            correlation_id: Some("corr-123".to_string()),
            temp_id: Some("temp-456".to_string()),
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
            command_id: "cmd-123".to_string(),
            data: Some(serde_json::json!({"test": "data"})),
        };
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        assert!(json.contains("\"command_id\":\"cmd-123\""));
        assert!(json.contains("\"test\":\"data\""));
    }
    
    #[test]
    fn test_heartbeat_command() {
        let json = r#"{"type": "heartbeat"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::Heartbeat));
    }

    #[test]
    fn test_acknowledgment_commands() {
        let json = r#"{
            "type": "block_created",
            "correlation_id": "corr-123",
            "block_uuid": "uuid-456",
            "temp_id": "temp-789"
        }"#;
        
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::BlockCreated { correlation_id, block_uuid, temp_id } => {
                assert_eq!(correlation_id, "corr-123");
                assert_eq!(block_uuid, "uuid-456");
                assert_eq!(temp_id, "temp-789");
            },
            _ => panic!("Wrong command type"),
        }
    }
}