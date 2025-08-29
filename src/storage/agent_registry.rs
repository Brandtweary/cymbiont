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
use tokio::sync::RwLock;
use std::sync::Arc;

// Import shared UUID serialization utilities
use crate::storage::registry_utils::{uuid_hashmap_serde, uuid_hashset_serde, uuid_vec_serde};
use crate::storage::{TransactionCoordinator, Operation};
use crate::storage::transaction_log::{AgentRegistryOp, RegistryOperation};
use crate::agent::agent::Agent;
use crate::error::*;
use crate::lock::AsyncRwLockExt;
use crate::AppState;
use std::sync::Weak;
use crate::Result;



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
    
    /// Transaction coordinator for WAL operations (not serialized)
    #[serde(skip)]
    transaction_coordinator: Option<Arc<TransactionCoordinator>>,
    
    /// Reference to AppState for accessing agents and other resources
    /// Uses Weak to avoid reference cycles
    #[serde(skip)]
    app_state: Option<Weak<AppState>>,
}


impl AgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        AgentRegistry {
            agents: HashMap::new(),
            active_agents: HashSet::new(),
            prime_agent_id: None,
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
    
    /// Get a reference to the agents map from AppState
    pub fn get_agents(&self) -> Option<Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>> {
        self.app_state.as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|app_state| app_state.agents.clone())
    }
    
    /// Activate an agent (complete workflow with loading and WAL rebuild)
    /// 
    /// This method orchestrates the full activation workflow:
    /// 1. Mark agent as active in registry
    /// 2. Load or create the Agent instance
    /// 3. Rebuild from WAL if needed
    /// 
    /// Uses Arc<RwLock<Self>> to minimize lock holding time.
    pub async fn activate_agent_complete(
        registry: Arc<RwLock<AgentRegistry>>, 
        agent_id: Uuid,
        skip_wal: bool
    ) -> Result<()> {
        // Step 1: Update registry to mark active (brief lock)
        let agent_info = {
            let mut reg = registry.write_or_panic("activate agent - registry update").await;
            reg.activate_agent(&agent_id, skip_wal).await?
        };
        // Registry lock released
        
        // Step 2: Get app_state for further operations
        let app_state = {
            let reg = registry.read_or_panic("activate agent - get app_state").await;
            reg.app_state.as_ref()
                .and_then(|weak| weak.upgrade())
                .ok_or_else(|| StorageError::agent_registry("No AppState reference"))?
        };
        
        // Step 3: Load or create the Agent instance if not in memory
        let needs_rebuild = {
            let agents = app_state.agents.read().await;
            
            if !agents.contains_key(&agent_id) {
                drop(agents); // Release read lock
                
                // Create empty agent for WAL rebuild
                let agent = Agent::new_empty(
                    agent_id,
                    agent_info.name.clone(),
                    app_state.transaction_coordinator.clone()
                );
                
                // Insert into HashMap
                let mut agents_write = app_state.agents.write().await;
                agents_write.insert(agent_id, Arc::new(RwLock::new(agent)));
                
                // New agents need rebuilding from WAL
                true
            } else {
                // Check if existing agent needs rebuilding
                if let Some(agent_arc) = agents.get(&agent_id) {
                    let agent = agent_arc.read_or_panic("check agent history").await;
                    agent.conversation_history.is_empty()
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
            
            crate::storage::recovery::rebuild_agent_from_wal(
                &agent_id,
                &app_state.transaction_coordinator,
                &context
            ).await?;
        }
        
        Ok(())
    }
    
    
    /// Authorize an agent for a graph (complete workflow)
    /// 
    /// Uses Arc<RwLock<Self>> to properly acquire both registries in the correct order.
    pub async fn authorize_agent_for_graph_complete(
        agent_registry: Arc<RwLock<AgentRegistry>>,
        agent_id: Uuid,
        graph_id: Uuid,
    ) -> Result<()> {
        // Get app_state
        let app_state = {
            let reg = agent_registry.read_or_panic("authorize - get app_state").await;
            reg.app_state.as_ref()
                .and_then(|weak| weak.upgrade())
                .ok_or_else(|| StorageError::agent_registry("No AppState reference"))?
        };
        
        // Acquire both registries in correct order
        {
            use crate::lock::lock_registries_for_write;
            let (mut graph_registry, mut agent_registry) = lock_registries_for_write(
                &app_state.graph_registry,
                &app_state.agent_registry
            ).await?;
            
            agent_registry.authorize_agent_for_graph(&agent_id, &graph_id, &mut graph_registry, false).await?;
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
    /// Uses Arc<RwLock<Self>> to minimize lock holding time.
    pub async fn create_agent_complete(
        registry: Arc<RwLock<AgentRegistry>>,
        name: Option<String>,
        description: Option<String>,
        system_prompt: Option<String>,
    ) -> Result<AgentInfo> {
        // Step 1: Register the agent (brief lock)
        let agent_info = {
            let mut reg = registry.write_or_panic("create agent - registry").await;
            reg.register_agent(
                None,  // Let it generate a new UUID
                name.clone(),
                description.clone(),
                false,  // Do log to WAL
            ).await?
        };
        // Registry lock released
        
        // Step 2: Get app_state for further operations
        let app_state = {
            let reg = registry.read_or_panic("create agent - get app_state").await;
            reg.app_state.as_ref()
                .and_then(|weak| weak.upgrade())
                .ok_or_else(|| StorageError::agent_registry("No AppState reference"))?
        };
        
        // Step 3: Create the actual Agent instance (no registry lock)
        {
            use crate::agent::agent::Agent;
            use crate::agent::llm::LLMConfig;
            
            // Ensure agent directory exists
            std::fs::create_dir_all(&agent_info.data_path)?;
            
            // Create agent with default MockLLM config
            let agent = Agent::new(
                agent_info.id,
                agent_info.name.clone(),
                LLMConfig::default(),  // MockLLM by default
                system_prompt.or(Some("You are a helpful assistant".to_string())),
                app_state.transaction_coordinator.clone(),
            );
            
            // Insert into active agents HashMap
            let mut agents_map = app_state.agents.write_or_panic("create agent - insert").await;
            agents_map.insert(agent_info.id, Arc::new(RwLock::new(agent)));
        }
        
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
        id: Option<Uuid>,
        name: Option<String>,
        description: Option<String>,
        skip_wal: bool,
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
        
        // Create WAL operation for new agent conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::RegisterAgent {
                agent_id,
                name: Some(final_name.clone()),
                description: description.clone(),
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let data_dir = self.data_dir.clone()
            .ok_or_else(|| StorageError::agent_registry("No data directory set"))?;
        let agents = &mut self.agents;
        let active_agents = &mut self.active_agents;
        
        let tx = coordinator.begin(operation).await?;
        
        // Create new agent metadata
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

        agents.insert(agent_id, agent_info.clone());
        
        // New agents start as active
        active_agents.insert(agent_id);
        info!("✅ Created agent: {} ({})", final_name, agent_id);
        
        tx.commit().await?;
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
    pub async fn activate_agent(&mut self, agent_id: &Uuid, skip_wal: bool) -> Result<AgentInfo> {
        // Validate agent exists
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::ActivateAgent {
                agent_id: *agent_id,
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let active_agents = &mut self.active_agents;
        let agents = &mut self.agents;
        
        let tx = coordinator.begin(operation).await?;
        
        // Add to active set
        if active_agents.insert(*agent_id) {
        }
        
        // Update last active time
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.last_active = Utc::now();
        }
        
        tx.commit().await?;
        Ok(agent_info)
    }

    /// Deactivate an agent (unload from memory and mark as inactive)
    pub async fn deactivate_agent(&mut self, agent_id: &Uuid, skip_wal: bool) -> Result<()> {
        // Validate agent is active
        if !self.active_agents.contains(agent_id) {
            return Err(StorageError::agent_registry(format!("Agent '{}' was not active", agent_id)).into());
        }
        
        // Remove agent instance from memory
        if let Some(app_state) = self.app_state.as_ref().and_then(|w| w.upgrade()) {
            let mut agents_map = app_state.agents.write().await;
            agents_map.remove(agent_id);
        }
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::DeactivateAgent {
                agent_id: *agent_id,
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let active_agents = &mut self.active_agents;
        
        let tx = coordinator.begin(operation).await?;
        
        if !active_agents.remove(agent_id) {
            // Agent was not active, but we still commit the transaction
            // since the desired state is achieved
        }
        
        tx.commit().await
    }

    /// Set an agent as the prime agent
    /// 
    /// The prime agent is the default agent with special privileges.
    /// This operation is logged to WAL for persistence across restarts.
    pub async fn set_prime_agent(&mut self, agent_id: &Uuid, skip_wal: bool) -> Result<()> {
        // Verify agent exists
        if !self.agents.contains_key(agent_id) {
            return Err(StorageError::not_found("agent", "id", agent_id.to_string()).into());
        }
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(
                AgentRegistryOp::SetPrimeAgent {
                    agent_id: *agent_id,
                }
            )))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        let agents = &mut self.agents;
        let prime_agent_id = &mut self.prime_agent_id;
        
        let tx = coordinator.begin(operation).await?;
        
        // Update agent's prime status
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.is_prime = true;
        }
        
        // Set registry's prime agent ID
        *prime_agent_id = Some(*agent_id);
        
        tx.commit().await
    }
    
    /// Ensure at least one agent exists (for first-run experience)
    /// 
    /// If no agents exist, creates the prime agent which gets auto-authorized for all graphs.
    /// If agents exist but none are active, activates the first one.
    /// 
    /// Uses Arc<RwLock<Self>> to properly create agents without holding locks.
    pub async fn ensure_default_agent(
        registry: Arc<RwLock<AgentRegistry>>,
    ) -> Result<AgentInfo> {
        // Check if we need to create prime agent
        let needs_creation = {
            let reg = registry.read_or_panic("check for prime agent").await;
            reg.prime_agent_id.is_none()  // Check for prime agent, not just any agent
        };
        
        if needs_creation {
            // Create the prime agent using the complete workflow
            let agent_info = Self::create_agent_complete(
                registry.clone(),
                Some("Prime Agent".to_string()),
                Some("Primary assistant with full graph access".to_string()),
                Some("You are the prime agent, a helpful assistant with full access to knowledge graphs.".to_string()),
            ).await?;
            
            // Mark as prime agent using proper transaction
            {
                let mut reg = registry.write_or_panic("set prime agent").await;
                reg.set_prime_agent(&agent_info.id, false).await?;
            }
            
            info!("👑 Created prime agent: {} ({})", agent_info.name, agent_info.id);
            Ok(agent_info)
        } else {
            // Check if we need to activate an agent
            let (needs_activation, first_id) = {
                let reg = registry.read_or_panic("check active agents").await;
                if reg.active_agents.is_empty() {
                    // Need to activate first agent
                    let first_id = *reg.agents.keys().next().unwrap();
                    (true, Some(first_id))
                } else {
                    // Return existing active agent
                    let active_id = *reg.active_agents.iter().next().unwrap();
                    return Ok(reg.agents[&active_id].clone());
                }
            };
            
            if needs_activation {
                let first_id = first_id.unwrap();
                let mut reg = registry.write_or_panic("activate first agent").await;
                reg.activate_agent(&first_id, false).await
            } else {
                unreachable!()
            }
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
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut crate::storage::GraphRegistry,
        skip_wal: bool,
    ) -> Result<()> {
        // Validate agent exists
        if !self.agents.contains_key(agent_id) {
            return Err(StorageError::not_found("agent", "ID", agent_id.to_string()).into());
        }
        
        // Track if this is the first graph (check before modifying)
        // No longer used here since default graph setting moved to Phase 4
        let _is_first_graph = self.agents.get(agent_id)
            .map(|a| a.authorized_graphs.is_empty())
            .unwrap_or(false);
        
        // Create WAL operation for authorization conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::AuthorizeAgent {
                agent_id: *agent_id,
                graph_id: *graph_id,
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // First transaction: Authorize the agent
        {
            // Extract fields needed by the closure
            let agents = &mut self.agents;
            
            let tx = coordinator.begin(operation).await?;
            
            // Get the agent
            let agent = agents.get_mut(agent_id)
                .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?;
            
            // Add graph to agent's authorized list if not already there
            if !agent.authorized_graphs.contains(graph_id) {
                agent.authorized_graphs.push(*graph_id);
            }
            
            // Update graph's authorized_agents list for bidirectional tracking
            graph_registry.add_authorized_agent(graph_id, agent_id)?;
            
            tx.commit().await?;
        }
        
        
        // NOTE: Default graph setting moved to Phase 4 of message queue to avoid deadlock
        // The first authorized graph becomes the default, but this is handled elsewhere
        // to prevent same-task reentrancy issues with tokio RwLocks.
        
        Ok(())
    }
    
    /// Remove agent authorization from a graph
    pub async fn deauthorize_agent_from_graph(
        &mut self,
        agent_id: &Uuid,
        graph_id: &Uuid,
        graph_registry: &mut crate::storage::GraphRegistry,
        skip_wal: bool,
    ) -> Result<()> {
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::DeauthorizeAgent {
                agent_id: *agent_id,
                graph_id: *graph_id,
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let agents = &mut self.agents;
        
        let tx = coordinator.begin(operation).await?;
        
        // Update agent's authorized list
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.authorized_graphs.retain(|id| id != graph_id);
        }
        
        // Update graph's authorized_agents list
        graph_registry.remove_authorized_agent(graph_id, agent_id);
        
        tx.commit().await
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
    pub async fn remove_agent(&mut self, agent_id: &Uuid, skip_wal: bool) -> Result<()> {
        // Get the agent info
        let agent_info = self.agents.get(agent_id)
            .ok_or_else(|| StorageError::not_found("agent", "ID", agent_id.to_string()))?
            .clone();
        
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Registry(RegistryOperation::Agent(AgentRegistryOp::RemoveAgent {
                agent_id: *agent_id,
            })))
        };
        
        let coordinator = self.transaction_coordinator.as_ref()
            .ok_or_else(|| StorageError::agent_registry("No transaction coordinator set"))?
            .clone();
        
        // Extract fields needed by the closure
        let data_dir = self.data_dir.clone();
        let agents = &mut self.agents;
        let active_agents = &mut self.active_agents;
        
        let tx = coordinator.begin(operation).await?;
        
        // Archive the agent data if it exists
        if agent_info.data_path.exists() {
            if let Some(data_dir) = &data_dir {
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
        agents.remove(agent_id);
        
        // Also remove from active agents if it was active
        if active_agents.remove(agent_id) {
        }
        
        tx.commit().await
    }
    
    /// Export the registry to JSON for debugging/inspection
    /// 
    /// Note: This is NOT for persistence - WAL is the source of truth
    /// The test harness (tests/common/agent_validation.rs) reads this file for validation
    pub fn export_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| StorageError::agent_registry(format!("Failed to serialize agent registry: {}", e)))?;
        
        fs::write(path, json)
            .map_err(|e| StorageError::agent_registry(format!("Failed to write agent registry JSON: {}", e)))?;
        
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
        let registry = AgentRegistry::new();
        assert!(registry.agents.is_empty());
        assert!(registry.active_agents.is_empty());
    }
}
    