/**
 * @module config
 * @description Configuration management for the PKM Knowledge Graph backend
 * 
 * This module provides a flexible configuration system that supports both file-based
 * and default configurations, with special emphasis on maintaining consistency between
 * the Rust backend and external client components.
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
 * - `port`: Base port for HTTP server (default: 8888)
 * - `max_port_attempts`: Port search range (default: 10)
 * - `server_info_file`: Filename for server discovery info (default: "cymbiont_server.json")
 *   
 * When port 8888 is busy, the server tries 8889, 8890, etc., up to
 * 8888 + max_port_attempts. This enables multiple instances during development.
 * 
 * The server_info_file allows multiple Cymbiont instances to run simultaneously
 * without interfering with each other's discovery mechanisms.
 * 
 * ### DevelopmentConfig
 * - `default_duration`: Auto-exit after N seconds (default: None)
 * 
 * Useful for automated testing and development workflows. Production
 * deployments should leave this as None for indefinite operation.
 * 
 * 
 * ### Data Directory Configuration
 * - `data_dir`: Path for data storage (default: "data")
 * 
 * The data directory stores all persistent state including graph registries,
 * session information, knowledge graphs, and transaction logs. Can be specified
 * as absolute or relative path. Relative paths are resolved from the current
 * working directory. This setting enables:
 * - Storing data outside the Cymbiont installation directory
 * - Data isolation for testing environments
 * - Multi-user deployments with separate data stores
 * - CLI override via --data-dir flag
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
use tracing::error;

// Configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub backend: BackendConfig,
    #[serde(default)]
    pub development: DevelopmentConfig,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackendConfig {
    pub port: u16,
    pub max_port_attempts: u16,
    #[serde(default = "default_server_info_file")]
    pub server_info_file: String,
}


#[derive(Debug, Deserialize, Clone)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub default_duration: Option<u64>,
}


fn default_data_dir() -> String {
    "data".to_string()
}

fn default_server_info_file() -> String {
    "cymbiont_server.json".to_string()
}

// Default configuration
impl Default for Config {
    fn default() -> Self {
        Config {
            backend: BackendConfig {
                port: 8888,
                max_port_attempts: 10,
                server_info_file: default_server_info_file(),
            },
            development: DevelopmentConfig {
                default_duration: None,
            },
            data_dir: default_data_dir(),
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


// Load configuration from file
pub fn load_config(config_path: Option<String>) -> Config {
    // If explicit path provided, use it
    if let Some(path) = config_path {
        let config_file = PathBuf::from(path);
        match fs::read_to_string(&config_file) {
            Ok(contents) => {
                match serde_yaml::from_str(&contents) {
                    Ok(config) => {
                        return config;
                    },
                    Err(e) => {
                        error!("Error parsing {:?}: {}", config_file, e);
                    }
                }
            },
            Err(e) => {
                error!("Error reading {:?}: {}", config_file, e);
            }
        }
        // Fall through to default if explicit path fails
    }
    
    // Otherwise use the original logic
    // Check if we're in test mode via environment variable
    let is_test_mode = std::env::var("CYMBIONT_TEST_MODE").is_ok();
    
    // Determine the executable directory
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    
    // Try to find config file in parent directories
    let mut config_path = PathBuf::from(exe_dir);
    let mut found = false;
    
    // Determine which config file to look for
    let config_filename = if is_test_mode {
        "config.test.yaml"
    } else {
        "config.yaml"
    };
    
    // First check if config exists in the current directory
    if config_path.join(config_filename).exists() {
        found = true;
    } else {
        // Try up to 3 parent directories
        for _ in 0..3 {
            config_path = match config_path.parent() {
                Some(parent) => parent.to_path_buf(),
                None => break,
            };
            
            if config_path.join(config_filename).exists() {
                found = true;
                break;
            }
        }
    }
    
    // If config file was found, try to load it
    if found {
        let config_file = config_path.join(config_filename);
        match fs::read_to_string(&config_file) {
            Ok(contents) => {
                match serde_yaml::from_str(&contents) {
                    Ok(config) => {
                        return config;
                    },
                    Err(e) => {
                        error!("Error parsing {}: {}", config_filename, e);
                    }
                }
            },
            Err(e) => {
                error!("Error reading {}: {}", config_filename, e);
            }
        }
    }
    
    // If we get here, use default configuration
    Config::default()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.backend.port, 8888);
        assert_eq!(config.backend.max_port_attempts, 10);
        assert_eq!(config.development.default_duration, None);
        // Sync configuration removed - tests preserved for reference
        // assert_eq!(config.sync.incremental_interval_hours, 2);
        // assert_eq!(config.sync.full_interval_hours, 168);
        // assert_eq!(config.sync.enable_full_sync, false);
        assert_eq!(config.data_dir, "data");
    }

    #[test]
    fn test_development_config_default() {
        let config = DevelopmentConfig::default();
        assert_eq!(config.default_duration, None);
    }

    // Sync configuration tests removed - functionality no longer exists
    // #[test]
    // fn test_sync_config_default() {
    //     let config = SyncConfig::default();
    //     assert_eq!(config.incremental_interval_hours, 2);
    //     assert_eq!(config.full_interval_hours, 168);
    //     assert_eq!(config.enable_full_sync, false);
    // }

    // #[test]  
    // fn test_sync_default_functions() {
    //     assert_eq!(default_incremental_interval_hours(), 2);
    //     assert_eq!(default_full_interval_hours(), 168);
    //     assert_eq!(default_enable_full_sync(), false);
    // }

    #[test]
    fn test_data_dir_default() {
        assert_eq!(default_data_dir(), "data");
    }

    #[test]
    fn test_data_dir_serde_default() {
        let yaml = r#"
backend:
  port: 8888
  max_port_attempts: 10
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.data_dir, "data");
    }

    #[test]
    fn test_data_dir_custom() {
        let yaml = r#"
backend:
  port: 8888
  max_port_attempts: 10
data_dir: /custom/path
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.data_dir, "/custom/path");
    }
}