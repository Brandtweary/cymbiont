//! Graph Registry: Multi-Graph Knowledge Base Management
//!
//! This module provides the core infrastructure for managing multiple knowledge graphs,
//! enabling creation, registration, and switching between isolated graph instances.
//!
//! ## Overview
//!
//! The graph registry serves as the single source of truth for all knowledge graphs
//! in the system. Each graph is identified by a UUID and has its own isolated storage
//! directory, GraphManager instance, and TransactionCoordinator.
//!
//! ## Key Components
//!
//! ### GraphInfo
//! Metadata for a registered knowledge graph:
//! - **id**: UUID (stable identifier)
//! - **name**: Friendly display name
//! - **kg_path**: Storage directory (always `{data_dir}/graphs/{id}/`)
//! - **created**: Creation timestamp
//! - **last_accessed**: Last access timestamp
//! - **description**: Optional description
//!
//! ### GraphRegistry
//! Central registry that manages all graphs:
//! - Maintains mapping from UUID to GraphInfo
//! - Tracks the currently active graph
//! - Handles graph creation and switching
//! - Persists registry state to `{data_dir}/graph_registry.json`
//!
//! ## Graph Management
//!
//! Graphs are created with auto-generated UUIDs and stored in isolated directories.
//! The registry ensures each graph has a unique identifier and prevents duplicates.
//! All graph data is owned and managed by Cymbiont - there's no external data to recover.
//!
//! ## Data Directory Structure
//!
//! ```
//! {data_dir}/
//!   graph_registry.json        # Registry persistence
//!   graphs/
//!     {uuid-1}/               # Graph 1 data
//!       knowledge_graph.json
//!       transaction_log/
//!     {uuid-2}/               # Graph 2 data
//!       ...
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, error};
use thiserror::Error;

/// Graph registry errors
#[derive(Error, Debug)]
pub enum GraphRegistryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}

type Result<T> = std::result::Result<T, GraphRegistryError>;

/// Information about a registered graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInfo {
    /// Internal Cymbiont UUID
    pub id: String,
    /// Friendly name for the graph
    pub name: String,
    /// Path where we store the knowledge graph data
    pub kg_path: PathBuf,
    /// When this graph was created
    pub created: DateTime<Utc>,
    /// Last time this graph was accessed
    pub last_accessed: DateTime<Utc>,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Registry of all known graphs
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphRegistry {
    /// Map of graph ID to graph info
    graphs: HashMap<String, GraphInfo>,
    /// Currently active graph ID
    active_graph_id: Option<String>,
    /// Base data directory (not serialized)
    #[serde(skip)]
    data_dir: Option<PathBuf>,
}

