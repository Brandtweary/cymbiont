//! Graph Registry: Multi-Graph Knowledge Base Management
//!
//! This module provides the core infrastructure for managing multiple knowledge graphs,
//! enabling creation, registration, and open/closed state management for parallel graphs.
//!
//! ## Overview
//!
//! The graph registry serves as the single source of truth for all knowledge graphs
//! in the system. Each graph is identified by a UUID and has its own isolated storage
//! directory, GraphManager instance, and TransactionCoordinator. Multiple graphs can
//! be open simultaneously, replacing the previous single active graph model.
//!
//! ## Key Components
//!
//! ### GraphInfo
//! Metadata for a registered knowledge graph:
//! - **id**: UUID (stable identifier, type-safe throughout the system)
//! - **name**: Friendly display name
//! - **kg_path**: Storage directory (always `{data_dir}/graphs/{id}/`)
//! - **created**: Creation timestamp
//! - **last_accessed**: Last access timestamp
//! - **description**: Optional description
//!
//! ### GraphRegistry
//! Central registry that manages all graphs:
//! - Maintains mapping from UUID to GraphInfo with custom JSON serialization
//! - Tracks open graphs in `HashSet<Uuid>` (replaces single active_graph_id)
//! - Handles graph lifecycle: register, open, close, remove
//! - Provides centralized graph resolution by UUID or name
//! - Persists registry state to `{data_dir}/graph_registry.json`
//! - Offers complete workflow methods that coordinate with AgentRegistry for prime agent authorization
//!
//! ## Graph State Management
//!
//! Graphs exist in two states:
//! - **Open**: Loaded in memory with active manager and transaction coordinator
//! - **Closed**: Persisted to disk, resources freed from memory
//!
//! The registry tracks open graphs and persists this state across restarts,
//! enabling automatic recovery of all previously open graphs on startup.
//!
//! ## Concurrency Safety
//!
//! The GraphRegistry is accessed through `Arc<RwLock<GraphRegistry>>` with development-time
//! contention detection:
//! 
//! - **Write Pattern**: Use `debug_assert!(registry.try_write().is_ok()` before acquiring write locks
//! - **Purpose**: Acts as a tripwire to detect lock contention during development, causing fast failure instead of mysterious hangs
//! - **Scope Management**: Keep lock scopes minimal and never hold both registry and graph_resources locks simultaneously
//! 
//! **Important**: The debug assertions are tripwires for investigation, not concrete walls. 
//! If profiling shows that some degree of lock contention is acceptable for your use case, 
//! the assertions can be removed. However, this decision must be made after profiling and 
//! measuring actual performance impact, never preemptively.
//!
//! ## Complete Workflow Methods
//!
//! To reduce AppState verbosity, the registry provides complete workflow methods:
//! - `create_new_graph_complete()` - Full graph creation including prime agent authorization
//! - `delete_graph_complete()` - Complete graph deletion with data archival
//!
//! These methods handle cross-registry coordination and persistence, allowing AppState 
//! to focus on resource management rather than workflow orchestration.
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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::info;

// Import shared UUID serialization utilities
use crate::storage::registry_utils::{uuid_hashmap_serde, uuid_hashset_serde, uuid_vec_serde};
use crate::error::*;



/// Information about a registered graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInfo {
    /// Internal Cymbiont UUID
    pub id: Uuid,
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
    
    /// Agents authorized to access this graph (bidirectional tracking)
    /// Managed by AgentRegistry, not GraphRegistry
    #[serde(default, with = "uuid_vec_serde")]
    pub authorized_agents: Vec<Uuid>,
}

/// Registry of all known graphs
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphRegistry {
    /// Map of graph ID to graph info (public for AgentRegistry bidirectional tracking)
    #[serde(with = "uuid_hashmap_serde")]
    pub graphs: HashMap<Uuid, GraphInfo>,
    /// Currently open graph IDs (replaces active_graph_id)
    #[serde(default, with = "uuid_hashset_serde")]
    open_graphs: HashSet<Uuid>,
    /// Base data directory (not serialized)
    #[serde(skip)]
    data_dir: Option<PathBuf>,
}

