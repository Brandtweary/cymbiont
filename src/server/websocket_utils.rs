//! `@module websocket_utils`
//! @description Utility functions for WebSocket operations
//!
//! This module contains shared helper functions used by command handlers,
//! including graph resolution, authentication checks, and response sending.

use crate::error::{Result, ServerError};
use crate::server::websocket::Response;
use crate::utils::AsyncRwLockExt;
use crate::AppState;
use axum::extract::ws::Message;
use std::sync::Arc;
use uuid::Uuid;

/// Helper to resolve graph ID from optional `graph_id` and `graph_name`
pub async fn resolve_graph_for_command(
    state: &Arc<AppState>,
    graph_id: Option<&str>,
    graph_name: Option<&str>,
    allow_smart_default: bool,
) -> Result<Uuid> {
    let registry = state
        .graph_registry
        .read_or_panic("read graph registry")
        .await;

    let graph_uuid =
        if let Some(id_str) = graph_id {
            Some(Uuid::parse_str(id_str).map_err(|_| {
                ServerError::invalid_request(format!("Invalid graph UUID: {id_str}"))
            })?)
        } else {
            None
        };

    registry.resolve_graph_target(graph_uuid.as_ref(), graph_name, allow_smart_default)
}

/// Check if a connection is authenticated (safe, read-only)
pub async fn is_authenticated(connection_id: &str, state: &Arc<AppState>) -> bool {
    if let Some(ref connections) = state.ws_connections {
        let conns = connections
            .read_or_panic("send response - read connections")
            .await;
        if let Some(conn) = conns.get(connection_id) {
            return conn.authenticated;
        }
    }
    false
}

/// Set a connection as authenticated (safe, atomic write)
pub async fn set_authenticated(connection_id: &str, state: &Arc<AppState>) -> Result<bool> {
    if let Some(ref connections) = state.ws_connections {
        let mut conns = connections
            .write_or_panic("set auth state - write connections")
            .await;
        if let Some(conn) = conns.get_mut(connection_id) {
            conn.authenticated = true;
            // Return true if this is the first authenticated connection
            Ok(conns.values().filter(|c| c.authenticated).count() == 1)
        } else {
            Err(ServerError::websocket("Connection not found").into())
        }
    } else {
        Err(ServerError::websocket("No connection tracking").into())
    }
}

/// Get connection stats (safe, read-only)
pub async fn get_connection_stats(state: &Arc<AppState>) -> (usize, usize) {
    if let Some(ref connections) = state.ws_connections {
        let conns = connections
            .read_or_panic("send response - read connections")
            .await;
        let total = conns.len();
        let authenticated = conns.values().filter(|c| c.authenticated).count();
        drop(conns);
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
) -> Result<()> {
    // Get the sender without holding lock
    let sender = if let Some(ref connections) = state.ws_connections {
        let conns = connections
            .read_or_panic("send response - read connections")
            .await;
        if let Some(conn) = conns.get(connection_id) {
            conn.sender.clone()
        } else {
            tracing::warn!(
                "Attempted to send response to disconnected client: {}",
                connection_id
            );
            return Ok(()); // Connection gone, silently succeed
        }
    } else {
        return Ok(());
    };

    // Now send without holding any lock
    let msg = serde_json::to_string(&response)?;
    sender
        .send(Message::Text(msg.into()))
        .map_err(|_| ServerError::websocket("Failed to send message on channel"))?;
    Ok(())
}

/// Send success response
pub async fn send_success_response(
    connection_id: &str,
    state: &Arc<AppState>,
    data: Option<serde_json::Value>,
) -> Result<()> {
    let response = Response::Success { data };
    send_response(connection_id, state, response).await
}

/// Send error response
pub async fn send_error_response(
    connection_id: &str,
    state: &Arc<AppState>,
    message: &str,
) -> Result<()> {
    let response = Response::Error {
        message: message.to_string(),
    };
    send_response(connection_id, state, response).await
}
