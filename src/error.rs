//! Centralized error handling for Cymbiont
//! 
//! This module provides a hierarchical error system that replaces the fragmented
//! error handling patterns throughout the codebase. All errors in Cymbiont should
//! ultimately be convertible to `CymbiontError` for consistent propagation.

use thiserror::Error;
use uuid::Uuid;

// Re-export for convenience
pub use crate::lock::RwLockExt;

/// Global result type alias for convenience
pub type Result<T> = std::result::Result<T, CymbiontError>;

/// Top-level error type for all Cymbiont operations
/// 
/// This error type provides a hierarchical structure where domain-specific
/// errors are wrapped in appropriate variants. All errors should be convertible
/// to this type through the `From` trait.
#[derive(Error, Debug)]
pub enum CymbiontError {
    /// Storage layer errors (registries, persistence, transactions)
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Agent-related errors (operations, LLM, tools)
    #[error("Agent error: {0}")]
    Agent(#[from] AgentError),

    /// Graph operation errors (CRUD, queries, lifecycle)
    #[error("Graph error: {0}")]
    Graph(#[from] GraphError),

    /// Server errors (HTTP, WebSocket, authentication)
    #[error("Server error: {0}")]
    Server(#[from] ServerError),

    /// Data import errors (Logseq, validation, parsing)
    #[error("Import error: {0}")]
    Import(#[from] ImportError),

    /// Configuration errors (YAML parsing, validation)
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// Lock errors (RwLock poisoning - rarely used, mostly panic)
    #[error("Lock error: {0}")]
    Lock(#[from] LockError),

    /// File system and I/O errors
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),

    /// Other errors not fitting specific categories
    #[error("Other error: {0}")]
    Other(String),
}

/// Storage layer errors consolidating registry, persistence, and transaction errors
#[derive(Error, Debug)]
pub enum StorageError {
    /// Graph registry management errors
    #[error("Graph registry error: {message}")]
    GraphRegistry { message: String },

    /// Agent registry management errors  
    #[error("Agent registry error: {message}")]
    AgentRegistry { message: String },

    /// Graph persistence errors (save/load/archive)
    #[error("Graph persistence error: {message}")]
    GraphPersistence { message: String },

    /// Agent persistence errors (save/load)
    #[error("Agent persistence error: {message}")]
    AgentPersistence { message: String },

    /// Transaction log errors (WAL operations)
    #[error("Transaction log error: {message}")]
    TransactionLog { message: String },

    /// Transaction coordination errors
    #[error("Transaction error: {message}")]
    Transaction { message: String },

    /// Entity not found errors
    #[error("Not found: {entity_type} with {identifier_type} '{identifier}'")]
    NotFound {
        entity_type: String,
        identifier_type: String,
        identifier: String,
    },


    /// Sled database errors
    #[error("Database error: {0}")]
    Database(#[from] sled::Error),

    /// JSON serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Agent operation errors consolidating agent, LLM, and tool errors
#[derive(Error, Debug)]
pub enum AgentError {
    /// LLM backend errors
    #[error("LLM error: {message}")]
    LLM { message: String },

    /// Knowledge graph tool errors
    #[error("Tool error: {message}")]
    Tool { message: String },

}

/// Graph operation errors for CRUD operations and queries
#[derive(Error, Debug)]
pub enum GraphError {
    /// Graph lifecycle errors (create, open, close, delete)
    #[error("Graph lifecycle error: {message}")]
    Lifecycle { message: String },

    /// Node operation errors (create, update, delete, find)
    #[error("Node operation error: {message}")]
    NodeOperation { message: String },

    /// Graph not found
    #[error("Graph not found: {identifier}")]
    NotFound { identifier: String },

    /// Node not found in graph
    #[error("Node not found: {node_id} in graph {graph_id}")]
    NodeNotFound { node_id: String, graph_id: Uuid },

    /// Invalid graph state
    #[error("Invalid graph state: {message}")]
    InvalidState { message: String },
}

/// Server errors for HTTP and WebSocket operations
#[derive(Error, Debug)]
pub enum ServerError {
    /// WebSocket protocol errors
    #[error("WebSocket error: {message}")]
    WebSocket { message: String },

    /// Authentication errors
    #[error("Authentication error: {message}")]
    Authentication { message: String },

    /// Server startup errors
    #[error("Startup error: {message}")]
    Startup { message: String },

    /// Port binding errors
    #[error("Port binding error: {message}")]
    PortBinding { message: String },

    /// Invalid request format
    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

}

/// Data import errors for Logseq and other formats
#[derive(Error, Debug)]
pub enum ImportError {
    /// File parsing errors
    #[error("Parse error in {file_path}: {message}")]
    Parse { file_path: String, message: String },

    /// Data validation errors
    #[error("Validation error: {message}")]
    Validation { message: String },

    /// Reference resolution errors
    #[error("Reference resolution error: {message}")]
    ReferenceResolution { message: String },

    /// Import path errors
    #[error("Path error: {message}")]
    Path { message: String },

    /// File system errors during import
    #[error("File system error during import: {0}")]
    FileSystem(#[from] std::io::Error),

    /// YAML parsing errors
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Configuration errors for YAML parsing and validation
#[derive(Error, Debug)]
pub enum ConfigError {
    /// YAML parsing errors
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// I/O errors reading config
    #[error("I/O error reading config: {0}")]
    IO(#[from] std::io::Error),
}

/// Lock errors for RwLock poisoning (rarely used, mostly panic)
#[derive(Error, Debug)]
pub enum LockError {
    /// RwLock poisoning error
    #[error("Lock poisoned: {message}")]
    Poisoned { message: String },

    /// Lock contention error (development-time detection)
    #[error("Lock contention detected: {message}")]
    Contention { message: String },
}

// Convenience From implementations for common error types

impl From<String> for CymbiontError {
    fn from(message: String) -> Self {
        CymbiontError::Other(message)
    }
}

impl From<&str> for CymbiontError {
    fn from(message: &str) -> Self {
        CymbiontError::Other(message.to_string())
    }
}


impl From<serde_json::Error> for CymbiontError {
    fn from(error: serde_json::Error) -> Self {
        CymbiontError::Storage(StorageError::Serialization(error))
    }
}

impl From<serde_yaml::Error> for CymbiontError {
    fn from(error: serde_yaml::Error) -> Self {
        CymbiontError::Config(ConfigError::Yaml(error))
    }
}

impl From<sled::Error> for CymbiontError {
    fn from(error: sled::Error) -> Self {
        CymbiontError::Storage(StorageError::Database(error))
    }
}

// Domain error convenience constructors
impl StorageError {
    pub fn graph_registry(message: impl Into<String>) -> Self {
        StorageError::GraphRegistry { message: message.into() }
    }

    pub fn agent_registry(message: impl Into<String>) -> Self {
        StorageError::AgentRegistry { message: message.into() }
    }

    pub fn graph_persistence(message: impl Into<String>) -> Self {
        StorageError::GraphPersistence { message: message.into() }
    }

    pub fn agent_persistence(message: impl Into<String>) -> Self {
        StorageError::AgentPersistence { message: message.into() }
    }

    pub fn transaction_log(message: impl Into<String>) -> Self {
        StorageError::TransactionLog { message: message.into() }
    }

    pub fn transaction(message: impl Into<String>) -> Self {
        StorageError::Transaction { message: message.into() }
    }

    pub fn not_found(entity_type: impl Into<String>, identifier_type: impl Into<String>, identifier: impl Into<String>) -> Self {
        StorageError::NotFound {
            entity_type: entity_type.into(),
            identifier_type: identifier_type.into(),
            identifier: identifier.into(),
        }
    }

}

impl AgentError {
    pub fn llm(message: impl Into<String>) -> Self {
        AgentError::LLM { message: message.into() }
    }

    pub fn tool(message: impl Into<String>) -> Self {
        AgentError::Tool { message: message.into() }
    }

}

impl GraphError {
    pub fn lifecycle(message: impl Into<String>) -> Self {
        GraphError::Lifecycle { message: message.into() }
    }

    pub fn node_operation(message: impl Into<String>) -> Self {
        GraphError::NodeOperation { message: message.into() }
    }

    pub fn not_found(identifier: impl Into<String>) -> Self {
        GraphError::NotFound { identifier: identifier.into() }
    }

    pub fn node_not_found(node_id: impl Into<String>, graph_id: Uuid) -> Self {
        GraphError::NodeNotFound {
            node_id: node_id.into(),
            graph_id,
        }
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        GraphError::InvalidState { message: message.into() }
    }
}

impl ServerError {
    pub fn websocket(message: impl Into<String>) -> Self {
        ServerError::WebSocket { message: message.into() }
    }

    pub fn authentication(message: impl Into<String>) -> Self {
        ServerError::Authentication { message: message.into() }
    }

    pub fn startup(message: impl Into<String>) -> Self {
        ServerError::Startup { message: message.into() }
    }

    pub fn port_binding(message: impl Into<String>) -> Self {
        ServerError::PortBinding { message: message.into() }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        ServerError::InvalidRequest { message: message.into() }
    }

}

impl ImportError {
    pub fn parse(file_path: impl Into<String>, message: impl Into<String>) -> Self {
        ImportError::Parse {
            file_path: file_path.into(),
            message: message.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        ImportError::Validation { message: message.into() }
    }

    pub fn reference_resolution(message: impl Into<String>) -> Self {
        ImportError::ReferenceResolution { message: message.into() }
    }

    pub fn path(message: impl Into<String>) -> Self {
        ImportError::Path { message: message.into() }
    }

}


impl LockError {
    pub fn poisoned(message: impl Into<String>) -> Self {
        LockError::Poisoned { message: message.into() }
    }

    pub fn contention(message: impl Into<String>) -> Self {
        LockError::Contention { message: message.into() }
    }
}