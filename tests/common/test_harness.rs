//! Test harness for running integration tests with real Logseq instance
//!
//! This harness manages a single long-running Cymbiont+Logseq instance for the entire
//! test suite, providing graph-based isolation for individual tests.

use std::process::{Child, Command};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::fs;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::time::{sleep, timeout};
use tokio::sync::Mutex;
use reqwest::Client;
use serde_json::json;
use tracing::{info, error, warn, debug};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::StreamExt;

use super::graph_registry_validator::GraphRegistryValidator;

const CYMBIONT_BASE_URL: &str = "http://127.0.0.1:3000";

/// Information about a test graph and its backup
#[derive(Clone)]
struct TestGraphInfo {
    name: String,
    path: PathBuf,
    backup_path: PathBuf,
}

/// WebSocket connection state
pub struct WebSocketConnection {
    recent_messages: Vec<serde_json::Value>,
}

/// Main test harness that manages the Cymbiont+Logseq instance
pub struct IntegrationTestHarness {
    /// The Cymbiont process
    cymbiont_process: Child,
    
    /// HTTP client for API requests
    http_client: Client,
    
    /// Test graphs with their backup information
    test_graphs: HashMap<String, TestGraphInfo>,
    
    /// Currently active graph
    active_graph: Mutex<String>,
    
    /// Registry validator for staged validation
    validator: GraphRegistryValidator,
    
    /// WebSocket connection for real-time communication
    websocket: Arc<Mutex<Option<WebSocketConnection>>>,
}

impl IntegrationTestHarness {
    /// Setup the test harness by launching Cymbiont+Logseq
    pub async fn setup() -> Result<Self, Box<dyn StdError + Send + Sync>> {
        info!("Setting up integration test harness");
        
        // Initialize validator
        let validator = GraphRegistryValidator::new();
        
        // 1. Stage 1: Pre-launch validation
        if let Err(e) = validator.validate_pre_launch().await {
            warn!("Pre-launch validation failed: {}", e);
        }
        
        // 2. Ensure all test graphs have backups
        Self::ensure_backups()?;
        
        // 3. Copy test config
        if !Path::new("config.yaml").exists() {
            fs::copy("config.test.yaml", "config.yaml")?;
        }
        
        // 4. Start Cymbiont with test_graph_empty
        info!("Starting Cymbiont server with Logseq");
        let cymbiont_process = Command::new("cargo")
            .args(&["run", "--", "--graph", "test_graph_empty"])
            .env("RUST_LOG", "info,cymbiont=debug")
            .spawn()?;
            
        // 5. Wait for server to be ready
        let http_client = Client::new();
        Self::wait_for_server(&http_client).await?;
        
        // 6. Initialize the plugin by sending the initialized notification
        Self::initialize_plugin(&http_client, "test_graph_empty").await?;
        
        // 7. Wait for initial sync to stabilize
        sleep(Duration::from_secs(2)).await;
        
        // 8. Stage 2: Post-initialization validation
        if let Err(e) = validator.validate_post_initialization("test_graph_empty").await {
            warn!("Post-initialization validation failed: {}", e);
        }
        
        // Build test graph info
        let mut test_graphs = HashMap::new();
        let graphs = vec![
            "test_graph_switching",
            "test_graph_sync",
            "test_graph_websocket",
            "test_graph_multi_1",
            "test_graph_multi_2",
            "test_graph_empty",
        ];
        
        for graph_name in graphs {
            let info = TestGraphInfo {
                name: graph_name.to_string(),
                path: PathBuf::from("logseq_databases").join(graph_name),
                backup_path: PathBuf::from("logseq_databases").join("test_backups").join(format!("{}_backup", graph_name)),
            };
            test_graphs.insert(graph_name.to_string(), info);
        }
        
        let harness = Self {
            cymbiont_process,
            http_client,
            test_graphs,
            active_graph: Mutex::new("test_graph_empty".to_string()),
            validator,
            websocket: Arc::new(Mutex::new(None)),
        };
        
        // 9. Establish WebSocket connection
        harness.connect_websocket().await?;
        
        // 10. Switch to all graphs once to register them and perform Stage 4 validation
        harness.register_all_graphs().await?;
        
        Ok(harness)
    }
    
