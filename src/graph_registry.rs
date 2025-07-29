//! Graph Registry: Multi-Graph Identification and Management
//!
//! This module provides the core infrastructure for Cymbiont's multi-graph architecture,
//! enabling automatic identification, registration, and management of multiple Logseq graphs
//! with complete isolation between them.
//!
//! ## Overview
//!
//! The graph registry system allows Cymbiont to work with multiple Logseq graphs simultaneously
//! by automatically identifying each graph through name and path information sent by the plugin.
//! Each graph receives a unique UUID that remains stable across sessions, enabling consistent
//! tracking and isolation of graph-specific data.
//!
//! ## Key Components
//!
//! ### GraphInfo
//! Represents metadata for a single registered graph:
//! - **id**: Internal Cymbiont UUID (stable identifier)
//! - **name**: Human-readable graph name from Logseq
//! - **path**: File system path to the graph directory
//! - **kg_path**: Where Cymbiont stores knowledge graph data for this graph
//! - **last_seen**: Timestamp of last interaction with this graph
//! - **config_updated**: Whether config.edn has been modified for property hiding
//!
//! ### GraphRegistry
//! Central coordinator that manages the collection of graphs:
//! - Maintains mapping from UUID to GraphInfo
//! - Tracks which graph is currently active
//! - Provides automatic graph creation and registration
//! - Handles graph switching and validation
//! - Persists registry state to `data/graph_registry.json`
//!
//! ## Graph Identification Strategy
//!
//! Graphs are identified using a multi-layered approach:
//! 1. **Primary**: UUID from config.edn (`:cymbiont/graph-id` property)
//! 2. **Recovery**: Forgiving name/path matching for graphs missing UUIDs
//! 3. **Creation**: Automatic UUID generation for new graphs
//!
//! This strategy ensures that graphs maintain their identity even if the UUID is lost
//! or not yet stamped, while preventing duplicate registrations.
//!
//! ## Multi-Graph Architecture
//!
//! The registry enables Cymbiont's parallel multi-graph architecture:
//! - **Complete Isolation**: Each graph gets its own GraphManager and TransactionCoordinator
//! - **Automatic Switching**: Request middleware validates headers and switches active graph
//! - **Lazy Creation**: Graph infrastructure is created only when first accessed
//! - **Persistent Identity**: UUIDs are stamped in config.edn for permanent identification
//!
//! ## Usage Patterns
//!
//! ### Plugin Initialization
//! ```rust,no_run
//! # use cymbiont::graph_registry::GraphRegistry;
//! # let mut registry = GraphRegistry::new();
//! // Plugin sends graph context via headers
//! let data_dir = std::path::Path::new("data");
//! let (graph_info, is_new) = registry.validate_and_switch(
//!     Some("My Graph"),
//!     Some("path/to/graph"),
//!     Some("optional-uuid"),
//!     data_dir
//! ).unwrap();
//! ```
//!
//! ### Graph Management
//! ```rust,no_run
//! # use cymbiont::graph_registry::GraphRegistry;
//! # let mut registry = GraphRegistry::new();
//! // Register new graph
//! let data_dir = std::path::Path::new("data");
//! let graph_info = registry.register_graph(
//!     "My Knowledge Base".to_string(),
//!     "/path/to/graph".to_string(),
//!     None,  // Auto-generate UUID
//!     data_dir
//! ).unwrap();
//!
//! // Get active graph
//! if let Some(active_id) = registry.get_active_graph_id() {
//!     let graph_info = registry.get_graph(&active_id).unwrap();
//! }
//! ```
//!
//! ## Configurable Data Directory
//!
//! All storage operations now accept a configurable data directory path:
//! - **Storage Path Resolution**: Graph data stored under `{data_dir}/graphs/{graph-id}/`
//! - **Registry Persistence**: Registry saved to `{data_dir}/graph_registry.json`
//! - **CLI Override**: Data directory configurable via config.yaml or --data-dir flag
//! - **Testing Isolation**: Enables separate data directories for tests vs production
//! - **Multi-User Support**: Allows different users to have isolated data stores
//!
//! ## Persistence and Recovery
//!
//! The registry automatically persists to `{data_dir}/graph_registry.json` and implements
//! recovery logic for resilience:
//! - **Startup**: Loads existing registry or creates new one
//! - **Graph Recovery**: Matches graphs by name+path if UUID is missing
//! - **Forgiving Matching**: Handles renamed graphs and moved directories
//! - **Automatic Save**: Persists changes immediately for durability
//!
//! ## Configuration Integration
//!
//! The registry tracks whether each graph's config.edn has been updated for property hiding:
//! - **config_updated Flag**: Prevents redundant config.edn modifications
//! - **Performance Optimization**: Skips expensive EDN writes for already-updated graphs
//! - **Session Integration**: Future session management will use this for optimization

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, warn, error};
use thiserror::Error;

