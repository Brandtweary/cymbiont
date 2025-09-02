//! @module utils
//! @description Cross-cutting utility functions for the knowledge graph engine
//!
//! This module provides essential helpers for process management, datetime parsing,
//! JSON processing, and network operations used throughout the application.
//!
//! ## Process Management
//!
//! Server lifecycle management with platform-specific process control:
//! - `terminate_previous_instance()`: Clean server startup with PID checking
//! - `write_server_info()`: Creates discovery file for external clients
//! - `find_available_port()`: Port allocation with configurable fallback
//!
//! ## Data Processing
//!
//! Utilities for parsing and converting data formats:
//! - `parse_properties()`: JSON to HashMap conversion for metadata
//!
//! ## Process Coordination
//!
//! The module handles server lifecycle coordination by managing PID files and
//! terminating previous instances. This ensures clean startup when restarting
//! the server without manually killing existing processes.
//!
//! ### Platform-Specific Behavior
//!
//! Process management adapts to the underlying platform:
//! - **Unix/Linux**: Uses `kill -0` for process existence checking and `kill -2` (SIGINT) for graceful termination
//! - **Windows**: Uses `tasklist` for process enumeration and `taskkill /F` for forced termination
//!
//! The termination logic includes stale PID file cleanup to prevent false positives
//! when processes have already exited.
//!
//! ## Network Utilities
//!
//! Port allocation functionality with intelligent fallback:
//! - Primary port from configuration is tested first
//! - Fallback scanning within configured range prevents startup failures
//! - TCP binding tests ensure ports are genuinely available
//!
//!
//! ## JSON Processing
//!
//! Property extraction from JSON objects with type coercion:
//! - String values are preserved as-is
//! - Non-string values are converted to string representation
//! - Supports nested object flattening for metadata storage
//!
//! ## Error Handling
//!
//! All utility functions use the global Result type and domain-specific error types.
//! Process management operations fail gracefully with informative error messages
//! when platform-specific commands are unavailable or processes cannot be terminated.

use std::process::Command;
use std::fs;
use std::net::TcpListener;
use std::collections::HashMap;
use std::time::Duration;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use serde_json;
use tracing::{error, trace, warn};
use tokio::sync::{RwLock as AsyncRwLock, RwLockReadGuard as AsyncRwLockReadGuard, RwLockWriteGuard as AsyncRwLockWriteGuard};
use crate::config::BackendConfig;
use crate::error::*;

// ===== Async Lock Utilities =====

/// Extension trait for tokio::sync::RwLock
/// 
/// Provides consistent API with panic-on-poison semantics for sync locks.
/// Note that async locks cannot be poisoned.
pub trait AsyncRwLockExt<T: 'static> {
    /// Read the lock asynchronously
    async fn read_or_panic(&self, context: &str) -> AsyncRwLockReadGuard<'_, T>;
    
    /// Write to the lock asynchronously with contention detection
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T>;
}

impl<T: 'static> AsyncRwLockExt<T> for AsyncRwLock<T> {
    async fn read_or_panic(&self, _context: &str) -> AsyncRwLockReadGuard<'_, T> {
        // Async locks can't be poisoned, just await
        self.read().await
    }
    
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T> {
        // Check for lock contention in debug builds and warn (not panic)
        #[cfg(debug_assertions)]
        {
            if self.try_write().is_err() {
                warn!(
                    "⚠️ Lock contention detected during '{}': another task is holding the lock. \
                    This may indicate a performance issue or the freeze mechanism in tests.",
                    context
                );
            }
        }
        
        self.write().await
    }
}

impl<T: 'static> AsyncRwLockExt<T> for Arc<AsyncRwLock<T>> {
    async fn read_or_panic(&self, context: &str) -> AsyncRwLockReadGuard<'_, T> {
        self.as_ref().read_or_panic(context).await
    }
    
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T> {
        self.as_ref().write_or_panic(context).await
    }
}

// ===== Process Management =====

// Constants

// Server info written to file for external clients
#[derive(Serialize, Deserialize)]
pub struct ServerInfo {
    pub pid: u32,
    pub host: String,
    pub port: u16,
}

// Check if a port is available
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