    /// Run a test with automatic graph management
    pub async fn run_test<'a, F, Fut>(&'a self, graph_name: &str, test_fn: F) -> Result<(), Box<dyn StdError + Send + Sync>>
    where
        F: FnOnce(&'a Self) -> Fut,
        Fut: std::future::Future<Output = Result<(), Box<dyn StdError + Send + Sync>>> + 'a,
    {
        info!("Running test on graph: {}", graph_name);
        
        // Switch to the test graph
        self.switch_to_graph(graph_name).await?;
        
        // Run the test
        let result = test_fn(self).await;
        
        // Always restore the graph, even if test failed
        if let Err(e) = self.restore_graph(graph_name).await {
            error!("Failed to restore graph {}: {}", graph_name, e);
        }
        
        result
    }
    
    /// Switch to a different graph
    pub async fn switch_to_graph(&self, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("Switching to graph: {}", graph_name);
        
        // Get graph info and validate
        let graph_info = self.test_graphs.get(graph_name)
            .ok_or_else(|| format!("Unknown test graph: {}", graph_name))?;
            
        // Validate graph name consistency
        if graph_info.name != graph_name {
            return Err(format!("Graph name mismatch: expected '{}', got '{}'", graph_name, graph_info.name).into());
        }
        
        // Request switch via API
        let response = self.http_client
            .post(format!("{}/api/session/switch", CYMBIONT_BASE_URL))
            .json(&json!({
                "name": graph_name
            }))
            .send()
            .await?;
            
        if !response.status().is_success() {
            return Err(format!("Failed to switch graph: {}", response.status()).into());
        }
        
        // Wait for WebSocket confirmation of graph switch
        self.wait_for_graph_switch_confirmation(graph_name).await?;
        
        // Stage 3: Validate graph switch
        if let Err(e) = self.validator.validate_graph_switch(graph_name).await {
            warn!("Graph switch validation failed for '{}': {}", graph_name, e);
        }
        
        // Update active graph
        *self.active_graph.lock().await = graph_name.to_string();
        
        Ok(())
    }
    
    /// Restore a graph from its backup
    async fn restore_graph(&self, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("Restoring graph {} from backup", graph_name);
        
        // 1. Switch to empty graph first
        self.switch_to_graph("test_graph_empty").await?;
        
        // 2. Get graph info (clone it to avoid borrow issues)
        let graph_info = self.test_graphs.get(graph_name)
            .ok_or_else(|| format!("Unknown test graph: {}", graph_name))?
            .clone();
        
        // 3. Delete the modified graph
        if graph_info.path.exists() {
            fs::remove_dir_all(&graph_info.path)?;
        }
        
        // 4. Copy backup back
        Self::copy_dir(&graph_info.backup_path, &graph_info.path)?;
        
        info!("Graph {} restored successfully", graph_name);
        Ok(())
    }
    
    /// Teardown the test harness
    pub async fn teardown(mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("Tearing down integration test harness");
        
        // Shutdown server gracefully
        let shutdown_result = Command::new("cargo")
            .args(&["run", "--", "--shutdown-server"])
            .output()?;
            
        if !shutdown_result.status.success() {
            warn!("Graceful shutdown failed, killing process");
            self.cymbiont_process.kill()?;
        }
        
        // Wait a bit for cleanup
        sleep(Duration::from_millis(500)).await;
        
        Ok(())
    }
    
    /// Get the HTTP client for making API requests
    pub fn http_client(&self) -> &Client {
        &self.http_client
    }
    
    /// Get the base URL for API requests
    pub fn base_url(&self) -> &str {
        CYMBIONT_BASE_URL
    }
    
    /// Wait for the server to be ready
    async fn wait_for_server(client: &Client) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let max_attempts = 40; // 20 seconds
        
        for attempt in 1..=max_attempts {
            match timeout(
                Duration::from_secs(1),
                client.get(CYMBIONT_BASE_URL).send()
            ).await {
                Ok(Ok(response)) if response.status().is_success() => {
                    info!("Server ready after {} attempts", attempt);
                    return Ok(());
                }
                _ => {
                    if attempt < max_attempts {
                        sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }
        
        Err("Server failed to become ready within timeout".into())
    }
    
    /// Initialize the plugin connection
    async fn initialize_plugin(client: &Client, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graph_path = format!("logseq_databases/{}", graph_name);
        
        let response = client
            .post(format!("{}/plugin/initialized", CYMBIONT_BASE_URL))
            .header("X-Cymbiont-Graph-Name", graph_name)
            .header("X-Cymbiont-Graph-Path", &graph_path)
            .send()
            .await?;
            
        if !response.status().is_success() {
            return Err(format!("Failed to initialize plugin: {}", response.status()).into());
        }
        
        Ok(())
    }
    
    
    /// Ensure all test graphs have backups
    fn ensure_backups() -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = vec![
            ("test_graph_switching", "test_graph_switching_backup"),
            ("test_graph_sync", "test_graph_sync_backup"),
            ("test_graph_websocket", "test_graph_websocket_backup"),
            ("test_graph_multi_1", "test_graph_multi_1_backup"),
            ("test_graph_multi_2", "test_graph_multi_2_backup"),
            ("test_graph_empty", "test_graph_empty_backup"),
        ];
        
        for (graph, backup) in graphs {
            let graph_path = PathBuf::from("logseq_databases").join(graph);
            let backup_path = PathBuf::from("logseq_databases").join("test_backups").join(backup);
            
            if !backup_path.exists() && graph_path.exists() {
                warn!("Creating missing backup for {}", graph);
                Self::copy_dir(&graph_path, &backup_path)?;
            }
        }
        
        Ok(())
    }
    
    /// Recursively copy a directory
    fn copy_dir(src: &Path, dst: &Path) -> Result<(), Box<dyn StdError + Send + Sync>> {
        fs::create_dir_all(dst)?;
        
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            
            if file_type.is_dir() {
                Self::copy_dir(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }
        
        Ok(())
    }
    
    /// Establish WebSocket connection to Cymbiont
    async fn connect_websocket(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("Establishing WebSocket connection");
        
        let ws_url = format!("{}/ws", CYMBIONT_BASE_URL.replace("http", "ws"));
        let (ws_stream, _) = connect_async(&ws_url).await
            .map_err(|e| format!("Failed to connect to WebSocket at {}: {}", ws_url, e))?;
        
        // Split the stream to handle receiving
        let (_, mut receiver) = ws_stream.split();
        
        let ws_connection = WebSocketConnection {
            recent_messages: Vec::new(),
        };
        
        let mut websocket_guard = self.websocket.lock().await;
        *websocket_guard = Some(ws_connection);
        drop(websocket_guard);
        
        // Spawn a task to handle incoming messages
        let websocket_clone = self.websocket.clone();
        tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                            debug!("WebSocket received: {:?}", json_msg);
                            let mut ws_guard = websocket_clone.lock().await;
                            if let Some(ws) = ws_guard.as_mut() {
                                ws.recent_messages.push(json_msg);
                                // Keep only last 100 messages
                                if ws.recent_messages.len() > 100 {
                                    ws.recent_messages.remove(0);
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket connection closed");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
        
        info!("✅ WebSocket connection established");
        Ok(())
    }
    
    /// Verify WebSocket connection is healthy
    pub async fn verify_websocket_connected(&self) -> Result<bool, Box<dyn StdError + Send + Sync>> {
        let websocket_guard = self.websocket.lock().await;
        Ok(websocket_guard.is_some())
    }
    
    /// Get recent WebSocket messages (for verification)
    pub async fn get_recent_websocket_messages(&self) -> Result<Vec<serde_json::Value>, Box<dyn StdError + Send + Sync>> {
        let websocket_guard = self.websocket.lock().await;
        if let Some(ws) = websocket_guard.as_ref() {
            Ok(ws.recent_messages.clone())
        } else {
            Err("WebSocket not connected".into())
        }
    }
    
    /// Clear recent WebSocket messages (for test isolation)
    pub async fn clear_websocket_messages(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut websocket_guard = self.websocket.lock().await;
        if let Some(ws) = websocket_guard.as_mut() {
            ws.recent_messages.clear();
            Ok(())
        } else {
            Err("WebSocket not connected".into())
        }
    }
    
    /// Register all test graphs by switching to each one, then perform final validation
    async fn register_all_graphs(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("Registering all test graphs for validation");
        
        // Get list of all graphs except the one we started with
        let graphs_to_register: Vec<String> = self.test_graphs.keys()
            .filter(|&name| name != "test_graph_empty")
            .cloned()
            .collect();
        
        // Switch to each graph to ensure it gets registered
        for graph_name in &graphs_to_register {
            info!("Registering graph: {}", graph_name);
            self.switch_to_graph(graph_name).await?;
            
            // Small delay between switches
            sleep(Duration::from_millis(500)).await;
        }
        
        // Switch back to empty graph
        self.switch_to_graph("test_graph_empty").await?;
        
        // Stage 4: Final validation after all graphs are registered
        if let Err(e) = self.validator.validate_final_state().await {
            warn!("Final validation failed: {}", e);
        }
        
        info!("All test graphs registered and validated");
        Ok(())
    }
    
    /// Wait for WebSocket confirmation that graph switch completed
    async fn wait_for_graph_switch_confirmation(&self, expected_graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(10);
        
        info!("⏳ Waiting for graph switch confirmation to '{}'...", expected_graph_name);
        
        loop {
            // Check if we've received the confirmation
            let websocket_guard = self.websocket.lock().await;
            if let Some(ws) = websocket_guard.as_ref() {
                for msg in ws.recent_messages.iter().rev() {
                    if let Some(msg_type) = msg.get("type").and_then(|v| v.as_str()) {
                        if msg_type == "graph_switch_confirmed" {
                            if let (Some(name), Some(id)) = (
                                msg.get("graph_name").and_then(|v| v.as_str()),
                                msg.get("graph_id").and_then(|v| v.as_str())
                            ) {
                                if name == expected_graph_name {
                                    info!("✅ Graph switch confirmed: {} ({})", name, id);
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
            drop(websocket_guard);
            
            // Check timeout
            if start_time.elapsed() > timeout {
                return Err(format!("Graph switch confirmation timeout after {:?}", timeout).into());
            }
            
            // Small delay before checking again
            sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for IntegrationTestHarness {
    fn drop(&mut self) {
        // Ensure process is killed even if teardown wasn't called
        let _ = self.cymbiont_process.kill();
    }
}