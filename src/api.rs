/**
 * @module api
 * @description HTTP API implementation for the Cymbiont Knowledge Graph backend
 * 
 * This module provides core API functionality including data ingestion endpoints,
 * WebSocket support, and multi-graph validation middleware. It serves as the HTTP
 * interface for the terminal-first Cymbiont architecture.
 * 
 * ## Router Creation
 * The main HTTP router is created by the `create_router()` function,
 * which is called from main.rs to set up all API endpoints.
 * 
 * ## Multi-Graph Support
 * 
 * The API accepts graph identification headers (X-Cymbiont-Graph-ID, 
 * X-Cymbiont-Graph-Name, X-Cymbiont-Graph-Path) with every request. The current
 * implementation assumes only ONE graph is active at a time. The graph headers
 * are used for:
 * - Detecting when the user switches to a different graph
 * - Registering new graphs as they connect
 * 
 * ## API Types
 * 
 * - `ApiResponse`: Standard JSON response format
 *   - success: bool - Indicates operation success/failure
 *   - message: String - Human-readable status or error message
 *   - graph_id: Option<String> - Graph ID when relevant
 * 
 * - `PKMData`: Incoming data wrapper
 *   - source: String - Origin identifier
 *   - type_: Option<String> - Data type for routing ("block", "page", etc.)
 *   - payload: String - Serialized JSON data (parsed based on type)
 * 
 * ## Endpoints
 * 
 * ### GET /
 * Health check endpoint returning static string "PKM Knowledge Graph Backend Server".
 * 
 * ### POST /data
 * Main data ingestion endpoint handling multiple data types:
 * - "block": Single PKMBlockData - Creates/updates individual block node
 * - "blocks" or "block_batch": Vec<PKMBlockData> - Batch block processing
 * - "page": Single PKMPageData - Creates/updates page node
 * - "pages" or "page_batch": Vec<PKMPageData> - Batch page processing
 * - "sync_complete": Signal after sync completion
 * - null/other: Generic acknowledgment for events
 * 
 * ### GET /api/websocket/status
 * Returns WebSocket connection metrics.
 * 
 * ### GET /api/websocket/recent-activity
 * Returns recent WebSocket activity (placeholder for future implementation).
 * 
 * ## Batch Processing
 * 
 * Batch endpoints optimize performance for bulk operations:
 * 1. Acquire single graph manager lock for entire batch
 * 2. Disable auto-save to prevent interleaved disk writes
 * 3. Process all items, tracking success/error counts
 * 4. Re-enable auto-save and force save if any successes
 * 5. Return detailed success/error statistics
 */

use axum::{
    extract::{State, Request}, 
    Json, 
    Router, 
    routing::{get, post, patch, any},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    body::Body,
};
use std::sync::Arc;
use tracing::{info, warn, error, debug, trace};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::AppState;
use crate::pkm_data::{PKMBlockData, PKMPageData};
use crate::utils::parse_json_data;
use crate::websocket::websocket_handler;

// ===== API Types =====

// Graph context extracted from headers
#[derive(Debug, Clone)]
pub struct GraphContext {
    pub graph_id: Option<String>,
    pub graph_name: Option<String>,
    pub graph_path: Option<String>,
}

impl GraphContext {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            graph_id: headers.get("x-cymbiont-graph-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            graph_name: headers.get("x-cymbiont-graph-name")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            graph_path: headers.get("x-cymbiont-graph-path")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
        }
    }
}

// Basic response for API calls
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_id: Option<String>,
}

// Incoming data wrapper
#[derive(Deserialize, Debug)]
pub struct PKMData {
    pub source: String,
    #[serde(default)]
    pub type_: Option<String>,
    pub payload: String,
}



// ===== Route Configuration =====

/// Create and configure the API router with graph validation middleware
pub fn create_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/data", post(receive_data))
        .route("/ws", any(websocket_handler))
        // WebSocket status endpoints
        .route("/api/websocket/status", get(get_websocket_status))
        .route("/api/websocket/recent-activity", get(get_websocket_activity))
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            graph_validation_middleware,
        ))
        .with_state(app_state)
}

// ===== Handlers =====

// Root endpoint
pub async fn root() -> &'static str {
    "PKM Knowledge Graph Backend Server"
}

