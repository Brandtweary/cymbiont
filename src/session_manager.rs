//! Session Manager: Logseq Database Session Management
//!
//! This module provides session management for launching Logseq with specific databases
//! and switching between them using the URL scheme. It coordinates with the GraphRegistry
//! to resolve user-friendly names and paths to internal graph IDs.
//!
//! ## Key Features
//!
//! - **Flexible Launch**: Support for last active, configured default, or CLI-specified database
//! - **Name/Path Resolution**: Accept both database names and paths, resolving to graph IDs
//! - **Pre-registration**: Register configured databases before first connection
//! - **Session Persistence**: Remember last active database across restarts
//! - **Platform Support**: Cross-platform URL scheme invocation
//!
//! ## Launch Behavior
//!
//! The session manager determines which database to launch based on:
//! 1. CLI override (--graph or --graph-path)
//! 2. Config preference (launch_specific_database)
//! 3. Last active database (default behavior)

use crate::graph_registry::GraphRegistry;
use crate::config::LogseqConfig;
use crate::utils;
use std::sync::{Arc, Mutex};
use tokio::sync::{RwLock, oneshot};
use std::path::PathBuf;
use std::fs;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, warn, error, debug};
use async_trait::async_trait;

/// Session manager errors
#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Failed to launch Logseq: {0}")]
    LaunchError(String),
    
    #[error("Failed to switch database: {0}")]
    SwitchError(String),
    
    #[error("Database not found: {0}")]
    DatabaseNotFound(String),
    
    #[error("Registry error: {0}")]
    RegistryError(#[from] crate::graph_registry::GraphRegistryError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Graph switch timeout")]
    SwitchTimeout,
}

type Result<T> = std::result::Result<T, SessionError>;

