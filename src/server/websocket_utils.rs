/**
 * @module websocket_utils
 * @description Utility functions for WebSocket operations
 * 
 * This module contains shared helper functions used by command handlers,
 * including graph resolution, authentication checks, and response sending.
 */

use axum::extract::ws::Message;
use std::sync::Arc;
use crate::AppState;
use crate::server::websocket::Response;

/// Helper to resolve graph ID from optional graph_id and graph_name
pub async fn resolve_graph_for_command(
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

/// Check if a connection is authenticated (safe, read-only)
pub async fn is_authenticated(
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
pub async fn set_authenticated(
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
pub async fn get_connection_stats(
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
pub async fn send_response(
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
pub async fn send_success_response(
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
pub async fn send_error_response(
    connection_id: &str,
    state: &Arc<AppState>,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let response = Response::Error {
        message: message.to_string(),
    };
    send_response(connection_id, state, response).await
}