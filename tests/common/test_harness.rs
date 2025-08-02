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
//! ## Usage Pattern
//! 
//! ```rust
//! use crate::common::test_harness::{setup_test_env, cleanup_test_env, TestServer, PreShutdown, PostShutdown, assert_phase};
//! 
//! pub fn test_example() {
//!     let test_env = setup_test_env();
//!     
//!     // Use panic handler to ensure cleanup
//!     let result = std::panic::catch_unwind(move || {
//!         // Start server - it will wait until ready
//!         let server = TestServer::start(test_env);
//!         
//!         // Phase 1: Server is running - do WebSocket/HTTP operations
//!         assert_phase(PreShutdown);
//!         
//!         // ... perform tests against running server ...
//!         // Use server.port() to get the actual port
//!         
//!         // Phase 2: Shutdown server and wait for saves
//!         let test_env = server.shutdown();
//!         
//!         // Phase 3: Server has shutdown - safe to validate persisted data
//!         assert_phase(PostShutdown);
//!         
//!         // ... validate saved files, graph state, etc ...
//!         
//!         test_env // Return for cleanup
//!     });
//!     
//!     // Always cleanup regardless of test outcome
//!     if let Ok(test_env) = result {
//!         cleanup_test_env(test_env);
//!     } else if let Err(panic) = result {
//!         // For panics, we need the cloned env
//!         // Better pattern: clone test_env before catch_unwind
//!         std::panic::resume_unwind(panic);
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
//! ## Important Notes
//! 
//! - Tests run in parallel by default - each test gets unique ports and data directories
//! - The server automatically waits for startup before returning from `start()`
//! - `shutdown()` gracefully stops the server and waits for all saves to complete
//! - `wait_for_completion()` waits for natural process exit (e.g., --duration timeout)
//! - Always use `assert_phase` markers to document when assertions should happen
//! - The `Drop` trait ensures servers are killed even if tests panic

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

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
  # 3 second duration for tests
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
    // Remove test data directory
    if test_env.data_dir.exists() {
        match fs::remove_dir_all(&test_env.data_dir) {
            Ok(_) => {},
            Err(e) => eprintln!("Failed to remove test data directory: {}", e),
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
    process: Child,
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