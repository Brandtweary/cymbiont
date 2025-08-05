//! Server Configuration and Setup
//! 
//! This module handles server-specific concerns like HTTP listener setup,
//! port binding, and axum server creation. Runtime lifecycle management
//! (signal handling, graceful shutdown) should be handled by main.rs.

use std::net::SocketAddr;
use std::error::Error;
use std::time::{Duration, Instant};
use tracing::{info, error};

use crate::{
    AppState,
    utils::{write_server_info, find_available_port, terminate_previous_instance},
    server::http_api::create_router,
};

/// Run server with all the necessary setup and teardown
pub async fn run_server_with_duration(
    app_state: std::sync::Arc<AppState>,
    duration: Option<u64>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let server_info_file = &app_state.config.backend.server_info_file;
    
    // Terminate any previous instance
    if std::fs::metadata(server_info_file).is_ok() {
        terminate_previous_instance(server_info_file);
        let _ = std::fs::remove_file(server_info_file);
    }
    
    // No signal handler needed - we'll handle shutdown in the main async context
    
    // Determine duration
    let duration_secs = duration.or(app_state.config.development.default_duration);
    
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
    
    // NOW start the duration timer - server is ready to serve requests
    let start_time = Instant::now();
    info!("⏱️ Server duration timer started - server is ready");
    
    // Run server with integrated shutdown handling
    if let Some(duration) = duration_secs {
        
        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    error!("Server error: {}", e);
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(duration)) => {
                info!("⏱️ Duration limit reached, shutting down gracefully");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("🛑 Received shutdown signal, shutting down gracefully");
            }
        }
    } else {
        // Run indefinitely with shutdown signal handling
        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    error!("Server error: {}", e);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("🛑 Received shutdown signal, shutting down gracefully");
            }
        }
    }
    
    // Cleanup server info file
    cleanup_server_info(server_info_file);
    let total_runtime = start_time.elapsed();
    info!("🧹 Server completed after {:.2}s", total_runtime.as_secs_f64());
    
    Ok(())
}

/// Clean up server info file
fn cleanup_server_info(filename: &str) {
    let _ = std::fs::remove_file(filename);
}