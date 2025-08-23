//! @module server
//! @description Server lifecycle management and HTTP listener setup
//! 
//! This module provides the core server infrastructure for Cymbiont, handling
//! port discovery, TCP listener creation, and Axum server initialization.
//! It delegates runtime lifecycle concerns (signal handling, shutdown) to main.rs.
//! 
//! ## Responsibilities
//! 
//! ### Port Management
//! - Attempts to bind to configured base port (default: 8888)
//! - Automatically searches for available ports up to max_port_attempts
//! - Writes server info file for process discovery and management
//! 
//! ### Previous Instance Handling
//! - Detects existing server via server_info_file
//! - Terminates previous instances gracefully (SIGTERM)
//! - Cleans up stale server info files
//! 
//! ### Server Creation
//! - Creates Axum router with all HTTP/WebSocket routes
//! - Binds TCP listener to discovered port
//! - Spawns server task and returns handle for external control
//! 
//! ## Multi-Instance Support
//! 
//! The server supports multiple concurrent instances through configurable
//! server_info_file paths. Each instance writes its PID and port to a
//! unique file, enabling independent process management.
//! 
//! ## Integration
//! 
//! The `start_server()` function returns a JoinHandle that main.rs uses
//! to manage the server lifecycle. This separation allows main.rs to
//! coordinate shutdown across both CLI and server modes uniformly.
//! 
//! ## Error Handling
//! 
//! - Port binding failures trigger automatic port search
//! - Previous instance termination failures are logged but non-fatal
//! - Listener creation failures are fatal and propagate to caller
//! 
//! ## Configuration
//! 
//! Server behavior controlled via BackendConfig:
//! - `port`: Base port to attempt binding (default: 8888)
//! - `max_port_attempts`: Number of ports to try (default: 10)
//! - `server_info_file`: Path for process discovery file

use std::net::SocketAddr;
use tracing::info;

use crate::error::*;
use crate::{
    AppState,
    utils::{write_server_info, find_available_port, terminate_previous_instance},
    server::http_api::create_router,
};

/// Start the HTTP server and return it for external lifecycle management
/// The caller is responsible for handling shutdown signals and cleanup
pub async fn start_server(
    app_state: std::sync::Arc<AppState>,
) -> Result<(tokio::task::JoinHandle<std::result::Result<(), std::io::Error>>, String)> {
    let server_info_file = &app_state.config.backend.server_info_file;
    
    // Terminate any previous instance
    if std::fs::metadata(server_info_file).is_ok() {
        terminate_previous_instance(server_info_file);
        let _ = std::fs::remove_file(server_info_file);
    }
    
    // Create and start server
    let app = create_router(app_state.clone());
    let port = find_available_port(&app_state.config.backend)
        .map_err(|e| ServerError::port_binding(e.to_string()))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    write_server_info("127.0.0.1", port, server_info_file)
        .map_err(|e| ServerError::startup(format!("Failed to write server info: {}", e)))?;
    
    let listener = tokio::net::TcpListener::bind(addr).await
        .map_err(|e| ServerError::port_binding(format!("Failed to bind to port {}: {}", port, e)))?;
    
    info!("🚀 Cymbiont Server listening on {}", addr);
    
    // Spawn the server task and return handle
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await
    });
    
    Ok((server_handle, server_info_file.to_string()))
}

/// Clean up server info file
pub fn cleanup_server_info(filename: &str) {
    let _ = std::fs::remove_file(filename);
}