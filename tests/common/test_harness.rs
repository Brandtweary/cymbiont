//! # Cymbiont Test Harness
//! 
//! This module provides a structured approach to integration testing with Cymbiont.
//! It enforces clear separation between test phases to prevent common mistakes like
//! trying to validate persisted data before the server has shut down.
//! 
//! ## Usage Pattern
//! 
//! ```rust
//! use common::{setup_test_env, cleanup_test_env};
//! use common::test_harness::{TestServer, PreShutdown, PostShutdown, assert_phase};
//! 
//! #[test]
//! fn test_example() {
//!     let test_env = setup_test_env();
//!     
//!     // Use panic handler to ensure cleanup
//!     let result = std::panic::catch_unwind(move || {
//!         // Start server - it will wait until ready
//!         let server = TestServer::start(test_env);
//!         
//!         // Phase 1: Server is running - do WebSocket/HTTP operations
//!         assert_phase::<PreShutdown>();
//!         
//!         // ... perform tests against running server ...
//!         // Use server.port() to get the actual port
//!         
//!         // Phase 2: Shutdown server and wait for saves
//!         let test_env = server.shutdown();
//!         
//!         // Phase 3: Server has shutdown - safe to validate persisted data
//!         assert_phase::<PostShutdown>();
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
//! ## Important Notes
//! 
//! - The server automatically waits for startup before returning from `start()`
//! - `shutdown()` gracefully stops the server and waits for all saves to complete
//! - Always use `assert_phase` markers to document when assertions should happen
//! - The `Drop` trait ensures servers are killed even if tests panic

use std::process::{Command, Child};
use std::thread;
use std::time::Duration;
use super::TestEnv;
use tracing::{debug, trace};

/// Represents a running Cymbiont server instance for testing
pub struct TestServer {
    process: Child,
    // TODO: These fields are used by WebSocket tests but not HTTP tests, causing warnings
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    test_env: TestEnv,
}

impl TestServer {
    /// Start a new test server with the given environment
    // TODO: Used by WebSocket tests but not HTTP tests, causing warnings
    #[allow(dead_code)]
    pub fn start(test_env: TestEnv) -> Self {
        let config_path = test_env.config_path.to_str().unwrap();
        
        // Read config to get server info filename
        let config_content = std::fs::read_to_string(&test_env.config_path)
            .expect("Failed to read config");
        let config: serde_yaml::Value = serde_yaml::from_str(&config_content)
            .expect("Failed to parse config");
        let server_info_file = config["backend"]["server_info_file"].as_str()
            .expect("server_info_file not found in config");
        
        let mut cmd = Command::new("cargo");
        cmd.env("CYMBIONT_TEST_MODE", "1")
            .args(&["run", "--", "--server", "--config", config_path]);
        
        // Only inherit stdout/stderr if --nocapture was passed
        if !is_nocapture() {
            cmd.stdout(std::process::Stdio::null())
               .stderr(std::process::Stdio::null());
        }
        
        let mut process = cmd.spawn().expect("Failed to start server");
        
        // Wait for server to be ready
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 150;  // 15 seconds with 100ms intervals
        let actual_port = loop {
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
        };
        
        TestServer {
            process,
            port: actual_port,
            test_env,
        }
    }
    
    /// Shutdown the server gracefully
    #[allow(dead_code)]
    pub fn shutdown(mut self) -> TestEnv {
        trace!("[TEST-HARNESS] shutdown() called");
        let config_path = self.test_env.config_path.to_str().unwrap();
        trace!("[TEST-HARNESS] Using config path: {}", config_path);
        
        let mut shutdown_cmd = Command::new("cargo");
        shutdown_cmd.args(&["run", "--", "--shutdown", "--config", config_path]);
        trace!("[TEST-HARNESS] Running shutdown command: cargo run -- --shutdown --config {}", config_path);
        
        // Always capture shutdown output to see what's happening
        // if !is_nocapture() {
        //     shutdown_cmd.stdout(std::process::Stdio::null())
        //                .stderr(std::process::Stdio::null());
        // }
        
        let shutdown_output = shutdown_cmd.output()
            .expect("Failed to run shutdown command");
        
        debug!("[TEST-HARNESS] Shutdown command exit status: {:?}", shutdown_output.status);
        
        // Always log the output to see what happened
        let stdout = String::from_utf8_lossy(&shutdown_output.stdout);
        let stderr = String::from_utf8_lossy(&shutdown_output.stderr);
        
        if !stdout.is_empty() {
            debug!("[TEST-HARNESS] Shutdown stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            debug!("[TEST-HARNESS] Shutdown stderr:\n{}", stderr);
        }
        
        if !shutdown_output.status.success() {
            // Fallback to kill if shutdown fails
            let _ = self.process.kill();
        } else {
            debug!("[TEST-HARNESS] Shutdown command succeeded");
        }
        
        // Wait for server to fully shutdown and save
        thread::sleep(Duration::from_secs(1));
        
        // Clone before moving out (since we implement Drop)
        let test_env = self.test_env.clone();
        
        // Don't kill the process here - it interrupts graceful shutdown!
        // The Drop trait will handle cleanup if needed
        // let _ = self.process.kill();
        
        test_env
    }
    
    /// Get the server port
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Get a reference to the test environment
    #[allow(dead_code)]
    pub fn test_env(&self) -> &TestEnv {
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
#[allow(dead_code)] // TODO: Used by WebSocket tests but not HTTP tests
fn is_nocapture() -> bool {
    std::env::args().any(|arg| arg == "--nocapture")
}

/// Test phase markers for clarity
#[allow(dead_code)] // TODO: Used by WebSocket tests but not HTTP tests  
pub struct PreShutdown;
#[allow(dead_code)] // TODO: Used by WebSocket tests but not HTTP tests
pub struct PostShutdown;

/// Marker trait for test phases
#[allow(dead_code)] // TODO: Used by WebSocket tests but not HTTP tests
pub trait TestPhase {}
impl TestPhase for PreShutdown {}
impl TestPhase for PostShutdown {}

/// Helper to make assertions only valid in certain phases
#[allow(dead_code)] // TODO: Used by WebSocket tests but not HTTP tests
pub fn assert_phase<P: TestPhase>(_phase: P) {
    // This is just a marker to make it clear when assertions should happen
}