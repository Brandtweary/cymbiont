//! Graph Registry: Multi-Graph Knowledge Base Management
//!
//! This module provides the core infrastructure for managing multiple knowledge graphs,
//! enabling creation, registration, and open/closed state management for parallel graphs.
//!
//! ## Overview
//!
//! The graph registry serves as the single source of truth for all knowledge graphs
//! in the system. Each graph is identified by a UUID and has its own isolated storage
//! directory and GraphManager instance. Multiple graphs can be open simultaneously, 
//! with all mutations flowing through the CQRS CommandQueue for deadlock-free operation.
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
//! - Maintains mapping from UUID to GraphInfo
//! - Tracks open graphs in `HashSet<Uuid>` (replaces single active_graph_id)
//! - Handles graph lifecycle: register, open, close, remove
//! - Provides centralized graph resolution by UUID or name
//! - Maintains registry state in memory
//! - Offers complete workflow methods that coordinate with AgentRegistry for prime agent authorization
//!
//! ## Graph State Management
//!
//! Graphs exist in two states:
//! - **Open**: Loaded in memory with active GraphManager instance
//! - **Closed**: Persisted to disk, resources freed from memory
//!
//! The registry tracks open graphs in memory during runtime.
//!
//! ## Concurrency Safety
//!
//! The GraphRegistry is owned by CommandProcessor in the CQRS architecture:
//! 
//! - **Sequential Access**: CommandProcessor ensures sequential mutations
//! - **RouterToken**: All mutations require RouterToken authorization
//! - **Read Access**: External reads via Arc<RwLock> for queries
//! - **Write Access**: Only CommandProcessor can mutate via RouterToken
//! 
//! The CQRS pattern eliminates deadlocks by serializing all mutations through
//! a single-threaded command processor while allowing unlimited concurrent reads.
//!
//! ## Complete Workflow Methods
//!
//!
//! ## Data Directory Structure
//!
//! ```
//! {data_dir}/
//!   graphs/
//!     {uuid-1}/               # Graph 1 data
//!       knowledge_graph.json
//!       wal/
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
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::graph::graph_manager::GraphManager;
use crate::agent::agent_registry::AgentRegistry;
use crate::error::*;
// Result type from error module
use crate::Result;
use crate::cqrs::router::RouterToken;



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
    graphs: HashMap<Uuid, GraphInfo>,
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

    /// Set the data directory for graph persistence
    pub fn set_data_dir(&mut self, data_dir: &Path) {
        self.data_dir = Some(data_dir.to_path_buf());
    }
    
    /// Open a graph (complete workflow with loading)
    /// 
    /// This method orchestrates the full open workflow:
    /// 1. Mark graph as open in registry
    /// 2. Load or create the GraphManager
    /// 
    /// Takes resources as parameters to avoid weak references.
    pub async fn open_graph_complete(
        &mut self,
        _token: &RouterToken,
        graph_id: Uuid,
        graph_managers: &mut HashMap<Uuid, Arc<RwLock<GraphManager>>>,
        data_dir: &Path,
    ) -> Result<()> {
        // Step 1: Mark graph as open in registry
        self.open_graph(&graph_id).await?;
        
        // Step 2: Create GraphManager if not in memory
        if !graph_managers.contains_key(&graph_id) {
            // Create graph manager
            let graph_dir = data_dir.join("graphs").join(graph_id.to_string());
            std::fs::create_dir_all(&graph_dir)?;
            let graph_manager = GraphManager::new(graph_dir)?;
            
            // Insert into HashMap
            graph_managers.insert(graph_id, Arc::new(RwLock::new(graph_manager)));
        }
        
        Ok(())
    }
    
    /// Create a new knowledge graph (complete workflow)
    /// 
    /// This method orchestrates the full creation workflow:
    /// 1. Register graph metadata
    /// 2. Create GraphManager
    /// 3. Authorize prime agent
    /// 
    /// Takes resources as parameters to avoid weak references.
    pub async fn create_graph_complete(
        &mut self,
        _token: &RouterToken,
        graph_id: Uuid,  // Now passed as parameter from resolved command
        name: Option<String>,  
        description: Option<String>,
        graph_managers: &mut HashMap<Uuid, Arc<RwLock<GraphManager>>>,
        agent_registry: &mut AgentRegistry,
        data_dir: &Path,
    ) -> Result<GraphInfo> {
        // Create the graph directory
        let graph_dir = data_dir.join("graphs").join(graph_id.to_string());
        std::fs::create_dir_all(&graph_dir)?;
        
        // Step 1: Register the graph
        let graph_info = self.register_graph(Some(graph_id), name, description, &graph_dir).await?;
        
        // Step 2: Create the GraphManager
        let graph_manager = GraphManager::new(&graph_dir)?;
        graph_managers.insert(graph_id, Arc::new(RwLock::new(graph_manager)));
        
        // Step 3: Authorize prime agent if it exists
        if let Some(prime_id) = agent_registry.get_prime_agent_id() {
            agent_registry.authorize_agent_for_graph(_token, &prime_id, &graph_info.id, self).await?;
        }
        
        info!("✅ Created graph {} with prime agent authorization", graph_info.name);
        Ok(graph_info)
    }

    

    /// Register a new knowledge graph
    /// 
    /// TODO: Add name uniqueness validation to prevent duplicate graph names.
    /// Currently, multiple graphs can have the same name, which could cause
    /// confusion when using name-based resolution. Consider rejecting duplicate
    /// names or warning the user.
    pub async fn register_graph(
        &mut self, 
        id: Option<Uuid>, 
        name: Option<String>,
        description: Option<String>,
        data_dir: &Path,
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
    
    /// Open a graph (pure registry operation)
    /// 
    /// This method ONLY updates registry state. It does not create GraphManagers or rebuild from command log.
    /// For the complete workflow, use open_graph_complete().
    pub async fn open_graph(&mut self, graph_id: &Uuid) -> Result<GraphInfo> {
        // Validate graph exists
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone();
        
        // Add to open set
        self.open_graphs.insert(*graph_id);
        
        // Update last accessed time
        if let Some(graph) = self.graphs.get_mut(graph_id) {
            graph.last_accessed = Utc::now();
        }
        
        Ok(graph_info)
    }
    
    /// Close a graph (remove from open set and unload manager)
    /// 
    /// Takes graph_managers to properly remove the manager from memory.
    pub async fn close_graph(
        &mut self,
        _token: &RouterToken,
        graph_id: &Uuid,
        graph_managers: &mut HashMap<Uuid, Arc<RwLock<GraphManager>>>,
    ) -> Result<()> {
        // Validate graph is open
        if !self.open_graphs.contains(graph_id) {
            return Err(StorageError::graph_registry(format!("Graph '{}' was not open", graph_id)).into());
        }
        
        // Remove manager from memory to prevent memory leak
        graph_managers.remove(graph_id);
        
        // Remove from open set
        self.open_graphs.remove(graph_id);
        
        Ok(())
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
    
    /// Remove a graph from the registry and archive its data
    /// 
    /// Archives the graph directory to `{data_dir}/archived_graphs/` with timestamp.
    /// Takes graph_managers to properly remove the manager from memory if the graph is open.
    pub async fn remove_graph(
        &mut self,
        _token: &RouterToken,
        graph_id: &Uuid,
        graph_managers: &mut HashMap<Uuid, Arc<RwLock<GraphManager>>>,
        agent_registry: &mut AgentRegistry,
    ) -> Result<()> {
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
                fs::create_dir_all(&archive_dir)
                    .map_err(|e| StorageError::graph_registry(format!("Failed to create archive directory: {}", e)))?;
                
                // Move to archive with timestamp
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                let archive_path = archive_dir.join(format!("{}_{}", graph_id, timestamp));
                
                fs::rename(&graph_data_dir, &archive_path)
                    .map_err(|e| StorageError::graph_registry(format!("Failed to archive graph data: {}", e)))?;
                
                info!("Archived knowledge graph: {} ({}) to {:?}", 
                      graph_info.name, graph_id, archive_path);
            }
        }
        
        // Remove from registry
        self.graphs.remove(graph_id);
        
        // Also remove from open graphs if it was open
        if self.open_graphs.remove(graph_id) {
            // Remove manager from memory to prevent memory leak
            graph_managers.remove(graph_id);
        }
        
        // Clean up this graph from all agents' authorized_graphs lists
        agent_registry.remove_graph_from_all_agents(graph_id);
        
        Ok(())
    }
    
    // ========== Recovery-Only Methods ==========
    // These methods are ONLY for command recovery and bypass logging
    
    
    /// Add an agent to a graph's authorized list (called by AgentRegistry)
    pub fn add_authorized_agent(&mut self, graph_id: &Uuid, agent_id: &Uuid) -> Result<()> {
        if let Some(graph) = self.graphs.get_mut(graph_id) {
            if !graph.authorized_agents.contains(agent_id) {
                graph.authorized_agents.push(*agent_id);
            }
            Ok(())
        } else {
            Err(StorageError::not_found("graph", "id", graph_id.to_string()).into())
        }
    }
    
    /// Remove an agent from a graph's authorized list (called by AgentRegistry)
    pub fn remove_authorized_agent(&mut self, graph_id: &Uuid, agent_id: &Uuid) {
        if let Some(graph) = self.graphs.get_mut(graph_id) {
            graph.authorized_agents.retain(|id| id != agent_id);
        }
    }
    
    /// Remove an agent from ALL graphs' authorized lists (used when deleting an agent)
    pub fn remove_agent_from_all_graphs(&mut self, agent_id: &Uuid) {
        for graph in self.graphs.values_mut() {
            graph.authorized_agents.retain(|id| id != agent_id);
        }
    }
    
    /// Export the registry to JSON for debugging/inspection
    /// 
    /// Note: This is NOT for persistence - command log is the source of truth
    /// The test harness (tests/common/wal_validation.rs) reads the command log for validation
    pub fn export_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self)?;
        
        fs::write(path, json)?;
        
        Ok(())
    }
}

