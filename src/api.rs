/**
 * @module api
 * @description HTTP API implementation for the PKM Knowledge Graph backend
 * 
 * This module consolidates all API-related functionality including type definitions,
 * request handlers, and router configuration. It serves as the HTTP interface between
 * the JavaScript Logseq plugin and the Rust backend server, handling all data ingestion,
 * synchronization, and logging operations.
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

use axum::{extract::State, Json, Router, routing::{get, post, patch, any}};
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

// Basic response for API calls
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
}

// Incoming data from the PKM plugin
#[derive(Deserialize, Debug)]
pub struct PKMData {
    pub source: String,
    // #[serde(rename = "graphName")]
    // graph_name: String,
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

// ===== Route Configuration =====

/// Create and configure the API router
pub fn create_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/data", post(receive_data))
        .route("/sync/status", get(get_sync_status))
        .route("/sync", patch(update_sync_timestamp))
        .route("/sync/verify", post(verify_pkm_ids))
        .route("/log", post(receive_log))
        .route("/ws", any(websocket_handler))
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
    let graph_manager = state.graph_manager.lock().unwrap();
    let mut status = graph_manager.get_sync_status(&state.config.sync);
    drop(graph_manager);
    
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
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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
            });
        }
    };
    
    match result {
        Ok(()) => {
            trace!("{} sync timestamp updated successfully", request.sync_type);
            Json(ApiResponse {
                success: true,
                message: format!("{} sync timestamp updated successfully", request.sync_type),
            })
        },
        Err(e) => {
            error!("Error updating {} sync timestamp: {e:?}", request.sync_type);
            Json(ApiResponse {
                success: false,
                message: format!("Error updating {} sync timestamp: {e:?}", request.sync_type),
            })
        }
    }
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
    })
}

// Endpoint to verify PKM IDs and detect deletions
pub async fn verify_pkm_ids(
    State(state): State<Arc<AppState>>,
    Json(verification): Json<PkmIdVerification>,
) -> Json<ApiResponse> {
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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
            })
        },
        Err(e) => {
            error!("Error during PKM ID verification: {:?}", e);
            Json(ApiResponse {
                success: false,
                message: format!("Error during verification: {}", e),
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
            match handle_block_data(state, &data.payload) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                    })
                }
            }
        },
        Some("block_batch") | Some("blocks") => {
            match handle_batch_blocks(state, &data.payload) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                    })
                }
            }
        },
        Some("page") => {
            match handle_page_data(state, &data.payload) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                    })
                }
            }
        },
        Some("page_batch") | Some("pages") => {
            match handle_batch_pages(state, &data.payload) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
                    })
                }
            }
        },
        Some("plugin_initialized") => {
            info!("🔌 Plugin initialization confirmed");
            // Signal plugin initialization if we have a waiting channel
            if let Ok(mut tx_guard) = state.plugin_init_tx.lock() {
                if let Some(tx) = tx_guard.take() {
                    let _ = tx.send(());
                }
            }
            
            Json(ApiResponse {
                success: true,
                message: "Plugin initialization acknowledged".to_string(),
            })
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
            })
        },
        // For DB change events and other unspecified types
        _ => {
            match handle_default_data(&data.source) {
                Ok(message) => {
                    Json(ApiResponse {
                        success: true,
                        message,
                    })
                },
                Err(message) => {
                    Json(ApiResponse {
                        success: false,
                        message,
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
fn handle_block_data(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as a PKMBlockData
    let block_data = parse_block_data(payload)
        .map_err(|e| format!("Could not parse block data: {e}"))?;
    
    // Validate the block data
    if block_data.id.is_empty() {
        return Err("Block ID is empty".to_string());
    }
    
    // Check if this content is already being processed by a transaction
    // This prevents race conditions where LLM creates content, sends to Logseq,
    // and then we receive it back via real-time sync
    let content_hash = compute_content_hash(&block_data.content);
    let is_pending = tokio::task::block_in_place(|| {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            state.transaction_coordinator.is_content_pending(&content_hash).await
        })
    });
    
    if is_pending {
        trace!("Skipping block {} - content already being processed by transaction", block_data.id);
        return Ok("Block skipped - duplicate content".to_string());
    }
    
    // Process the block data
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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
fn handle_page_data(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as a PKMPageData
    let page_data = parse_page_data(payload)
        .map_err(|e| format!("Could not parse page data: {e}"))?;
    
    // Validate the page data
    if page_data.name.is_empty() {
        return Err("Page name is empty".to_string());
    }
    
    // Process the page data
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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
fn handle_batch_blocks(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as an array of PKMBlockData
    let blocks: Vec<PKMBlockData> = parse_json_data(payload)
        .map_err(|e| format!("Could not parse batch blocks: {e}"))?;
    
    debug!("📦 Processing batch of {} blocks", blocks.len());
    
    let mut success_count = 0;
    let mut error_count = 0;
    let total_blocks = blocks.len();
    
    // Get a single lock on the graph for the entire batch
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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
fn handle_batch_pages(state: Arc<AppState>, payload: &str) -> Result<String, String> {
    // Parse the payload as an array of PKMPageData
    let pages: Vec<PKMPageData> = parse_json_data(payload)
        .map_err(|e| format!("Could not parse batch pages: {e}"))?;
    
    debug!("📦 Processing batch of {} pages", pages.len());
    
    let mut success_count = 0;
    let mut error_count = 0;
    let total_pages = pages.len();
    
    // Get a single lock on the graph for the entire batch
    let mut graph_manager = state.graph_manager.lock().unwrap();
    
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_response_serialization() {
        let response = ApiResponse {
            success: true,
            message: "Test message".to_string(),
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
}