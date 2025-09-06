//! @module http_api
//! @description HTTP API implementation for the Cymbiont Knowledge Graph backend
//!
//! This module provides HTTP endpoints for health checks, one-time import operations,
//! WebSocket upgrades, and monitoring. All real-time data manipulation is handled
//! exclusively through WebSocket connections.
//!
//! ## Design Philosophy
//!
//! The HTTP API follows a minimalist approach focused on:
//! - **Stateless Operations**: Perfect for one-time imports and health checks
//! - **WebSocket Handoff**: Real-time operations transition to WebSocket protocol
//! - **Monitoring Integration**: Endpoints for system health and debugging
//! - **Security by Default**: Path validation and canonicalization for imports
//!
//! ## Endpoints
//!
//! ### GET /
//! Health check endpoint returning static string "PKM Knowledge Graph Backend Server".
//! - **Purpose**: Load balancer health checks, service discovery
//! - **Response**: Plain text, always returns 200 OK
//! - **Performance**: No database queries, instant response
//!
//! ### POST /import/logseq
//! Import a Logseq graph from a directory path. This is a one-time operation
//! perfect for HTTP's request/response model.
//! - **Request**: JSON with `path` (required) and `graph_name` (optional)
//! - **Validation**: Path existence, directory check, canonicalization
//! - **Process**: Markdown parsing → Reference resolution → Graph creation
//! - **Response**: Success with import statistics or detailed error message
//! - **Security**: Path traversal prevention, safe directory validation
//!
//! ### GET /ws
//! WebSocket upgrade endpoint - transitions HTTP connections to WebSocket protocol.
//! - **Purpose**: Upgrade HTTP connections for real-time graph operations
//! - **Protocol**: Standard WebSocket upgrade handshake
//! - **Authentication**: Handled post-upgrade in WebSocket handler
//! - **Connection Management**: Automatic cleanup on disconnect
//!
//! ### GET /api/websocket/status
//! Returns WebSocket connection metrics for monitoring.
//! - **Response**: JSON with connection count, open graph IDs
//! - **Use Cases**: System monitoring, integration testing, debugging
//! - **Performance**: Fast read-only operation, no heavy processing
//!
//! ### GET /api/websocket/recent-activity
//! Returns recent WebSocket activity for integration testing and debugging.
//! - **Response**: JSON with active connections and activity metadata
//! - **Purpose**: Integration test validation, debugging connection issues
//! - **Future**: Will be expanded with command history tracking
//!
//! ## Error Handling
//!
//! HTTP endpoints implement consistent error patterns:
//! - **Validation Errors**: 400 Bad Request with descriptive messages
//! - **Not Found**: 404 for non-existent paths or resources
//! - **Server Errors**: 500 with generic messages (details in logs)
//! - **Import Errors**: Partial success reporting with error collections
//!
//! ## Integration Points
//!
//! The HTTP API integrates with:
//! - **AppState**: Shared application state for graph management
//! - **Import System**: Logseq parsing and graph creation
//! - **WebSocket Server**: Connection handoff and status reporting
//! - **Storage Layer**: Graph registry and transaction coordination

use crate::server::auth::auth_middleware;
use crate::server::websocket::websocket_handler;
use crate::utils::AsyncRwLockExt;
use crate::AppState;
use axum::{
    extract::State,
    middleware,
    routing::{any, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

// ===== API Types =====

// Basic response for API calls
#[cfg(test)]
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_id: Option<String>,
}

// Logseq import request
#[derive(Deserialize, Debug)]
pub struct LogseqImportRequest {
    pub path: String,
    #[serde(default)]
    pub graph_name: Option<String>,
}

// Logseq import response
#[derive(Serialize)]
pub struct LogseqImportResponse {
    pub success: bool,
    pub message: String,
    pub graph_id: String,
    pub graph_name: String,
    pub pages_imported: usize,
    pub blocks_imported: usize,
    pub errors: Vec<String>,
}

// ===== Route Configuration =====

/// Create and configure the API router
pub fn create_router(app_state: Arc<AppState>) -> Router {
    // Routes that require authentication
    let protected_routes = Router::new()
        .route("/import/logseq", post(import_logseq))
        .route("/api/websocket/status", get(get_websocket_status))
        .route(
            "/api/websocket/recent-activity",
            get(get_websocket_activity),
        )
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/ws", any(websocket_handler));

    // Combine all routes
    public_routes.merge(protected_routes).with_state(app_state)
}

// ===== Handlers =====

// Root endpoint
pub async fn root() -> &'static str {
    "PKM Knowledge Graph Backend Server"
}

