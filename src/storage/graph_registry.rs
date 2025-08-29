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
//! - **Open**: Loaded in memory with active manager and transaction coordinator
//! - **Closed**: Persisted to disk, resources freed from memory
//!
//! The registry tracks open graphs in memory during runtime.
//!
//! ## Concurrency Safety
//!
//! The GraphRegistry is accessed through `Arc<RwLock<GraphRegistry>>` with panic-on-poison
//! strategy implemented via the lock.rs module:
//! 
//! - **Lock Pattern**: Use `read_or_panic()` and `write_or_panic()` for all lock operations
//! - **Contention Detection**: Write operations automatically warn about lock contention in debug builds
//! - **Lock Ordering**: When acquiring both graph_registry and agent_registry, use `lock_registries_for_write()`
//! - **Scope Management**: Keep lock scopes minimal to reduce contention
//! 
//! The panic-on-poison strategy ensures data integrity by immediately halting execution
//! when a thread panics while holding a lock, preventing continued operation with
//! potentially corrupted state.
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
use tokio::sync::RwLock;
use std::sync::Arc;

// Import shared UUID serialization utilities
use crate::storage::registry_utils::{uuid_hashmap_serde, uuid_hashset_serde, uuid_vec_serde};
use crate::storage::TransactionCoordinator;
use crate::storage::transaction_log::Operation;
use crate::graph_manager::GraphManager;
use crate::error::*;
use crate::lock::AsyncRwLockExt;
use crate::AppState;
use std::sync::Weak;
// Result type from error module
use crate::Result;



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
    
    /// Transaction coordinator for WAL operations (not serialized)
    #[serde(skip)]
    transaction_coordinator: Option<Arc<TransactionCoordinator>>,
    
    /// Reference to AppState for accessing graph managers and other resources
    /// Uses Weak to avoid reference cycles
    #[serde(skip)]
    app_state: Option<Weak<AppState>>,
}


