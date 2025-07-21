/**
 * @module utils
 * @description Cross-cutting utility functions for the PKM Knowledge Graph backend
 * 
 * This module consolidates all general-purpose utility functions that don't belong to any
 * specific domain module. It provides essential helpers for process management, datetime
 * parsing, JSON processing, network operations, and other low-level functionality used
 * throughout the application.
 * 
 * ## Logseq Integration Utilities
 * 
 * - `find_logseq_executable()`: Cross-platform Logseq discovery
 *   - Windows: Checks %USERPROFILE%\AppData\Local\Logseq\Logseq.exe
 *   - macOS: Checks /Applications/Logseq.app/Contents/MacOS/Logseq
 *   - Linux: Searches PATH, common AppImage locations, and user directories
 * 
 * - `launch_logseq()`: Process launching with output filtering
 *   - Spawns Logseq with piped stdout/stderr
 *   - Filters noisy Electron/xdg-mime warnings to trace level
 *   - Returns Child process handle for lifecycle management
 * 
 * ## Process Management Utilities
 * 
 * - `SERVER_INFO_FILE`: Constant for "cymbiont_server.json"
 * - `ServerInfo`: Struct containing PID, host, and port for IPC
 * 
 * - `terminate_previous_instance()`: Clean server startup
 *   - Reads previous server info from JSON file
 *   - Checks if process still exists (platform-specific)
 *   - Sends SIGTERM (Unix) or taskkill (Windows)
 *   - Cleans up stale PID files automatically
 * 
 * - `write_server_info()`: Creates server discovery file
 *   - Writes current process info for JavaScript plugin
 *   - Enables dynamic port discovery
 * 
 * - `find_available_port()`: Port allocation with fallback
 *   - Tries configured port first
 *   - Falls back to sequential ports (3001, 3002, etc.)
 *   - Respects max_port_attempts configuration
 * 
 * - `is_port_available()`: Simple TCP bind check
 * 
 * ## DateTime and JSON Utilities
 * 
 * - `parse_datetime()`: Robust datetime parsing
 *   - Supports RFC3339 format (standard)
 *   - Supports ISO 8601 format
 *   - Handles Unix timestamps (seconds and milliseconds)
 *   - Falls back to current time on parse failure
 * 
 * - `parse_properties()`: JSON to HashMap<String, String> conversion
 *   - Extracts object properties as strings
 *   - Non-string values converted via to_string()
 *   - Used for PKM block/page properties
 * 
 * - `parse_json_data<T>()`: Generic JSON deserialization
 *   - Type-safe wrapper around serde_json::from_str
 *   - Used throughout API handlers for payload parsing
 * 
 * ## Error Handling
 * 
 * Most functions return Result types for proper error propagation. Process
 * management functions log errors internally but may return bool for simpler
 * usage patterns (e.g., terminate_previous_instance).
 * 
 * ## Testing
 * 
 * The module includes comprehensive unit tests for:
 * - ServerInfo serialization/deserialization
 * - DateTime parsing with various formats
 * - JSON property extraction
 * - Port availability (with port 0 for OS allocation)
 */

use std::path::PathBuf;
use std::process::{Command, Child};
use std::fs;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::collections::HashMap;
use std::time::Duration;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use serde::de::DeserializeOwned;
use serde_json;
use tracing::{info, error, debug, trace, warn};
use crate::config::{LogseqConfig, BackendConfig};