// Endpoint to receive data from the PKM plugin
pub async fn receive_data(
    State(state): State<Arc<AppState>>,
    Json(data): Json<PKMData>,
) -> Json<ApiResponse> {
    // Process based on the type of data
    match data.type_.as_deref() {
        Some("block") => {
            match handle_block_data(state, &data.payload).await {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                        graph_id: None,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                        graph_id: None,
                    })
                }
            }
        },
        Some("block_batch") | Some("blocks") => {
            match handle_batch_blocks(state, &data.payload).await {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                        graph_id: None,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                        graph_id: None,
                    })
                }
            }
        },
        Some("page") => {
            match handle_page_data(state, &data.payload).await {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                        graph_id: None,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                        graph_id: None,
                    })
                }
            }
        },
        Some("page_batch") | Some("pages") => {
            match handle_batch_pages(state, &data.payload).await {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                        graph_id: None,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                        graph_id: None,
                    })
                }
            }
        },
        Some("sync_complete") => {
            // Signal sync completion if we have a waiting channel
            if let Ok(mut tx_guard) = state.sync_complete_tx.lock() {
                if let Some(tx) = tx_guard.take() {
                    let _ = tx.send(());
                    debug!("Sync completion signal received");
                }
            }
            
            Json(ApiResponse {
                success: true,
                message: "Sync completion acknowledged".to_string(),
                graph_id: None,
            })
        },
        // For DB change events and other unspecified types
        _ => {
            match handle_default_data(&data.source) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                        graph_id: None,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                        graph_id: None,
                    })
                }
            }
        }
    }
}

// ===== Helper Functions =====

// Helper functions for data parsing
fn parse_block_data(payload: &str) -> Result<PKMBlockData, serde_json::Error> {
    parse_json_data::<PKMBlockData>(payload)
}

fn parse_page_data(payload: &str) -> Result<PKMPageData, serde_json::Error> {
    parse_json_data::<PKMPageData>(payload)
}

// Helper function for handling block data
async fn handle_block_data(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as a PKMBlockData
    let block_data = parse_block_data(payload)
        .map_err(|e| format!("Could not parse block data: {e}"))?;
    
    // Validate the block data
    if block_data.id.is_empty() {
        return Err("Block ID is empty".to_string());
    }
    
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Err("No active graph".to_string());
    }
    let graph_id = active_graph_id.unwrap();
    
    // Check if this content is already being processed by a transaction
    let content_hash = compute_content_hash(&block_data.content);
    let coordinators = state.transaction_coordinators.read().await;
    if let Some(coordinator) = coordinators.get(&graph_id) {
        if coordinator.is_content_pending(&content_hash).await {
            trace!("Skipping block {} - content already being processed by transaction", block_data.id);
            return Ok("Block skipped - duplicate content".to_string());
        }
    }
    drop(coordinators);
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => return Err("Graph manager not found".to_string()),
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    match graph_manager.create_or_update_node_from_pkm_block(&block_data) {
        Ok(_node_idx) => {
            // Block processed successfully
            // Note: GraphManager already saves periodically
            drop(graph_manager);
            Ok("Block processed successfully".to_string())
        },
        Err(e) => {
            drop(graph_manager);
            Err(format!("Error processing block: {e:?}"))
        }
    }
}

// Helper function for handling page data
async fn handle_page_data(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as a PKMPageData
    let page_data = parse_page_data(payload)
        .map_err(|e| format!("Could not parse page data: {e}"))?;
    
    // Validate the page data
    if page_data.name.is_empty() {
        return Err("Page name is empty".to_string());
    }
    
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Err("No active graph".to_string());
    }
    let graph_id = active_graph_id.unwrap();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => return Err("Graph manager not found".to_string()),
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    match graph_manager.create_or_update_node_from_pkm_page(&page_data) {
        Ok(_node_idx) => {
            // Page processed successfully
            // Note: GraphManager already saves periodically
            drop(graph_manager);
            Ok("Page processed successfully".to_string())
        },
        Err(e) => {
            drop(graph_manager);
            Err(format!("Error processing page: {e:?}"))
        }
    }
}

// Helper function for handling default data
fn handle_default_data(source: &str) -> Result<String, String> {
    // For DB changes, just acknowledge receipt without verbose logging
    if source == "PKM DB Change" {
        // Minimal logging for DB changes
        // Processing DB change event
    } else {
        // Processing data with unspecified type
    }
    
    Ok("Data received".to_string())
}