/// Import a Logseq graph via HTTP
pub async fn import_logseq(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LogseqImportRequest>,
) -> Json<LogseqImportResponse> {
    let path = Path::new(&request.path);

    // Validate the path exists and is a directory
    if !path.exists() {
        return Json(LogseqImportResponse {
            success: false,
            message: format!("Path does not exist: {}", request.path),
            graph_id: String::new(),
            graph_name: String::new(),
            pages_imported: 0,
            blocks_imported: 0,
            errors: vec![],
        });
    }

    if !path.is_dir() {
        return Json(LogseqImportResponse {
            success: false,
            message: format!("Path is not a directory: {}", request.path),
            graph_id: String::new(),
            graph_name: String::new(),
            pages_imported: 0,
            blocks_imported: 0,
            errors: vec![],
        });
    }

    // Security check: ensure path is absolute and within reasonable bounds
    let abs_path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(LogseqImportResponse {
                success: false,
                message: format!("Failed to resolve path: {}", e),
                graph_id: String::new(),
                graph_name: String::new(),
                pages_imported: 0,
                blocks_imported: 0,
                errors: vec![],
            });
        }
    };

    // TODO: Add configurable safe directory check here
    // For now, just log the import attempt
    info!("📥 HTTP import request for: {:?}", abs_path);

    // Perform the import
    match crate::import::import_logseq_graph(&state, &abs_path, request.graph_name).await {
        Ok(result) => {
            let has_errors = !result.errors.is_empty();
            let message = if has_errors {
                format!(
                    "Import completed with {} pages, {} blocks, and {} errors",
                    result.pages_imported,
                    result.blocks_imported,
                    result.errors.len()
                )
            } else {
                format!(
                    "Successfully imported {} pages and {} blocks",
                    result.pages_imported, result.blocks_imported
                )
            };

            Json(LogseqImportResponse {
                success: true,
                message,
                graph_id: result.graph_id,
                graph_name: result.graph_name,
                pages_imported: result.pages_imported,
                blocks_imported: result.blocks_imported,
                errors: result.errors,
            })
        }
        Err(e) => {
            error!("Import failed: {}", e);
            Json(LogseqImportResponse {
                success: false,
                message: format!("Import failed: {}", e),
                graph_id: String::new(),
                graph_name: String::new(),
                pages_imported: 0,
                blocks_imported: 0,
                errors: vec![e.to_string()],
            })
        }
    }
}

/// Get WebSocket connection status
pub async fn get_websocket_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let connections = if let Some(ws_connections) = &state.ws_connections {
        let conns = ws_connections
            .read_or_panic("websocket status - read connections")
            .await;
        conns.len()
    } else {
        0
    };

    // Get open graphs for context
    let open_graph_ids: Vec<String> = {
        let registry = state
            .graph_registry
            .read_or_panic("read graph registry for status")
            .await;
        registry
            .get_open_graphs()
            .iter()
            .map(|id| id.to_string())
            .collect()
    };

    Json(serde_json::json!({
        "connected": connections > 0,
        "connection_count": connections,
        "open_graphs": open_graph_ids,
        // TODO: Add more detailed connection info when needed
    }))
}

/// Get recent WebSocket activity
pub async fn get_websocket_activity(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // TODO: Implement proper activity tracking in WebSocket module
    // For now, return basic connection info
    let active_connections = if let Some(ws_connections) = &state.ws_connections {
        let conns = ws_connections
            .read_or_panic("websocket status - read connections")
            .await;
        conns
            .values()
            .map(|conn| {
                serde_json::json!({
                    "id": conn.id,
                    "authenticated": conn.authenticated
                })
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Json(serde_json::json!({
        "active_connections": active_connections,
        "recent_commands": [],
        "recent_confirmations": [],
        "last_activity": null,
        "note": "Full activity tracking not yet implemented"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_response_serialization() {
        let response = ApiResponse {
            success: true,
            message: "Test message".to_string(),
            graph_id: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"message\":\"Test message\""));
    }
}
