//! Configuration system
//! Loads config from config.yaml or falls back to defaults

use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub graphiti: GraphitiConfig,
    pub similarity: SimilarityConfig,
    pub corpus: CorpusConfig,
    pub logging: LoggingConfig,
    pub verbosity: VerbosityConfig,
}

/// Graphiti backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphitiConfig {
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_group_id: String,
    pub server_path: String,
}

/// Similarity search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SimilarityConfig {
    pub min_score: f64,
}

/// Document corpus sync configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CorpusConfig {
    pub path: String,
    pub sync_interval_hours: f64,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub output: String,
    pub log_directory: String,
    pub max_files: usize,
    pub max_size_mb: usize,
    pub console_output: bool,
}

/// Verbosity monitoring thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VerbosityConfig {
    pub info_threshold: usize,
    pub debug_threshold: usize,
    pub trace_threshold: usize,
}

// Default implementations

impl Default for Config {
    fn default() -> Self {
        Self {
            graphiti: GraphitiConfig::default(),
            similarity: SimilarityConfig::default(),
            corpus: CorpusConfig::default(),
            logging: LoggingConfig::default(),
            verbosity: VerbosityConfig::default(),
        }
    }
}

impl Default for GraphitiConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8000".to_string(),
            timeout_secs: 30,
            default_group_id: "default".to_string(),
            server_path: "/home/brandt/projects/hector/graphiti-cymbiont/server".to_string(),
        }
    }
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self { min_score: 0.7 }
    }
}

impl Default for CorpusConfig {
    fn default() -> Self {
        Self {
            path: "/home/brandt/projects/hector/corpus".to_string(),
            sync_interval_hours: 1.0,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            output: "file".to_string(),
            log_directory: "/home/brandt/projects/hector/cymbiont/logs".to_string(),
            max_files: 10,
            max_size_mb: 5,
            console_output: false, // CRITICAL for MCP mode
        }
    }
}

impl Default for VerbosityConfig {
    fn default() -> Self {
        Self {
            info_threshold: 50,
            debug_threshold: 100,
            trace_threshold: 200,
        }
    }
}

// Config loading

impl Config {
    /// Load config from config.yaml or use defaults
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Path::new("config.yaml");

        if config_path.exists() {
            let contents = fs::read_to_string(config_path)
                .map_err(|e| ConfigError::Io(e.to_string()))?;

            let mut config: Config = serde_yaml::from_str(&contents)
                .map_err(|e| ConfigError::Parse(e.to_string()))?;

            // Validate and enforce absolute paths
            config.validate_paths()?;

            Ok(config)
        } else {
            // No config file - use defaults
            Ok(Config::default())
        }
    }

    /// Validate that all paths are absolute
    /// This is critical for MCP mode where the working directory is unpredictable
    fn validate_paths(&mut self) -> Result<(), ConfigError> {
        // Check log_directory
        let log_path = Path::new(&self.logging.log_directory);
        if !log_path.is_absolute() {
            return Err(ConfigError::Validation(format!(
                "log_directory must be an absolute path, got: {}",
                self.logging.log_directory
            )));
        }

        // Check corpus path
        let corpus_path = Path::new(&self.corpus.path);
        if !corpus_path.is_absolute() {
            return Err(ConfigError::Validation(format!(
                "corpus.path must be an absolute path, got: {}",
                self.corpus.path
            )));
        }

        // Check graphiti server path
        let server_path = Path::new(&self.graphiti.server_path);
        if !server_path.is_absolute() {
            return Err(ConfigError::Validation(format!(
                "graphiti.server_path must be an absolute path, got: {}",
                self.graphiti.server_path
            )));
        }

        Ok(())
    }
}