// Helper function for handling batch block data
async fn handle_batch_blocks(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as an array of PKMBlockData
    let blocks: Vec<PKMBlockData> = parse_json_data(payload)
        .map_err(|e| format!("Could not parse batch blocks: {e}"))?;
    
    debug!("📦 Processing batch of {} blocks", blocks.len());
    
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Err("No active graph".to_string());
    }
    let graph_id = active_graph_id.unwrap();
    
    let mut success_count = 0;
    let mut error_count = 0;
    let total_blocks = blocks.len();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => return Err("Graph manager not found".to_string()),
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    // Disable auto-save during batch processing to avoid interleaved saves
    graph_manager.disable_auto_save();
    
    for block_data in blocks {
        // Validate and process each block
        if block_data.validate().is_ok() {
            match graph_manager.create_or_update_node_from_pkm_block(&block_data) {
                Ok(_) => {
                    success_count += 1;
                },
                Err(_) => {
                    error_count += 1;
                }
            }
        } else {
            error_count += 1;
        }
    }
    
    // Re-enable auto-save and force save after batch
    graph_manager.enable_auto_save();
    if success_count > 0 {
        if let Err(e) = graph_manager.save_graph() {
            error!("Error saving graph after batch processing: {e:?}");
        }
    }
    
    // Release the lock
    drop(graph_manager);
    
    // Report results
    if error_count == 0 {
        Ok(format!("Successfully processed all {total_blocks} blocks"))
    } else if success_count > 0 {
        Ok(format!("Processed {success_count}/{total_blocks} blocks successfully, {error_count} errors"))
    } else {
        Err(format!("Failed to process any blocks, {error_count} errors"))
    }
}

// Helper function for handling batch page data
async fn handle_batch_pages(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as an array of PKMPageData
    let pages: Vec<PKMPageData> = parse_json_data(payload)
        .map_err(|e| format!("Could not parse batch pages: {e}"))?;
    
    debug!("📦 Processing batch of {} pages", pages.len());
    
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Err("No active graph".to_string());
    }
    let graph_id = active_graph_id.unwrap();
    
    let mut success_count = 0;
    let mut error_count = 0;
    let total_pages = pages.len();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => return Err("Graph manager not found".to_string()),
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    // Disable auto-save during batch processing to avoid interleaved saves
    graph_manager.disable_auto_save();
    
    for page_data in pages {
        // Validate and process each page
        if page_data.validate().is_ok() {
            match graph_manager.create_or_update_node_from_pkm_page(&page_data) {
                Ok(_) => {
                    success_count += 1;
                },
                Err(_) => {
                    error_count += 1;
                }
            }
        } else {
            error_count += 1;
        }
    }
    
    // Re-enable auto-save and force save after batch
    graph_manager.enable_auto_save();
    if success_count > 0 {
        if let Err(e) = graph_manager.save_graph() {
            error!("Error saving graph after batch processing: {e:?}");
        }
    }
    
    // Release the lock
    drop(graph_manager);
    
    // Report results
    if error_count == 0 {
        Ok(format!("Successfully processed all {total_pages} pages"))
    } else if success_count > 0 {
        Ok(format!("Processed {success_count}/{total_pages} pages successfully, {error_count} errors"))
    } else {
        Err(format!("Failed to process any pages, {error_count} errors"))
    }
}

