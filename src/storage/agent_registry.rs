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
//! - `authorize_prime_for_new_graph()` - Auto-authorize prime agent
//! - `is_agent_authorized()` - Check authorization status
//!
//! ### Agent Resolution
//! - `resolve_agent_target()` - Flexible UUID/name resolution with smart defaults
//! - `get_agent()` - Retrieve agent metadata by ID
//! - `get_all_agents()` - List all registered agents
//! - `get_active_agents()` - List currently loaded agents
//!
//! ### Prime Agent
//! - `ensure_default_agent()` - Create prime agent on first run
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

// Import shared UUID serialization utilities
use crate::storage::registry_utils::{uuid_hashmap_serde, uuid_hashset_serde, uuid_vec_serde};
use crate::error::*;



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

    /// Load registry from disk or create new if not found
    pub fn load_or_create(registry_path: &Path, data_dir: &Path) -> Result<Self> {
        let mut registry = if registry_path.exists() {
            let content = fs::read_to_string(registry_path)?;
            let loaded: AgentRegistry = serde_json::from_str(&content)?;
            info!("🤖 Loaded agent registry with {} agents, {} active", 
                  loaded.agents.len(), loaded.active_agents.len());
            loaded
        } else {
            AgentRegistry::new()
        };
        
        // Set data directory from the provided path
        registry.data_dir = Some(data_dir.to_path_buf());
        
        Ok(registry)
    }

    /// Save registry to disk at the default location
    pub fn save(&self) -> Result<()> {
        if let Some(data_dir) = &self.data_dir {
            let registry_path = data_dir.join("agent_registry.json");
            
            // Ensure parent directory exists
            if let Some(parent) = registry_path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            let content = serde_json::to_string_pretty(self)?;
            fs::write(registry_path, content)?;
            Ok(())
        } else {
            Err(StorageError::agent_registry("No data directory set for registry").into())
        }
    }

    /// Register a new agent (creates metadata only)
    /// 
    /// TODO: Add name uniqueness validation to prevent duplicate agent names.
    /// Currently, multiple agents can have the same name, which could cause
    /// confusion when using name-based resolution. Consider rejecting duplicate
    /// names or warning the user.
    pub fn register_agent(
        &mut self,
        id: Option<Uuid>,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<AgentInfo> {
        let agent_id = id.unwrap_or_else(|| Uuid::new_v4());
        let name = name.unwrap_or_else(|| format!("Agent {}", &agent_id.to_string()[..8]));
        
        // Check if this ID already exists
        if let Some(existing) = self.agents.get_mut(&agent_id) {
            // Update metadata and return existing
            existing.name = name;
            existing.last_active = Utc::now();
            if description.is_some() {
                existing.description = description;
            }
            return Ok(existing.clone());
        }
        
        // Create new agent metadata
        let data_path = self.data_dir
            .as_ref()
            .ok_or_else(|| StorageError::agent_registry("No data directory set"))?
            .join("agents")
            .join(agent_id.to_string());
        
        let agent_info = AgentInfo {
            id: agent_id,
            name: name.clone(),
            created: Utc::now(),
            last_active: Utc::now(),
            description,
            data_path,
            authorized_graphs: Vec::new(),
            is_prime: false,  // Will be set separately if needed
        };

        self.agents.insert(agent_id, agent_info.clone());
        
        // New agents start as active
        self.active_agents.insert(agent_id);
        info!("✅ Created agent: {} ({})", name, agent_id);
        
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

    /// Activate an agent (mark as loaded in memory)
    pub fn activate_agent(&mut self, agent_id: &Uuid) -> Result<AgentInfo> {
        // Validate agent exists
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Add to active set
        if self.active_agents.insert(*agent_id) {
        }
        
        // Update last active time
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.last_active = Utc::now();
        }
        
        Ok(agent_info)
    }

    /// Deactivate an agent (mark as unloaded from memory)
    /// 
    /// Note: This only updates the registry state. The caller (AppState) is
    /// responsible for saving the Agent instance before deactivation.
    pub fn deactivate_agent(&mut self, agent_id: &Uuid) -> Result<()> {
        if self.active_agents.remove(agent_id) {
            Ok(())
        } else {
            Err(StorageError::agent_registry(format!("Agent '{}' was not active", agent_id)).into())
        }
    }

    /// Ensure at least one agent exists (for first-run experience)
    /// 
    /// If no agents exist, creates the prime agent which gets auto-authorized for all graphs.
    /// If agents exist but none are active, activates the first one.
    pub fn ensure_default_agent(&mut self) -> Result<AgentInfo> {
        if self.agents.is_empty() {
            // No agents exist - create the prime agent
            let mut agent_info = self.register_agent(
                None,
                Some("Prime Agent".to_string()),
                Some("Primary assistant with full graph access".to_string()),
            )?;
            
            // Mark as prime agent
            agent_info.is_prime = true;
            let agent_id = agent_info.id;
            self.prime_agent_id = Some(agent_id);
            
            // Update the stored agent info
            if let Some(stored) = self.agents.get_mut(&agent_id) {
                stored.is_prime = true;
            }
            
            // Create the actual Agent instance
            use crate::agent::agent::Agent;
            use crate::agent::llm::LLMConfig;
            
            // Ensure agent directory exists
            std::fs::create_dir_all(&agent_info.data_path)
                ?;
            
            // Create prime agent with default MockLLM config
            let mut agent = Agent::new(
                agent_id,
                "Prime Agent".to_string(),
                LLMConfig::default(),  // MockLLM by default
                agent_info.data_path.clone(),
                Some("You are the prime agent, a helpful assistant with full access to knowledge graphs.".to_string()),
            );
            
            // Save the agent to disk
            agent.save()
                .map_err(|e| StorageError::agent_registry(format!("Failed to save prime agent: {:?}", e)))?;
            
            info!("👑 Created prime agent: {} ({})", agent_info.name, agent_id);
            
            Ok(agent_info)
        } else if self.active_agents.is_empty() {
            // Agents exist but none are active - activate the first one
            let first_id = *self.agents.keys().next().unwrap();
            self.activate_agent(&first_id)
        } else {
            // Return the first active agent
            let active_id = *self.active_agents.iter().next().unwrap();
            Ok(self.agents[&active_id].clone())
        }
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
    
    /// Helper to authorize prime agent for a newly created graph
    /// 
    /// Call this whenever a new graph is created to ensure the prime agent
    /// has access by default. Does nothing if no prime agent exists.
    pub fn authorize_prime_for_new_graph(
        &mut self,
        graph_id: &Uuid,
        graph_registry: &mut crate::storage::GraphRegistry,
    ) -> Result<()> {
        
        if let Some(prime_id) = self.prime_agent_id {
            self.authorize_agent_for_graph(&prime_id, graph_id, graph_registry)?;
        } else {
        }
        Ok(())
    }
    
    /// Authorize an agent to access a graph
    /// 
    /// This also updates the graph's authorized_agents list in GraphRegistry
    /// for bidirectional tracking. Call this with the prime agent ID when
    /// creating new graphs to ensure the prime agent has access by default.
    pub fn authorize_agent_for_graph(
        &mut self,
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut crate::storage::GraphRegistry,
    ) -> Result<()> {
        // Get the agent
        let agent = self.agents.get_mut(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?;
        
        // Add graph to agent's authorized list if not already there
        if !agent.authorized_graphs.contains(graph_id) {
            agent.authorized_graphs.push(*graph_id);
        }
        
        // Update graph's authorized_agents list for bidirectional tracking
        if let Some(graph) = graph_registry.graphs.get_mut(graph_id) {
            if !graph.authorized_agents.contains(agent_id) {
                graph.authorized_agents.push(*agent_id);
            }
        }
        
        Ok(())
    }
    
    /// Remove agent authorization from a graph
    pub fn deauthorize_agent_from_graph(
        &mut self,
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut crate::storage::GraphRegistry,
    ) -> Result<()> {
        // Update agent's authorized list
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.authorized_graphs.retain(|id| id != graph_id);
        }
        
        // Update graph's authorized_agents list
        if let Some(graph) = graph_registry.graphs.get_mut(graph_id) {
            graph.authorized_agents.retain(|id| id != agent_id);
        }
        
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
        graph_registry: &crate::storage::GraphRegistry,
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
    pub fn remove_agent(&mut self, agent_id: &Uuid) -> Result<()> {
        // Get the agent info
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Archive the agent data if it exists
        if agent_info.data_path.exists() {
            if let Some(data_dir) = &self.data_dir {
                // Create archive directory if it doesn't exist
                let archive_dir = data_dir.join("archived_agents");
                fs::create_dir_all(&archive_dir)?;
                
                // Move to archive with timestamp
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                let archive_path = archive_dir.join(format!("{}_{}", agent_id, timestamp));
                
                fs::rename(&agent_info.data_path, &archive_path)?;
                
                info!("Archived agent: {} ({}) to {:?}", 
                      agent_info.name, agent_id, archive_path);
            }
        }
        
        // Remove from registry
        self.agents.remove(agent_id);
        
        // Also remove from active agents if it was active
        if self.active_agents.remove(agent_id) {
        }
        
        Ok(())
    }
    
    /// Deactivate agent with persistence workflow
    /// 
    /// This provides the registry-side of deactivation that AppState can call
    /// after it has saved the Agent instance itself.
    pub fn deactivate_agent_complete(&mut self, agent_id: &Uuid) -> Result<()> {
        // Deactivate the agent
        self.deactivate_agent(agent_id)?;
        
        // Save the registry
        self.save()?;
        
        Ok(())
    }
    
    /// Complete agent activation workflow with validation
    /// 
    /// Enhanced version that includes persistence and better error handling.
    pub fn activate_agent_complete(&mut self, agent_id: &Uuid) -> Result<AgentInfo> {
        // Activate the agent (validates existence)
        let agent_info = self.activate_agent(agent_id)?;
        
        // Save the registry
        self.save()?;
        
        Ok(agent_info)
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_new_registry() {
        let registry = AgentRegistry::new();
        assert!(registry.agents.is_empty());
        assert!(registry.active_agents.is_empty());
    }

    #[test]
    fn test_register_new_agent() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let mut registry = AgentRegistry::new();
        registry.data_dir = Some(data_dir.to_path_buf());
        
        let info = registry.register_agent(
            None,
            Some("TestAgent".to_string()),
            Some("A test agent".to_string()),
        ).unwrap();

        assert_eq!(info.name, "TestAgent");
        assert_eq!(info.data_path, data_dir.join("agents").join(info.id.to_string()));
        assert_eq!(info.description, Some("A test agent".to_string()));
        assert!(registry.is_agent_active(&info.id));
    }

    #[test]
    fn test_ensure_default_agent() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let mut registry = AgentRegistry::new();
        registry.data_dir = Some(data_dir.to_path_buf());
        
        // First call should create prime agent
        let agent1 = registry.ensure_default_agent().unwrap();
        assert_eq!(agent1.name, "Prime Agent");
        assert!(agent1.is_prime);
        assert!(registry.is_agent_active(&agent1.id));
        
        // Second call should return same agent
        let agent2 = registry.ensure_default_agent().unwrap();
        assert_eq!(agent1.id, agent2.id);
    }

    #[test]
    fn test_activate_deactivate() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut registry = AgentRegistry::new();
        registry.data_dir = Some(data_dir.to_path_buf());
        
        let agent = registry.register_agent(
            None,
            Some("TestAgent".to_string()),
            None,
        ).unwrap();
        
        // Should start active
        assert!(registry.is_agent_active(&agent.id));
        
        // Deactivate
        registry.deactivate_agent(&agent.id).unwrap();
        assert!(!registry.is_agent_active(&agent.id));
        
        // Reactivate
        registry.activate_agent(&agent.id).unwrap();
        assert!(registry.is_agent_active(&agent.id));
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("agent_registry.json");
        let data_dir = dir.path();

        // Create registry and register an agent
        let mut registry = AgentRegistry::load_or_create(&registry_path, data_dir).unwrap();
        
        let agent_id = Uuid::new_v4();
        registry.register_agent(
            Some(agent_id),
            Some("PersistentAgent".to_string()),
            None,
        ).unwrap();
        
        // Save registry
        registry.save().unwrap();
        
        // Load registry from disk
        let loaded_registry = AgentRegistry::load_or_create(&registry_path, data_dir).unwrap();
        
        // Verify agent was persisted
        let loaded_agent = loaded_registry.get_agent(&agent_id).unwrap();
        assert_eq!(loaded_agent.name, "PersistentAgent");
        assert!(loaded_registry.is_agent_active(&agent_id));
    }

    #[test]
    fn test_agent_authorization_basics() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut agent_registry = AgentRegistry::new();
        agent_registry.data_dir = Some(data_dir.to_path_buf());
        
        // Create a graph registry for bidirectional updates
        let mut graph_registry = crate::storage::GraphRegistry::new();
        
        // Create two agents
        let agent1 = agent_registry.register_agent(
            None,
            Some("Agent1".to_string()),
            None,
        ).unwrap();
        
        let agent2 = agent_registry.register_agent(
            None,
            Some("Agent2".to_string()),
            None,
        ).unwrap();
        
        // Create a graph ID
        let graph_id = Uuid::new_v4();
        
        // Initially, neither agent should be authorized
        assert!(!agent_registry.is_agent_authorized(&agent1.id, &graph_id),
            "Agent1 should not be authorized initially");
        assert!(!agent_registry.is_agent_authorized(&agent2.id, &graph_id),
            "Agent2 should not be authorized initially");
        
        // Authorize agent1 for the graph
        agent_registry.authorize_agent_for_graph(&agent1.id, &graph_id, &mut graph_registry).unwrap();
        
        // Verify agent1 is now authorized but agent2 is not
        assert!(agent_registry.is_agent_authorized(&agent1.id, &graph_id),
            "Agent1 should be authorized after authorization");
        assert!(!agent_registry.is_agent_authorized(&agent2.id, &graph_id),
            "Agent2 should still not be authorized");
    }

    #[test]
    fn test_authorization_revocation() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut agent_registry = AgentRegistry::new();
        agent_registry.data_dir = Some(data_dir.to_path_buf());
        
        // Create a graph registry for bidirectional updates
        let mut graph_registry = crate::storage::GraphRegistry::new();
        
        // Create an agent
        let agent = agent_registry.register_agent(
            None,
            Some("TestAgent".to_string()),
            None,
        ).unwrap();
        
        let graph_id = Uuid::new_v4();
        
        // Authorize the agent
        agent_registry.authorize_agent_for_graph(&agent.id, &graph_id, &mut graph_registry).unwrap();
        assert!(agent_registry.is_agent_authorized(&agent.id, &graph_id),
            "Agent should be authorized");
        
        // Revoke authorization
        agent_registry.deauthorize_agent_from_graph(&agent.id, &graph_id, &mut graph_registry).unwrap();
        assert!(!agent_registry.is_agent_authorized(&agent.id, &graph_id),
            "Agent should not be authorized after revocation");
        
        // Verify agent still exists
        assert!(agent_registry.get_agent(&agent.id).is_some(),
            "Agent should still exist after deauthorization");
    }

    #[test]
    fn test_multiple_graph_authorization() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut agent_registry = AgentRegistry::new();
        agent_registry.data_dir = Some(data_dir.to_path_buf());
        
        // Create a graph registry for bidirectional updates
        let mut graph_registry = crate::storage::GraphRegistry::new();
        
        // Create one agent
        let agent = agent_registry.register_agent(
            None,
            Some("MultiGraphAgent".to_string()),
            None,
        ).unwrap();
        
        // Create two graphs
        let graph1 = Uuid::new_v4();
        let graph2 = Uuid::new_v4();
        
        // Authorize for first graph only
        agent_registry.authorize_agent_for_graph(&agent.id, &graph1, &mut graph_registry).unwrap();
        
        // Verify authorization state
        assert!(agent_registry.is_agent_authorized(&agent.id, &graph1),
            "Agent should be authorized for graph1");
        assert!(!agent_registry.is_agent_authorized(&agent.id, &graph2),
            "Agent should not be authorized for graph2");
        
        // Add authorization for second graph
        agent_registry.authorize_agent_for_graph(&agent.id, &graph2, &mut graph_registry).unwrap();
        
        // Verify agent is now authorized for both
        assert!(agent_registry.is_agent_authorized(&agent.id, &graph1),
            "Agent should still be authorized for graph1");
        assert!(agent_registry.is_agent_authorized(&agent.id, &graph2),
            "Agent should now be authorized for graph2");
        
        // Verify the agent's authorized_graphs list
        let agent_info = agent_registry.get_agent(&agent.id).unwrap();
        assert_eq!(agent_info.authorized_graphs.len(), 2,
            "Agent should have 2 authorized graphs");
        assert!(agent_info.authorized_graphs.contains(&graph1));
        assert!(agent_info.authorized_graphs.contains(&graph2));
    }

    #[test]
    fn test_prime_agent_authorization() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let mut agent_registry = AgentRegistry::new();
        agent_registry.data_dir = Some(data_dir.to_path_buf());
        
        // Create a graph registry for bidirectional updates
        let mut graph_registry = crate::storage::GraphRegistry::new();
        
        // Create prime agent
        let prime_agent = agent_registry.ensure_default_agent().unwrap();
        assert!(prime_agent.is_prime);
        
        // Create regular agent
        let regular_agent = agent_registry.register_agent(
            None,
            Some("RegularAgent".to_string()),
            None,
        ).unwrap();
        assert!(!regular_agent.is_prime);
        
        // Create a graph
        let graph_id = Uuid::new_v4();
        
        // Neither should be authorized initially (even prime agent)
        assert!(!agent_registry.is_agent_authorized(&prime_agent.id, &graph_id),
            "Prime agent should not be auto-authorized");
        assert!(!agent_registry.is_agent_authorized(&regular_agent.id, &graph_id),
            "Regular agent should not be authorized");
        
        // Authorize prime agent
        agent_registry.authorize_agent_for_graph(&prime_agent.id, &graph_id, &mut graph_registry).unwrap();
        
        // Verify only prime agent is authorized
        assert!(agent_registry.is_agent_authorized(&prime_agent.id, &graph_id),
            "Prime agent should be authorized after explicit authorization");
        assert!(!agent_registry.is_agent_authorized(&regular_agent.id, &graph_id),
            "Regular agent should still not be authorized");
    }

    #[test]
    fn test_authorization_for_nonexistent_agent() {
        let temp_dir = tempdir().unwrap();
        let _data_dir = temp_dir.path();
        let registry = AgentRegistry::new();
        
        let fake_agent_id = Uuid::new_v4();
        let graph_id = Uuid::new_v4();
        
        // Should return false for non-existent agent
        assert!(!registry.is_agent_authorized(&fake_agent_id, &graph_id),
            "Non-existent agent should not be authorized");
    }

    #[test]
    fn test_authorization_persistence() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("agent_registry.json");
        let data_dir = dir.path();

        let agent_id = Uuid::new_v4();
        let graph_id = Uuid::new_v4();
        
        // Create registry and authorize agent
        {
            let mut registry = AgentRegistry::load_or_create(&registry_path, data_dir).unwrap();
            
            registry.register_agent(
                Some(agent_id),
                Some("AuthorizedAgent".to_string()),
                None,
            ).unwrap();
            
            let mut graph_registry = crate::storage::GraphRegistry::new();
            registry.authorize_agent_for_graph(&agent_id, &graph_id, &mut graph_registry).unwrap();
            registry.save().unwrap();
        }
        
        // Load registry from disk and verify authorization persisted
        {
            let loaded_registry = AgentRegistry::load_or_create(&registry_path, data_dir).unwrap();
            
            assert!(loaded_registry.is_agent_authorized(&agent_id, &graph_id),
                "Authorization should persist across save/load");
            
            let agent = loaded_registry.get_agent(&agent_id).unwrap();
            assert!(agent.authorized_graphs.contains(&graph_id),
                "Authorized graphs list should persist");
        }
    }
}