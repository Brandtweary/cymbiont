//! Agent Registry: Multi-Agent Lifecycle Management
//!
//! This module provides centralized metadata tracking and lifecycle management for agents,
//! serving as the single source of truth for agent authorization and state. It parallels
//! GraphRegistry in design, tracking metadata while the Agent struct owns the actual data.
//!
//! ## Design Philosophy
//!
//! AgentRegistry serves as the authoritative source for:
//! - Agent existence and metadata
//! - Active/inactive state tracking
//! - Graph authorization mappings
//! - Prime agent designation
//!
//! This centralization prevents synchronization bugs that would occur if agents
//! tracked their own authorizations. The registry maintains bidirectional tracking
//! with GraphRegistry to ensure consistency.
//!
//! ## Key Public Functions
//!
//! ### Lifecycle Management
//! - `register_agent()` - Create new agent with optional name/description
//! - `activate_agent()` - Load agent into memory
//! - `deactivate_agent()` - Save and unload agent
//! - `remove_agent()` - Archive agent to `archived_agents/`
//! - `activate_agent_complete()`, `deactivate_agent_complete()` - Complete workflows with persistence
//!
//! ### Authorization Management
//! - `authorize_agent_for_graph()` - Grant agent access to a graph
//! - `deauthorize_agent_from_graph()` - Revoke agent access
//! - `is_agent_authorized()` - Check authorization status
//!
//! ### Agent Resolution
//! - `resolve_agent_target()` - Flexible UUID/name resolution with smart defaults
//! - `get_agent()` - Retrieve agent metadata by ID
//! - `get_all_agents()` - List all registered agents
//! - `get_active_agents()` - List currently loaded agents
//!
//! ### Prime Agent
//! - `ensure_prime_agent()` - Check if prime agent needs creation
//! - `get_prime_agent_id()` - Get the prime agent UUID
//!
//! ## Prime Agent Behavior
//!
//! The prime agent is automatically:
//! - Created on first run if no agents exist
//! - Authorized for all new graphs
//! - Protected from deletion
//! - Used as smart default for agent_info operations
//!
//! ## Complete Workflow Methods
//!
//! To reduce AppState verbosity, the registry provides complete workflow methods:
//! - `activate_agent_complete()` - Agent activation with registry persistence
//! - `deactivate_agent_complete()` - Agent deactivation with registry persistence
//!
//! These methods handle validation, state updates, and persistence, allowing AppState
//! to focus on memory management rather than workflow orchestration.
//!
//! ## Data Structure
//!
//! ```
//! {data_dir}/
//!   agent_registry.json     # Agent metadata and authorization
//!   agents/
//!     {agent-id}/
//!       agent.json          # Full agent state (owned by Agent)
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

use crate::agent::agent::Agent;
use crate::agent::llm::LLMConfig;
use crate::graph::graph_registry::GraphRegistry;
use crate::error::*;
use crate::Result;
use crate::cqrs::router::RouterToken;



/// Minimal metadata about a registered agent
/// 
/// The actual agent data (configuration, conversation history, etc.)
/// is stored separately and managed by the Agent struct itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Internal Cymbiont UUID
    pub id: Uuid,
    
    /// Friendly name for the agent
    pub name: String,
    
    /// When this agent was created
    pub created: DateTime<Utc>,
    
    /// Last time this agent was active
    pub last_active: DateTime<Utc>,
    
    /// Optional description of agent's purpose
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    /// Path where agent data is stored
    pub data_path: PathBuf,
    
    /// Graphs this agent is authorized to access
    #[serde(default, with = "uuid_vec_serde")]
    pub authorized_graphs: Vec<Uuid>,
    
    /// Whether this is the prime agent (gets auto-authorized for all graphs)
    #[serde(default)]
    pub is_prime: bool,
}

