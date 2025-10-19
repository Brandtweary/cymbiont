//! Configuration system with flexible discovery and validation
//!
//! Loads YAML configuration from multiple standard locations, supporting both
//! development workflows (git repo with local config) and production deployments
//! (system-wide install with XDG standard paths).
//!
//! # Configuration Discovery
//!
//! The system searches for `config.yaml` in the following order:
//!
//! 1. **CYMBIONT_CONFIG environment variable**: Explicit override path
//!    - If set but file doesn't exist, returns error (fail-fast)
//!    - Use for testing or custom deployments
//!
//! 2. **./config.yaml**: Current working directory
//!    - When CWD is set to cymbiont repo root
//!    - Ignored by git (in .gitignore)
//!
//! 3. **cymbiont/config.yaml**: Repo root (relative to binary location)
//!    - Binary is at `cymbiont/target/debug/cymbiont`
//!    - Goes up 2 levels to find `cymbiont/config.yaml`
//!    - Works regardless of where MCP server is launched from
//!
//! 4. **~/.config/cymbiont/config.yaml**: XDG standard location
//!    - For production system-wide installs
//!    - Uses `directories` crate for cross-platform XDG support
//!
//! 5. **Defaults**: If no config found, use hardcoded defaults
//!    - Logs warning
//!    - Corpus path defaults to None (document sync disabled)
//!
//! # Configuration Structure
//!
//! The config file is divided into logical sections:
//!
//! - **graphiti**: Graphiti backend connection (base_url, timeout, server_path)
//! - **similarity**: Search thresholds for semantic matching
//! - **corpus**: Document sync settings (path, sync_interval)
//! - **logging**: Log output configuration (level, directory, rotation)
//! - **verbosity**: Autodebugger verbosity monitoring thresholds
//!
//! # Path Requirements
//!
//! Paths are validated and normalized during config load:
//!
//! - **log_directory**: Can be relative (resolved from binary location) or absolute
//! - **corpus.path**: Optional; if provided, must be absolute
//! - **graphiti.server_path**: Can be relative (resolved from binary location) or absolute
//!
//! Relative paths are resolved from the binary's parent directory, enabling portable
//! installs where bundled components live alongside the binary.
//!
//! # Example
//!
//! ```yaml
//! graphiti:
//!   base_url: "http://localhost:8000"
//!   timeout_secs: 30
//!   default_group_id: "default"
//!   server_path: "/absolute/path/to/graphiti-cymbiont"
//!
//! corpus:
//!   path: "/absolute/path/to/corpus"
//!   sync_interval_hours: 1.0
//!
//! logging:
//!   level: "info"
//!   log_directory: "logs"  # Relative to binary, or absolute path
//!   max_files: 10
//!   max_size_mb: 5
//! ```

use crate::error::ConfigError;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
    #[serde(default = "default_server_path")]
    pub server_path: String,
}

