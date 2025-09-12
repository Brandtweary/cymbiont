//! # Cymbiont Test Harness
//!
//! This module provides a structured approach to integration testing with Cymbiont.
//! It enforces clear separation between test phases to prevent common mistakes like
//! trying to validate persisted data before the server has shut down.
//!
//! For test validation, use `TestValidator` from `test_validator.rs`
//! to validate that expected operations resulted in correct JSON persistence.
//!
//! ## Universal Test Infrastructure
//!
//! All integration tests use this harness to ensure consistent server lifecycle management
//! and reliable shutdown behavior. The `TestServer` supports both CLI and server modes
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
//!         let response = send_command(&mut ws, &cmd);
//!         let data = expect_success(&response);
//!         let block_id = data.unwrap()["block_id"].as_str().unwrap();
//!         
//!         // Test tool execution directly (debug builds only)
//!         let tool_result = execute_tool_sync(&mut ws, "list_graphs", json!({}));
//!         assert!(tool_result.get("graphs").is_some());
//!         
//!         // Graceful shutdown
//!         let test_env = server.shutdown();
//!         
//!         assert_phase(PostShutdown);
//!         
//!         // Verify persistence - graph should be saved
//!         let saved_graph_id = get_single_open_graph_id(&test_env.data_dir);
//!         assert_eq!(saved_graph_id, graph_id);
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
//! ## Key Functions
//!
//! **Environment Setup:**
//! - `setup_test_env()` - Creates isolated test environment with unique ports/directories
//! - `cleanup_test_env()` - Removes all test artifacts
//! - `setup_with_graph()` - Combines `import_dummy_graph()` and `TestServer::start()`
//!
//! **Server Control:**
//! - `TestServer::start()` - Start server mode and wait for HTTP ready
//! - `TestServer::start_with_args()` - Start with custom CLI arguments
//! - `TestServer::shutdown()` - Graceful shutdown with SIGINT
//! - `TestServer::send_sigint()` - Send SIGINT without waiting
//! - `TestServer::force_kill()` - Immediate termination with SIGKILL
//! - `TestServer::wait_for_completion()` - Wait for natural process exit
//!
//! **WebSocket Testing:**
//! - `connect_websocket()` - Connect with automatic retries
//! - `authenticate_websocket()` - Send auth command and verify success
//! - `send_command()` - Send command and return response (skips heartbeats)
//! - `execute_tool_sync()` - Execute tool directly via TestToolCall (debug builds)
//! - `expect_success()` - Assert success response and extract data
//!
//! **Data Access:**
//! - `read_auth_token()` - Read authentication token from data directory
//! - `get_single_open_graph_id()` - Get ID when exactly one graph is open
//! - `import_dummy_graph()` - Import test graph via CLI
//!
//! ## Important Notes
//!
//! - Tests run in parallel - each gets unique ports and data directories
//! - Always use panic handler + cleanup pattern for proper teardown
//! - Use `assert_phase()` markers to document test phases
//! - The `Drop` trait ensures servers are killed even if tests panic

use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use autodebugger::{init_logging, VerbosityCheckLayer};
use std::sync::Mutex;

// Global counter for unique test directories
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

// Ensure tracing is initialized only once
static INIT: Once = Once::new();

// Global storage for verbosity layer to check at test completion
static VERBOSITY_LAYER: Mutex<Option<VerbosityCheckLayer>> = Mutex::new(None);

