//! Server Configuration and Setup
//! 
//! This module handles server-specific concerns like HTTP listener setup,
//! port binding, and axum server creation. Runtime lifecycle management
//! (signal handling, graceful shutdown) should be handled by main.rs.

use std::net::SocketAddr;
use std::error::Error;
use tracing::info;

use crate::{
    AppState,
    utils::{write_server_info, find_available_port, terminate_previous_instance},
    server::http_api::create_router,
};

/// Start the HTTP server and return it for external lifecycle management
/// The caller is responsible for handling shutdown signals and cleanup
pub async fn start_server(
    app_state: std::sync::Arc<AppState>,
) -> Result<(tokio::task::JoinHandle<Result<(), std::io::Error>>, String), Box<dyn Error + Send + Sync>> {
    let server_info_file = &app_state.config.backend.server_info_file;
    
    // Terminate any previous instance
    if std::fs::metadata(server_info_file).is_ok() {
        terminate_previous_instance(server_info_file);
        let _ = std::fs::remove_file(server_info_file);
    }
    
    // Create and start server
    let app = create_router(app_state.clone());
    let port = find_available_port(&app_state.config.backend)
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e.to_string()))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    write_server_info("127.0.0.1", port, server_info_file)
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e.to_string()))?;
    
    let listener = tokio::net::TcpListener::bind(addr).await
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Listener error: {e}")))?;
    
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