impl GraphRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        GraphRegistry {
            graphs: HashMap::new(),
            open_graphs: HashSet::new(),
            data_dir: None,
        }
    }

    /// Load registry from disk or create new if not found
    pub fn load_or_create(registry_path: &Path, data_dir: &Path) -> Result<Self> {
        let mut registry = if registry_path.exists() {
            let content = fs::read_to_string(registry_path)?;
            let loaded: GraphRegistry = serde_json::from_str(&content)?;
            info!("📊 Loaded graph registry with {} graphs, {} open", 
                  loaded.graphs.len(), loaded.open_graphs.len());
            loaded
        } else {
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
            Err(StorageError::graph_registry("No data directory set for registry").into())
        }
    }
    

    /// Register a new knowledge graph
    /// 
    /// TODO: Add name uniqueness validation to prevent duplicate graph names.
    /// Currently, multiple graphs can have the same name, which could cause
    /// confusion when using name-based resolution. Consider rejecting duplicate
    /// names or warning the user.
    pub fn register_graph(
        &mut self, 
        id: Option<Uuid>, 
        name: Option<String>,
        description: Option<String>,
        data_dir: &Path
    ) -> Result<GraphInfo> {
        let graph_id = id.unwrap_or_else(|| Uuid::new_v4());
        let name = name.unwrap_or_else(|| format!("Graph {}", &graph_id.to_string()[..8]));
        
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
        let kg_path = data_dir.join("graphs").join(graph_id.to_string());
        
        let graph_info = GraphInfo {
            id: graph_id,
            name,
            kg_path,
            created: Utc::now(),
            last_accessed: Utc::now(),
            description,
            authorized_agents: Vec::new(),  // AgentRegistry will manage this
        };

        self.graphs.insert(graph_id, graph_info.clone());
        
        // Always open newly registered graphs
        self.open_graphs.insert(graph_id);
        
        
        Ok(graph_info)
    }


    /// Get graph info by ID
    pub fn get_graph(&self, id: &Uuid) -> Option<&GraphInfo> {
        self.graphs.get(id)
    }



    
    /// Get all registered graphs
    pub fn get_all_graphs(&self) -> Vec<GraphInfo> {
        self.graphs.values().cloned().collect()
    }

    /// Get all currently open graph IDs
    pub fn get_open_graphs(&self) -> Vec<Uuid> {
        self.open_graphs.iter().copied().collect()
    }
    
    /// Check if a graph is open
    pub fn is_graph_open(&self, graph_id: &Uuid) -> bool {
        self.open_graphs.contains(graph_id)
    }
    
    /// Open a graph (add to open set)
    pub fn open_graph(&mut self, graph_id: &Uuid) -> Result<GraphInfo> {
        // Validate graph exists
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone();
        
        // Add to open set
        if self.open_graphs.insert(*graph_id) {
        }
        
        // Update last accessed time
        if let Some(graph) = self.graphs.get_mut(graph_id) {
            graph.last_accessed = Utc::now();
        }
        
        Ok(graph_info)
    }
    
    /// Close a graph (remove from open set)
    pub fn close_graph(&mut self, graph_id: &Uuid) -> Result<()> {
        if self.open_graphs.remove(graph_id) {
            Ok(())
        } else {
            Err(StorageError::graph_registry(format!("Graph '{}' was not open", graph_id)).into())
        }
    }
    
    /// Resolve graph target from optional UUID and name with smart defaults
    /// 
    /// Priority order:
    /// 1. If graph_id provided, validate it exists
    /// 2. Else if graph_name provided, resolve to UUID
    /// 3. Else if allow_smart_default and exactly one open, use it
    /// 4. Else error
    pub fn resolve_graph_target(
        &self,
        graph_id: Option<&Uuid>,
        graph_name: Option<&str>,
        allow_smart_default: bool,
    ) -> Result<Uuid> {
        if let Some(id) = graph_id {
            // Validate the UUID exists
            if self.graphs.contains_key(id) {
                Ok(*id)
            } else {
                Err(StorageError::not_found("graph", "ID", id.to_string()).into())
            }
        } else if let Some(name) = graph_name {
            // Find graph by name
            self.graphs.values()
                .find(|g| g.name == name)
                .map(|g| g.id)
                .ok_or_else(|| StorageError::not_found("graph", "name", name).into())
        } else if allow_smart_default {
            let open_graphs = self.get_open_graphs();
            match open_graphs.len() {
                0 => Err(StorageError::graph_registry("No graphs are open").into()),
                1 => Ok(open_graphs[0]),
                _ => Err(StorageError::graph_registry("Multiple graphs open, must specify target").into()),
            }
        } else {
            Err(StorageError::graph_registry("Must specify graph_id or graph_name").into())
        }
    }
    
    
    /// Ensure at least one graph is open. If no graphs are open and graphs exist,
    /// opens the first available graph.
    pub fn ensure_graph_open(&mut self) -> Result<()> {
        if self.open_graphs.is_empty() && !self.graphs.is_empty() {
            // Get the first graph (deterministic, not random)
            if let Some((&graph_id, _graph_info)) = self.graphs.iter().next() {
                self.open_graphs.insert(graph_id);
            }
        }
        Ok(())
    }
    
    /// Remove a graph from the registry and archive its data
    /// 
    /// Archives the graph directory to `{data_dir}/archived_graphs/` with timestamp.
    pub fn remove_graph(&mut self, graph_id: &Uuid) -> Result<()> {
        // Get the graph info
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone();
        
        // Archive the graph data if we have a data directory
        if let Some(data_dir) = &self.data_dir {
            let graph_data_dir = data_dir.join("graphs").join(graph_id.to_string());
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
        
        // Also remove from open graphs if it was open
        if self.open_graphs.remove(graph_id) {
        }
        
        Ok(())
    }
    
    /// Complete graph creation workflow with prime agent authorization
    /// 
    /// This handles the full workflow that AppState was previously orchestrating:
    /// 1. Register the graph in this registry
    /// 2. Authorize the prime agent for the new graph (bidirectional)
    /// 3. Save both registries
    pub fn create_new_graph_complete(
        &mut self,
        name: Option<String>,
        description: Option<String>,
        agent_registry: &mut crate::storage::AgentRegistry,
    ) -> Result<GraphInfo> {
        let data_dir = self.data_dir.as_ref()
            .ok_or_else(|| StorageError::graph_registry("No data directory set"))?
            .clone();
        
        // Register the graph
        let graph_info = self.register_graph(None, name, description, &data_dir)?;
        
        // Authorize prime agent for the new graph
        agent_registry.authorize_prime_for_new_graph(&graph_info.id, self)?;
        
        // Save both registries
        agent_registry.save()?;
        self.save()?;
        
        info!("✅ Created graph: {} ({})", graph_info.name, graph_info.id);
        Ok(graph_info)
    }
    
    /// Complete graph deletion workflow with cleanup
    /// 
    /// This handles the full workflow that AppState was previously orchestrating:
    /// 1. Remove from this registry (which archives the data)
    /// 2. Save the updated registry
    pub fn delete_graph_complete(&mut self, graph_id: &Uuid) -> Result<()> {
        // Remove graph (this handles archival)
        self.remove_graph(graph_id)?;
        
        // Save registry
        self.save()?;
        
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
        assert!(registry.open_graphs.is_empty());
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
        assert_eq!(info.kg_path, data_dir.join("graphs").join(info.id.to_string()));
        assert_eq!(info.description, Some("A test graph".to_string()));
        assert!(registry.is_graph_open(&info.id));
    }

    #[test]
    fn test_register_with_existing_id() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        let existing_uuid = Uuid::new_v4();
        
        // Register first time
        let info1 = registry.register_graph(
            Some(existing_uuid),
            Some("Graph One".to_string()),
            None,
            data_dir
        ).unwrap();

        // Register again with same ID - should update metadata
        let info2 = registry.register_graph(
            Some(existing_uuid),
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
    fn test_open_close_graphs() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        
        // Register two graphs
        let _graph1 = registry.register_graph(
            Some(uuid1),
            Some("First Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        let _graph2 = registry.register_graph(
            Some(uuid2),
            Some("Second Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        // Both graphs should be open (all newly registered graphs are auto-opened)
        assert!(registry.is_graph_open(&uuid1));
        assert!(registry.is_graph_open(&uuid2));
        assert_eq!(registry.get_open_graphs().len(), 2);
        
        // Close first graph
        registry.close_graph(&uuid1).unwrap();
        assert!(!registry.is_graph_open(&uuid1));
        assert!(registry.is_graph_open(&uuid2));
        
        // Try to open non-existent graph
        let non_existent = Uuid::new_v4();
        assert!(registry.open_graph(&non_existent).is_err());
    }

    #[test]
    fn test_ensure_graph_open() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        // Test 1: Empty registry - ensure_graph_open should do nothing
        registry.ensure_graph_open().unwrap();
        assert_eq!(registry.get_open_graphs().len(), 0);
        
        // Test 2: Register graphs and close them all
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        
        registry.register_graph(
            Some(uuid1),
            Some("Graph 1".to_string()),
            None,
            data_dir
        ).unwrap();
        
        registry.register_graph(
            Some(uuid2),
            Some("Graph 2".to_string()),
            None,
            data_dir
        ).unwrap();
        
        // Both should be open after registration
        assert_eq!(registry.get_open_graphs().len(), 2);
        
        // Close all graphs
        registry.close_graph(&uuid1).unwrap();
        registry.close_graph(&uuid2).unwrap();
        assert_eq!(registry.get_open_graphs().len(), 0);
        
        // Test 3: ensure_graph_open should open one graph
        registry.ensure_graph_open().unwrap();
        assert_eq!(registry.get_open_graphs().len(), 1);
        
        // Test 4: If graphs are already open, ensure_graph_open does nothing
        registry.ensure_graph_open().unwrap();
        assert_eq!(registry.get_open_graphs().len(), 1);
    }
    
    #[test]
    fn test_open_graphs_persistence() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("graph_registry.json");
        let data_dir = dir.path();

        // Create registry and register multiple graphs
        let mut registry = GraphRegistry::load_or_create(&registry_path, data_dir).unwrap();
        
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        let uuid3 = Uuid::new_v4();
        
        // Register three graphs (all will be auto-opened)
        registry.register_graph(
            Some(uuid1),
            Some("Graph 1".to_string()),
            None,
            data_dir
        ).unwrap();
        
        registry.register_graph(
            Some(uuid2),
            Some("Graph 2".to_string()),
            None,
            data_dir
        ).unwrap();
        
        registry.register_graph(
            Some(uuid3),
            Some("Graph 3".to_string()),
            None,
            data_dir
        ).unwrap();
        
        // All should be open after registration
        assert_eq!(registry.get_open_graphs().len(), 3);
        
        // Close one graph
        registry.close_graph(&uuid2).unwrap();
        assert_eq!(registry.get_open_graphs().len(), 2);
        assert!(registry.is_graph_open(&uuid1));
        assert!(!registry.is_graph_open(&uuid2));
        assert!(registry.is_graph_open(&uuid3));
        
        // Save registry
        registry.save().unwrap();
        
        // Load registry from disk
        let loaded_registry = GraphRegistry::load_or_create(&registry_path, data_dir).unwrap();
        
        // Verify open graphs were persisted
        let open_graphs = loaded_registry.get_open_graphs();
        assert_eq!(open_graphs.len(), 2);
        assert!(open_graphs.contains(&uuid1));
        assert!(!open_graphs.contains(&uuid2));
        assert!(open_graphs.contains(&uuid3));
        
        // Verify all graphs still exist in registry
        assert!(loaded_registry.get_graph(&uuid1).is_some());
        assert!(loaded_registry.get_graph(&uuid2).is_some());
        assert!(loaded_registry.get_graph(&uuid3).is_some());
    }
    
    #[test]
    fn test_resolve_graph_target() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = GraphRegistry::new();
        
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        
        // Register two graphs
        registry.register_graph(
            Some(uuid1),
            Some("First Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        registry.register_graph(
            Some(uuid2),
            Some("Second Graph".to_string()),
            None,
            data_dir
        ).unwrap();
        
        // Test 1: Resolve by UUID
        let resolved = registry.resolve_graph_target(Some(&uuid1), None, false).unwrap();
        assert_eq!(resolved, uuid1);
        
        // Test 2: Resolve by name
        let resolved = registry.resolve_graph_target(None, Some("Second Graph"), false).unwrap();
        assert_eq!(resolved, uuid2);
        
        // Test 3: UUID takes precedence over name
        let resolved = registry.resolve_graph_target(Some(&uuid1), Some("Second Graph"), false).unwrap();
        assert_eq!(resolved, uuid1);
        
        // Test 4: Smart default with single open graph
        // Both graphs are open from registration, so close uuid2
        registry.close_graph(&uuid2).unwrap();
        let resolved = registry.resolve_graph_target(None, None, true).unwrap();
        assert_eq!(resolved, uuid1);
        
        // Test 5: Smart default with multiple open graphs should fail
        registry.open_graph(&uuid2).unwrap(); // Both are open now
        let result = registry.resolve_graph_target(None, None, true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Multiple graphs open"));
        
        // Test 6: Smart default with no open graphs should fail
        registry.close_graph(&uuid1).unwrap();
        registry.close_graph(&uuid2).unwrap();
        let result = registry.resolve_graph_target(None, None, true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No graphs are open"));
        
        // Test 7: Invalid UUID should fail
        let invalid_uuid = Uuid::new_v4();
        let result = registry.resolve_graph_target(Some(&invalid_uuid), None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not found: graph with ID"));
        
        // Test 8: Invalid name should fail
        let result = registry.resolve_graph_target(None, Some("Non-existent Graph"), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not found: graph with name"));
        
        // Test 9: No parameters without smart default should fail
        let result = registry.resolve_graph_target(None, None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Must specify graph_id or graph_name"));
    }
}