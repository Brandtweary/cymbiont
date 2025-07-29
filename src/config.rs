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
use tracing::{debug, error};
use std::error::Error;

// Configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub backend: BackendConfig,
    #[serde(default)]
    pub development: DevelopmentConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackendConfig {
    pub port: u16,
    pub max_port_attempts: u16,
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

fn default_incremental_interval_hours() -> u64 {
    2
}

fn default_full_interval_hours() -> u64 {
    168 // 7 days
}

fn default_enable_full_sync() -> bool {
    false
}

fn default_data_dir() -> String {
    "data".to_string()
}

// Default configuration
impl Default for Config {
    fn default() -> Self {
        Config {
            backend: BackendConfig {
                port: 3000,
                max_port_attempts: 10,
            },
            development: DevelopmentConfig {
                default_duration: None,
            },
            sync: SyncConfig::default(),
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.backend.port, 3000);
        assert_eq!(config.backend.max_port_attempts, 10);
        assert_eq!(config.development.default_duration, None);
        assert_eq!(config.sync.incremental_interval_hours, 2);
        assert_eq!(config.sync.full_interval_hours, 168);
        assert_eq!(config.sync.enable_full_sync, false);
        assert_eq!(config.data_dir, "data");
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

    #[test]
    fn test_data_dir_default() {
        assert_eq!(default_data_dir(), "data");
    }

    #[test]
    fn test_data_dir_serde_default() {
        let yaml = r#"
backend:
  port: 3000
  max_port_attempts: 10
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.data_dir, "data");
    }

    #[test]
    fn test_data_dir_custom() {
        let yaml = r#"
backend:
  port: 3000
  max_port_attempts: 10
data_dir: /custom/path
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.data_dir, "/custom/path");
    }
}