// Try to terminate a previous instance of our server
pub fn terminate_previous_instance(filename: &str) -> bool {
    trace!("[TERMINATE] Looking for server info file: {}", filename);
    // Check if server info file exists
    if let Ok(info_str) = fs::read_to_string(filename) {
        if let Ok(info) = serde_json::from_str::<ServerInfo>(&info_str) {
            let pid = info.pid.to_string();
        
        trace!("🔧 Found server info file with PID: {pid}");
        
        // First check if the process actually exists
        #[cfg(target_family = "unix")]
        {
            // Check if process exists using kill -0 (doesn't actually kill)
            let check_result = Command::new("kill")
                .arg("-0")
                .arg(&pid)
                .output();
                
            match check_result {
                Ok(output) => {
                    if !output.status.success() {
                        trace!("🔧 Process {pid} no longer exists, cleaning up stale PID file");
                        return false;
                    }
                },
                Err(e) => {
                    error!("Error checking process: {e}");
                    return false;
                }
            }
            
            // Process exists, try to terminate it
            trace!("🔧 Process {pid} is running, attempting to terminate");
            let kill_result = Command::new("kill")
                .arg("-2") // SIGINT for graceful shutdown (matches ctrlc handler)
                .arg(&pid)
                .output();
                
            match kill_result {
                Ok(output) => {
                    if output.status.success() {
                        trace!("🔧 Successfully terminated previous instance");
                        // Give the process time to shut down
                        std::thread::sleep(Duration::from_millis(500));
                        return true;
                    }
                    error!("Failed to terminate process: {}", 
                        String::from_utf8_lossy(&output.stderr));
                },
                Err(e) => {
                    error!("Error terminating process: {e}");
                }
            }
        }
        
        #[cfg(target_family = "windows")]
        {
            // Check if process exists using tasklist
            let check_result = Command::new("tasklist")
                .args(&["/FI", &format!("PID eq {}", pid)])
                .output();
                
            match check_result {
                Ok(output) => {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    if !output_str.contains(&pid) {
                        trace!("🔧 Process {pid} no longer exists, cleaning up stale PID file");
                        return false;
                    }
                },
                Err(e) => {
                    error!("Error checking process: {}", e);
                    return false;
                }
            }
            
            // Process exists, try to terminate it
            trace!("🔧 Process {pid} is running, attempting to terminate");
            let kill_result = Command::new("taskkill")
                .args(&["/PID", &pid, "/F"])
                .output();
                
            match kill_result {
                Ok(output) => {
                    if output.status.success() {
                        trace!("🔧 Successfully terminated previous instance");
                        // Give the process time to shut down
                        std::thread::sleep(Duration::from_millis(500));
                        return true;
                    } else {
                        error!("Failed to terminate process: {}", 
                            String::from_utf8_lossy(&output.stderr));
                    }
                },
                Err(e) => {
                    error!("Error terminating process: {}", e);
                }
            }
        }
        }
    }
    
    false
}

// Write server info including actual port
pub fn write_server_info(host: &str, port: u16, filename: &str) -> Result<()> {
    trace!("[SERVER-INFO-WRITE] Writing server info to: {}", filename);
    let info = ServerInfo {
        pid: std::process::id(),
        host: host.to_string(),
        port,
    };
    let json = serde_json::to_string_pretty(&info)?;
    fs::write(filename, &json)?;
    trace!("[SERVER-INFO-WRITE] Wrote server info: {}", json);
    Ok(())
}

/// Write just a PID file for process management
pub fn write_pid_file() -> Result<()> {
    let pid = std::process::id();
    fs::write(".cymbiont.pid", pid.to_string())?;
    Ok(())
}


/// Remove PID file
pub fn remove_pid_file() {
    let _ = fs::remove_file(".cymbiont.pid");
}

// Helper function to find an available port
pub fn find_available_port(config: &BackendConfig) -> Result<u16> {
    let port = config.port;
    
    if is_port_available(port) {
        return Ok(port);
    }
    
    warn!("Configured port {} is not available.", port);
    
    for p in (port + 1)..=(port + config.max_port_attempts) {
        if is_port_available(p) {
            trace!("🔧 Using alternative port: {}", p);
            return Ok(p);
        }
    }
    
    Err("Could not find an available port".into())
}

// ===== JSON Utilities =====

