//! # Cymbiont Test Harness
//! 
//! This module provides a structured approach to integration testing with Cymbiont.
//! It enforces clear separation between test phases to prevent common mistakes like
//! trying to validate persisted data before the server has shut down.
//! 
//! ## Universal Test Infrastructure
//! 
//! All integration tests use this harness to ensure consistent server lifecycle management
//! and reliable shutdown behavior. The TestServer supports both CLI and server modes
//! through flexible argument passing while maintaining identical shutdown detection.
//! 
//! ## Comprehensive Example: WebSocket Test with Graph Operations
//! 
//! ```rust
//! use crate::common::test_harness::*;
//! use serde_json::{json, Value};
//! 
//! pub fn test_websocket_operations() {
//!     let test_env = setup_test_env();
//!     let cleanup_env = test_env.clone();
//!     
//!     let result = std::panic::catch_unwind(move || {
//!         // Setup: Import graph and start server
//!         let (server, graph_id) = setup_with_graph(test_env);
//!         let port = server.port();
//!         let data_dir = server.test_env().data_dir.clone();
//!         
//!         assert_phase(PreShutdown);
//!         
//!         // Connect to WebSocket
//!         let mut ws = connect_websocket(port);
//!         
//!         // Authenticate using token from data directory
//!         let token = read_auth_token(&data_dir);
//!         assert!(authenticate_websocket(&mut ws, &token));
//!         
//!         // Send a command and expect success
//!         let cmd = json!({
//!             "type": "create_block",
//!             "content": "Test block"
//!         });
//!         let response = send_command(&mut ws, cmd);
//!         let data = expect_success(response);
//!         let block_id = data.unwrap()["block_id"].as_str().unwrap();
//!         
//!         // Graceful shutdown
//!         let test_env = server.shutdown();
//!         
//!         assert_phase(PostShutdown);
//!         
//!         // Verify persistence - graph should be saved
//!         let active_id = get_active_graph_id(&test_env.data_dir);
//!         assert_eq!(active_id, graph_id);
//!         
//!         test_env
//!     });
//!     
//!     // Cleanup
//!     match result {
//!         Ok(test_env) => cleanup_test_env(test_env),
//!         Err(panic) => {
//!             cleanup_test_env(cleanup_env);
//!             std::panic::resume_unwind(panic);
//!         }
//!     }
//! }
//! ```
//! 
//! ## CLI Mode Example
//! 
//! ```rust
//! let server = TestServer::start_with_args(test_env, vec!["--import-logseq", "path/"]);
//! let test_env = server.wait_for_completion(); // Wait for natural exit
//! ```
//! 
//! ## Crash Recovery Test Example
//! 
//! ```rust
//! // Start server and create some in-flight transactions
//! let mut server = TestServer::start(test_env);
//! // ... create transactions via WebSocket ...
//! 
//! // Simulate crash
//! server.force_kill();
//! 
//! // Start new server - recovery should happen
//! let server2 = TestServer::start(test_env);
//! // ... verify transactions were recovered ...
//! ```
//! 
//! ## Key Functions
//! 
//! **Environment Setup:**
//! - `setup_test_env()` - Creates isolated test environment with unique ports/directories
//! - `cleanup_test_env()` - Removes all test artifacts
//! - `setup_with_graph()` - Combines import_dummy_graph() and TestServer::start()
//! 
//! **Server Control:**
//! - `TestServer::start()` - Start server mode and wait for HTTP ready
//! - `TestServer::start_with_args()` - Start with custom CLI arguments
//! - `TestServer::shutdown()` - Graceful shutdown with SIGINT
//! - `TestServer::force_kill()` - Immediate termination with SIGKILL
//! - `TestServer::wait_for_completion()` - Wait for natural process exit
//! 
//! **WebSocket Testing:**
//! - `connect_websocket()` - Connect with automatic retries
//! - `authenticate_websocket()` - Send auth command and verify success
//! - `send_command()` - Send command and return response (skips heartbeats)
//! - `expect_success()` - Assert success response and extract data
//! 
//! **Data Access:**
//! - `read_auth_token()` - Read authentication token from data directory
//! - `get_active_graph_id()` - Extract active graph ID from registry
//! - `import_dummy_graph()` - Import test graph via CLI
//! 
//! **Freeze Operations (Test Infrastructure):**
//! - `freeze_operations()` - Pause graph operations after transaction creation
//! - `unfreeze_operations()` - Resume paused graph operations
//! - `get_freeze_state()` - Check if operations are currently frozen
//! - `send_command_async()` - Send command without waiting for response
//! - `read_pending_response()` - Read response with timeout handling
//! 
//! ## Important Notes
//! 
//! - Tests run in parallel - each gets unique ports and data directories
//! - Always use panic handler + cleanup pattern for proper teardown
//! - Use `assert_phase()` markers to document test phases
//! - The `Drop` trait ensures servers are killed even if tests panic

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use serde_json::{json, Value};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;