fn default_server_path() -> String {
    "../../graphiti-cymbiont/server".to_string()
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
    pub path: Option<String>,
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
            server_path: "../../graphiti-cymbiont/server".to_string(), // Bundled graphiti-cymbiont
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
            path: None,
            sync_interval_hours: 1.0,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "debug".to_string(),
            output: "file".to_string(),
            log_directory: "logs".to_string(), // Relative to binary location
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
    /// Load config from config.yaml with flexible location search
    ///
    /// Search order:
    /// 1. CYMBIONT_CONFIG environment variable (explicit override)
    /// 2. ./config.yaml (current directory)
    /// 3. cymbiont/config.yaml (repo root, relative to binary)
    /// 4. ~/.config/cymbiont/config.yaml (XDG standard location)
    /// 5. Defaults (if no config file found)
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::find_config_file()?;

        if let Some(path) = config_path {
            tracing::info!("Loading config from: {}", path.display());

            let contents = fs::read_to_string(&path)
                .map_err(|e| ConfigError::Io(e.to_string()))?;

            let mut config: Config = serde_yaml::from_str(&contents)
                .map_err(|e| ConfigError::Parse(e.to_string()))?;

            // Validate and enforce absolute paths
            config.validate_paths()?;

            Ok(config)
        } else {
            tracing::warn!("No config.yaml found, using defaults");
            Ok(Config::default())
        }
    }

    /// Search for config.yaml in standard locations
    fn find_config_file() -> Result<Option<PathBuf>, ConfigError> {
        // 1. Check CYMBIONT_CONFIG environment variable
        if let Ok(env_path) = std::env::var("CYMBIONT_CONFIG") {
            let path = PathBuf::from(env_path);
            if path.exists() {
                return Ok(Some(path));
            } else {
                return Err(ConfigError::Io(format!(
                    "CYMBIONT_CONFIG points to non-existent file: {}",
                    path.display()
                )));
            }
        }

        // 2. Check current directory
        let cwd_config = PathBuf::from("./config.yaml");
        if cwd_config.exists() {
            return Ok(Some(cwd_config));
        }

        // 3. Check cymbiont repo root (relative to binary location)
        // Binary is at cymbiont/target/debug/cymbiont, go up 2 levels to cymbiont/
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                // exe_dir is target/debug/, go up 2 levels
                if let Some(repo_root) = exe_dir.parent().and_then(|p| p.parent()) {
                    let repo_config = repo_root.join("config.yaml");
                    if repo_config.exists() {
                        return Ok(Some(repo_config));
                    }
                }
            }
        }

        // 4. Check XDG config directory (~/.config/cymbiont/config.yaml)
        if let Some(proj_dirs) = ProjectDirs::from("", "", "cymbiont") {
            let xdg_config = proj_dirs.config_dir().join("config.yaml");
            if xdg_config.exists() {
                return Ok(Some(xdg_config));
            }
        }

        // No config found
        Ok(None)
    }

    /// Validate and normalize paths
    /// - log_directory: Can be relative (resolved from binary location) or absolute
    /// - corpus.path: Optional; if provided, must be absolute
    /// - server_path: Can be relative (resolved from binary location) or absolute
    fn validate_paths(&mut self) -> Result<(), ConfigError> {
        // Resolve log_directory relative to binary location if not absolute
        let log_path = Path::new(&self.logging.log_directory);
        if !log_path.is_absolute() {
            // Get binary directory
            let exe_path = std::env::current_exe()
                .map_err(|e| ConfigError::Validation(format!("Failed to get binary path: {}", e)))?;
            let exe_dir = exe_path.parent()
                .ok_or_else(|| ConfigError::Validation("Binary has no parent directory".to_string()))?;

            // Resolve relative path from binary location
            let resolved = exe_dir.join(&self.logging.log_directory);
            self.logging.log_directory = resolved.to_string_lossy().to_string();
        }

        // Corpus path is optional; if provided, must be absolute
        if let Some(corpus_path_str) = &self.corpus.path {
            let corpus_path = Path::new(corpus_path_str);
            if !corpus_path.is_absolute() {
                return Err(ConfigError::Validation(format!(
                    "corpus.path must be an absolute path, got: {}",
                    corpus_path_str
                )));
            }
            if !corpus_path.exists() {
                return Err(ConfigError::Validation(format!(
                    "corpus.path does not exist: {}",
                    corpus_path_str
                )));
            }
            if !corpus_path.is_dir() {
                return Err(ConfigError::Validation(format!(
                    "corpus.path must be a directory, got: {}",
                    corpus_path_str
                )));
            }
        }

        // Graphiti server path - resolve relative paths from binary location
        if self.graphiti.server_path.is_empty() {
            return Err(ConfigError::Validation(
                "graphiti.server_path is required - please configure it in config.yaml".to_string()
            ));
        }

        let server_path = Path::new(&self.graphiti.server_path);
        let resolved_server_path = if !server_path.is_absolute() {
            // Get binary directory and resolve relative path
            let exe_path = std::env::current_exe()
                .map_err(|e| ConfigError::Validation(format!("Failed to get binary path: {}", e)))?;
            let exe_dir = exe_path.parent()
                .ok_or_else(|| ConfigError::Validation("Binary has no parent directory".to_string()))?;

            exe_dir.join(&self.graphiti.server_path)
        } else {
            server_path.to_path_buf()
        };

        // Verify resolved path exists
        if !resolved_server_path.exists() {
            return Err(ConfigError::Validation(format!(
                "graphiti.server_path does not exist: {} (resolved to: {})",
                self.graphiti.server_path,
                resolved_server_path.display()
            )));
        }

        // Update to absolute path
        self.graphiti.server_path = resolved_server_path.to_string_lossy().to_string();

        Ok(())
    }
}