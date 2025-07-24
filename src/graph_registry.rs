use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, warn};
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
    pub fn register_graph(&mut self, name: String, path: String, id: Option<String>) -> Result<GraphInfo> {
        // Check if we already know this graph by name/path
        let existing_id = self.find_graph_id(&name, &path);
        
        let graph_id = match (existing_id, id) {
            // Graph exists and client knows the ID - verify they match
            (Some(existing), Some(provided)) => {
                if existing != provided {
                    warn!(
                        "Graph ID mismatch for {}: expected {}, got {}",
                        name, existing, provided
                    );
                }
                existing
            },
            // Graph exists but client doesn't know the ID
            (Some(existing), None) => existing,
            // New graph with provided ID
            (None, Some(provided)) => provided,
            // New graph, generate ID
            (None, None) => Uuid::new_v4().to_string(),
        };

        // Generate knowledge graph path
        let kg_path = PathBuf::from("data").join("graphs").join(&graph_id);

        let graph_info = GraphInfo {
            id: graph_id.clone(),
            name,
            path,
            kg_path,
            last_seen: Utc::now(),
        };

        self.graphs.insert(graph_id.clone(), graph_info.clone());
        
        // If this is the first graph, make it active
        if self.active_graph_id.is_none() {
            self.active_graph_id = Some(graph_id);
        }

        Ok(graph_info)
    }

    /// Find a graph ID by name and path
    fn find_graph_id(&self, name: &str, path: &str) -> Option<String> {
        self.graphs.iter()
            .find(|(_, info)| info.name == name || info.path == path)
            .map(|(id, _)| id.clone())
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
    pub fn get_or_create_graph(&mut self, name: String, path: String, id: Option<String>) -> Result<GraphInfo> {
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
        self.register_graph(name, path, id)
    }

    /// Validate request headers and switch active graph if needed
    /// Returns (graph_info, is_new_graph)
    pub fn validate_and_switch(&mut self, name: Option<&str>, path: Option<&str>, id: Option<&str>) -> Result<(GraphInfo, bool)> {
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
        let graph_info = self.get_or_create_graph(name.to_string(), path.to_string(), id_string)?;
        let is_new = self.graphs.len() > previous_count;

        // Switch to this graph if it's not already active
        if self.active_graph_id.as_ref() != Some(&graph_info.id) {
            info!("Switching active graph from {:?} to {}", self.active_graph_id, graph_info.id);
            self.active_graph_id = Some(graph_info.id.clone());
        }

        Ok((graph_info, is_new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_new_registry() {
        let registry = GraphRegistry::new();
        assert!(registry.graphs.is_empty());
        assert!(registry.active_graph_id.is_none());
    }

    #[test]
    fn test_register_new_graph() {
        let mut registry = GraphRegistry::new();
        let info = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            None
        ).unwrap();

        assert_eq!(info.name, "TestGraph");
        assert_eq!(info.path, "/path/to/test");
        assert!(!info.id.is_empty());
        assert_eq!(registry.active_graph_id, Some(info.id.clone()));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("test_registry.json");

        // Create and save
        let mut registry = GraphRegistry::new();
        let info = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            Some("test-id-123".to_string())
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
        let mut registry = GraphRegistry::new();
        
        // Register first time
        let info1 = registry.register_graph(
            "TestGraph".to_string(),
            "/path/to/test".to_string(),
            None
        ).unwrap();

        // Register again with same name
        let info2 = registry.register_graph(
            "TestGraph".to_string(),
            "/different/path".to_string(),
            None
        ).unwrap();

        // Should get the same ID
        assert_eq!(info1.id, info2.id);
    }
}