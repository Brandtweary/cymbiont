/**
 * @module config
 * @description Configuration management for the PKM Knowledge Graph backend
 * 
 * This module provides a flexible configuration system that supports both file-based
 * and default configurations, with special emphasis on maintaining consistency between
 * the Rust backend and JavaScript plugin components.
 * 
 * ## Configuration Loading Strategy
 * 
 * The `load_config()` function implements a smart search algorithm:
 * 1. Start from the executable's directory
 * 2. Search up to 3 parent directories for config.yaml
 * 3. Fall back to hardcoded defaults if no file found
 * 
 * This approach supports multiple deployment scenarios:
 * - Development: config.yaml in project root
 * - Testing: config.yaml in backend directory
 * - Production: config.yaml alongside executable
 * 
 * ## Configuration Structures
 * 
 * ### Config (root)
 * Top-level container aggregating all configuration sections.
 * 
 * ### BackendConfig
 * - `port`: Base port for HTTP server (default: 3000)
 * - `max_port_attempts`: Port search range (default: 10)
 *   
 * When port 3000 is busy, the server tries 3001, 3002, etc., up to
 * 3000 + max_port_attempts. This enables multiple instances during development.
 * 
 * ### LogseqConfig
 * - `auto_launch`: Enable automatic Logseq startup (default: false)
 * - `executable_path`: Optional override for Logseq location
 * 
 * If executable_path is not specified, the system uses platform-specific
 * discovery logic (see utils::find_logseq_executable).
 * 
 * ### DevelopmentConfig
 * - `default_duration`: Auto-exit after N seconds (default: None)
 * 
 * Useful for automated testing and development workflows. Production
 * deployments should leave this as None for indefinite operation.
 * 
 * ### SyncConfig
 * - `incremental_interval_hours`: Hours between incremental syncs (default: 2)
 * - `full_interval_hours`: Hours between full database syncs (default: 168/7 days)
 * - `enable_full_sync`: Whether to perform full syncs at all (default: false)
 * 
 * Incremental sync only processes blocks/pages modified since last sync.
 * Full sync re-processes the entire PKM, catching external file modifications.
 * 
 * ## JavaScript Plugin Validation
 * 
 * The `validate_js_plugin_config()` function ensures the JavaScript plugin's
 * hardcoded configuration matches the Rust backend:
 * 
 * 1. Reads logseq_plugin/api.js to extract defaultPort and maxPortAttempts constants
 * 2. Compares with loaded Rust configuration
 * 3. Logs detailed errors if mismatches found
 * 4. Continues operation (non-fatal) but plugin may fail to connect
 * 
 * This validation catches a common deployment error where config.yaml is
 * updated but the JavaScript constants are forgotten.
 * 
 * ## Default Values
 * 
 * All configuration structures implement Default trait for robustness:
 * - Missing sections use defaults via serde(default)
 * - Individual fields use field-level defaults where appropriate
 * - Entire config falls back to Config::default() if file errors occur
 * 
 * ## Error Handling
 * 
 * Configuration loading is resilient:
 * - File not found: Use defaults (common in development)
 * - Parse errors: Log and use defaults (prevents startup failure)
 * - Validation errors: Log warnings but continue operation
 * 
 * This approach prioritizes service availability over configuration perfection.
 */

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::fs;
use regex::Regex;
use tracing::{debug, error, warn};
use std::error::Error;

// Configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub backend: BackendConfig,
    #[serde(default)]
    pub logseq: LogseqConfig,
    #[serde(default)]
    pub development: DevelopmentConfig,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackendConfig {
    pub port: u16,
    pub max_port_attempts: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LogseqConfig {
    #[serde(default = "default_auto_launch")]
    pub auto_launch: bool,
    #[serde(default)]
    pub executable_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub default_duration: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SyncConfig {
    #[serde(default = "default_incremental_interval_hours")]
    pub incremental_interval_hours: u64,
    #[serde(default = "default_full_interval_hours")]
    pub full_interval_hours: u64,
    #[serde(default = "default_enable_full_sync")]
    pub enable_full_sync: bool,
}

fn default_auto_launch() -> bool {
    false
}

fn default_incremental_interval_hours() -> u64 {
    2
}

fn default_full_interval_hours() -> u64 {
    168 // 7 days
}

fn default_enable_full_sync() -> bool {
    false
}

// Default configuration
impl Default for Config {
    fn default() -> Self {
        Config {
            backend: BackendConfig {
                port: 3000,
                max_port_attempts: 10,
            },
            logseq: LogseqConfig {
                auto_launch: false,
                executable_path: None,
            },
            development: DevelopmentConfig {
                default_duration: None,
            },
            sync: SyncConfig::default(),
        }
    }
}

impl Default for LogseqConfig {
    fn default() -> Self {
        LogseqConfig {
            auto_launch: false,
            executable_path: None,
        }
    }
}

impl Default for DevelopmentConfig {
    fn default() -> Self {
        DevelopmentConfig {
            default_duration: None,
        }
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            incremental_interval_hours: default_incremental_interval_hours(),
            full_interval_hours: default_full_interval_hours(),
            enable_full_sync: default_enable_full_sync(),
        }
    }
}

// Load configuration from file
pub fn load_config() -> Config {
    // Determine the executable directory
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    
    // Try to find config.yaml in parent directories
    let mut config_path = PathBuf::from(exe_dir);
    let mut found = false;
    
    // First check if config exists in the current directory
    if config_path.join("config.yaml").exists() {
        found = true;
    } else {
        // Try up to 3 parent directories
        for _ in 0..3 {
            config_path = match config_path.parent() {
                Some(parent) => parent.to_path_buf(),
                None => break,
            };
            
            if config_path.join("config.yaml").exists() {
                found = true;
                break;
            }
        }
    }
    
    // If config.yaml was found, try to load it
    if found {
        let config_file = config_path.join("config.yaml");
        match fs::read_to_string(&config_file) {
            Ok(contents) => {
                match serde_yaml::from_str(&contents) {
                    Ok(config) => {
                        debug!("📄 Loaded configuration from {:?}", config_file);
                        return config;
                    },
                    Err(e) => {
                        error!("Error parsing config.yaml: {}", e);
                    }
                }
            },
            Err(e) => {
                error!("Error reading config.yaml: {}", e);
            }
        }
    }
    
    // If we get here, use default configuration
    debug!("📄 Using default configuration");
    Config::default()
}

// Validate that JavaScript plugin configuration matches Rust configuration
pub fn validate_js_plugin_config(config: &Config) -> Result<(), Box<dyn Error>> {
    // api.js is now in logseq_plugin/api.js relative to the manifest directory
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let api_js_path = manifest_dir
        .join("logseq_plugin")
        .join("api.js");
    
    if !api_js_path.exists() {
        warn!("JavaScript API file not found at {:?} - skipping config validation", api_js_path);
        return Ok(());
    };
    
    let api_js_content = fs::read_to_string(&api_js_path)?;
    
    // Extract defaultPort and maxPortAttempts from JavaScript
    let default_port_regex = Regex::new(r"const\s+defaultPort\s*=\s*(\d+)")?;
    let max_attempts_regex = Regex::new(r"const\s+maxPortAttempts\s*=\s*(\d+)")?;
    
    let js_default_port = default_port_regex
        .captures(&api_js_content)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u16>().ok());
        
    let js_max_attempts = max_attempts_regex
        .captures(&api_js_content)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u16>().ok());
    
    // Compare with Rust configuration
    let rust_default_port = config.backend.port;
    let rust_max_attempts = config.backend.max_port_attempts;
    
    let mut config_errors = Vec::new();
    
    if let Some(js_port) = js_default_port {
        if js_port != rust_default_port {
            config_errors.push(format!(
                "Port mismatch: JavaScript defaultPort={}, Rust port={}",
                js_port, rust_default_port
            ));
        }
    } else {
        config_errors.push("Could not find defaultPort in JavaScript API file".to_string());
    }
    
    if let Some(js_attempts) = js_max_attempts {
        if js_attempts != rust_max_attempts {
            config_errors.push(format!(
                "Max attempts mismatch: JavaScript maxPortAttempts={}, Rust max_port_attempts={}",
                js_attempts, rust_max_attempts
            ));
        }
    } else {
        config_errors.push("Could not find maxPortAttempts in JavaScript API file".to_string());
    }
    
    if !config_errors.is_empty() {
        error!("JavaScript plugin configuration validation failed:");
        for err in &config_errors {
            error!("  {}", err);
        }
        error!("Please ensure api.js uses the same port configuration as config.yaml");
        error!("JavaScript: const defaultPort = {}; const maxPortAttempts = {};", 
               rust_default_port, rust_max_attempts);
        
        // Don't fail the server startup, just warn loudly
        warn!("Continuing startup despite configuration mismatch - plugin may not work correctly");
    } else {
        debug!("✅ JavaScript plugin configuration validated successfully");
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.backend.port, 3000);
        assert_eq!(config.backend.max_port_attempts, 10);
        assert_eq!(config.logseq.auto_launch, false);
        assert_eq!(config.logseq.executable_path, None);
        assert_eq!(config.development.default_duration, None);
        assert_eq!(config.sync.incremental_interval_hours, 2);
        assert_eq!(config.sync.full_interval_hours, 168);
        assert_eq!(config.sync.enable_full_sync, false);
    }

    #[test]
    fn test_default_auto_launch() {
        assert_eq!(default_auto_launch(), false);
    }

    #[test]
    fn test_logseq_config_default() {
        let config = LogseqConfig::default();
        assert_eq!(config.auto_launch, false);
        assert_eq!(config.executable_path, None);
    }

    #[test]
    fn test_development_config_default() {
        let config = DevelopmentConfig::default();
        assert_eq!(config.default_duration, None);
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.incremental_interval_hours, 2);
        assert_eq!(config.full_interval_hours, 168);
        assert_eq!(config.enable_full_sync, false);
    }

    #[test]
    fn test_sync_default_functions() {
        assert_eq!(default_incremental_interval_hours(), 2);
        assert_eq!(default_full_interval_hours(), 168);
        assert_eq!(default_enable_full_sync(), false);
    }
}