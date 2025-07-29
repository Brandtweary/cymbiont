/**
 * @module api
 * @description HTTP API implementation for the PKM Knowledge Graph backend
 * 
 * This module consolidates all API-related functionality including type definitions,
 * request handlers, and router configuration. It serves as the HTTP interface between
 * the JavaScript Logseq plugin and the Rust backend server, handling all data ingestion,
 * synchronization, and logging operations.
 * 
 * ## Router Creation
 * The main HTTP router is created by the `create_router()` function (line ~226), 
 * which is called from main.rs to set up all API endpoints.
 * 
 * ## Multi-Graph Support Limitations
 * 
 * While the API accepts graph identification headers (X-Cymbiont-Graph-ID, 
 * X-Cymbiont-Graph-Name, X-Cymbiont-Graph-Path) with every request, the current
 * implementation assumes only ONE graph is active at a time. The graph headers
 * are used solely for:
 * - Detecting when the user switches to a different graph
 * - Registering new graphs as they connect
 * 
 * The system does NOT support parallel graph operations. All handlers operate on
 * the single active GraphManager instance in AppState. Future versions may support
 * true multi-graph parallelism, but the current architecture is designed for
 * sequential graph switching only.
 * 
 * ## API Types
 * 
 * - `ApiResponse`: Standard JSON response format
 *   - success: bool - Indicates operation success/failure
 *   - message: String - Human-readable status or error message
 * 
 * - `PKMData`: Incoming data wrapper from JavaScript plugin
 *   - source: String - Origin identifier (e.g., "PKM DB Change")
 *   - type_: Option<String> - Data type for routing ("block", "page", etc.)
 *   - payload: String - Serialized JSON data (parsed based on type)
 * 
 * - `LogMessage`: Frontend logging passthrough
 *   - level: String - Log level ("error", "warn", "info", "debug", "trace")
 *   - message: String - Log message text
 *   - source: Option<String> - Optional source identifier
 *   - details: Option<Value> - Additional structured data
 * 
 * ## Endpoints
 * 
 * ### GET /
 * Health check endpoint returning static string "PKM Knowledge Graph Backend Server".
 * Used by JavaScript plugin to verify server availability during startup.
 * 
 * ### POST /data
 * Main data ingestion endpoint handling multiple data types:
 * - "block": Single PKMBlockData - Creates/updates individual block node
 * - "blocks" or "block_batch": Vec<PKMBlockData> - Batch block processing
 * - "page": Single PKMPageData - Creates/updates page node
 * - "pages" or "page_batch": Vec<PKMPageData> - Batch page processing
 * - "plugin_initialized": Signal from JS plugin after successful load
 * - "sync_complete": Signal after full database sync completion
 * - null/other: Generic acknowledgment for real-time sync events
 * 
 * ### GET /sync/status
 * Returns current synchronization status:
 * ```json
 * {
 *   "last_full_sync": 1697815845000,            // Unix timestamp in ms or null
 *   "last_full_sync_iso": "2023-10-20T15:30:45Z", // ISO timestamp string or null
 *   "hours_since_sync": 2.5,                    // Float hours
 *   "full_sync_needed": false,                  // True if >2 hours or never
 *   "node_count": 1234,                         // Total graph nodes
 *   "reference_count": 5678                     // Total graph edges
 * }
 * ```
 * 
 * ### PATCH /sync
 * Updates the last full sync timestamp after successful database synchronization.
 * Called by JavaScript plugin every 2 hours after complete graph sync.
 * 
 * ### POST /sync/verify
 * Verifies PKM IDs and archives any nodes that no longer exist in the PKM.
 * Request body:
 * ```json
 * {
 *   "pages": ["Page1", "Page2", ...],    // All current page names
 *   "blocks": ["uuid1", "uuid2", ...]    // All current block UUIDs
 * }
 * ```
 * Archives deleted nodes to timestamped JSON files in archived_nodes/
 * 
 * ### POST /plugin/initialized
 * Plugin initialization endpoint called when the Logseq plugin starts up.
 * - Validates and registers the graph using headers (X-Cymbiont-Graph-ID, etc.)
 * - Returns the graph ID for the plugin to store in Logseq config
 * - Updates config.edn to hide the cymbiont-updated-ms property
 * 
 * ### POST /log
 * Receives log messages from JavaScript plugin and routes to Rust tracing system.
 * Maps JavaScript log levels to appropriate tracing macros. Source defaults to
 * "JS Plugin" if not specified.
 * 
 * ## Batch Processing
 * 
 * Batch endpoints optimize performance for bulk operations:
 * 1. Acquire single graph manager lock for entire batch
 * 2. Disable auto-save to prevent interleaved disk writes
 * 3. Process all items, tracking success/error counts
 * 4. Re-enable auto-save and force save if any successes
 * 5. Return detailed success/error statistics
 * 
 * ## Error Handling
 * 
 * All handlers return consistent ApiResponse with:
 * - success: false on any error
 * - message: Detailed error description
 * - HTTP 200 status (errors indicated in response body)
 * 
 * ## Helper Functions
 * 
 * - `parse_block_data()`: Deserializes PKMBlockData with validation
 * - `parse_page_data()`: Deserializes PKMPageData with validation
 * - `handle_block_data()`: Processes single block with graph update
 * - `handle_page_data()`: Processes single page with graph update
 * - `handle_batch_blocks()`: Optimized batch block processing
 * - `handle_batch_pages()`: Optimized batch page processing
 * - `handle_default_data()`: Generic data acknowledgment
 * 
 * All helpers follow consistent error propagation patterns, returning
 * Result<String, String> for success/error messages.
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
use crate::edn;
use crate::session_manager::DbIdentifier;

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

// Incoming data from the PKM plugin
#[derive(Deserialize, Debug)]
pub struct PKMData {
    pub source: String,
    #[serde(default)]
    pub type_: Option<String>,
    pub payload: String,
}

// Log message from frontend
#[derive(Deserialize, Debug)]
pub struct LogMessage {
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

// PKM ID verification request - sent after full sync to detect deletions
#[derive(Debug, Deserialize)]
pub struct PkmIdVerification {
    pub pages: Vec<String>,
    pub blocks: Vec<String>,
}

// Config validation request - sent by plugin to ensure properties are set
#[derive(Debug, Deserialize)]
pub struct ConfigValidationRequest {
    pub graph_id: String,
    #[allow(dead_code)]
    pub has_hidden_property: bool,
    #[allow(dead_code)]
    pub has_graph_id: bool,
}


// ===== Route Configuration =====

/// Create and configure the API router with graph validation middleware
pub fn create_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/data", post(receive_data))
        .route("/plugin/initialized", post(plugin_initialized))
        .route("/sync/status", get(get_sync_status))
        .route("/sync", patch(update_sync_timestamp))
        .route("/sync/verify", post(verify_pkm_ids))
        .route("/config/validate", post(validate_config))
        .route("/log", post(receive_log))
        .route("/ws", any(websocket_handler))
        // Session management endpoints
        .route("/api/session/switch", post(switch_database))
        .route("/api/session/current", get(get_current_session))
        .route("/api/session/databases", get(list_databases))
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

// Endpoint to get sync status
pub async fn get_sync_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Json(serde_json::json!({
            "error": "No active graph"
        }));
    }
    let graph_id = active_graph_id.unwrap();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => {
            return Json(serde_json::json!({
                "error": "Graph manager not found"
            }));
        }
    };
    
    let graph_manager = manager_lock.read().await;
    let mut status = graph_manager.get_sync_status(&state.config.sync);
    drop(graph_manager);
    drop(managers);
    
    // Add force sync flags to the response
    if let Some(obj) = status.as_object_mut() {
        // Override sync needed flags if force flags are set
        if state.force_full_sync {
            obj.insert("full_sync_needed".to_string(), serde_json::Value::Bool(true));
            obj.insert("incremental_sync_needed".to_string(), serde_json::Value::Bool(true));
            obj.insert("true_full_sync_needed".to_string(), serde_json::Value::Bool(true));
            obj.insert("force_full_sync".to_string(), serde_json::Value::Bool(true));
        } else if state.force_incremental_sync {
            obj.insert("full_sync_needed".to_string(), serde_json::Value::Bool(true));
            obj.insert("incremental_sync_needed".to_string(), serde_json::Value::Bool(true));
            obj.insert("force_incremental_sync".to_string(), serde_json::Value::Bool(true));
        }
    }
    
    Json(status)
}

// Request body for sync timestamp update
#[derive(Debug, Deserialize)]
pub struct UpdateSyncRequest {
    #[serde(default = "default_sync_type")]
    pub sync_type: String,
}

fn default_sync_type() -> String {
    "incremental".to_string()
}

// Endpoint to update sync timestamp after a full sync
pub async fn update_sync_timestamp(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateSyncRequest>,
) -> Json<ApiResponse> {
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Json(ApiResponse {
            success: false,
            message: "No active graph".to_string(),
            graph_id: None,
        });
    }
    let graph_id = active_graph_id.unwrap();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => {
            return Json(ApiResponse {
                success: false,
                message: "Graph manager not found".to_string(),
                graph_id: None,
            });
        }
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    let result = match request.sync_type.as_str() {
        "incremental" => {
            trace!("Updating incremental sync timestamp");
            graph_manager.update_incremental_sync_timestamp()
        },
        "full" => {
            trace!("Updating full sync timestamp");
            graph_manager.update_full_sync_timestamp()
        },
        _ => {
            error!("Invalid sync type: {}", request.sync_type);
            return Json(ApiResponse {
                success: false,
                message: format!("Invalid sync type: {}. Expected 'incremental' or 'full'", request.sync_type),
                graph_id: None,
            });
        }
    };
    
    match result {
        Ok(()) => {
            trace!("{} sync timestamp updated successfully", request.sync_type);
            Json(ApiResponse {
                success: true,
                message: format!("{} sync timestamp updated successfully", request.sync_type),
                graph_id: None,
            })
        },
        Err(e) => {
            error!("Error updating {} sync timestamp: {e:?}", request.sync_type);
            Json(ApiResponse {
                success: false,
                message: format!("Error updating {} sync timestamp: {e:?}", request.sync_type),
                graph_id: None,
            })
        }
    }
}

// Endpoint for plugin initialization
#[axum::debug_handler]
pub async fn plugin_initialized(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse> {
    info!("🔌 Plugin initialization confirmed");
    
    // Get the active graph ID that was set by middleware
    let graph_id = state.get_active_graph_manager().await;
    
    // Log the active graph path if available
    if let Some(ref active_id) = graph_id {
        let actual_path = {
            let registry = state.graph_registry.lock().unwrap();
            registry.get_graph(active_id).map(|info| info.path.clone())
        };
        
        if let Some(path) = actual_path {
            info!("Active graph path: {}", path);
        }
    }
    
    // Signal plugin initialization if we have a waiting channel
    if let Ok(mut tx_guard) = state.plugin_init_tx.lock() {
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(());
        }
    }
    
    Json(ApiResponse {
        success: true,
        message: "Plugin initialization acknowledged".to_string(),
        graph_id,
    })
}

// Endpoint to receive log messages from the frontend
pub async fn receive_log(
    State(_state): State<Arc<AppState>>,
    Json(log): Json<LogMessage>,
) -> Json<ApiResponse> {
    let source = log.source.as_deref().unwrap_or("JS Plugin");
    
    // Convert JS log level to Rust tracing level and log appropriately
    match log.level.to_lowercase().as_str() {
        "error" => {
            if let Some(details) = &log.details {
                error!("[{}] {}: {:?}", source, log.message, details);
            } else {
                error!("[{}] {}", source, log.message);
            }
        },
        "warn" => {
            if let Some(details) = &log.details {
                warn!("[{}] {}: {:?}", source, log.message, details);
            } else {
                warn!("[{}] {}", source, log.message);
            }
        },
        "info" => {
            if let Some(details) = &log.details {
                info!("[{}] {}: {:?}", source, log.message, details);
            } else {
                info!("[{}] {}", source, log.message);
            }
        },
        "debug" => {
            if let Some(details) = &log.details {
                debug!("[{}] {}: {:?}", source, log.message, details);
            } else {
                debug!("[{}] {}", source, log.message);
            }
        },
        "trace" => {
            if let Some(details) = &log.details {
                trace!("[{}] {}: {:?}", source, log.message, details);
            } else {
                trace!("[{}] {}", source, log.message);
            }
        },
        _ => {
            // Default to info for unknown levels
            if let Some(details) = &log.details {
                info!("[{}] {}: {:?}", source, log.message, details);
            } else {
                info!("[{}] {}", source, log.message);
            }
        }
    }
    
    Json(ApiResponse {
        success: true,
        message: "Log received".to_string(),
        graph_id: None,
    })
}

// Endpoint to verify PKM IDs and detect deletions
pub async fn verify_pkm_ids(
    State(state): State<Arc<AppState>>,
    Json(verification): Json<PkmIdVerification>,
) -> Json<ApiResponse> {
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Json(ApiResponse {
            success: false,
            message: "No active graph".to_string(),
            graph_id: None,
        });
    }
    let graph_id = active_graph_id.unwrap();
    
    // Get the graph manager for active graph
    let managers = state.graph_managers.read().await;
    let manager_lock = match managers.get(&graph_id) {
        Some(m) => m,
        None => {
            return Json(ApiResponse {
                success: false,
                message: "Graph manager not found".to_string(),
                graph_id: None,
            });
        }
    };
    
    let mut graph_manager = manager_lock.write().await;
    
    match graph_manager.verify_and_archive_missing_nodes(&verification.pages, &verification.blocks) {
        Ok((archived_count, message)) => {
            if archived_count > 0 {
                // During development, archival events are rare and worth noticing
                // In production, consider moving this to trace level if it becomes noisy
                debug!("🗄️ Archived {} nodes: {}", archived_count, message);
            } else {
                // No archival needed - this is the common case
                trace!("No nodes to archive");
            }
            Json(ApiResponse {
                success: true,
                message,
                graph_id: None,
            })
        },
        Err(e) => {
            error!("Error during PKM ID verification: {:?}", e);
            Json(ApiResponse {
                success: false,
                message: format!("Error during verification: {}", e),
                graph_id: None,
            })
        }
    }
}

// Endpoint to validate and fix config.edn properties
pub async fn validate_config(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ConfigValidationRequest>,
) -> Json<ApiResponse> {
    debug!("validate_config handler reached with request: {:?}", request);
    
    // Get active graph
    let active_graph_id = state.get_active_graph_manager().await;
    if active_graph_id.is_none() {
        return Json(ApiResponse {
            success: false,
            message: "No active graph".to_string(),
            graph_id: None,
        });
    }
    let graph_id = active_graph_id.unwrap();
    
    // Verify the request graph ID matches the active graph (security check)
    if request.graph_id != graph_id {
        error!("Config validation request for wrong graph: {} vs active {}", request.graph_id, graph_id);
        return Json(ApiResponse {
            success: false,
            message: "Graph ID mismatch".to_string(),
            graph_id: None,
        });
    }
    
    // Get graph info from registry
    let graph_path = {
        let registry = state.graph_registry.lock().unwrap();
        match registry.get_graph(&graph_id) {
            Some(info) => info.path.clone(),
            None => {
                return Json(ApiResponse {
                    success: false,
                    message: "Graph not found in registry".to_string(),
                    graph_id: None,
                });
            }
        }
    };
    
    // Build path to config.edn
    let config_path = std::path::PathBuf::from(&graph_path)
        .join("logseq")
        .join("config.edn");
    
    // Update config if needed
    match edn::update_config_file(&config_path, &graph_id) {
        Ok(()) => {
            // Mark config as updated in registry
            {
                let mut registry = state.graph_registry.lock().unwrap();
                if let Err(e) = registry.mark_config_updated(&graph_id) {
                    error!("Failed to mark config as updated in registry: {}", e);
                }
            }
            
            info!("✅ Config validation successful for graph {}", graph_id);
            Json(ApiResponse {
                success: true,
                message: "Config validated and updated".to_string(),
                graph_id: None,
            })
        }
        Err(e) => {
            error!("Failed to validate/update config for graph {}: {}", graph_id, e);
            Json(ApiResponse {
                success: false,
                message: format!("Config validation failed: {}", e),
                graph_id: None,
            })
        }
    }
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
    if path == "/" || path == "/ws" || path == "/log" {
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

// ===== Session Management API =====

// Request types for session endpoints
#[derive(Deserialize)]
#[serde(untagged)]
pub enum SwitchDatabaseRequest {
    ByName { name: String },
    ByPath { path: String },
}

#[derive(Serialize)]
pub struct DatabaseInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Switch to a different database
#[axum::debug_handler]
pub async fn switch_database(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SwitchDatabaseRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<ApiResponse>)> {
    let identifier = match request {
        SwitchDatabaseRequest::ByName { name } => DbIdentifier::Name(name),
        SwitchDatabaseRequest::ByPath { path } => DbIdentifier::Path(path),
    };
    
    match state.session_manager.switch_database_with_notifier(identifier, &state).await {
        Ok(_) => Ok(Json(ApiResponse {
            success: true,
            message: "Database switched successfully".to_string(),
            graph_id: None,
        })),
        Err(e) => {
            error!("Failed to switch database: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to switch database: {}", e),
                    graph_id: None,
                }),
            ))
        }
    }
}

/// Get current session information
pub async fn get_current_session(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let session_info = state.session_manager.get_session_info().await;
    
    Json(serde_json::json!({
        "session_state": session_info.state,
        "active_graph_id": session_info.active_graph_id,
        "active_graph_name": session_info.active_graph_name,
        "active_graph_path": session_info.active_graph_path,
    }))
}

/// List all available databases
pub async fn list_databases(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<DatabaseInfo>> {
    let databases = if let Ok(registry) = state.graph_registry.lock() {
        registry.get_all_graphs()
            .into_iter()
            .map(|info| DatabaseInfo {
                id: info.id.clone(),
                name: info.name.clone(),
                path: info.path.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    
    Json(databases)
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
    fn test_log_message_deserialization() {
        let json = r#"{
            "level": "info",
            "message": "Test log",
            "source": "test",
            "details": {"key": "value"}
        }"#;
        
        let log: LogMessage = serde_json::from_str(json).unwrap();
        assert_eq!(log.level, "info");
        assert_eq!(log.message, "Test log");
        assert_eq!(log.source, Some("test".to_string()));
        assert!(log.details.is_some());
    }

    #[test]
    fn test_log_message_minimal() {
        let json = r#"{
            "level": "error",
            "message": "Error occurred"
        }"#;
        
        let log: LogMessage = serde_json::from_str(json).unwrap();
        assert_eq!(log.level, "error");
        assert_eq!(log.message, "Error occurred");
        assert_eq!(log.source, None);
        assert_eq!(log.details, None);
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

    #[test]
    fn test_update_sync_request_default() {
        let json = r#"{}"#;
        let request: UpdateSyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.sync_type, "incremental");
    }

    #[test]
    fn test_pkm_id_verification_deserialization() {
        let json = r#"{
            "pages": ["Page1", "Page2"],
            "blocks": ["block-123", "block-456"]
        }"#;
        
        let verification: PkmIdVerification = serde_json::from_str(json).unwrap();
        assert_eq!(verification.pages.len(), 2);
        assert_eq!(verification.blocks.len(), 2);
        assert_eq!(verification.pages[0], "Page1");
        assert_eq!(verification.blocks[1], "block-456");
    }
}