/// Trait for notifying about graph switches
#[async_trait]
pub trait GraphSwitchNotifier: Send + Sync {
    async fn notify_graph_switch(&self, target_graph_id: &str, target_graph_name: &str, target_graph_path: &str) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Session manager for Logseq database management
pub struct SessionManager {
    graph_registry: Arc<Mutex<GraphRegistry>>,
    logseq_config: LogseqConfig,
    session_state: Arc<RwLock<SessionState>>,
    session_file: PathBuf,
    switch_confirmations: Arc<RwLock<HashMap<String, oneshot::Sender<()>>>>,
}

/// Current session state
#[derive(Debug, Clone)]
pub enum SessionState {
    /// No Logseq process running
    Inactive,
    /// Logseq is starting up
    Starting,
    /// Logseq is running with active database
    Active { graph_id: String },
    /// Switching between databases
    Switching { from_id: String, to_id: String },
}

/// Database identifier (name or path)
#[derive(Debug, Clone)]
pub enum DbIdentifier {
    Name(String),
    Path(String),
}

/// Session persistence data
#[derive(Debug, Serialize, Deserialize)]
struct SessionData {
    last_active_graph_id: Option<String>,
    last_active_timestamp: Option<i64>,
}

/// Session information for API responses
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub state: String,
    pub active_graph_id: Option<String>,
    pub active_graph_name: Option<String>,
    pub active_graph_path: Option<String>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(registry: Arc<Mutex<GraphRegistry>>, config: LogseqConfig) -> Self {
        // Use absolute path for session file
        let session_file = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("data")
            .join("last_session.json");
            
        Self {
            graph_registry: registry,
            logseq_config: config,
            session_state: Arc::new(RwLock::new(SessionState::Inactive)),
            session_file,
            switch_confirmations: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Launch Logseq with optional target database
    /// Returns the child process handle if Logseq was launched
    pub async fn launch_logseq(&self, cli_override: Option<DbIdentifier>) -> Result<Option<std::process::Child>> {
        // Update state
        {
            let mut state = self.session_state.write().await;
            *state = SessionState::Starting;
        }
        
        // Pre-register any configured databases
        self.pre_register_databases()?;
        
        // Determine target
        let target_graph_id = self.determine_launch_target(cli_override)?;
        
        // Launch Logseq process
        info!("🚀 Launching Logseq...");
        let child = utils::launch_logseq(&self.logseq_config)
            .map_err(|e| SessionError::LaunchError(e.to_string()))?;
        
        if child.is_none() {
            warn!("Logseq launch was skipped (auto_launch may be disabled)");
            return Ok(None);
        }
        
        let child = child.unwrap();
        
        // If we have a target, switch to it after launch
        if let Some(graph_id) = target_graph_id {
            info!("Waiting for Logseq to initialize before switching database...");
            // Wait for Logseq to fully initialize
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            
            // Try to switch to the target database
            match self.switch_to_graph(&graph_id).await {
                Ok(_) => info!("Successfully switched to target database"),
                Err(e) => {
                    // Log error but don't fail launch
                    warn!("Could not switch to target database: {}", e);
                    warn!("Logseq will open with its default database");
                    warn!("This might be because the URL scheme is not properly registered");
                    warn!("Or the graph name in Logseq differs from the directory name");
                }
            }
        } else {
            // No specific target, just mark as active
            let mut state = self.session_state.write().await;
            *state = SessionState::Active { graph_id: String::new() };
        }
        
        Ok(Some(child))
    }
    
    /// Switch to a different database
    pub async fn switch_database(&self, identifier: DbIdentifier) -> Result<()> {
        let graph_id = self.resolve_identifier(identifier)?;
        self.switch_to_graph(&graph_id).await
    }
    
    /// Switch to a different database with notification support
    pub async fn switch_database_with_notifier<N>(&self, identifier: DbIdentifier, notifier: &N) -> Result<()> 
    where
        N: GraphSwitchNotifier
    {
        let graph_id = self.resolve_identifier(identifier)?;
        self.switch_to_graph_with_notifier(&graph_id, notifier).await
    }
    
    /// Get current session information
    pub async fn get_session_info(&self) -> SessionInfo {
        let state = self.session_state.read().await;
        
        let (state_str, active_id) = match &*state {
            SessionState::Inactive => ("inactive".to_string(), None),
            SessionState::Starting => ("starting".to_string(), None),
            SessionState::Active { graph_id } => ("active".to_string(), Some(graph_id.clone())),
            SessionState::Switching { from_id, to_id } => {
                (format!("switching from {} to {}", from_id, to_id), Some(from_id.clone()))
            }
        };
        
        // Get graph details if active
        let (name, path) = if let Some(ref id) = active_id {
            if let Ok(registry) = self.graph_registry.lock() {
                if let Some(info) = registry.get_graph(id) {
                    (Some(info.name.clone()), Some(info.path.clone()))
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        
        SessionInfo {
            state: state_str,
            active_graph_id: active_id,
            active_graph_name: name,
            active_graph_path: path,
        }
    }
    
    /// Internal switch implementation (without confirmation)
    async fn switch_to_graph(&self, graph_id: &str) -> Result<()> {
        // Update state to switching
        {
            let current_state = self.session_state.read().await.clone();
            let from_id = match current_state {
                SessionState::Active { graph_id } => graph_id,
                _ => String::new(),
            };
            
            let mut state = self.session_state.write().await;
            *state = SessionState::Switching { 
                from_id: from_id.clone(), 
                to_id: graph_id.to_string() 
            };
        }
        
        // Get graph info from registry
        let (db_name, url) = {
            let registry = self.graph_registry.lock()
                .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
                
            let graph_info = registry.get_graph(graph_id)
                .ok_or_else(|| SessionError::DatabaseNotFound(graph_id.to_string()))?;
            
            let db_name = graph_info.name.clone();
            let url = format!("logseq://graph/{}", db_name);
            
            debug!("Graph info - ID: {}, Name: {}, Path: {}", 
                   graph_info.id, graph_info.name, graph_info.path);
            debug!("Attempting to open URL: {}", url);
            
            (db_name, url)
        };
        
        info!("Switching to database '{}' via URL scheme: {}", db_name, url);
        
        // Open URL using platform-specific command
        utils::open_url(&url)
            .map_err(|e| SessionError::SwitchError(format!("Failed to open URL: {}", e)))?;
        
        // Update session state (no confirmation for internal switches)
        {
            let mut state = self.session_state.write().await;
            *state = SessionState::Active { graph_id: graph_id.to_string() };
        }
        
        // Update active graph in registry
        let mut registry = self.graph_registry.lock()
            .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
        registry.set_active_graph(graph_id)?;
        
        // Save as last active
        self.save_last_active(graph_id)?;
        
        Ok(())
    }
    
    /// Switch with notification support and confirmation
    async fn switch_to_graph_with_notifier<N>(&self, graph_id: &str, notifier: &N) -> Result<()> 
    where
        N: GraphSwitchNotifier
    {
        // Update state to switching
        {
            let current_state = self.session_state.read().await.clone();
            let from_id = match current_state {
                SessionState::Active { graph_id } => graph_id,
                _ => String::new(),
            };
            
            let mut state = self.session_state.write().await;
            *state = SessionState::Switching { 
                from_id: from_id.clone(), 
                to_id: graph_id.to_string() 
            };
        }
        
        // Get graph info from registry
        let (db_name, url, graph_info) = {
            let registry = self.graph_registry.lock()
                .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
                
            let graph_info = registry.get_graph(graph_id)
                .ok_or_else(|| SessionError::DatabaseNotFound(graph_id.to_string()))?
                .clone();
            
            let db_name = graph_info.name.clone();
            let url = format!("logseq://graph/{}", db_name);
            
            debug!("Graph info - ID: {}, Name: {}, Path: {}", 
                   graph_info.id, graph_info.name, graph_info.path);
            debug!("Attempting to open URL: {}", url);
            
            (db_name, url, graph_info)
        };
        
        info!("Switching to database '{}' via URL scheme: {}", db_name, url);
        
        // Set up confirmation channel
        let (tx, rx) = oneshot::channel();
        {
            let mut confirmations = self.switch_confirmations.write().await;
            confirmations.insert(graph_id.to_string(), tx);
        }
        
        // Notify via WebSocket
        if let Err(e) = notifier.notify_graph_switch(&graph_info.id, &graph_info.name, &graph_info.path).await {
            warn!("Failed to notify graph switch: {:?}", e);
        }
        
        // Open URL using platform-specific command
        utils::open_url(&url)
            .map_err(|e| SessionError::SwitchError(format!("Failed to open URL: {}", e)))?;
        
        // Wait for confirmation with timeout
        let timeout = tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            rx
        ).await;
        
        match timeout {
            Ok(Ok(())) => {
                info!("Graph switch confirmed by plugin");
                // Update session state
                {
                    let mut state = self.session_state.write().await;
                    *state = SessionState::Active { graph_id: graph_id.to_string() };
                }
            }
            Ok(Err(_)) => {
                warn!("Graph switch confirmation channel was dropped");
                // Update state anyway - best effort
                {
                    let mut state = self.session_state.write().await;
                    *state = SessionState::Active { graph_id: graph_id.to_string() };
                }
            }
            Err(_) => {
                // Timeout - clean up pending confirmation
                {
                    let mut confirmations = self.switch_confirmations.write().await;
                    confirmations.remove(graph_id);
                }
                
                error!("Graph switch confirmation timeout after 10 seconds");
                return Err(SessionError::SwitchTimeout);
            }
        }
        
        // Update active graph in registry
        let mut registry = self.graph_registry.lock()
            .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
        registry.set_active_graph(graph_id)?;
        
        // Save as last active
        self.save_last_active(graph_id)?;
        
        Ok(())
    }
    
    /// Resolve name or path to graph ID
    fn resolve_identifier(&self, identifier: DbIdentifier) -> Result<String> {
        let mut registry = self.graph_registry.lock()
            .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
        
        // Check if already registered
        let existing_id = match &identifier {
            DbIdentifier::Name(name) => {
                registry.get_all_graphs().iter()
                    .find(|info| info.name == *name)
                    .map(|info| info.id.clone())
            }
            DbIdentifier::Path(path) => {
                // Use the registry's path comparison logic
                registry.get_all_graphs().iter()
                    .find(|info| {
                        use crate::graph_registry::GraphRegistry;
                        info.path == *path || GraphRegistry::paths_equivalent(&info.path, path)
                    })
                    .map(|info| info.id.clone())
            }
        };
        
        if let Some(id) = existing_id {
            return Ok(id);
        }
        
        // Not registered yet - create entry
        match identifier {
            DbIdentifier::Name(name) => {
                // Check config for this name
                if let Some(db) = self.logseq_config.databases.iter()
                    .find(|d| d.name.as_ref() == Some(&name)) {
                    let info = registry.register_graph(name, db.path.clone(), None)?;
                    Ok(info.id)
                } else {
                    Err(SessionError::DatabaseNotFound(format!("Database '{}' not found in registry or config", name)))
                }
            }
            DbIdentifier::Path(path) => {
                // Register with path-derived name
                let name = PathBuf::from(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                    
                info!("Registering new database '{}' at path: {}", name, path);
                let info = registry.register_graph(name, path, None)?;
                Ok(info.id)
            }
        }
    }
    
    /// Pre-register configured databases
    fn pre_register_databases(&self) -> Result<()> {
        let mut registry = self.graph_registry.lock()
            .map_err(|_| SessionError::SwitchError("Failed to lock registry".into()))?;
        
        for db in &self.logseq_config.databases {
            // Skip if already registered by path
            if registry.get_all_graphs().iter().any(|info| info.path == db.path) {
                debug!("Database at {} already registered", db.path);
                continue;
            }
            
            // Use configured name or derive from path
            let name = db.name.clone()
                .unwrap_or_else(|| {
                    PathBuf::from(&db.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });
            
            info!("Pre-registering database '{}' at {}", name, db.path);
            registry.register_graph(name, db.path.clone(), None)?;
        }
        Ok(())
    }
    
    /// Determine which database to launch
    fn determine_launch_target(&self, cli_override: Option<DbIdentifier>) -> Result<Option<String>> {
        // 1. CLI override takes precedence
        if let Some(identifier) = cli_override {
            info!("Using CLI-specified database");
            return Ok(Some(self.resolve_identifier(identifier)?));
        }
        
        // 2. Check config preference
        if self.logseq_config.launch_specific_database {
            if let Some(ref default) = self.logseq_config.default_database {
                info!("Using configured default database: {}", default);
                
                // Try as name first, then path
                let identifier = if self.logseq_config.databases.iter()
                    .any(|d| d.name.as_ref() == Some(default)) {
                    DbIdentifier::Name(default.clone())
                } else {
                    DbIdentifier::Path(default.clone())
                };
                return Ok(Some(self.resolve_identifier(identifier)?));
            } else {
                warn!("launch_specific_database is true but no default_database specified");
            }
        }
        
        // 3. Default: last active
        let last_active = self.load_last_active();
        if last_active.is_some() {
            info!("Using last active database");
        } else {
            info!("No target database specified and no last active found");
        }
        Ok(last_active)
    }
    
    /// Load last active database from session file
    fn load_last_active(&self) -> Option<String> {
        if !self.session_file.exists() {
            return None;
        }
        
        match fs::read_to_string(&self.session_file) {
            Ok(content) => {
                match serde_json::from_str::<SessionData>(&content) {
                    Ok(data) => data.last_active_graph_id,
                    Err(e) => {
                        warn!("Failed to parse session file: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read session file: {}", e);
                None
            }
        }
    }
    
    /// Save last active database to session file
    fn save_last_active(&self, graph_id: &str) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = self.session_file.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let data = SessionData {
            last_active_graph_id: Some(graph_id.to_string()),
            last_active_timestamp: Some(chrono::Utc::now().timestamp()),
        };
        
        let content = serde_json::to_string_pretty(&data)?;
        fs::write(&self.session_file, content)?;
        
        Ok(())
    }
    
    /// Confirm that a graph switch has completed
    pub async fn confirm_graph_switch(&self, graph_id: &str) {
        let mut confirmations = self.switch_confirmations.write().await;
        if let Some((_id, sender)) = confirmations.remove_entry(graph_id) {
            let _ = sender.send(());
            info!("Graph switch to {} confirmed by plugin", graph_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_db_identifier_creation() {
        let name_id = DbIdentifier::Name("personal".to_string());
        let path_id = DbIdentifier::Path("/home/user/logseq/work".to_string());
        
        match name_id {
            DbIdentifier::Name(n) => assert_eq!(n, "personal"),
            _ => panic!("Expected Name variant"),
        }
        
        match path_id {
            DbIdentifier::Path(p) => assert_eq!(p, "/home/user/logseq/work"),
            _ => panic!("Expected Path variant"),
        }
    }
    
    #[test]
    fn test_session_data_serialization() {
        let data = SessionData {
            last_active_graph_id: Some("test-id-123".to_string()),
            last_active_timestamp: Some(1234567890),
        };
        
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: SessionData = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.last_active_graph_id, data.last_active_graph_id);
        assert_eq!(deserialized.last_active_timestamp, data.last_active_timestamp);
    }
}