// Include the main logging module directly (avoids needing lib.rs)
#[path = "../../src/logging.rs"]
mod logging;

use self::logging::{create_base_env_filter, create_subscriber_builder};

// Global counter for unique test directories
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

// Ensure tracing is initialized only once
static INIT: Once = Once::new();

/// Initialize test tracing
fn init_test_tracing() {
    INIT.call_once(|| {
        // Use RUST_LOG if set, otherwise default to warn for tests
        let env_filter = create_base_env_filter("warn");
        create_subscriber_builder(env_filter).init();
    });
}

/// Test environment with paths
#[derive(Clone)]
pub struct TestEnv {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

/// Set up test environment with unique config and data directory
pub fn setup_test_env() -> TestEnv {
    init_test_tracing();
    
    // Create unique test ID
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let test_dir = format!("test_data_{}", test_id);
    let test_data_dir = Path::new(&test_dir);
    
    // Clean up if it exists (shouldn't happen but be safe)
    if test_data_dir.exists() {
        fs::remove_dir_all(test_data_dir).expect("Failed to remove existing test directory");
    }
    
    // Create the data directory
    fs::create_dir_all(test_data_dir).expect("Failed to create test data directory");
    
    // Create unique config file with unique port
    let test_port = 8888 + test_id as u16;
    let config_path = PathBuf::from(format!("config.test.{}.yaml", test_id));
    let config_content = format!(
        r#"# Cymbiont Test Configuration for test {}

# Backend server configuration
backend:
  host: 127.0.0.1
  port: {}
  max_port_attempts: 10
  server_info_file: "cymbiont_server_test_{}.json"

# Development-only settings
development:
  # 3 second duration for tests - DO NOT CHANGE THIS
  # Individual tests can pass --duration if they need more time
  default_duration: 3

# Data storage directory - unique per test
data_dir: {}
"#,
        test_id, test_port, test_id, test_dir
    );
    
    fs::write(&config_path, config_content).expect("Failed to write test config");
    
    // Set environment variable to use test config
    env::set_var("CYMBIONT_TEST_MODE", "1");
    
    TestEnv {
        data_dir: test_data_dir.to_path_buf(),
        config_path,
    }
}

/// Get the path to the cymbiont binary
pub fn get_cymbiont_binary() -> PathBuf {
    // Use CARGO_BIN_EXE_cymbiont which Cargo sets at compile time
    PathBuf::from(env!("CARGO_BIN_EXE_cymbiont"))
}

/// Clean up test environment after tests
pub fn cleanup_test_env(test_env: TestEnv) {
    // Check if KEEP_TEST_DATA environment variable is set
    if env::var("KEEP_TEST_DATA").is_ok() {
        use tracing::info;
        info!("🔍 KEEPING TEST DATA for debugging:");
        info!("   Data directory: {}", test_env.data_dir.display());
        info!("   Config file: {}", test_env.config_path.display());
        info!("   To clean up manually: rm -rf {} {}", 
              test_env.data_dir.display(), test_env.config_path.display());
        return;
    }
    
    // Remove test data directory
    if test_env.data_dir.exists() {
        match fs::remove_dir_all(&test_env.data_dir) {
            Ok(_) => {},
            Err(e) => {
                use tracing::error;
                error!("Failed to remove test data directory: {}", e);
            }
        }
    }
    
    // Remove test config file
    if test_env.config_path.exists() {
        fs::remove_file(&test_env.config_path).expect("Failed to remove test config file");
    }
    
    // Clean up server info file (extract test_id from config path)
    if let Some(config_name) = test_env.config_path.file_stem() {
        if let Some(config_str) = config_name.to_str() {
            if let Some(test_id_str) = config_str.strip_prefix("config.test.") {
                let server_info_file = format!("cymbiont_server_test_{}.json", test_id_str);
                let _ = fs::remove_file(&server_info_file);
            }
        }
    }
    
    // Unset test environment variable
    env::remove_var("CYMBIONT_TEST_MODE");
}

/// Represents a running Cymbiont instance for testing
/// 
/// Despite the name, this can run both server mode (the primary use case) and CLI mode.
/// Use `start()` for server mode or `start_with_args()` for CLI commands.
pub struct TestServer {
    pub process: Child,
    port: u16,
    test_env: TestEnv,
}

impl TestServer {
    /// Start a new test server with custom command line arguments
    pub fn start_with_args(test_env: TestEnv, args: Vec<&str>) -> Self {
        let config_path = test_env.config_path.to_str().unwrap();
        
        let mut cmd = Command::new(get_cymbiont_binary());
        cmd.env("CYMBIONT_TEST_MODE", "1")
            .args(&["--config", config_path])
            .args(&args);
        
        // Only inherit stdout/stderr if --nocapture was passed
        if !is_nocapture() {
            cmd.stdout(std::process::Stdio::null())
               .stderr(std::process::Stdio::null());
        }
        
        let mut process = cmd.spawn().expect("Failed to start process");
        
        // Different startup detection based on args
        let actual_port = if args.contains(&"--server") {
            // Server mode: Wait for server info file + health check
            // Read config to get server info filename
            let config_content = std::fs::read_to_string(&test_env.config_path)
                .expect("Failed to read config");
            let config: serde_yaml::Value = serde_yaml::from_str(&config_content)
                .expect("Failed to parse config");
            let server_info_file = config["backend"]["server_info_file"].as_str()
                .expect("server_info_file not found in config");
                
            let mut attempts = 0;
            const MAX_ATTEMPTS: u32 = 150;  // 15 seconds with 100ms intervals
            loop {
                attempts += 1;
                
                if let Ok(info_str) = std::fs::read_to_string(server_info_file) {
                    if let Ok(info) = serde_json::from_str::<serde_json::Value>(&info_str) {
                        if let Some(port) = info["port"].as_u64() {
                            let port = port as u16;
                            
                            // Try to connect to verify it's really ready
                            let client = reqwest::blocking::Client::new();
                            let url = format!("http://localhost:{}/", port);
                            match client.get(&url).timeout(Duration::from_secs(1)).send() {
                                Ok(response) if response.status().is_success() => {
                                    break port;
                                }
                                _ => {
                                    // Server not ready yet
                                }
                            }
                        }
                    }
                }
                
                if attempts >= MAX_ATTEMPTS {
                    let _ = process.kill();
                    panic!("Server failed to start after {} attempts", MAX_ATTEMPTS);
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        } else {
            // CLI mode: Just verify process is running 
            thread::sleep(Duration::from_millis(100)); // Give it a moment to start
            match process.try_wait() {
                Ok(Some(status)) => {
                    panic!("Process exited immediately with status: {:?}", status);
                }
                Ok(None) => {
                    // Process is running, good
                }
                Err(e) => {
                    panic!("Failed to check process status: {}", e);
                }
            }
            0 // No port for CLI mode
        };
        
        TestServer {
            process,
            port: actual_port,
            test_env,
        }
    }
    
    /// Start a new test server (convenience method for server mode)
    pub fn start(test_env: TestEnv) -> Self {
        Self::start_with_args(test_env, vec!["--server"])
    }
    
    /// Shutdown the server gracefully
    pub fn shutdown(mut self) -> TestEnv {
        // First check if process has already exited naturally
        match self.process.try_wait() {
            Ok(Some(_status)) => {
                // Process already exited, no need to send signal
            }
            Ok(None) => {
                // Process still running, send SIGINT
                let pid = self.process.id();
                
                #[cfg(target_family = "unix")]
                {
                    let _ = Command::new("kill")
                        .args(&["-2", &pid.to_string()])
                        .output();
                }
                
                #[cfg(not(target_family = "unix"))]
                {
                    // For non-Unix, just kill the process
                    let _ = self.process.kill();
                }
            }
            Err(_) => {
                // Error checking process status, try to kill anyway
                let _ = self.process.kill();
            }
        }
        
        // Wait for the process to exit with a timeout
        let start = Instant::now();
        let timeout = Duration::from_secs(5);
        
        loop {
            match self.process.try_wait() {
                Ok(Some(_status)) => {
                    break;
                }
                Ok(None) => {
                    // Process still running
                    if start.elapsed() > timeout {
                        let _ = self.process.kill();
                        let _ = self.process.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_e) => {
                    break;
                }
            }
        }
        
        // Additional wait for any remaining file operations
        thread::sleep(Duration::from_millis(500));
        
        // Clone before moving out (since we implement Drop)
        let test_env = self.test_env.clone();
        
        test_env
    }
    
    /// Get the server port
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Get a reference to the test environment
    pub fn test_env(&self) -> &TestEnv {
        &self.test_env
    }
    
    /// Wait for the process to complete naturally (for CLI tests with duration)
    pub fn wait_for_completion(mut self) -> TestEnv {
        // Just wait for the process to exit on its own
        let _ = self.process.wait().expect("Failed to wait for process");
        
        // Additional wait for any remaining file operations
        thread::sleep(Duration::from_millis(500));
        
        // Clone before moving out (since we implement Drop)
        let test_env = self.test_env.clone();
        
        test_env
    }
    
    /// Force kill the process (for crash tests)
    #[allow(dead_code)] // TODO: Remove when crash recovery test is implemented
    pub fn force_kill(&mut self) {
        let pid = self.process.id();
        
        #[cfg(target_family = "unix")]
        {
            // Use SIGKILL (-9) for immediate termination
            let _ = Command::new("kill")
                .args(&["-9", &pid.to_string()])
                .output();
        }
        
        #[cfg(not(target_family = "unix"))]
        {
            // On non-Unix, just use kill()
            let _ = self.process.kill();
        }
        
        // Wait a moment for the process to die
        thread::sleep(Duration::from_millis(100));
    }
    
    /// Get process ID
    #[allow(dead_code)] // TODO: Remove when crash recovery test is implemented
    pub fn pid(&self) -> u32 {
        self.process.id()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Ensure server is killed if not properly shutdown
        let _ = self.process.kill();
    }
}

/// Check if --nocapture was passed to cargo test
fn is_nocapture() -> bool {
    std::env::args().any(|arg| arg == "--nocapture")
}

/// Test phase markers for clarity
pub struct PreShutdown;
pub struct PostShutdown;

/// Marker trait for test phases
pub trait TestPhase {}
impl TestPhase for PreShutdown {}
impl TestPhase for PostShutdown {}

/// Helper to make assertions only valid in certain phases
pub fn assert_phase<P: TestPhase>(_phase: P) {
    // This is just a marker to make it clear when assertions should happen
}

// ===== WebSocket Utilities =====

/// WebSocket connection type
pub type WsConnection = WebSocket<MaybeTlsStream<TcpStream>>;

/// Connect to WebSocket endpoint with retries
pub fn connect_websocket(port: u16) -> WsConnection {
    let url = format!("ws://localhost:{}/ws", port);
    
    // Retry connection a few times as server may still be initializing WebSocket
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 10;
    
    loop {
        attempts += 1;
        match connect(&url) {
            Ok((socket, _)) => return socket,
            Err(e) => {
                if attempts >= MAX_ATTEMPTS {
                    panic!("Failed to connect to WebSocket after {} attempts: {}", MAX_ATTEMPTS, e);
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Send a command and wait for response (skipping heartbeats)
pub fn send_command(ws: &mut WsConnection, command: Value) -> Value {
    // Send command
    let msg = Message::Text(command.to_string());
    ws.send(msg).expect("Failed to send WebSocket message");
    
    // Wait for response
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let response: Value = serde_json::from_str(&text)
                    .expect("Failed to parse WebSocket response");
                
                // Skip heartbeats
                if response["type"] == "heartbeat" {
                    continue;
                }
                
                return response;
            }
            Ok(Message::Close(_)) => {
                panic!("WebSocket connection closed unexpectedly");
            }
            Ok(_) => continue,  // Skip other message types
            Err(e) => panic!("WebSocket read error: {}", e),
        }
    }
}

/// Expect a success response and return the data
pub fn expect_success(response: Value) -> Option<Value> {
    assert_eq!(
        response["type"], "success",
        "Expected success response, got: {}",
        response
    );
    response.get("data").cloned()
}

/// Authenticate the WebSocket connection
pub fn authenticate_websocket(ws: &mut WsConnection, token: &str) -> bool {
    let auth_cmd = json!({
        "type": "auth",
        "token": token
    });
    
    let response = send_command(ws, auth_cmd);
    response["type"] == "success"
}

// ===== Common Test Setup Functions =====

/// Import dummy graph via CLI and return the graph ID
pub fn import_dummy_graph(test_env: &TestEnv) -> String {
    let output = Command::new(get_cymbiont_binary())
        .env("CYMBIONT_TEST_MODE", "1")
        .args(&["--config", test_env.config_path.to_str().unwrap(),
            "--import-logseq", "logseq_databases/dummy_graph/"])
        .output()
        .expect("Failed to run cymbiont import");
    
    assert!(output.status.success(), 
        "Import failed with exit code: {:?}", 
        output.status.code());
    
    // Get the imported graph ID from registry
    get_active_graph_id(&test_env.data_dir)
}

/// Read auth token from test data directory
pub fn read_auth_token(data_dir: &Path) -> String {
    let auth_token_path = data_dir.join("auth_token");
    fs::read_to_string(&auth_token_path)
        .expect("Failed to read auth token")
        .trim()
        .to_string()
}

/// Get active graph ID from registry
pub fn get_active_graph_id(data_dir: &Path) -> String {
    let registry_path = data_dir.join("graph_registry.json");
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read graph registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse graph registry");
    
    registry["active_graph_id"].as_str()
        .expect("No active graph ID in registry")
        .to_string()
}

/// Combined setup: import graph + start server
pub fn setup_with_graph(test_env: TestEnv) -> (TestServer, String) {
    // Import dummy graph first
    let graph_id = import_dummy_graph(&test_env);
    
    // Start server
    let server = TestServer::start(test_env);
    
    (server, graph_id)
}

// ===== Freeze Operation Utilities =====

/// Freeze all graph operations
pub fn freeze_operations(ws: &mut WsConnection) -> bool {
    let cmd = json!({
        "type": "freeze_operations"
    });
    
    let response = send_command(ws, cmd);
    response["type"] == "success"
}

/// Unfreeze all graph operations
pub fn unfreeze_operations(ws: &mut WsConnection) -> bool {
    let cmd = json!({
        "type": "unfreeze_operations"
    });
    
    let response = send_command(ws, cmd);
    response["type"] == "success"
}

/// Check if operations are frozen
pub fn get_freeze_state(ws: &mut WsConnection) -> bool {
    let cmd = json!({
        "type": "get_freeze_state"
    });
    
    let response = send_command(ws, cmd);
    if response["type"] == "success" {
        response["data"]["frozen"].as_bool().unwrap_or(false)
    } else {
        false
    }
}

/// Send command without waiting for response (for frozen operations)
pub fn send_command_async(ws: &mut WsConnection, command: Value) {
    let msg = Message::Text(command.to_string());
    ws.send(msg).expect("Failed to send WebSocket message");
}

/// Try to read a pending response with timeout
pub fn read_pending_response(ws: &mut WsConnection) -> Value {
    // Try reading with a reasonable timeout
    let timeout = Duration::from_secs(5);
    let start = Instant::now();
    
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let response: Value = serde_json::from_str(&text)
                    .expect("Failed to parse WebSocket response");
                
                // Skip heartbeats
                if response["type"] == "heartbeat" {
                    continue;
                }
                
                return response;
            }
            Ok(Message::Close(_)) => {
                panic!("WebSocket connection closed unexpectedly");
            }
            Ok(_) => continue,  // Skip other message types
            Err(e) => {
                if start.elapsed() > timeout {
                    panic!("Timeout waiting for response: {}", e);
                }
                // Brief sleep before retrying
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

