//! Simple Server Utilities
//! 
//! This module provides utility functions for server operations.
//! Main.rs should just call these functions without complex logic.

use std::net::SocketAddr;
use std::error::Error;
use std::time::{Duration, Instant};
use std::process::exit;
use tracing::{info, debug, error};

use crate::{
    AppState,
    utils::{write_server_info, find_available_port, terminate_previous_instance},
    server::http_api::create_router,
};

/// Run server with all the necessary setup and teardown
pub async fn run_server_with_duration(
    config_path: Option<String>,
    data_dir: Option<String>,
    duration: Option<u64>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let start_time = Instant::now();
    
    // Create application state with server support
    let app_state = AppState::new_server(config_path, data_dir).await?;
    let server_info_file = &app_state.config.backend.server_info_file;
    
    // Terminate any previous instance
    if std::fs::metadata(server_info_file).is_ok() {
        terminate_previous_instance(server_info_file);
        let _ = std::fs::remove_file(server_info_file);
    }
    
    // Set up exit handler
    let app_state_clone = app_state.clone();
    let server_info_file_clone = server_info_file.clone();
    ctrlc::set_handler(move || {
        info!("🛑 Received shutdown signal");
        info!("🔍 SHUTDOWN: Signal handler triggered");
        // Save all graphs
        // Create a new runtime since we're not in an async context
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            app_state_clone.cleanup_and_save().await;
        });
        cleanup_server_info(&server_info_file_clone);
        let total_runtime = start_time.elapsed();
        info!("🧹 Total runtime: {:.2}s", total_runtime.as_secs_f64());
        exit(0);
    }).expect("Error setting Ctrl-C handler");
    
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
    
    // Run server
    if let Some(duration) = duration_secs {
        debug!("Server will run for {} seconds", duration);
        
        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    error!("Server error: {}", e);
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(duration)) => {
                info!("⏱️ Duration limit reached, shutting down gracefully");
            }
        }
    } else {
        // Run indefinitely
        axum::serve(listener, app).await
            .map_err(|e| Box::<dyn Error + Send + Sync>::from(format!("Server error: {e}")))?;
    }
    
    // Cleanup
    app_state.cleanup_and_save().await;
    cleanup_server_info(server_info_file);
    let total_runtime = start_time.elapsed();
    info!("🧹 Total runtime: {:.2}s", total_runtime.as_secs_f64());
    
    Ok(())
}

/// Clean up server info file
fn cleanup_server_info(filename: &str) {
    if let Err(e) = std::fs::remove_file(filename) {
        debug!("Could not remove server info file: {}", e);
    }
}