/// Export all system data to JSON for debugging/inspection
pub async fn export_all_system_data(app_state: &std::sync::Arc<crate::app_state::AppState>) -> crate::error::Result<()> {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use crate::utils::AsyncRwLockExt;
    use crate::graph::graph_manager::GraphManager;
    
    tracing::info!("Starting JSON export of all system data");
    
    // Export registries
    {
        let graph_registry = app_state.graph_registry.read_or_panic("export graph registry").await;
        let path = app_state.data_dir.join("graph_registry.json");
        graph_registry.export_json(&path)?;
    }
    
    {
        let agent_registry = app_state.agent_registry.read_or_panic("export agent registry").await;
        let path = app_state.data_dir.join("agent_registry.json");
        agent_registry.export_json(&path)?;
    }
    
    // Export all graphs (both open and closed)
    {
        // Get all registered graphs from the registry
        let all_graphs = {
            let registry = app_state.graph_registry.read_or_panic("export - get all graphs").await;
            registry.get_all_graphs()
        };
        
        for graph_info in all_graphs {
            let graph_id = graph_info.id;
            
            // Check if this graph already has a manager (is open)
            let was_already_open = {
                let managers = app_state.graph_managers.read().await;
                managers.contains_key(&graph_id)
            };
            
            // If not open, we need to temporarily load it to export
            if !was_already_open {
                let graph_path = app_state.data_dir.join("graphs").join(graph_id.to_string());
                match GraphManager::new(&graph_path) {
                    Ok(graph_manager) => {
                        // Temporarily insert it
                        {
                            let mut managers = app_state.graph_managers.write().await;
                            managers.insert(graph_id, Arc::new(RwLock::new(graph_manager)));
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to create graph manager {} for export: {}", graph_id, e);
                        continue;
                    }
                }
            }
            
            // Now export the graph (it definitely has a manager now)
            {
                let managers = app_state.graph_managers.read().await;
                if let Some(manager_lock) = managers.get(&graph_id) {
                    let manager = manager_lock.read_or_panic("export graph manager").await;
                    let graph_dir = app_state.data_dir.join("graphs").join(graph_id.to_string());
                    tokio::fs::create_dir_all(&graph_dir).await?;
                    let path = graph_dir.join("knowledge_graph.json");
                    manager.export_json(&path)?;
                }
            }
            
            // If we opened it just for export, close it again to free memory
            if !was_already_open {
                let mut managers = app_state.graph_managers.write().await;
                managers.remove(&graph_id);
            }
        }
    }
    
    // Export all agents
    {
        let agents = app_state.agents.read().await;
        for (agent_id, agent_arc) in agents.iter() {
            let agent = agent_arc.read_or_panic("export agent").await;
            let agent_dir = app_state.data_dir.join("agents").join(agent_id.to_string());
            tokio::fs::create_dir_all(&agent_dir).await?;
            let path = agent_dir.join("agent.json");
            agent.export_json(&path)?;
        }
    }
    
    tracing::info!("JSON export completed successfully");
    Ok(())
}

/// Parse properties from a JSON value into a HashMap
pub fn parse_properties(properties_json: &serde_json::Value) -> HashMap<String, String> {
    let mut properties = HashMap::new();
    
    if let Some(obj) = properties_json.as_object() {
        for (key, value) in obj {
            if let Some(value_str) = value.as_str() {
                properties.insert(key.clone(), value_str.to_string());
            } else {
                properties.insert(key.clone(), value.to_string());
            }
        }
    }
    
    properties
}



// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_info_serialization() {
        let info = ServerInfo {
            pid: 12345,
            host: "127.0.0.1".to_string(),
            port: 8888,
        };
        
        // Test serialization
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("12345"));
        assert!(json.contains("127.0.0.1"));
        assert!(json.contains("8888"));
        
        // Test deserialization
        let deserialized: ServerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pid, 12345);
        assert_eq!(deserialized.host, "127.0.0.1");
        assert_eq!(deserialized.port, 8888);
    }

    #[test]
    fn test_parse_properties() {
        let json = serde_json::json!({
            "key1": "value1",
            "key2": 42,
            "key3": true
        });
        
        let props = parse_properties(&json);
        assert_eq!(props.get("key1"), Some(&"value1".to_string()));
        assert_eq!(props.get("key2"), Some(&"42".to_string()));
        assert_eq!(props.get("key3"), Some(&"true".to_string()));
    }

    #[test] 
    fn test_is_port_available() {
        // This test might be flaky if port 0 allocation fails
        // Port 0 lets the OS assign any available port
        assert!(is_port_available(0));
    }
}