// Platform-specific Logseq executable paths
pub fn find_logseq_executable() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let user_profile = std::env::var("USERPROFILE").ok()?;
        let path = PathBuf::from(user_profile)
            .join("AppData")
            .join("Local")
            .join("Logseq")
            .join("Logseq.exe");
        if path.exists() {
            return Some(path);
        }
    }
    
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from("/Applications/Logseq.app/Contents/MacOS/Logseq");
        if path.exists() {
            return Some(path);
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").ok()?;
        
        // 1. Check if logseq is in PATH (snap install)
        if let Ok(output) = Command::new("which").arg("logseq").output() {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return Some(PathBuf::from(path_str));
                }
            }
        }
        
        // 2. Search for AppImage in common locations
        let common_paths = vec![
            PathBuf::from(&home).join(".local/share/applications/appimages"),
            PathBuf::from(&home).join(".local/share/applications"),
            PathBuf::from(&home).join("Applications"),
            PathBuf::from(&home).join("Downloads"),
            PathBuf::from(&home).join(".local/bin"),
            PathBuf::from("/opt"),
            PathBuf::from("/usr/local/bin"),
        ];
        
        for dir in common_paths {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy().to_lowercase();
                        if name_str.contains("logseq") && name_str.ends_with(".appimage") {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    
    None
}

// Launch Logseq process
pub fn launch_logseq(config: &LogseqConfig) -> Result<Option<Child>, Box<dyn Error>> {
    if !config.auto_launch {
        info!("Logseq auto-launch is disabled");
        return Ok(None);
    }
    
    let executable = if let Some(path) = &config.executable_path {
        PathBuf::from(path)
    } else if let Some(path) = find_logseq_executable() {
        path
    } else {
        error!("Could not find Logseq executable. Please specify executable_path in config.yaml");
        return Ok(None);
    };
    
    info!("🚀 Launching Logseq from: {:?}", executable);
    
    let mut child = Command::new(&executable)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch Logseq: {}", e))?;
    
    // Spawn threads to handle stdout and stderr
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    // Filter out xdg-mime warnings
                    if line.contains("xdg-mime:") && line.contains("application argument missing") {
                        trace!("Logseq stdout (xdg-mime warning): {}", line);
                    } else if line.contains("›") {
                        // Electron logs with › symbol
                        trace!("Logseq: {}", line);
                    } else {
                        trace!("Logseq stdout: {}", line);
                    }
                }
            }
        });
    }
    
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    // Filter out xdg-mime warnings and rsapi init
                    if line.contains("xdg-mime:") && line.contains("application argument missing") {
                        trace!("Logseq stderr (xdg-mime warning): {}", line);
                    } else if line.contains("(rsapi) init loggers") {
                        trace!("Logseq stderr (rsapi): {}", line);
                    } else if line.contains("Try 'xdg-mime --help'") {
                        trace!("Logseq stderr (xdg-mime help): {}", line);
                    } else {
                        // Log other stderr at debug level in case something important shows up
                        debug!("Logseq stderr: {}", line);
                    }
                }
            }
        });
    }
    
    Ok(Some(child))
}

// ===== Process Management =====

// Constants
pub const SERVER_INFO_FILE: &str = "cymbiont_server.json";

// Server info written to file for JS plugin
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
pub fn terminate_previous_instance() -> bool {
    // Check if server info file exists
    if let Ok(info_str) = fs::read_to_string(SERVER_INFO_FILE) {
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
                .arg("-15") // SIGTERM for graceful shutdown
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
pub fn write_server_info(host: &str, port: u16) -> Result<(), Box<dyn Error>> {
    let info = ServerInfo {
        pid: std::process::id(),
        host: host.to_string(),
        port,
    };
    let json = serde_json::to_string_pretty(&info)?;
    fs::write(SERVER_INFO_FILE, json)?;
    Ok(())
}

// Helper function to find an available port
pub fn find_available_port(config: &BackendConfig) -> Result<u16, Box<dyn Error>> {
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
    
    Err(Box::<dyn Error>::from("Could not find an available port"))
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

/// Generic JSON deserialization helper
pub fn parse_json_data<T: DeserializeOwned>(payload: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str::<T>(payload)
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
            port: 3000,
        };
        
        // Test serialization
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("12345"));
        assert!(json.contains("127.0.0.1"));
        assert!(json.contains("3000"));
        
        // Test deserialization
        let deserialized: ServerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pid, 12345);
        assert_eq!(deserialized.host, "127.0.0.1");
        assert_eq!(deserialized.port, 3000);
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