/**
 * @module utils
 * @description Cross-cutting utility functions for the knowledge graph engine
 * 
 * This module provides essential helpers for process management, datetime parsing,
 * JSON processing, and network operations used throughout the application.
 * 
 * ## Process Management
 * 
 * Server lifecycle management with platform-specific process control:
 * - `terminate_previous_instance()`: Clean server startup with PID checking
 * - `write_server_info()`: Creates discovery file for external clients
 * - `find_available_port()`: Port allocation with configurable fallback
 * 
 * ## Data Processing
 * 
 * Utilities for parsing and converting data formats:
 * - `parse_datetime()`: Multi-format datetime parsing with fallback
 * - `parse_properties()`: JSON to HashMap conversion for metadata
 */

use std::process::Command;
use std::fs;
use std::net::TcpListener;
use std::collections::HashMap;
use std::time::Duration;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use serde_json;
use tracing::{error, trace, warn};
use crate::config::BackendConfig;
use crate::error::*;


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

// ===== DateTime and JSON Utilities =====

/// Parse a datetime string from PKM with multiple format support
pub fn parse_datetime(datetime_str: &str) -> DateTime<Utc> {
    // Try parsing with different formats
    if let Ok(dt) = DateTime::parse_from_rfc3339(datetime_str) {
        return dt.with_timezone(&Utc);
    }
    
    // Try ISO 8601 format
    if let Ok(dt) = DateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return dt.with_timezone(&Utc);
    }
    
    // Try Unix timestamp (milliseconds)
    if let Ok(timestamp_millis) = datetime_str.parse::<i64>() {
        // Handle both millisecond and second timestamps
        let timestamp_millis = if timestamp_millis > 1_000_000_000_000 {
            // Already in milliseconds
            timestamp_millis
        } else {
            // Convert seconds to milliseconds
            timestamp_millis * 1000
        };
        
        if let Some(dt) = DateTime::from_timestamp_millis(timestamp_millis) {
            return dt;
        }
    }
    
    // If all parsing attempts fail, log the issue and use current time
    warn!("Could not parse datetime '{datetime_str}', using current time");
    Utc::now()
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
    use chrono::Datelike;

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
    fn test_parse_datetime_rfc3339() {
        let dt_str = "2023-10-20T15:30:45Z";
        let dt = parse_datetime(dt_str);
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 10);
        assert_eq!(dt.day(), 20);
    }

    #[test]
    fn test_parse_datetime_timestamp() {
        let timestamp_millis = "1697817045000";
        let dt = parse_datetime(timestamp_millis);
        assert_eq!(dt.year(), 2023);
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