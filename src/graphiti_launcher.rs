//! Graphiti server launcher - ensures backend is running
//!
//! This module manages the Graphiti FastAPI server lifecycle to prevent data loss
//! during episode ingestion. The server is launched as a detached background process
//! and intentionally left running (resource leak) until system shutdown.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::path::Path;
use std::process::Stdio;
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
/// - stdin redirected to null
/// - stdout and stderr redirected to log_path (append mode)
/// - Working directory set to server_path
/// - Using uvicorn ASGI server to run the FastAPI app
///
/// Both stdout and stderr are redirected to the same log file using File::try_clone()
/// to ensure proper interleaving of output (equivalent to shell's `>> log 2>&1`).
///
/// Note: This is an intentional "resource leak" - the process will continue
/// running after Cymbiont exits, ensuring no data loss during episode ingestion.
pub async fn launch_graphiti(server_path: &str, log_path: &Path) -> Result<()> {
    tracing::info!("Graphiti not running, launching background server...");

    // Ensure log directory exists
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
    }

    // Open log file in truncate mode (create if doesn't exist, overwrite if exists)
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

    // Clone file handle for stderr (shares same cursor position for proper interleaving)
    let log_file_stderr = log_file
        .try_clone()
        .context("Failed to clone log file handle for stderr")?;

    // Spawn fully detached process using uv run to manage dependencies
    tokio::process::Command::new("uv")
        .arg("run")
        .arg("uvicorn")
        .arg("graph_service.main:app")
        .current_dir(server_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_stderr))
        .spawn()
        .context("Failed to spawn Graphiti server")?;

    tracing::info!(
        "Graphiti server spawned, logging to: {}",
        log_path.display()
    );
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
/// * `server_path` - Path to graphiti-cymbiont/server directory
/// * `log_path` - Path to log file for stdout/stderr redirection
///
/// # Returns
/// * `Ok(())` if Graphiti is running (either already or after launch)
/// * `Err` if unable to launch or server doesn't become healthy within timeout
pub async fn ensure_graphiti_running(
    base_url: &str,
    server_path: &str,
    log_path: &Path,
) -> Result<()> {
    // Try waiting for Graphiti first (maybe it's starting up)
    if wait_for_graphiti(base_url, 10).await.is_ok() {
        tracing::info!("Graphiti already running");
        return Ok(());
    }

    // Still not running - launch it
    launch_graphiti(server_path, log_path).await?;
    wait_for_graphiti(base_url, 10).await?; // 10 attempts * 500ms = 5s max

    Ok(())
}