/// Custom serialization modules for UUID collections
mod uuid_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::{HashMap, HashSet};
    use uuid::Uuid;
    
    pub mod uuid_hashmap_serde {
        use super::*;
        
        pub fn serialize<S, V>(map: &HashMap<Uuid, V>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
            V: Serialize,
        {
            let string_map: HashMap<String, &V> = map
                .iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
            string_map.serialize(serializer)
        }
        
        pub fn deserialize<'de, D, V>(deserializer: D) -> Result<HashMap<Uuid, V>, D::Error>
        where
            D: Deserializer<'de>,
            V: Deserialize<'de>,
        {
            let string_map = HashMap::<String, V>::deserialize(deserializer)?;
            string_map
                .into_iter()
                .map(|(k, v)| {
                    Uuid::parse_str(&k)
                        .map(|uuid| (uuid, v))
                        .map_err(serde::de::Error::custom)
                })
                .collect()
        }
    }
    
    pub mod uuid_hashset_serde {
        use super::*;
        
        pub fn serialize<S>(set: &HashSet<Uuid>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let string_vec: Vec<String> = set
                .iter()
                .map(|uuid| uuid.to_string())
                .collect();
            string_vec.serialize(serializer)
        }
        
        pub fn deserialize<'de, D>(deserializer: D) -> Result<HashSet<Uuid>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let string_vec = Vec::<String>::deserialize(deserializer)?;
            string_vec
                .into_iter()
                .map(|s| Uuid::parse_str(&s).map_err(serde::de::Error::custom))
                .collect()
        }
    }
    
    pub mod uuid_vec_serde {
        use super::*;
        
        pub fn serialize<S>(vec: &Vec<Uuid>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let string_vec: Vec<String> = vec
                .iter()
                .map(|uuid| uuid.to_string())
                .collect();
            string_vec.serialize(serializer)
        }
        
        pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Uuid>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let string_vec = Vec::<String>::deserialize(deserializer)?;
            string_vec
                .into_iter()
                .map(|s| Uuid::parse_str(&s).map_err(serde::de::Error::custom))
                .collect()
        }
    }
}

use uuid_serde::{uuid_hashmap_serde, uuid_hashset_serde, uuid_vec_serde};

#[cfg(test)]
mod tests {
    // Tests removed: These operations now require a transaction coordinator
    // and are better tested through integration tests that set up the full
    // AppState and transaction system. The business logic is thoroughly 
    // tested in tests/integration/
    
    use super::*;

    #[test]
    fn test_new_registry() {
        let registry = GraphRegistry::new();
        assert!(registry.graphs.is_empty());
        assert!(registry.open_graphs.is_empty());
    }
}