impl GraphRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        GraphRegistry {
            graphs: HashMap::new(),
            open_graphs: HashSet::new(),
            data_dir: None,
            transaction_coordinator: None,
            app_state: None,
        }
    }

    /// Set the data directory and transaction coordinator
    pub fn set_resources(&mut self, data_dir: &Path, transaction_coordinator: Arc<TransactionCoordinator>) {
        self.data_dir = Some(data_dir.to_path_buf());
        self.transaction_coordinator = Some(transaction_coordinator);
    }
    
    /// Set the AppState reference
    /// Called during AppState initialization to give registry access to resources
    pub fn set_app_state(&mut self, app_state: &Arc<AppState>) {
        self.app_state = Some(Arc::downgrade(app_state));
    }
    
    /// Open a graph (complete workflow with loading and WAL rebuild)
    /// 
    /// This method orchestrates the full open workflow:
    /// 1. Mark graph as open in registry
    /// 2. Load or create the GraphManager
    /// 3. Rebuild from WAL if needed
    /// 
    /// Uses Arc<RwLock<Self>> to minimize lock holding time.
    pub async fn open_graph_complete(
        registry: Arc<RwLock<GraphRegistry>>,
        graph_id: Uuid,
        skip_wal: bool,
    ) -> Result<()> {
        // Step 1: Update registry to mark open (brief lock)
        let _graph_info = {
            let mut reg = registry.write_or_panic("open graph - registry update").await;
            reg.open_graph(&graph_id, skip_wal).await?
        };
        // Registry lock released
        
        // Step 2: Get app_state for further operations
        let app_state = {
            let reg = registry.read_or_panic("open graph - get app_state").await;
            reg.app_state.as_ref()
                .and_then(|weak| weak.upgrade())
                .ok_or_else(|| StorageError::graph_registry("No AppState reference"))?
        };
        
        // Step 3: Create GraphManager if not in memory
        let needs_rebuild = {
            let managers = app_state.graph_managers.read().await;
            
            if !managers.contains_key(&graph_id) {
                drop(managers); // Release read lock
                
                // Create graph manager
                let graph_dir = app_state.data_dir.join("graphs").join(graph_id.to_string());
                std::fs::create_dir_all(&graph_dir)?;
                let graph_manager = GraphManager::new(graph_dir)?;
                
                // Insert into HashMap
                let mut managers_write = app_state.graph_managers.write().await;
                managers_write.insert(graph_id, RwLock::new(graph_manager));
                
                // New managers need rebuilding from WAL
                true
            } else {
                // Check if existing manager needs rebuilding
                if let Some(manager_lock) = managers.get(&graph_id) {
                    let manager = manager_lock.read().await;
                    manager.graph.node_count() == 0
                } else {
                    false
                }
            }
        };
        
        // Step 4: Rebuild from WAL if needed (no locks held)
        if needs_rebuild {
            let context = crate::storage::recovery::RecoveryContext {
                app_state: app_state.clone(),
                is_rebuilding: true,
            };
            
            crate::storage::recovery::rebuild_graph_from_wal(
                &graph_id,
                &app_state.transaction_coordinator,
                &context
            ).await?;
        } else {
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
    /// Uses Arc<RwLock<Self>> to minimize lock holding time.
    pub async fn create_graph_complete(
        registry: Arc<RwLock<GraphRegistry>>,
        name: Option<String>,  
        description: Option<String>,
    ) -> Result<GraphInfo> {
        tracing::debug!("create_graph_complete: Starting");
        // Get app_state
        let app_state = {
            let reg = registry.read_or_panic("create graph - get app_state").await;
            let state = reg.app_state.as_ref()
                .and_then(|weak| weak.upgrade())
                .ok_or_else(|| StorageError::graph_registry("No AppState reference"))?;
            state
        };
        tracing::debug!("create_graph_complete: Got app_state");
        
        // Create the graph directory
        let graph_id = Uuid::new_v4();
        let graph_dir = app_state.data_dir.join("graphs").join(graph_id.to_string());
        std::fs::create_dir_all(&graph_dir)?;
        
        // Step 1: Register the graph (brief lock)
        let graph_info = {
            let mut reg = registry.write_or_panic("create graph - register").await;
            let info = reg.register_graph(Some(graph_id), name, description, &graph_dir, false).await?;
            info
        };
        
        // Step 2: Create the GraphManager (no registry lock)
        {
            let graph_manager = GraphManager::new(&graph_dir)?;
            tracing::debug!("create_graph_complete: Created GraphManager, acquiring managers write lock");
            let mut managers = app_state.graph_managers.write().await;
            tracing::debug!("create_graph_complete: Got managers write lock, inserting");
            managers.insert(graph_id, RwLock::new(graph_manager));
            tracing::debug!("create_graph_complete: GraphManager inserted");
        }
        
        // Step 3: Authorize prime agent if it exists (proper lock ordering)
        let prime_id = {
            tracing::debug!("create_graph_complete: Getting prime agent ID");
            let agent_registry = app_state.agent_registry.read_or_panic("get prime agent").await;
            let id = agent_registry.get_prime_agent_id();
            tracing::debug!("create_graph_complete: Prime agent ID: {:?}", id);
            id
        };
        
        if let Some(prime_id) = prime_id {
            // Use proper lock ordering with both registries
            tracing::debug!("create_graph_complete: Authorizing prime agent for graph");
            use crate::lock::lock_registries_for_write;
            
            let (mut graph_registry, mut agent_registry) = lock_registries_for_write(
                &app_state.graph_registry,
                &app_state.agent_registry
            ).await?;
            agent_registry.authorize_agent_for_graph(&prime_id, &graph_info.id, &mut graph_registry, false).await?;
            tracing::debug!("create_graph_complete: Prime agent authorized");
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
        skip_wal: bool
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
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(crate::storage::transaction_log::RegistryOperation::Graph(
                crate::storage::transaction_log::GraphRegistryOp::RegisterGraph {
                    graph_id,
                    name: Some(name.clone()),
                    description: description.clone(),
                }
            )))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::graph_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let graphs = &mut self.graphs;
        let open_graphs = &mut self.open_graphs;
        let data_dir = data_dir.to_path_buf();
        
        let tx = coordinator.begin(operation).await?;
        
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

        graphs.insert(graph_id, graph_info.clone());
        
        // Always open newly registered graphs
        open_graphs.insert(graph_id);
        
        tx.commit().await?;
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
    /// This method ONLY updates registry state. It does not create GraphManagers or rebuild from WAL.
    /// For the complete workflow, use open_graph_complete().
    pub async fn open_graph(&mut self, graph_id: &Uuid, skip_wal: bool) -> Result<GraphInfo> {
        // Validate graph exists
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone();
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(crate::storage::transaction_log::RegistryOperation::Graph(
                crate::storage::transaction_log::GraphRegistryOp::OpenGraph {
                    graph_id: *graph_id,
                }
            )))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::graph_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let open_graphs = &mut self.open_graphs;
        let graphs = &mut self.graphs;
        
        let tx = coordinator.begin(operation).await?;
        
        // Add to open set
        if open_graphs.insert(*graph_id) {
        }
        
        // Update last accessed time
        if let Some(graph) = graphs.get_mut(graph_id) {
            graph.last_accessed = Utc::now();
        }
        
        tx.commit().await?;
        Ok(graph_info)
    }
    
    /// Close a graph (remove from open set and unload manager)
    pub async fn close_graph(&mut self, graph_id: &Uuid, skip_wal: bool) -> Result<()> {
        // Validate graph is open
        if !self.open_graphs.contains(graph_id) {
            return Err(StorageError::graph_registry(format!("Graph '{}' was not open", graph_id)).into());
        }
        
        // Remove manager from memory
        if let Some(app_state) = self.app_state.as_ref().and_then(|w| w.upgrade()) {
            let mut managers = app_state.graph_managers.write().await;
            managers.remove(graph_id);
        }
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(crate::storage::transaction_log::RegistryOperation::Graph(
                crate::storage::transaction_log::GraphRegistryOp::CloseGraph {
                    graph_id: *graph_id,
                }
            )))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::graph_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let open_graphs = &mut self.open_graphs;
        
        let tx = coordinator.begin(operation).await?;
        
        if !open_graphs.remove(graph_id) {
            // Graph was not open, but we still commit since desired state is achieved
        }
        
        tx.commit().await
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
    pub async fn remove_graph(&mut self, graph_id: &Uuid, skip_wal: bool) -> Result<()> {
        // Get the graph info
        let graph_info = self.graphs.get(graph_id)
            .ok_or_else(|| StorageError::not_found("graph", "ID", graph_id.to_string()))?
            .clone();
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(crate::storage::transaction_log::RegistryOperation::Graph(
                crate::storage::transaction_log::GraphRegistryOp::RemoveGraph {
                    graph_id: *graph_id,
                }
            )))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::graph_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let data_dir = self.data_dir.clone();
        let graphs = &mut self.graphs;
        let open_graphs = &mut self.open_graphs;
        
        let tx = coordinator.begin(operation).await?;
        
        // Archive the graph data if we have a data directory
        if let Some(data_dir) = &data_dir {
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
        graphs.remove(graph_id);
        
        // Also remove from open graphs if it was open
        if open_graphs.remove(graph_id) {
        }
        
        tx.commit().await
    }
    
    // ========== Recovery-Only Methods ==========
    // These methods are ONLY for WAL recovery and bypass transaction logging
    
    
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
    
    /// Export the registry to JSON for debugging/inspection
    /// 
    /// Note: This is NOT for persistence - WAL is the source of truth
    /// The test harness (tests/common/graph_validation.rs) reads this file for validation
    pub fn export_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self)?;
        
        fs::write(path, json)?;
        
        Ok(())
    }
}

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