/// Registry of all known agents
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentRegistry {
    /// Map of agent ID to agent info
    #[serde(with = "uuid_hashmap_serde")]
    agents: HashMap<Uuid, AgentInfo>,
    
    /// Currently active agent IDs (loaded in memory)
    #[serde(default, with = "uuid_hashset_serde")]
    active_agents: HashSet<Uuid>,
    
    /// The prime agent ID (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    prime_agent_id: Option<Uuid>,
    
    /// Base data directory (not serialized)
    #[serde(skip)]
    data_dir: Option<PathBuf>,
}


impl AgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        AgentRegistry {
            agents: HashMap::new(),
            active_agents: HashSet::new(),
            prime_agent_id: None,
            data_dir: None,
        }
    }

    /// Set the data directory for agent persistence
    pub fn set_data_dir(&mut self, data_dir: &Path) {
        self.data_dir = Some(data_dir.to_path_buf());
    }
    
    /// Activate an agent (complete workflow with loading)
    /// 
    /// This method orchestrates the full activation workflow:
    /// 1. Mark agent as active in registry
    /// 2. Load or create the Agent instance
    /// 
    /// Takes resources as parameters to avoid weak references.
    pub async fn activate_agent_complete(
        &mut self,
        _token: &RouterToken,
        agent_id: Uuid,
        agents: &mut HashMap<Uuid, Arc<RwLock<Agent>>>,
    ) -> Result<()> {
        // Step 1: Mark agent as active in registry
        let agent_info = self.activate_agent(&agent_id).await?;
        
        // Step 2: Load or create the Agent instance if not in memory
        if !agents.contains_key(&agent_id) {
            // Create empty agent (will be rebuilt from WAL if needed)
            let agent = Agent::new_empty(
                agent_id,
                agent_info.name.clone()
            );
            
            // Insert into HashMap
            agents.insert(agent_id, Arc::new(RwLock::new(agent)));
        }
        
        Ok(())
    }
    
    /// Create a new agent (complete workflow)
    /// 
    /// This method orchestrates the full agent creation workflow:
    /// 1. Registers the agent metadata
    /// 2. Creates the Agent instance
    /// 3. Inserts it into the active agents HashMap
    /// 
    /// Takes resources as parameters to avoid weak references.
    pub async fn create_agent_complete(
        &mut self,
        _token: &RouterToken,
        agent_id: Uuid,  // Now passed as parameter from resolved command
        name: Option<String>,
        description: Option<String>,
        system_prompt: Option<String>,
        agents_map: &mut HashMap<Uuid, Arc<RwLock<Agent>>>,
        data_dir: &Path,
    ) -> Result<AgentInfo> {
        // Step 1: Register the agent
        let agent_info = self.register_agent(
            _token,
            Some(agent_id),  // Use the resolved ID
            name.clone(),
            description.clone(),
        ).await?;
        
        // Step 2: Create the actual Agent instance
        // Ensure agent directory exists
        let agent_dir = data_dir.join("agents").join(agent_info.id.to_string());
        std::fs::create_dir_all(&agent_dir)?;
        
        // Create agent with default MockLLM config
        let agent = Agent::new(
            agent_info.id,
            agent_info.name.clone(),
            LLMConfig::default(),  // MockLLM by default
            system_prompt.or(Some("You are a helpful assistant".to_string())),
        );
        
        // Step 3: Insert into active agents HashMap
        agents_map.insert(agent_info.id, Arc::new(RwLock::new(agent)));
        
        // Step 4: Mark as active
        self.active_agents.insert(agent_info.id);
        
        info!("✅ Created and activated agent: {} ({})", agent_info.name, agent_info.id);
        
        Ok(agent_info)
    }

    /// Register a new agent (creates metadata only)
    /// 
    /// TODO: Add name uniqueness validation to prevent duplicate agent names.
    /// Currently, multiple agents can have the same name, which could cause
    /// confusion when using name-based resolution. Consider rejecting duplicate
    /// names or warning the user.
    pub async fn register_agent(
        &mut self,
        _token: &RouterToken,
        id: Option<Uuid>,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<AgentInfo> {
        let agent_id = id.unwrap_or_else(|| Uuid::new_v4());
        let final_name = name.unwrap_or_else(|| format!("Agent {}", &agent_id.to_string()[..8]));
        
        // Check if this ID already exists - if so, just update metadata (no WAL needed for updates)
        if let Some(existing) = self.agents.get_mut(&agent_id) {
            // Update metadata and return existing
            existing.name = final_name;
            existing.last_active = Utc::now();
            if description.is_some() {
                existing.description = description;
            }
            return Ok(existing.clone());
        }
        
        // Create new agent metadata
        let data_dir = self.data_dir.clone()
            .ok_or_else(|| StorageError::agent_registry("No data directory set"))?;
        
        let data_path = data_dir
            .join("agents")
            .join(agent_id.to_string());
        
        let agent_info = AgentInfo {
            id: agent_id,
            name: final_name.clone(),
            created: Utc::now(),
            last_active: Utc::now(),
            description: description.clone(),
            data_path,
            authorized_graphs: Vec::new(),
            is_prime: false,  // Will be set separately if needed
        };

        self.agents.insert(agent_id, agent_info.clone());
        
        // New agents start as active
        self.active_agents.insert(agent_id);
        info!("✅ Created agent: {} ({})", final_name, agent_id);
        Ok(agent_info)
    }

    /// Get agent info by ID
    pub fn get_agent(&self, id: &Uuid) -> Option<&AgentInfo> {
        self.agents.get(id)
    }

    /// Get all registered agents
    pub fn get_all_agents(&self) -> Vec<AgentInfo> {
        self.agents.values().cloned().collect()
    }

    /// Get all currently active agent IDs
    pub fn get_active_agents(&self) -> Vec<Uuid> {
        self.active_agents.iter().copied().collect()
    }

    /// Check if an agent is active
    pub fn is_agent_active(&self, agent_id: &Uuid) -> bool {
        self.active_agents.contains(agent_id)
    }

    /// Activate an agent (pure registry operation)
    /// 
    /// This method ONLY updates registry state. It does not load agents or rebuild from WAL.
    /// For the complete workflow, use activate_agent_complete().
    pub async fn activate_agent(&mut self, agent_id: &Uuid) -> Result<AgentInfo> {
        // Validate agent exists
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Add to active set
        self.active_agents.insert(*agent_id);
        
        // Update last active time
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.last_active = Utc::now();
        }
        Ok(agent_info)
    }

    /// Deactivate an agent (complete workflow with unloading)
    /// 
    /// This method orchestrates the full deactivation workflow:
    /// 1. Mark agent as inactive in registry
    /// 2. Remove agent from memory to prevent memory leak
    /// 
    /// Takes resources as parameters to avoid weak references.
    pub async fn deactivate_agent_complete(
        &mut self,
        _token: &RouterToken,
        agent_id: Uuid,
        agents: &mut HashMap<Uuid, Arc<RwLock<Agent>>>,
    ) -> Result<()> {
        // Step 1: Mark agent as inactive in registry
        self.deactivate_agent(&agent_id).await?;
        
        // Step 2: Remove agent from memory to prevent memory leak
        agents.remove(&agent_id);
        
        Ok(())
    }
    
    /// Deactivate an agent (mark as inactive only)
    /// 
    /// Note: The caller is responsible for removing the agent from memory.
    pub async fn deactivate_agent(&mut self, agent_id: &Uuid) -> Result<()> {
        // Validate agent is active
        if !self.active_agents.contains(agent_id) {
            return Err(StorageError::agent_registry(format!("Agent '{}' was not active", agent_id)).into());
        }
        
        if !self.active_agents.remove(agent_id) {
            // Agent was not active, but the desired state is achieved
        }
        
        Ok(())
    }

    /// Set an agent as the prime agent
    /// 
    /// The prime agent is the default agent with special privileges.
    /// This operation is logged to WAL for persistence across restarts.
    pub async fn set_prime_agent(&mut self, _token: &RouterToken, agent_id: &Uuid) -> Result<()> {
        // Verify agent exists
        if !self.agents.contains_key(agent_id) {
            return Err(StorageError::not_found("agent", "id", agent_id.to_string()).into());
        }
        
        // Update agent's prime status
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.is_prime = true;
        }
        
        // Set registry's prime agent ID
        self.prime_agent_id = Some(*agent_id);
        
        Ok(())
    }
    
    /// Check if a prime agent exists.
    /// Returns true if prime agent needs to be created, false if it already exists.
    /// 
    /// This is a read-only check. If true is returned, the caller should submit
    /// a CreateAgent command through the CQRS system to actually create the prime agent.
    pub fn ensure_prime_agent(&self) -> bool {
        self.prime_agent_id.is_none()
    }
    
    /// Get the prime agent ID (if any)
    pub fn get_prime_agent_id(&self) -> Option<Uuid> {
        self.prime_agent_id
    }
    
    /// Resolve agent target from optional UUID and name with smart defaults
    /// 
    /// Priority order:
    /// 1. If agent_id provided, validate it exists
    /// 2. Else if agent_name provided, resolve to UUID
    /// 3. Else if allow_smart_default, use prime agent
    /// 4. Else error
    pub fn resolve_agent_target(
        &self,
        agent_id: Option<&Uuid>,
        agent_name: Option<&str>,
        allow_smart_default: bool,
    ) -> Result<Uuid> {
        if let Some(id) = agent_id {
            // Validate the UUID exists
            if self.agents.contains_key(id) {
                Ok(*id)
            } else {
                Err(StorageError::not_found("agent", "ID", id.to_string()).into())
            }
        } else if let Some(name) = agent_name {
            // Find agent by name
            self.agents.values()
                .find(|a| a.name == name)
                .map(|a| a.id)
                .ok_or_else(|| StorageError::not_found("agent", "name", name).into())
        } else if allow_smart_default {
            // Use prime agent as default
            self.prime_agent_id
                .ok_or_else(|| StorageError::agent_registry("No prime agent exists. Create an agent first").into())
        } else {
            Err(StorageError::agent_registry("Must specify agent_id or agent_name").into())
        }
    }
    /// Authorize an agent to access a graph
    /// 
    /// This also updates the graph's authorized_agents list in GraphRegistry
    /// for bidirectional tracking. Call this with the prime agent ID when
    /// creating new graphs to ensure the prime agent has access by default.
    /// 
    /// If this is the agent's first graph authorization, it will automatically
    /// set this graph as the agent's default graph.
    pub async fn authorize_agent_for_graph(
        &mut self,
        _token: &RouterToken,
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut GraphRegistry,
    ) -> Result<()> {
        // Validate agent exists
        if !self.agents.contains_key(agent_id) {
            return Err(StorageError::not_found("agent", "ID", agent_id.to_string()).into());
        }
        
        
        // Get the agent
        let agent = self.agents.get_mut(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?;
        
        // Add graph to agent's authorized list if not already there
        if !agent.authorized_graphs.contains(graph_id) {
            agent.authorized_graphs.push(*graph_id);
        }
        
        // Update graph's authorized_agents list for bidirectional tracking
        graph_registry.add_authorized_agent(graph_id, agent_id)?;
        
        
        // NOTE: Default graph setting moved to Phase 4 of message queue to avoid deadlock
        // The first authorized graph becomes the default, but this is handled elsewhere
        // to prevent same-task reentrancy issues with tokio RwLocks.
        
        Ok(())
    }
    
    /// Remove agent authorization from a graph
    pub async fn deauthorize_agent_from_graph(
        &mut self,
        _token: &RouterToken,
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut GraphRegistry,
    ) -> Result<()> {
        // Update agent's authorized list
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.authorized_graphs.retain(|id| id != graph_id);
        }
        
        // Update graph's authorized_agents list
        graph_registry.remove_authorized_agent(graph_id, agent_id);
        
        Ok(())
    }
    
    /// Check if an agent is authorized for a graph
    /// 
    /// All agents (including prime) must be explicitly authorized for graphs.
    pub fn is_agent_authorized(&self, agent_id: &Uuid, graph_id: &Uuid) -> bool {
        self.agents.get(agent_id)
            .map(|agent| agent.authorized_graphs.contains(graph_id))
            .unwrap_or(false)
    }
    
    /// Find orphaned graphs (graphs with no authorized agents)
    /// 
    /// Note: This is mostly an edge case. The prime agent is explicitly authorized
    /// for all new graphs by default, so orphaning only occurs if a user manually
    /// deauthorizes their prime agent from a specific graph. Most users will
    /// only ever use the prime agent and never encounter orphaned graphs.
    pub fn find_orphaned_graphs(
        &self,
        graph_registry: &GraphRegistry,
    ) -> Vec<Uuid> {
        // In practice, this should return an empty list unless someone explicitly
        // deauthorized the prime agent from specific graphs
        graph_registry.get_all_graphs()
            .into_iter()
            .filter(|graph| {
                // A graph is orphaned if NO agent can access it
                !self.agents.values().any(|agent| {
                    self.is_agent_authorized(&agent.id, &graph.id)
                })
            })
            .map(|graph| graph.id)
            .collect()
    }

    /// Remove an agent from the registry and archive its data
    /// 
    /// Takes agents parameter to properly remove the agent from memory if it's active.
    /// Also removes the agent from all graphs' authorized_agents lists.
    pub async fn remove_agent(
        &mut self,
        _token: &RouterToken,
        agent_id: &Uuid,
        agents_map: &mut HashMap<Uuid, Arc<RwLock<Agent>>>,
        graph_registry: &mut GraphRegistry,
    ) -> Result<()> {
        // Get the agent info
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Archive the agent data if it exists
        if agent_info.data_path.exists() {
            if let Some(data_dir) = &self.data_dir {
                // Create archive directory if it doesn't exist
                let archive_dir = data_dir.join("archived_agents");
                fs::create_dir_all(&archive_dir)
                    .map_err(|e| StorageError::agent_registry(format!("Failed to create archive directory: {}", e)))?;
                
                // Move to archive with timestamp
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                let archive_path = archive_dir.join(format!("{}_{}", agent_id, timestamp));
                
                fs::rename(&agent_info.data_path, &archive_path)
                    .map_err(|e| StorageError::agent_registry(format!("Failed to archive agent data: {}", e)))?;
                
                info!("Archived agent: {} ({}) to {:?}", 
                      agent_info.name, agent_id, archive_path);
            }
        }
        
        // Remove from registry
        self.agents.remove(agent_id);
        
        // Also remove from active agents if it was active
        if self.active_agents.remove(agent_id) {
            // Remove agent from memory to prevent memory leak
            agents_map.remove(agent_id);
        }
        
        // Clean up this agent from all graphs' authorized_agents lists
        graph_registry.remove_agent_from_all_graphs(agent_id);
        
        Ok(())
    }
    
    /// Remove a graph from ALL agents' authorized lists (used when deleting a graph)
    pub fn remove_graph_from_all_agents(&mut self, graph_id: &Uuid) {
        for agent in self.agents.values_mut() {
            agent.authorized_graphs.retain(|id| id != graph_id);
        }
    }
    
    /// Export the registry to JSON for debugging/inspection
    /// 
    /// Note: This is NOT for persistence - WAL is the source of truth
    /// The test harness (tests/common/wal_validation.rs) reads the WAL for validation
    pub fn export_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| StorageError::agent_registry(format!("Failed to serialize agent registry: {}", e)))?;
        
        fs::write(path, json)
            .map_err(|e| StorageError::agent_registry(format!("Failed to write agent registry JSON: {}", e)))?;
        
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
        let registry = AgentRegistry::new();
        assert!(registry.agents.is_empty());
        assert!(registry.active_agents.is_empty());
    }
}
    