impl GraphRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        GraphRegistry {
            graphs: HashMap::new(),
            active_graph_id: None,
            data_dir: None,
        }
    }

    /// Load registry from disk or create new if not found
    pub fn load_or_create(registry_path: &Path, data_dir: &Path) -> Result<Self> {
        let mut registry = if registry_path.exists() {
            let content = fs::read_to_string(registry_path)?;
            let loaded: GraphRegistry = serde_json::from_str(&content)?;
            info!("Loaded graph registry with {} graphs", loaded.graphs.len());
            loaded
        } else {
            info!("Creating new graph registry");
            GraphRegistry::new()
        };
        
        // Set data directory from the provided path (from config)
        registry.data_dir = Some(data_dir.to_path_buf());
        
        Ok(registry)
    }

    /// Save registry to disk at the default location
    pub fn save(&self) -> Result<()> {
        if let Some(data_dir) = &self.data_dir {
            let registry_path = data_dir.join("graph_registry.json");
            
            // Ensure parent directory exists
            if let Some(parent) = registry_path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            let content = serde_json::to_string_pretty(self)?;
            fs::write(registry_path, content)?;
            Ok(())
        } else {
            Err(GraphRegistryError::ValidationError(
                "No data directory set for registry".to_string()
            ))
        }
    }
    

    /// Register a new knowledge graph
    pub fn register_graph(
        &mut self, 
        id: Option<String>, 
        name: Option<String>,
        description: Option<String>,
        data_dir: &Path
    ) -> Result<GraphInfo> {
        let graph_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let name = name.unwrap_or_else(|| format!("Graph {}", &graph_id[..8]));
        
        // Check if this ID already exists
        if let Some(existing) = self.graphs.get_mut(&graph_id) {
            // Update metadata and return existing
            existing.name = name;
            existing.last_accessed = Utc::now();
            if description.is_some() {
                existing.description = description;
            }
            return Ok(existing.clone());
        }
        
        // Create new graph
        let kg_path = data_dir.join("graphs").join(&graph_id);
        
        let graph_info = GraphInfo {
            id: graph_id.clone(),
            name,
            kg_path,
            created: Utc::now(),
            last_accessed: Utc::now(),
            description,
        };

        self.graphs.insert(graph_id.clone(), graph_info.clone());
        
        // If this is the first graph, make it active
        if self.active_graph_id.is_none() {
            self.active_graph_id = Some(graph_id);
        }
        
        info!("Registered knowledge graph: {} ({})", graph_info.name, graph_info.id);
        
        Ok(graph_info)
    }


    /// Get graph info by ID
    pub fn get_graph(&self, id: &str) -> Option<&GraphInfo> {
        self.graphs.get(id)
    }



    
    /// Get all registered graphs
    pub fn get_all_graphs(&self) -> Vec<GraphInfo> {
        self.graphs.values().cloned().collect()
    }

    /// Get the currently active graph ID
    pub fn get_active_graph_id(&self) -> Option<&str> {
        self.active_graph_id.as_deref()
    }
    
    /// Switch active graph by ID
    pub fn switch_graph(&mut self, graph_id: &str) -> Result<GraphInfo> {
        // Simple validation - does this graph exist?
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| GraphRegistryError::ValidationError(
                format!("Graph '{}' not found in registry", graph_id)
            ))?
            .clone();
        
        // Update active graph
        if self.active_graph_id.as_ref() != Some(&graph_id.to_string()) {
            info!("Switching active graph: {:?} -> {} ({})", 
                  self.active_graph_id, graph_id, graph_info.name);
            self.active_graph_id = Some(graph_id.to_string());
        }
        
        Ok(graph_info)
    }
    
    /// Remove a graph from the registry and archive its data
    pub fn remove_graph(&mut self, graph_id: &str) -> Result<()> {
        // Get the graph info
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| GraphRegistryError::ValidationError(
                format!("Graph '{}' not found", graph_id)
            ))?
            .clone();
        
        // Archive the graph data if we have a data directory
        if let Some(data_dir) = &self.data_dir {
            let graph_data_dir = data_dir.join("graphs").join(graph_id);
            if graph_data_dir.exists() {
                // Create archive directory if it doesn't exist
                let archive_dir = data_dir.join("archived_graphs");
                fs::create_dir_all(&archive_dir)?;
                
                // Move to archive with timestamp
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                let archive_path = archive_dir.join(format!("{}_{}", graph_id, timestamp));
                
                fs::rename(&graph_data_dir, &archive_path)?;
                
                info!("Archived knowledge graph: {} ({}) to {:?}", 
                      graph_info.name, graph_id, archive_path);
            }
        }
        
        // Remove from registry
        self.graphs.remove(graph_id);
        
        // If this was the active graph, clear it
        if self.active_graph_id.as_ref() == Some(&graph_id.to_string()) {
            self.active_graph_id = None;
            info!("Removed active graph, no graph is now active");
        }
        
        Ok(())
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
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let mut registry = GraphRegistry::new();
        let info = registry.register_graph(
            None,
            Some("TestGraph".to_string()),
            Some("A test graph".to_string()),
            data_dir
        ).unwrap();

        assert_eq!(info.name, "TestGraph");
        assert!(!info.id.is_empty());
        assert_eq!(info.kg_path, data_dir.join("graphs").join(&info.id));
        assert_eq!(info.description, Some("A test graph".to_string()));
        assert_eq!(registry.active_graph_id, Some(info.id.clone()));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("graph_registry.json");
        let data_dir = dir.path();

        // Create and save
        let mut registry = GraphRegistry::load_or_create(&registry_path, data_dir).unwrap();
        let info = registry.register_graph(
            Some("test-id-123".to_string()),
            Some("TestGraph".to_string()),
            None,
            data_dir
        ).unwrap();
        registry.save().unwrap();

        // Load and verify
        let loaded = GraphRegistry::load_or_create(&registry_path, data_dir).unwrap();
        let loaded_info = loaded.get_graph("test-id-123").unwrap();
        assert_eq!(loaded_info.name, info.name);
        assert_eq!(loaded_info.id, "test-id-123");
    }

    #[test]
    fn test_register_with_existing_id() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        // Register first time
        let info1 = registry.register_graph(
            Some("existing-id".to_string()),
            Some("Graph One".to_string()),
            None,
            data_dir
        ).unwrap();

        // Register again with same ID - should update metadata
        let info2 = registry.register_graph(
            Some("existing-id".to_string()),
            Some("Updated Name".to_string()),
            Some("New description".to_string()),
            data_dir
        ).unwrap();

        // Should get the same ID
        assert_eq!(info1.id, info2.id);
        assert_eq!(info2.name, "Updated Name");
        assert_eq!(info2.description, Some("New description".to_string()));
        
        // Registry should still have only one graph
        assert_eq!(registry.graphs.len(), 1);
    }

    #[test]
    fn test_switch_graph() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        // Register two graphs
        let _graph1 = registry.register_graph(
            Some("graph-1".to_string()),
            Some("First Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        let graph2 = registry.register_graph(
            Some("graph-2".to_string()),
            Some("Second Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        // First graph should be active
        assert_eq!(registry.get_active_graph_id(), Some("graph-1"));
        
        // Switch to second graph
        let switched = registry.switch_graph("graph-2").unwrap();
        assert_eq!(switched.id, graph2.id);
        assert_eq!(registry.get_active_graph_id(), Some("graph-2"));
        
        // Try to switch to non-existent graph
        assert!(registry.switch_graph("non-existent").is_err());
    }
}