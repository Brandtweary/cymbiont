//! Error types for Cymbiont MCP server

use thiserror::Error;

/// Graphiti HTTP client errors
#[derive(Error, Debug)]
pub enum GraphitiError {
    #[error("HTTP request failed: {0}")]
    Request(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Configuration loading errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Parse error: {0}")]
    Parse(String),
}