/// Initialize test tracing
fn init_test_tracing() {
    INIT.call_once(|| {
        // Use RUST_LOG if set, otherwise default to warn for tests
        // Use stderr for tests to avoid polluting test output
        let verbosity_layer = init_logging(Some("warn"), None, Some("stderr"));

        // Store the layer for later checking
        if let Ok(mut guard) = VERBOSITY_LAYER.lock() {
            *guard = Some(verbosity_layer);
        }
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
    let test_dir = format!("test_data_{test_id}");
    let test_data_dir = Path::new(&test_dir);

    // Clean up if it exists (shouldn't happen but be safe)
    if test_data_dir.exists() {
        fs::remove_dir_all(test_data_dir).expect("Failed to remove existing test directory");
    }

    // Create the data directory
    fs::create_dir_all(test_data_dir).expect("Failed to create test data directory");

    // Create unique config file with unique port
    #[allow(clippy::cast_possible_truncation)] // test_id is small, port range is u16
    let test_port = 8888 + test_id as u16;
    let config_path = PathBuf::from(format!("config.test.{test_id}.yaml"));
    let config_content = format!(
        r#"# Cymbiont Test Configuration for test {test_id}

# Backend server configuration
backend:
  host: 127.0.0.1
  port: {test_port}
  max_port_attempts: 10
  server_info_file: "cymbiont_server_test_{test_id}.json"

# Development-only settings
development:
  # 3 second duration for tests - DO NOT CHANGE THIS
  # Individual tests can pass --duration if they need more time
  default_duration: 3

# Data storage directory - unique per test
data_dir: {test_dir}
"#
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

/// Check test verbosity and report if excessive
fn check_test_verbosity() {
    if let Ok(guard) = VERBOSITY_LAYER.lock() {
        if let Some(ref layer) = *guard {
            if let Some(report) = layer.check_and_report() {
                use tracing::warn;
                warn!("{}", report);
            }
        }
    }
}

/// Clean up test environment after tests
pub fn cleanup_test_env(test_env: &TestEnv) {
    // Check log verbosity before cleanup
    check_test_verbosity();

    // Check if KEEP_TEST_DATA environment variable is set
    if env::var("KEEP_TEST_DATA").is_ok() {
        use tracing::info;
        info!("🔍 KEEPING TEST DATA for debugging:");
        info!("   Data directory: {}", test_env.data_dir.display());
        info!("   Config file: {}", test_env.config_path.display());
        info!(
            "   To clean up manually: rm -rf {} {}",
            test_env.data_dir.display(),
            test_env.config_path.display()
        );
        return;
    }

    // Remove test data directory
    if test_env.data_dir.exists() {
        match fs::remove_dir_all(&test_env.data_dir) {
            Ok(()) => {}
            Err(e) => {
                use tracing::error;
                error!("Failed to remove test data directory: {e}");
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
                let server_info_file = format!("cymbiont_server_test_{test_id_str}.json");
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
    pub fn start_with_args(test_env: TestEnv, args: &[&str]) -> Self {
        let config_path = test_env.config_path.to_str().unwrap();

        let mut cmd = Command::new(get_cymbiont_binary());
        cmd.env("CYMBIONT_TEST_MODE", "1")
            .args(["--config", config_path])
            .args(args);

        // Only inherit stdout/stderr if --nocapture was passed
        if !is_nocapture() {
            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
        }

        let mut process = cmd.spawn().expect("Failed to start process");

        // Different startup detection based on args
        let actual_port = if args.contains(&"--server") {
            const MAX_ATTEMPTS: u32 = 150; // 15 seconds with 100ms intervals

            // Server mode: Wait for server info file + health check
            // Read config to get server info filename
            let config_content =
                std::fs::read_to_string(&test_env.config_path).expect("Failed to read config");
            let config: serde_yaml::Value =
                serde_yaml::from_str(&config_content).expect("Failed to parse config");
            let server_info_file = config["backend"]["server_info_file"]
                .as_str()
                .expect("server_info_file not found in config");

            let mut attempts = 0;
            loop {
                attempts += 1;

                if let Ok(info_str) = std::fs::read_to_string(server_info_file) {
                    if let Ok(info) = serde_json::from_str::<serde_json::Value>(&info_str) {
                        if let Some(port) = info["port"].as_u64() {
                            #[allow(clippy::cast_possible_truncation)]
                            // port is always valid u16 range
                            let port = port as u16;

                            // Try to connect to verify it's really ready
                            let client = reqwest::blocking::Client::new();
                            let url = format!("http://localhost:{port}/");
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
                    panic!("Server failed to start after {MAX_ATTEMPTS} attempts");
                }

                thread::sleep(Duration::from_millis(100));
            }
        } else {
            // CLI mode: Just verify process is running
            thread::sleep(Duration::from_millis(100)); // Give it a moment to start
            match process.try_wait() {
                Ok(Some(status)) => {
                    panic!("Process exited immediately with status: {status:?}");
                }
                Ok(None) => {
                    // Process is running, good
                }
                Err(e) => {
                    panic!("Failed to check process status: {e}");
                }
            }
            0 // No port for CLI mode
        };

        Self {
            process,
            port: actual_port,
            test_env,
        }
    }

    /// Start a new test server (convenience method for server mode)
    pub fn start(test_env: TestEnv) -> Self {
        Self::start_with_args(test_env, &["--server"])
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
                    let _ = Command::new("kill").args(["-2", &pid.to_string()]).output();
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

        // Additional wait for JSON file operations
        thread::sleep(Duration::from_millis(1000));

        // Clone before moving out (since we implement Drop)
        self.test_env.clone()
    }

    /// Get the server port
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Get a reference to the test environment
    pub const fn test_env(&self) -> &TestEnv {
        &self.test_env
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
    const MAX_ATTEMPTS: u32 = 10;

    let url = format!("ws://localhost:{port}/ws");

    // Retry connection a few times as server may still be initializing WebSocket
    let mut attempts = 0;

    loop {
        attempts += 1;
        match connect(&url) {
            Ok((socket, _)) => return socket,
            Err(e) => {
                assert!(
                    attempts < MAX_ATTEMPTS,
                    "Failed to connect to WebSocket after {MAX_ATTEMPTS} attempts: {e}"
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Send a command and wait for response (skipping heartbeats)
pub fn send_command(ws: &mut WsConnection, command: &Value) -> Value {
    // Send command
    let msg = Message::Text(command.to_string().into());
    ws.send(msg).expect("Failed to send WebSocket message");

    // Wait for response with timeout
    let timeout = Duration::from_secs(10);
    let start = Instant::now();

    loop {
        assert!(
            start.elapsed() <= timeout,
            "Timeout waiting for response to command: {command}"
        );

        match ws.read() {
            Ok(Message::Text(text)) => {
                let response: Value =
                    serde_json::from_str(&text).expect("Failed to parse WebSocket response");

                // Skip heartbeats - continue is REQUIRED here to loop and get the actual response
                if response["type"] == "heartbeat" {
                    continue; // NOT redundant - needed to skip heartbeat and read next message
                }

                return response;
            }
            Ok(Message::Close(_)) => {
                panic!("WebSocket connection closed unexpectedly");
            }
            Ok(_) => {} // Skip other message types
            Err(e) => {
                // Check if this is a timeout/would-block error
                if e.to_string().contains("would block") {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                panic!("WebSocket read error: {e}");
            }
        }
    }
}

/// Expect a success response and return the data
pub fn expect_success(response: &Value) -> Option<Value> {
    assert_eq!(
        response["type"], "success",
        "Expected success response, got: {response}"
    );
    response.get("data").cloned()
}

/// Authenticate the WebSocket connection
pub fn authenticate_websocket(ws: &mut WsConnection, token: &str) -> bool {
    let auth_cmd = json!({
        "type": "auth",
        "token": token
    });

    let response = send_command(ws, &auth_cmd);
    response["type"] == "success"
}

// ===== Common Test Setup Functions =====

// DELETED: import_dummy_graph() - Migrating to HTTP import to avoid 2-process architecture
// Use import_dummy_graph_http() or setup_with_graph() instead

/// Import dummy graph via HTTP (requires running server)
/// This replaces the old CLI-based import to avoid 2-process architecture issues
pub async fn import_dummy_graph_http(
    port: u16,
    data_dir: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    // Read auth token for HTTP authentication
    let token = read_auth_token(data_dir);

    // Make HTTP POST request to import endpoint
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{port}/import/logseq"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "path": "logseq_databases/dummy_graph/"
        }))
        .send()
        .await?;

    // Check status
    if !response.status().is_success() {
        let text = response.text().await?;
        return Err(format!("Import failed: {text}").into());
    }

    // Parse response
    let import_response: serde_json::Value = response.json().await?;

    // Extract graph_id
    let graph_id = import_response["graph_id"]
        .as_str()
        .ok_or("No graph_id in response")?
        .to_string();

    Ok(graph_id)
}

/// Run async code in tests - helper for tests that aren't async
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

/// Read auth token from test data directory
pub fn read_auth_token(data_dir: &Path) -> String {
    let auth_token_path = data_dir.join("auth_token");
    fs::read_to_string(&auth_token_path)
        .expect("Failed to read auth token")
        .trim()
        .to_string()
}

/// Get the single open graph ID from registry (panics if not exactly one open graph)
pub fn get_single_open_graph_id(data_dir: &Path) -> String {
    const MAX_ATTEMPTS: u32 = 10;

    // Wait a bit for registry to be flushed after import
    thread::sleep(Duration::from_millis(200));

    let registry_path = data_dir.join("graph_registry.json");

    // Retry a few times in case the registry isn't written yet
    let mut attempts = 0;

    loop {
        attempts += 1;

        if registry_path.exists() {
            if let Ok(registry_content) = fs::read_to_string(&registry_path) {
                if let Ok(registry) = serde_json::from_str::<Value>(&registry_content) {
                    if let Some(open_graphs) = registry["open_graphs"].as_array() {
                        if open_graphs.len() == 1 {
                            if let Some(graph_id) = open_graphs[0].as_str() {
                                return graph_id.to_string();
                            }
                        } else if open_graphs.len() > 1 {
                            panic!(
                                "Expected exactly one open graph, found {}",
                                open_graphs.len()
                            );
                        }
                    }
                }
            }
        }

        assert!(
            attempts < MAX_ATTEMPTS,
            "Failed to find single open graph after {MAX_ATTEMPTS} attempts. Registry may not be flushed yet."
        );

        thread::sleep(Duration::from_millis(100));
    }
}

/// Combined setup: start server + import graph via HTTP
/// This replaces the old version that used CLI import
pub fn setup_with_graph(test_env: TestEnv) -> (TestServer, String) {
    // Start server FIRST
    let server = TestServer::start(test_env);
    let port = server.port();
    let data_dir = server.test_env().data_dir.clone();

    // Import graph via HTTP
    let graph_id = block_on(async {
        // Wait for server to be ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        import_dummy_graph_http(port, &data_dir).await
    })
    .expect("Failed to import dummy graph");

    (server, graph_id)
}

// ===== Tool Testing Utilities =====

/// Execute a tool directly via TestToolCall command (for testing)
pub fn execute_tool_sync(
    ws: &mut WsConnection,
    tool_name: &str,
    tool_args: Value,
) -> Value {
    let cmd = json!({
        "type": "test_tool_call",
        "tool_name": tool_name,
        "tool_args": tool_args,
    });
    
    let response = send_command(ws, &cmd);
    
    // Extract the tool result from the success response
    if response["type"] == "success" {
        response.get("data").cloned().unwrap_or(json!({}))
    } else {
        panic!("Tool execution failed: {}", response);
    }
}

// ===== MCP Testing Utilities =====

/// Start MCP server with piped stdin/stdout
pub fn start_mcp_server(test_env: TestEnv) -> (Child, ChildStdin, BufReader<ChildStdout>, TestEnv) {
    let config_path = test_env.config_path.to_str().unwrap();
    
    let mut cmd = Command::new(get_cymbiont_binary());
    cmd.env("CYMBIONT_TEST_MODE", "1")
        .args(["--config", config_path, "--mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if is_nocapture() { 
            Stdio::inherit() 
        } else { 
            Stdio::null() 
        });
    
    let mut process = cmd.spawn().expect("Failed to start MCP server");
    
    let stdin = process.stdin.take().expect("Failed to get stdin");
    let stdout = BufReader::new(process.stdout.take().expect("Failed to get stdout"));
    
    // Give the MCP server a moment to start its async loop
    thread::sleep(Duration::from_millis(500));
    
    (process, stdin, stdout, test_env)
}

/// Send JSON-RPC request and read response
pub fn mcp_request(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    request_id: u64,
    method: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": request_id
    });
    
    // Write request
    writeln!(stdin, "{}", request).map_err(|e| format!("Failed to write request: {}", e))?;
    stdin.flush().map_err(|e| format!("Failed to flush stdin: {}", e))?;
    
    // Read response
    let mut line = String::new();
    stdout.read_line(&mut line).map_err(|e| format!("Failed to read response: {}", e))?;
    
    let response: Value = serde_json::from_str(&line)
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    // Check for error
    if let Some(error) = response.get("error") {
        return Err(format!("MCP error: {}", error));
    }
    
    Ok(response.get("result").cloned().unwrap_or(json!(null)))
}

/// Initialize MCP protocol
pub fn mcp_initialize(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
) -> Result<(), String> {
    // Send initialize request
    mcp_request(stdin, stdout, 1, "initialize", Some(json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {}
    })))?;
    
    // Send initialized notification (no response expected)
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "initialized"
    });
    writeln!(stdin, "{}", notification).map_err(|e| format!("Failed to send notification: {}", e))?;
    stdin.flush().map_err(|e| format!("Failed to flush: {}", e))?;
    
    Ok(())
}

/// List available MCP tools
pub fn mcp_list_tools(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    request_id: u64,
) -> Result<Vec<String>, String> {
    let result = mcp_request(stdin, stdout, request_id, "tools/list", None)?;
    let tools = result["tools"].as_array()
        .ok_or("Invalid tools response")?;
    
    Ok(tools.iter()
        .filter_map(|t| t["name"].as_str())
        .map(|s| s.to_string())
        .collect())
}

/// Call an MCP tool
pub fn mcp_call_tool(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    request_id: u64,
    tool_name: &str,
    args: Value,
) -> Result<Value, String> {
    let result = mcp_request(stdin, stdout, request_id, "tools/call", Some(json!({
        "name": tool_name,
        "arguments": args
    })))?;
    
    // Extract the text content from MCP response format
    if let Some(content) = result["content"].as_array() {
        if let Some(first) = content.first() {
            if let Some(text) = first["text"].as_str() {
                // Try to parse as JSON
                if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                    return Ok(parsed);
                }
                // Otherwise return as string value
                return Ok(json!(text));
            }
        }
    }
    
    Ok(result)
}

/// Shutdown MCP server gracefully
pub fn shutdown_mcp_server(mut process: Child, test_env: TestEnv) -> TestEnv {
    // Send Ctrl+C signal
    #[cfg(target_family = "unix")]
    {
        let pid = process.id();
        let _ = Command::new("kill").args(["-2", &pid.to_string()]).output();
    }
    
    #[cfg(not(target_family = "unix"))]
    {
        let _ = process.kill();
    }
    
    // Wait for shutdown with timeout
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    
    loop {
        match process.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = process.kill();
                    let _ = process.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    
    // Wait for JSON persistence
    thread::sleep(Duration::from_millis(1000));
    
    test_env
}
