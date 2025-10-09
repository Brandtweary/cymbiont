//! Graphiti server launcher - ensures backend is running
//!
//! This module manages the Graphiti FastAPI server lifecycle to prevent data loss
//! during episode ingestion. The server is launched as a detached background process
//! and intentionally left running (resource leak) until system shutdown.

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time::sleep;

/// Check if Graphiti is already running by hitting the health endpoint
pub async fn is_graphiti_running(base_url: &str) -> bool {
    let health_url = format!("{}/healthcheck", base_url);
    let client = reqwest::Client::new();

    client.get(&health_url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Launch Graphiti as detached background process
///
/// The process is spawned with:
/// - All stdio streams redirected to null (fully detached)
/// - Working directory set to server_path
/// - Using uvicorn ASGI server to run the FastAPI app
///
/// Note: This is an intentional "resource leak" - the process will continue
/// running after Cymbiont exits, ensuring no data loss during episode ingestion.
pub async fn launch_graphiti(server_path: &str) -> Result<()> {
    tracing::info!("Graphiti not running, launching background server...");

    // Spawn fully detached process using uv run to manage dependencies
    tokio::process::Command::new("uv")
        .arg("run")
        .arg("uvicorn")
        .arg("graph_service.main:app")
        .current_dir(server_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn Graphiti server")?;

    tracing::info!("Graphiti server spawned, waiting for startup...");
    Ok(())
}

/// Wait for Graphiti to become healthy with exponential backoff
///
/// Attempts health checks with 500ms intervals up to max_attempts times.
pub async fn wait_for_graphiti(base_url: &str, max_attempts: u32) -> Result<()> {
    for attempt in 1..=max_attempts {
        if is_graphiti_running(base_url).await {
            tracing::info!("Graphiti server is healthy");
            return Ok(());
        }

        if attempt < max_attempts {
            tracing::debug!("Waiting for Graphiti... (attempt {}/{})", attempt, max_attempts);
            sleep(Duration::from_millis(500)).await;
        }
    }

    anyhow::bail!("Graphiti failed to start after {} attempts", max_attempts)
}

/// Ensure Graphiti is running, launch if needed
///
/// This function checks if Graphiti is already running at base_url.
/// If not, it launches the server from server_path and waits for it to become healthy.
/// If already running, it simply logs and continues.
///
/// # Arguments
/// * `base_url` - The base URL where Graphiti should be running (e.g., "http://localhost:8000")
/// * `server_path` - Path to graphiti-cymbiont/mcp_server directory
///
/// # Returns
/// * `Ok(())` if Graphiti is running (either already or after launch)
/// * `Err` if unable to launch or server doesn't become healthy within timeout
pub async fn ensure_graphiti_running(base_url: &str, server_path: &str) -> Result<()> {
    if is_graphiti_running(base_url).await {
        tracing::info!("Graphiti already running");
        return Ok(());
    }

    launch_graphiti(server_path).await?;
    wait_for_graphiti(base_url, 10).await?; // 10 attempts * 500ms = 5s max

    Ok(())
}