/// Graph registry errors
#[derive(Error, Debug)]
pub enum GraphRegistryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Graph not found: {0}")]
    GraphNotFound(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}

type Result<T> = std::result::Result<T, GraphRegistryError>;

/// Information about a registered graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInfo {
    /// Internal Cymbiont UUID
    pub id: String,
    /// Graph name from Logseq
    pub name: String,
    /// Graph path from Logseq
    pub path: String,
    /// Path where we store the knowledge graph data
    pub kg_path: PathBuf,
    /// Last time we saw this graph
    pub last_seen: DateTime<Utc>,
    /// Whether config.edn has been updated with property hiding
    #[serde(default)]
    pub config_updated: bool,
}

/// Registry of all known graphs
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphRegistry {
    /// Map of graph ID to graph info
    graphs: HashMap<String, GraphInfo>,
    /// Currently active graph ID
    active_graph_id: Option<String>,
}

impl GraphRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        GraphRegistry {
            graphs: HashMap::new(),
            active_graph_id: None,
        }
    }

    /// Load registry from disk or create new if not found
    pub fn load_or_create(registry_path: &Path) -> Result<Self> {
        if registry_path.exists() {
            let content = fs::read_to_string(registry_path)?;
            let registry: GraphRegistry = serde_json::from_str(&content)?;
            info!("Loaded graph registry with {} graphs", registry.graphs.len());
            Ok(registry)
        } else {
            info!("Creating new graph registry");
            Ok(GraphRegistry::new())
        }
    }

    /// Save registry to disk
    pub fn save(&self, registry_path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let content = serde_json::to_string_pretty(self)?;
        fs::write(registry_path, content)?;
        Ok(())
    }

    /// Register a new graph or update existing
    pub fn register_graph(&mut self, name: String, path: String, id: Option<String>, data_dir: &std::path::Path) -> Result<GraphInfo> {
        // Check if we already know this graph by name/path BEFORE normalizing
        let existing_id = self.find_graph_id(&name, &path);
        
        let graph_id = match (existing_id, id) {
            // Graph exists and client knows the ID - verify they match
            (Some(existing), Some(provided)) => {
                if existing != provided {
                    error!(
                        "Graph ID mismatch for {}: registry has {}, config.edn has {}. This is a serious data integrity issue!",
                        name, existing, provided
                    );
                    // Recovery: If name AND path match, we can be confident it's the same graph
                    // Update the registry to use the ID from config.edn
                    warn!(
                        "🔧 UUID Recovery: Since name '{}' AND path '{}' match, updating registry to use config.edn ID: {}",
                        name, path, provided
                    );
                    provided  // Use the ID from config.edn
                } else {
                    existing
                }
            },
            // Graph exists but client doesn't know the ID - RECOVERY MODE
            (Some(existing), None) => {
                info!(
                    "🔧 UUID Recovery: Graph '{}' at '{}' is missing its UUID. \
                    Recovering with existing ID: {} (matched by name AND path)",
                    name, path, existing
                );
                existing
            },
            // New graph with provided ID
            (None, Some(provided)) => provided,
            // New graph, generate ID
            (None, None) => Uuid::new_v4().to_string(),
        };

        // Generate knowledge graph path using provided data_dir
        let kg_path = data_dir.join("graphs").join(&graph_id);

        let graph_info = GraphInfo {
            id: graph_id.clone(),
            name,
            path,  // Store the path as provided, not normalized
            kg_path,
            last_seen: Utc::now(),
            config_updated: false,
        };

        self.graphs.insert(graph_id.clone(), graph_info.clone());
        
        // If this is the first graph, make it active
        if self.active_graph_id.is_none() {
            self.active_graph_id = Some(graph_id);
        }

        Ok(graph_info)
    }

    /// Find a graph ID by name and path
    /// Returns a match only if BOTH name AND path match (safe recovery)
    fn find_graph_id(&self, name: &str, path: &str) -> Option<String> {
        self.graphs.iter()
            .find(|(_, info)| {
                // Compare both exact paths AND normalized paths to handle duplicates
                let paths_match = info.path == path || Self::paths_equivalent(&info.path, path);
                info.name == name && paths_match
            })
            .map(|(id, _)| id.clone())
    }
    
    /// Check if two paths refer to the same location
    /// Handles relative vs absolute path comparisons
    pub fn paths_equivalent(path1: &str, path2: &str) -> bool {
        // Quick exact match check
        if path1 == path2 {
            return true;
        }
        
        // Try to resolve both paths to canonical form
        let p1 = PathBuf::from(path1);
        let p2 = PathBuf::from(path2);
        
        // If both are relative, compare as strings to avoid PathBuf normalization
        if p1.is_relative() && p2.is_relative() {
            return path1 == path2;
        }
        
        // Try to canonicalize for comparison
        let canonical1 = if p1.is_relative() {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| cwd.join(&p1).canonicalize().ok())
        } else {
            p1.canonicalize().ok()
        };
        
        let canonical2 = if p2.is_relative() {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| cwd.join(&p2).canonicalize().ok())
        } else {
            p2.canonicalize().ok()
        };
        
        // If we could canonicalize both, compare them
        match (canonical1, canonical2) {
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false
        }
    }

    /// Get graph info by ID
    pub fn get_graph(&self, id: &str) -> Option<&GraphInfo> {
        self.graphs.get(id)
    }

    /// Get the active graph
    pub fn get_active_graph(&self) -> Option<&GraphInfo> {
        self.active_graph_id.as_ref()
            .and_then(|id| self.graphs.get(id))
    }

    /// Set the active graph
    pub fn set_active_graph(&mut self, id: &str) -> Result<()> {
        if self.graphs.contains_key(id) {
            self.active_graph_id = Some(id.to_string());
            Ok(())
        } else {
            Err(GraphRegistryError::GraphNotFound(id.to_string()))
        }
    }

    /// Get or create a graph based on name and path
    pub fn get_or_create_graph(&mut self, name: String, path: String, id: Option<String>, data_dir: &std::path::Path) -> Result<GraphInfo> {
        // If we have an ID, try to find the graph
        if let Some(ref graph_id) = id {
            if let Some(info) = self.graphs.get_mut(graph_id) {
                // Update last seen
                info.last_seen = Utc::now();
                // Update name/path if they changed
                if info.name != name {
                    info!("Graph {} name changed from {} to {}", graph_id, info.name, name);
                    info.name = name;
                }
                if info.path != path {
                    info!("Graph {} path changed from {} to {}", graph_id, info.path, path);
                    info.path = path;
                }
                return Ok(info.clone());
            }
        }

        // Otherwise register as new
        self.register_graph(name, path, id, data_dir)
    }

    /// Validate request headers and switch active graph if needed
    /// Returns (graph_info, is_new_graph)
    pub fn validate_and_switch(&mut self, name: Option<&str>, path: Option<&str>, id: Option<&str>, data_dir: &std::path::Path) -> Result<(GraphInfo, bool)> {
        // Validate that we have at least name and path
        let name = name.ok_or_else(|| GraphRegistryError::ValidationError(
            "Missing X-Cymbiont-Graph-Name header".to_string()
        ))?;
        let path = path.ok_or_else(|| GraphRegistryError::ValidationError(
            "Missing X-Cymbiont-Graph-Path header".to_string()
        ))?;

        // Get or create the graph
        let id_string = id.map(|s| s.to_string());
        let previous_count = self.graphs.len();
        let graph_info = self.get_or_create_graph(name.to_string(), path.to_string(), id_string, data_dir)?;
        let is_new = self.graphs.len() > previous_count;

        // Switch to this graph if it's not already active
        if self.active_graph_id.as_ref() != Some(&graph_info.id) {
            info!("Switching active graph from {:?} to {}", self.active_graph_id, graph_info.id);
            self.active_graph_id = Some(graph_info.id.clone());
        }

        Ok((graph_info, is_new))
    }

    /// Mark a graph's config.edn as having been updated with property hiding
    pub fn mark_config_updated(&mut self, graph_id: &str) -> Result<()> {
        if let Some(graph_info) = self.graphs.get_mut(graph_id) {
            graph_info.config_updated = true;
            Ok(())
        } else {
            Err(GraphRegistryError::GraphNotFound(graph_id.to_string()))
        }
    }

    /// Check if a graph's config.edn has been updated
    pub fn is_config_updated(&self, graph_id: &str) -> bool {
        self.graphs.get(graph_id)
            .map(|info| info.config_updated)
            .unwrap_or(false)
    }
    
    /// Get all registered graphs
    pub fn get_all_graphs(&self) -> Vec<GraphInfo> {
        self.graphs.values().cloned().collect()
    }

    /// Get the currently active graph ID
    pub fn get_active_graph_id(&self) -> Option<&str> {
        self.active_graph_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::env;

    #[test]
    fn test_new_registry() {
        let registry = GraphRegistry::new();
        assert!(registry.graphs.is_empty());
        assert!(registry.active_graph_id.is_none());
    }

    #[test]
    fn test_register_new_graph() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let mut registry = GraphRegistry::new();
        let info = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            None,
            data_dir
        ).unwrap();

        assert_eq!(info.name, "TestGraph");
        assert_eq!(info.path, "/path/to/test");
        assert!(!info.id.is_empty());
        assert_eq!(info.kg_path, data_dir.join("graphs").join(&info.id));
        assert_eq!(registry.active_graph_id, Some(info.id.clone()));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("test_registry.json");
        let data_dir = dir.path();

        // Create and save
        let mut registry = GraphRegistry::new();
        let info = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            Some("test-id-123".to_string()),
            data_dir
        ).unwrap();
        registry.save(&registry_path).unwrap();

        // Load and verify
        let loaded = GraphRegistry::load_or_create(&registry_path).unwrap();
        let loaded_info = loaded.get_graph("test-id-123").unwrap();
        assert_eq!(loaded_info.name, info.name);
        assert_eq!(loaded_info.id, "test-id-123");
    }

    #[test]
    fn test_find_existing_graph() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        // Register first time
        let info1 = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            None,
            data_dir
        ).unwrap();

        // Register again with same name AND same path
        let info2 = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            None,
            data_dir
        ).unwrap();

        // Should get the same ID
        assert_eq!(info1.id, info2.id);
        
        // Register with same name but different path
        let info3 = registry.register_graph(
            "TestGraph".to_string(),
            "/different/path".to_string(),
            None,
            data_dir
        ).unwrap();
        
        // Should get a different ID (AND logic for safety)
        assert_ne!(info1.id, info3.id);
    }

    #[test]
    fn test_paths_equivalent_exact_match() {
        assert!(GraphRegistry::paths_equivalent("foo/bar", "foo/bar"));
        assert!(GraphRegistry::paths_equivalent("/abs/path", "/abs/path"));
    }

    #[test]
    fn test_paths_equivalent_both_relative() {
        assert!(GraphRegistry::paths_equivalent("foo/bar", "foo/bar"));
        assert!(!GraphRegistry::paths_equivalent("foo/bar", "foo/baz"));
        assert!(!GraphRegistry::paths_equivalent("foo/bar", "bar/foo"));
    }

    #[test]
    fn test_paths_equivalent_with_temp_dir() {
        let temp = tempdir().unwrap();
        let subdir = temp.path().join("subdir");
        fs::create_dir_all(&subdir).unwrap();
        
        // Change to temp directory to test relative paths
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&temp.path()).unwrap();
        
        // Test relative vs absolute
        let abs_path = subdir.to_str().unwrap();
        assert!(GraphRegistry::paths_equivalent("subdir", abs_path));
        assert!(GraphRegistry::paths_equivalent(abs_path, "subdir"));
        
        // Restore original directory
        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_duplicate_prevention_relative_absolute() {
        let mut registry = GraphRegistry::new();
        
        // Create a temp directory to ensure paths exist
        let temp = tempdir().unwrap();
        let graph_dir = temp.path().join("test_graph");
        fs::create_dir_all(&graph_dir).unwrap();
        
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&temp.path()).unwrap();
        
        // Register with relative path
        let info1 = registry.register_graph(
            "TestGraph".to_string(),
            "test_graph".to_string(),
            None,
            &temp.path()
        ).unwrap();
        
        // Try to register with absolute path - should get same ID
        let abs_path = graph_dir.to_str().unwrap().to_string();
        let info2 = registry.register_graph(
            "TestGraph".to_string(),
            abs_path,
            None,
            &temp.path()
        ).unwrap();
        
        assert_eq!(info1.id, info2.id, "Should not create duplicate for relative/absolute path");
        assert_eq!(registry.graphs.len(), 1, "Should only have one graph registered");
        
        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_path_normalization_edge_cases() {
        // For relative paths, we do exact string comparison
        assert!(!GraphRegistry::paths_equivalent("foo/bar", "foo/bar/"));
        assert!(!GraphRegistry::paths_equivalent("foo/bar/", "foo/bar"));
        
        // Test paths with . and .. - won't be equivalent without canonicalization
        assert!(!GraphRegistry::paths_equivalent("foo/./bar", "foo/bar"));
        assert!(!GraphRegistry::paths_equivalent("foo/../foo/bar", "foo/bar"));
        
        // Different paths should not be equivalent
        assert!(!GraphRegistry::paths_equivalent("foo", "bar"));
        assert!(!GraphRegistry::paths_equivalent("/abs/foo", "/abs/bar"));
    }

    #[test]
    fn test_registry_with_nonexistent_paths() {
        let mut registry = GraphRegistry::new();
        
        // Should still work with non-existent paths
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let info1 = registry.register_graph(
            "TestGraph".to_string(),
            "nonexistent/path".to_string(),
            None,
            data_dir
        ).unwrap();
        
        // Should find it again
        let info2 = registry.register_graph(
            "TestGraph".to_string(),
            "nonexistent/path".to_string(),
            None,
            data_dir
        ).unwrap();
        
        assert_eq!(info1.id, info2.id);
    }

    #[test]
    fn test_windows_vs_unix_paths() {
        // Even on Unix, we should handle Windows-style paths gracefully
        let mut registry = GraphRegistry::new();
        
        // These should be treated as different paths
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let info1 = registry.register_graph(
            "TestGraph".to_string(),
            "C:\\Users\\test".to_string(),
            None,
            data_dir
        ).unwrap();
        
        let info2 = registry.register_graph(
            "TestGraph".to_string(),
            "C:/Users/test".to_string(),
            None,
            data_dir
        ).unwrap();
        
        // On Unix, these are different paths
        // On Windows, they might be the same (but we're not on Windows in CI)
        #[cfg(unix)]
        assert_ne!(info1.id, info2.id);
    }

    #[test]
    fn test_find_graph_id_with_path_variations() {
        let mut registry = GraphRegistry::new();
        
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        // Register a graph
        registry.register_graph(
            "TestGraph".to_string(),
            "some/relative/path".to_string(),
            Some("test-id".to_string()),
            data_dir
        ).unwrap();
        
        // Should find it with exact path
        assert_eq!(
            registry.find_graph_id("TestGraph", "some/relative/path"),
            Some("test-id".to_string())
        );
        
        // Should not find with different name
        assert_eq!(
            registry.find_graph_id("OtherGraph", "some/relative/path"),
            None
        );
        
        // Should not find with different path
        assert_eq!(
            registry.find_graph_id("TestGraph", "different/path"),
            None
        );
    }
}