// Helper function to compute content hash
fn compute_content_hash(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Middleware to validate and switch graphs based on request headers
async fn graph_validation_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // Extract graph context from headers and path before any await
    let graph_context = GraphContext::from_headers(req.headers());
    let path = req.uri().path().to_string();
    
    // Only log non-heartbeat requests at trace level
    if path != "/" {
        trace!("Middleware processing request to {} with graph context: {:?}", path, graph_context);
    }
    
    // Skip validation for certain endpoints
    if path == "/" || path == "/ws" {
        return next.run(req).await;
    }
    
    // If we have graph information, validate and switch
    if graph_context.graph_name.is_some() || graph_context.graph_path.is_some() || graph_context.graph_id.is_some() {
        // Validate and switch graph
        let (graph_info, is_new) = {
            let mut registry = state.graph_registry.lock().unwrap();
            trace!("Calling validate_and_switch with name={:?}, path={:?}, id={:?}", 
                  graph_context.graph_name.as_deref(),
                  graph_context.graph_path.as_deref(),
                  graph_context.graph_id.as_deref());
            let data_dir = std::path::Path::new(&state.config.data_dir);
            match registry.validate_and_switch(
                graph_context.graph_name.as_deref(),
                graph_context.graph_path.as_deref(),
                graph_context.graph_id.as_deref(),
                data_dir,
            ) {
                Ok(result) => result,
                Err(e) => {
                    error!("Graph validation failed: {}", e);
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::empty())
                        .unwrap();
                }
            }
        }; // Drop the lock here before any await
        
        if is_new {
            info!("🆕 Registered new graph: {} ({})", graph_info.name, graph_info.id);
        }
        
        // Check if we need to switch active graph
        let current_active = state.get_active_graph_manager().await;
                if current_active.as_ref() != Some(&graph_info.id) {
                    // Save current graph if switching
                    if let Some(current_id) = current_active {
                        let managers = state.graph_managers.read().await;
                        if let Some(manager_lock) = managers.get(&current_id) {
                            let mut manager = manager_lock.write().await;
                            if let Err(e) = manager.save_graph() {
                                error!("Failed to save graph {} before switching: {}", current_id, e);
                            }
                        }
                    }
                    
                    info!("📊 Switching to graph: {} ({})", graph_info.name, graph_info.id);
                    
                    // Ensure graph manager exists
                    if let Err(e) = state.get_or_create_graph_manager(&graph_info.id).await {
                        error!("Failed to create graph manager: {}", e);
                        return Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::empty())
                            .unwrap();
                    }
                    
                    // Set active graph
                    state.set_active_graph(graph_info.id.clone()).await;
                }
    }
    
    next.run(req).await
}

/// Get WebSocket connection status
pub async fn get_websocket_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let connections = if let Some(ws_connections) = &state.ws_connections {
        let conns = ws_connections.read().await;
        conns.len()
    } else {
        0
    };
    
    // Get active graph for context
    let active_graph_id = state.get_active_graph_manager().await;
    
    Json(serde_json::json!({
        "connected": connections > 0,
        "connection_count": connections,
        "active_graph_id": active_graph_id,
        // TODO: Add more detailed connection info when needed
    }))
}

/// Get recent WebSocket activity
pub async fn get_websocket_activity(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    // TODO: Implement proper activity tracking in WebSocket module
    // For now, return basic connection info
    let active_connections = if let Some(ws_connections) = &state.ws_connections {
        let conns = ws_connections.read().await;
        conns.values()
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

    #[test]
    fn test_pkm_data_deserialization() {
        let json = r#"{
            "source": "test",
            "type_": "block",
            "payload": "{\"id\":\"123\"}"
        }"#;
        
        let data: PKMData = serde_json::from_str(json).unwrap();
        assert_eq!(data.source, "test");
        assert_eq!(data.type_, Some("block".to_string()));
        assert_eq!(data.payload, "{\"id\":\"123\"}");
    }

    #[test]
    fn test_pkm_data_optional_type() {
        let json = r#"{
            "source": "test",
            "payload": "data"
        }"#;
        
        let data: PKMData = serde_json::from_str(json).unwrap();
        assert_eq!(data.type_, None);
    }

    #[test]
    fn test_graph_context_from_headers() {
        use axum::http::HeaderMap;
        
        let mut headers = HeaderMap::new();
        headers.insert("x-cymbiont-graph-id", "test-id-123".parse().unwrap());
        headers.insert("x-cymbiont-graph-name", "TestGraph".parse().unwrap());
        headers.insert("x-cymbiont-graph-path", "/path/to/graph".parse().unwrap());
        
        let context = GraphContext::from_headers(&headers);
        assert_eq!(context.graph_id, Some("test-id-123".to_string()));
        assert_eq!(context.graph_name, Some("TestGraph".to_string()));
        assert_eq!(context.graph_path, Some("/path/to/graph".to_string()));
    }

    #[test]
    fn test_graph_context_partial_headers() {
        use axum::http::HeaderMap;
        
        let mut headers = HeaderMap::new();
        headers.insert("x-cymbiont-graph-name", "TestGraph".parse().unwrap());
        
        let context = GraphContext::from_headers(&headers);
        assert_eq!(context.graph_id, None);
        assert_eq!(context.graph_name, Some("TestGraph".to_string()));
        assert_eq!(context.graph_path, None);
    }
}