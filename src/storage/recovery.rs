//! WAL-Based Recovery System
//! 
//! This module implements Cymbiont's complete recovery infrastructure, reconstructing
//! all system state from the Write-Ahead Log (WAL). It serves as the foundation for
//! crash recovery, lazy entity loading, and system bootstrapping.
//!
//! ## Core Responsibilities
//!
//! ### System Reconstruction (`rebuild_from_wal`)
//! Complete system state rebuild from committed transactions on startup:
//! - Replays entire WAL to reconstruct registries and entities
//! - Identifies which graphs and agents need reconstruction
//! - Filters operations for specific entities being rebuilt
//! - Unloads entities that shouldn't remain in memory
//!
//! ### Crash Recovery (`recover_pending_transactions`)
//! Handles incomplete transactions after unexpected shutdown:
//! - Identifies pending transactions with Active state
//! - Temporarily opens closed graphs for recovery
//! - Temporarily activates inactive agents for recovery
//! - Commits successfully recovered transactions
//! - Restores original open/active states after recovery
//!
//! ### Lazy Entity Loading
//! On-demand reconstruction when entities are accessed:
//! - `rebuild_graph_from_wal`: Reconstructs specific graph from WAL
//! - `rebuild_agent_from_wal`: Reconstructs specific agent from WAL
//! - Filters for entity-specific operations
//! - Handles pending transactions for the entity
//!
//! ### JSON Export (`export_all_json`)
//! Debug snapshots for inspection and testing:
//! - Exports registries to JSON files
//! - Temporarily opens closed graphs for export
//! - Preserves memory efficiency by closing after export
//! - Creates complete system snapshot for debugging
//!
//! ## Architectural Design
//!
//! ### RecoveryContext
//! Encapsulates all resources needed for recovery operations:
//! - Reference to AppState for accessing registries and managers
//! - `is_rebuilding` flag to distinguish rebuild from normal recovery
//! - Executes operations with skip_wal to prevent recursive logging
//!
//! ### Operation Execution
//! Three-tier operation handling during recovery:
//! 1. **Graph Operations**: Direct GraphManager mutations (blocks, pages)
//! 2. **Agent Operations**: Conversation and configuration updates
//! 3. **Registry Operations**: Entity lifecycle and authorization
//!
//! Each operation category has specialized execution logic that:
//! - Ensures required entities are loaded before execution
//! - Calls original methods with skip_wal=true
//! - Handles deserialization of stored parameters
//!
//! ## Critical Design Constraint: Self-Referential Operations
//! 
//! Some operations create paradoxes by modifying their own execution prerequisites.
//! These "meta-operations" must be filtered during entity rebuild:
//!
//! ### Graph Meta-Operations
//! - `OpenGraph`: Would be redundant (graph is already open for rebuild)
//! - `CloseGraph`: Would close the graph we're actively rebuilding
//!
//! ### Agent Meta-Operations  
//! - `ActivateAgent`: Would be redundant (agent is already active)
//! - `DeactivateAgent`: Would deactivate the agent we're rebuilding
//!
//! ### Why This Matters
//! During `rebuild_graph_from_wal`, we open a graph to reconstruct it. If we
//! replay a CloseGraph operation from the WAL, it would close the very graph
//! we're trying to rebuild, creating an infinite loop of open-rebuild-close.
//!
//! This is a fundamental constraint: the WAL cannot contain operations that
//! prevent its own replay. When adding new operations, carefully consider
//! whether they affect replay infrastructure or just business data.
//!
//! ## Recovery Strategies
//!
//! ### Full Rebuild (Startup)
//! 1. Load registries to identify entities
//! 2. Mark entities as needing reconstruction
//! 3. Replay all committed transactions
//! 4. Filter operations for relevant entities
//! 5. Unload entities that shouldn't be in memory
//!
//! ### Incremental Recovery (Crash)
//! 1. Scan for pending transactions
//! 2. Temporarily load required entities
//! 3. Execute pending operations
//! 4. Commit successful recoveries
//! 5. Restore original entity states
//!
//! ### Lazy Loading (On-Demand)
//! 1. Entity requested but not in memory
//! 2. Create empty entity instance
//! 3. Replay entity-specific operations from WAL
//! 4. Skip meta-operations that would affect loaded state
//! 5. Process any pending transactions for entity
//!
//! ## Error Handling Philosophy
//!
//! Recovery operations follow a "best effort" approach:
//! - Individual operation failures are logged but don't halt recovery
//! - Recovery continues with remaining operations
//! - Failed operations remain in pending state for manual resolution
//! - System achieves best possible state given available operations
//!
//! This ensures maximum system availability even with partial data corruption
//! or operation incompatibilities after version changes.

use crate::{
    agent::{agent::Agent, agent_registry::AgentRegistry, llm::{Message, LLMConfig}},
    graph::{graph_manager::GraphManager, graph_operations::GraphOps},
    graph::graph_registry::GraphRegistry,
    storage::{
        TransactionCoordinator,
        wal::{
            Operation, GraphOperation, AgentOperation, 
            RegistryOperation, GraphRegistryOp, AgentRegistryOp,
            TransactionState,
        },
    },
    error::*,
    lock::{AsyncRwLockExt, lock_registries_for_write},
};
use crate::AppState;
use std::collections::HashSet;
use std::sync::Arc;
use std::fs;
use tokio::sync::RwLock;
use tracing::error;
use uuid::Uuid;

/// Context providing all resources needed for recovery
/// 
/// Simplified context - just needs AppState reference
pub struct RecoveryContext {
    pub app_state: Arc<crate::AppState>,
    /// When true, create empty entities instead of loading from JSON
    pub is_rebuilding: bool,
}

impl RecoveryContext {
    /// Execute any operation during recovery
    pub async fn execute_operation(&self, operation: Operation) -> Result<()> {
        match operation {
            Operation::Graph(graph_op) => {
                self.execute_graph_operation(graph_op).await
            }
            Operation::Agent(agent_op) => {
                self.execute_agent_operation(agent_op).await
            }
            Operation::Registry(registry_op) => {
                self.execute_registry_operation(registry_op).await
            }
        }
    }
    
    /// Execute a graph operation during recovery
    async fn execute_graph_operation(&self, op: GraphOperation) -> Result<()> {
        // Extract fields from the operation and call the original GraphOps methods
        match op {
            GraphOperation::CreateBlock { graph_id, agent_id, content, parent_id, page_name, properties } => {
                // Ensure graph manager exists
                self.ensure_graph_manager(&graph_id).await?;
                
                // Call the original add_block with skip_wal: true
                let _block_id = self.app_state.add_block(
                    agent_id,
                    content,
                    parent_id,
                    page_name,
                    properties,
                    &graph_id,
                    true  // skip_wal during recovery
                ).await?;
                
                // Successfully recovered block
            }
            GraphOperation::UpdateBlock { graph_id, agent_id, block_id, content } => {
                // Ensure graph manager exists
                self.ensure_graph_manager(&graph_id).await?;
                
                // Call the original update_block with skip_wal: true
                self.app_state.update_block(
                    agent_id,
                    block_id.clone(),
                    content,
                    &graph_id,
                    true  // skip_wal during recovery
                ).await?;
                
                // Successfully recovered block update
            }
            GraphOperation::DeleteBlock { graph_id, agent_id, block_id } => {
                // Ensure graph manager exists
                self.ensure_graph_manager(&graph_id).await?;
                
                // Call the original delete_block with skip_wal: true
                self.app_state.delete_block(
                    agent_id,
                    block_id.clone(),
                    &graph_id,
                    true  // skip_wal during recovery
                ).await?;
                
                // Successfully recovered block deletion
            }
            GraphOperation::CreatePage { graph_id, agent_id, page_name, properties } => {
                // Ensure graph manager exists
                self.ensure_graph_manager(&graph_id).await?;
                
                // Call the original create_page with skip_wal: true
                self.app_state.create_page(
                    agent_id,
                    page_name.clone(),
                    properties,
                    &graph_id,
                    true  // skip_wal during recovery
                ).await?;
                
                // Successfully recovered page
            }
            GraphOperation::DeletePage { graph_id, agent_id, page_name } => {
                // Ensure graph manager exists
                self.ensure_graph_manager(&graph_id).await?;
                
                // Call the original delete_page with skip_wal: true
                self.app_state.delete_page(
                    agent_id,
                    page_name.clone(),
                    &graph_id,
                    true  // skip_wal during recovery
                ).await?;
                
                // Successfully recovered page deletion
            }
        }
        
        Ok(())
    }
    
    /// Execute an agent operation during recovery
    async fn execute_agent_operation(&self, op: AgentOperation) -> Result<()> {
        // Extract agent_id from the operation
        let agent_id = match &op {
            AgentOperation::AddMessage { agent_id, .. } |
            AgentOperation::ClearHistory { agent_id } |
            AgentOperation::SetLLMConfig { agent_id, .. } |
            AgentOperation::SetSystemPrompt { agent_id, .. } |
            AgentOperation::SetDefaultGraph { agent_id, .. } => *agent_id,
        };
        
        // Ensure agent is loaded
        self.ensure_agent_loaded(&agent_id).await?;
        
        // Get the agent directly from AppState
        let agents = self.app_state.agents.read().await;
        let agent_arc = agents.get(&agent_id)
            .ok_or_else(|| CymbiontError::Other(format!("Agent not found: {}", agent_id)))?
            .clone();
        drop(agents); // Release the HashMap lock
        
        let mut agent = agent_arc.write_or_panic("execute agent op - write agent").await;
        
        match op {
            AgentOperation::AddMessage { message, .. } => {
                // Deserialize the message and add to conversation
                let msg: Message = serde_json::from_value(message)
                    .map_err(|e| CymbiontError::Other(format!("Failed to deserialize message: {}", e)))?;
                agent.add_message(msg, true).await?;  // skip_wal during recovery
                // Successfully recovered message
            }
            AgentOperation::ClearHistory { .. } => {
                agent.clear_history(true).await?;  // skip_wal during recovery
                // Successfully recovered history clear
            }
            AgentOperation::SetLLMConfig { config, .. } => {
                let llm_config: LLMConfig = serde_json::from_value(config)
                    .map_err(|e| CymbiontError::Other(format!("Failed to deserialize LLM config: {}", e)))?;
                agent.set_llm_config(llm_config, true).await?;  // skip_wal during recovery
                // Successfully recovered LLM config
            }
            AgentOperation::SetSystemPrompt { prompt, .. } => {
                agent.set_system_prompt(prompt, true).await?;  // skip_wal during recovery
                // Successfully recovered system prompt
            }
            AgentOperation::SetDefaultGraph { graph_id, .. } => {
                agent.set_default_graph_id(graph_id, true).await?;  // skip_wal during recovery
                // Successfully recovered default graph
            }
        }
        
        Ok(())
    }
    
    /// Execute a registry operation during recovery
    async fn execute_registry_operation(&self, op: RegistryOperation) -> Result<()> {
        match op {
            RegistryOperation::Graph(graph_op) => {
                self.execute_graph_registry_operation(graph_op).await
            }
            RegistryOperation::Agent(agent_op) => {
                self.execute_agent_registry_operation(agent_op).await
            }
        }
    }
    
    /// Execute a graph registry operation during recovery
    async fn execute_graph_registry_operation(&self, op: GraphRegistryOp) -> Result<()> {
        match op {
            GraphRegistryOp::RegisterGraph { graph_id, name, description } => {
                let mut registry = self.app_state.graph_registry.write_or_panic("register graph").await;
                let graph_dir = self.app_state.data_dir.join("graphs").join(graph_id.to_string());
                registry.register_graph(Some(graph_id), name, description, &graph_dir, true).await?;  // skip_wal
                // Successfully recovered graph registration
            }
            GraphRegistryOp::RemoveGraph { graph_id } => {
                let mut registry = self.app_state.graph_registry.write_or_panic("remove graph").await;
                registry.remove_graph(&graph_id, true).await?;  // skip_wal
                // Successfully recovered graph removal
            }
            GraphRegistryOp::OpenGraph { graph_id } => {
                let mut registry = self.app_state.graph_registry.write_or_panic("open graph").await;
                registry.open_graph(&graph_id, true).await?;  // skip_wal
                // Successfully recovered graph open
            }
            GraphRegistryOp::CloseGraph { graph_id } => {
                let mut registry = self.app_state.graph_registry.write_or_panic("close graph").await;
                registry.close_graph(&graph_id, true).await?;  // skip_wal
                // Successfully recovered graph close
            }
        }
        
        Ok(())
    }
    
    /// Execute an agent registry operation during recovery
    async fn execute_agent_registry_operation(&self, op: AgentRegistryOp) -> Result<()> {
        match op {
            AgentRegistryOp::RegisterAgent { agent_id, name, description } => {
                let mut registry = self.app_state.agent_registry.write_or_panic("register agent").await;
                registry.register_agent(Some(agent_id), name, description, true).await?;  // skip_wal
                // Successfully recovered agent registration
            }
            AgentRegistryOp::RemoveAgent { agent_id } => {
                let mut registry = self.app_state.agent_registry.write_or_panic("remove agent").await;
                registry.remove_agent(&agent_id, true).await?;  // skip_wal
                // Successfully recovered agent removal
            }
            AgentRegistryOp::ActivateAgent { agent_id } => {
                let mut registry = self.app_state.agent_registry.write_or_panic("activate agent").await;
                registry.activate_agent(&agent_id, true).await?;  // skip_wal
                // Successfully recovered agent activation
            }
            AgentRegistryOp::DeactivateAgent { agent_id } => {
                let mut registry = self.app_state.agent_registry.write_or_panic("deactivate agent").await;
                registry.deactivate_agent(&agent_id, true).await?;  // skip_wal
                // Successfully recovered agent deactivation
            }
            AgentRegistryOp::AuthorizeAgent { agent_id, graph_id } => {
                let (mut graph_reg, mut agent_reg) = lock_registries_for_write(
                    &self.app_state.graph_registry,
                    &self.app_state.agent_registry
                ).await?;
                
                agent_reg.authorize_agent_for_graph(&agent_id, &graph_id, &mut graph_reg, true).await?;
            }
            AgentRegistryOp::DeauthorizeAgent { agent_id, graph_id } => {
                let (mut graph_reg, mut agent_reg) = lock_registries_for_write(
                    &self.app_state.graph_registry,
                    &self.app_state.agent_registry
                ).await?;
                
                agent_reg.deauthorize_agent_from_graph(&agent_id, &graph_id, &mut graph_reg, true).await?;
                // Successfully recovered agent deauthorization
            }
            AgentRegistryOp::SetPrimeAgent { agent_id } => {
                let mut registry = self.app_state.agent_registry.write_or_panic("set prime agent").await;
                registry.set_prime_agent(&agent_id, true).await?;  // skip_wal during recovery
                // Successfully recovered prime agent designation
            }
        }
        
        Ok(())
    }
    
    /// Ensure a graph manager exists for the given graph
    async fn ensure_graph_manager(&self, graph_id: &Uuid) -> Result<()> {
        // Get managers directly from AppState
        let managers_read = self.app_state.graph_managers.read().await;
        if managers_read.contains_key(graph_id) {
            return Ok(());
        }
        drop(managers_read);
        
        // Need to create the manager
        let mut managers_write = self.app_state.graph_managers.write().await;
        
        // Double-check pattern
        if managers_write.contains_key(graph_id) {
            return Ok(());
        }
        
        // Create new GraphManager
        let data_dir = self.app_state.data_dir.join("graphs").join(graph_id.to_string());
        fs::create_dir_all(&data_dir)?;
        
        let graph_manager = GraphManager::new(data_dir)?;
        managers_write.insert(*graph_id, RwLock::new(graph_manager));
        
        Ok(())
    }
    
    /// Ensure an agent is loaded into memory
    async fn ensure_agent_loaded(&self, agent_id: &Uuid) -> Result<()> {
        // Get agents directly from AppState
        let agents_read = self.app_state.agents.read().await;
        if agents_read.contains_key(agent_id) {
            return Ok(());
        }
        drop(agents_read);
        
        // Need to load the agent
        let mut agents_write = self.app_state.agents.write().await;
        
        // Double-check pattern
        if agents_write.contains_key(agent_id) {
            return Ok(());
        }
        
        // Get agent info from registry
        let agent_info = {
            let registry = self.app_state.agent_registry.read_or_panic("ensure agent - registry").await;
            registry.get_agent(agent_id)
                .ok_or_else(|| CymbiontError::Other(format!("Agent {} not found in registry", agent_id)))?
                .clone()
        };
        
        // Create or load agent based on rebuild mode
        let agent = if self.is_rebuilding {
            // Create empty agent for WAL rebuild with proper transaction coordinator
            Agent::new_empty(
                *agent_id, 
                agent_info.name.clone(),
                self.app_state.transaction_coordinator.clone()
            )
        } else {
            // Load agent from disk (legacy path for JSON loading)
            let mut agent = Agent::load(&agent_info.data_path, self.app_state.transaction_coordinator.clone())?;
            
            // Set default graph if needed
            if agent.get_default_graph_id().is_none() && !agent_info.authorized_graphs.is_empty() {
                agent.set_default_graph_id(Some(agent_info.authorized_graphs[0]), true).await?;  // skip_wal during recovery
            }
            
            agent
        };
        
        agents_write.insert(*agent_id, Arc::new(RwLock::new(agent)));
        
        Ok(())
    }
}


/// Rebuild entire system state from WAL
/// 
/// This is the main entry point for full WAL replay. It rebuilds all state
/// from committed transactions, ensuring the system is in a consistent state.
pub async fn rebuild_from_wal(
    coordinator: &TransactionCoordinator,
    context: &RecoveryContext,
) -> Result<usize> {
    // Get all committed transactions
    let transactions = coordinator.log.list_committed_transactions()?;
    let total_count = transactions.len();
    
    if total_count == 0 {
return Ok(0);
    }
    
    // Found committed transactions to replay
    use tracing::info;
    info!("Replaying {} committed transactions from WAL", total_count);
    
    
    // Identify which entities need to be rebuilt
    let (graphs_to_rebuild, agents_to_rebuild) = identify_entities_to_rebuild(
        coordinator,
        context
    ).await?;
    
    // Identified entities to rebuild
    
    // Replay all transactions in order
    let mut _replayed = 0;
    for transaction in transactions {
        // Check if this operation affects an entity we're rebuilding
        let should_replay = should_replay_operation(
            &transaction.operation,
            &graphs_to_rebuild,
            &agents_to_rebuild
        );
        
        if should_replay {
            // Execute the operation
            if let Err(e) = context.execute_operation(transaction.operation).await {
                error!(
                    "Failed to replay transaction {} during rebuild: {}",
                    transaction.id, e
                );
                // Continue with other transactions even if one fails
            } else {
                _replayed += 1;
            }
        } else {
        }
    }
    
    // Successfully rebuilt state from WAL
    
    // Now unload any graphs/agents that shouldn't be in memory
    unload_inactive_entities(context).await?;
    
    Ok(_replayed)
}

/// Identify which entities need to be rebuilt based on their state
async fn identify_entities_to_rebuild(
    coordinator: &TransactionCoordinator,
    context: &RecoveryContext,
) -> Result<(HashSet<Uuid>, HashSet<Uuid>)> {
    let mut graphs_to_rebuild = HashSet::new();
    let mut agents_to_rebuild = HashSet::new();
    
    // Get currently open graphs from registry
    {
        let registry = context.app_state.graph_registry.read_or_panic("identify graphs to rebuild").await;
        for graph_id in registry.get_open_graphs() {
            graphs_to_rebuild.insert(graph_id);
        }
    }
    
    // Get currently active agents from registry
    {
        let registry = context.app_state.agent_registry.read_or_panic("identify agents to rebuild").await;
        for agent_id in registry.get_active_agents() {
            agents_to_rebuild.insert(agent_id);
        }
    }
    
    // Also check for entities with pending transactions
    let pending_transactions = coordinator.log.list_pending_transactions()?;
    for transaction in pending_transactions {
        if let Some(graph_id) = transaction.operation.extract_graph_id() {
            graphs_to_rebuild.insert(graph_id);
        }
        if let Some(agent_id) = transaction.operation.extract_agent_id() {
            agents_to_rebuild.insert(agent_id);
        }
    }
    
    Ok((graphs_to_rebuild, agents_to_rebuild))
}

/// Check if an operation should be replayed based on entities we're rebuilding
fn should_replay_operation(
    operation: &Operation,
    graphs_to_rebuild: &HashSet<Uuid>,
    agents_to_rebuild: &HashSet<Uuid>,
) -> bool {
    // Registry operations always need to be replayed
    if matches!(operation, Operation::Registry(_)) {
        return true;
    }
    
    // Check if operation affects a graph we're rebuilding
    if let Some(graph_id) = operation.extract_graph_id() {
        if graphs_to_rebuild.contains(&graph_id) {
            return true;
        }
    }
    
    // Check if operation affects an agent we're rebuilding
    if let Some(agent_id) = operation.extract_agent_id() {
        if agents_to_rebuild.contains(&agent_id) {
            return true;
        }
    }
    
    false
}

/// Recover pending and deferred transactions for a specific entity
async fn recover_entity_transactions(
    entity_id: &Uuid,
    entity_type: &str, // "graph" or "agent"  
    coordinator: &TransactionCoordinator,
    context: &RecoveryContext,
) -> Result<usize> {
    // Get all pending transactions (includes deferred ones marked with deferred_reason)
    let pending = coordinator.log.list_pending_transactions()?;
    
    
    let mut recovered = 0;
    for transaction in pending {
        // Check if this transaction is for our entity
        let is_for_entity = match entity_type {
            "graph" => transaction.operation.extract_graph_id() == Some(*entity_id),
            "agent" => transaction.operation.extract_agent_id() == Some(*entity_id),
            _ => false,
        };
        
        if is_for_entity {
            // Only process Active transactions (includes deferred with reasons)
            match transaction.state {
                TransactionState::Active => {
                    // Attempt recovery
                    if let Err(e) = context.execute_operation(transaction.operation).await {
                        error!("Failed to recover {} transaction {}: {}", entity_type, transaction.id, e);
                    } else {
                        recovered += 1;
                        coordinator.log.update_transaction_state(
                            &transaction.id,
                            TransactionState::Committed
                        )?;
                    }
                }
                _ => {
                    // Skip already committed or aborted transactions
                }
            }
        }
    }
    
    Ok(recovered)
}

/// Unload graphs and agents that shouldn't be in memory
async fn unload_inactive_entities(context: &RecoveryContext) -> Result<()> {
    // Get list of graphs that should be closed
    let graphs_to_unload = {
        let registry = context.app_state.graph_registry.read_or_panic("unload graphs - read registry").await;
        let open_graphs = registry.get_open_graphs();
        
        let managers = context.app_state.graph_managers.read().await;
        let mut to_unload = Vec::new();
        
        for graph_id in managers.keys() {
            if !open_graphs.contains(graph_id) {
                to_unload.push(*graph_id);
            }
        }
        
        to_unload
    };
    
    // Unload closed graphs
    if !graphs_to_unload.is_empty() {
        let mut managers = context.app_state.graph_managers.write().await;
        for graph_id in graphs_to_unload {
            managers.remove(&graph_id);
            // Unloaded closed graph
        }
    }
    
    // Get list of agents that should be inactive
    let agents_to_unload = {
        let registry = context.app_state.agent_registry.read_or_panic("unload agents - read registry").await;
        let active_agents = registry.get_active_agents();
        
        let agents = context.app_state.agents.read().await;
        let mut to_unload = Vec::new();
        
        for agent_id in agents.keys() {
            if !active_agents.contains(agent_id) {
                to_unload.push(*agent_id);
            }
        }
        
        to_unload
    };
    
    // Unload inactive agents
    if !agents_to_unload.is_empty() {
        let mut agents = context.app_state.agents.write().await;
        for agent_id in agents_to_unload {
            agents.remove(&agent_id);
            // Unloaded inactive agent
        }
    }
    
    Ok(())
}

/// Rebuild a specific graph from WAL when lazily loading
/// 
/// This is called when opening a closed graph to reconstruct its state
/// from the transaction log without loading JSON.
///
/// TODO: When we implement undo operations, revisit RemoveGraph/RemoveAgent handling.
/// Currently we don't filter them since they shouldn't occur during normal rebuild
/// (a removed entity wouldn't be getting rebuilt). However, with undo operations,
/// we might need to check if Remove operations are terminal (last operation for entity)
/// and only skip non-terminal ones. The transaction replay system should ideally
/// support full delete/restore cycles with proper tombstoning.
pub async fn rebuild_graph_from_wal(
    graph_id: &Uuid,
    coordinator: &TransactionCoordinator,
    context: &RecoveryContext,
) -> Result<()> {
    // Get all committed transactions
    let all_transactions = coordinator.log.list_committed_transactions()?;
    
    // Filter for operations affecting this graph
    for transaction in all_transactions {
        let should_replay = match &transaction.operation {
            // Registry operations for this graph
            Operation::Registry(RegistryOperation::Graph(op)) => {
                match op {
                    // CRITICAL: Skip OpenGraph/CloseGraph operations during rebuild
                    // These are "meta-operations" that would modify the execution context
                    // we're currently using. Replaying CloseGraph would close the graph
                    // we just opened for rebuilding, creating a self-referential paradox.
                    GraphRegistryOp::OpenGraph { .. } |
                    GraphRegistryOp::CloseGraph { .. } => false,
                    
                    // Replay RegisterGraph and other operations for this graph
                    GraphRegistryOp::RegisterGraph { graph_id: id, .. } => id == graph_id,
                    _ => false,
                }
            }
            // Graph operations
            Operation::Graph(_) => {
                transaction.operation.extract_graph_id() == Some(*graph_id)
            }
            _ => false,
        };
        
        if should_replay {
            if let Err(e) = context.execute_operation(transaction.operation).await {
                error!("Failed to replay transaction during graph rebuild: {}", e);
            }
        }
    }
    
    // Rebuilt graph from WAL
    
    // Now recover any pending/deferred transactions for this graph
    let recovered = recover_entity_transactions(
        graph_id,
        "graph",
        coordinator,
        context
    ).await?;
    
    if recovered > 0 {
        use tracing::info;
        info!("Recovered {} pending/deferred transactions for graph {}", recovered, graph_id);
    }
    
    Ok(())
}

/// Rebuild entire system state from WAL
/// This was previously AppState::rebuild_from_wal
pub async fn rebuild_from_wal_complete(app_state: &Arc<AppState>) -> Result<usize> {
    // Create recovery context with rebuild flag set
    let context = RecoveryContext {
        app_state: app_state.clone(),
        is_rebuilding: true,
    };
    
    // Run the rebuild
    let count = rebuild_from_wal(
        &app_state.transaction_coordinator,
        &context
    ).await?;
    
if count > 0 {
        // Successfully rebuilt state from transactions
    }
    
    Ok(count)
}

/// Recover pending and deferred transactions (crash recovery)
/// This was previously AppState::recover_pending_transactions
pub async fn recover_pending_transactions(app_state: &Arc<AppState>) -> Result<usize> {
    use tracing::info;
    
    // Create recovery context
    let context = RecoveryContext {
        app_state: app_state.clone(),
        is_rebuilding: false,
    };
    
    // Get all pending transactions (includes ones with deferred_reason)
    let pending = app_state.transaction_coordinator.log.list_pending_transactions()?;
    
    if pending.is_empty() {
        return Ok(0);
    }
    
    info!("Recovering {} pending transactions", pending.len());
    
    // Identify which entities need to be temporarily opened for recovery
    let mut graphs_to_open = HashSet::new();
    let mut agents_to_activate = HashSet::new();
    
    for transaction in &pending {
        if let Some(graph_id) = transaction.operation.extract_graph_id() {
            graphs_to_open.insert(graph_id);
        }
        if let Some(agent_id) = transaction.operation.extract_agent_id() {
            agents_to_activate.insert(agent_id);
        }
    }
    
    // Temporarily open closed graphs for recovery
    let mut temporarily_opened_graphs = Vec::new();
    for graph_id in graphs_to_open {
        let is_open = {
            let registry = app_state.graph_registry.read_or_panic("check if graph open").await;
            registry.is_graph_open(&graph_id)
        };
        
        if !is_open {
            // Open the graph temporarily (skip WAL since we'll close it again)
            if let Err(e) = GraphRegistry::open_graph_complete(
                app_state.graph_registry.clone(),
                graph_id,
                true  // skip_wal for temporary operation
            ).await {
                error!("Failed to open graph {} for recovery: {}", graph_id, e);
            } else {
                temporarily_opened_graphs.push(graph_id);
            }
        }
    }
    
    // Temporarily activate inactive agents for recovery
    let mut temporarily_activated_agents = Vec::new();
    for agent_id in agents_to_activate {
        let is_active = {
            let registry = app_state.agent_registry.read_or_panic("check if agent active").await;
            registry.get_active_agents().contains(&agent_id)
        };
        
        if !is_active {
            // Activate the agent temporarily (skip WAL since we'll deactivate again)
            if let Err(e) = AgentRegistry::activate_agent_complete(
                app_state.agent_registry.clone(),
                agent_id,
                true  // skip_wal for temporary operation
            ).await {
                error!("Failed to activate agent {} for recovery: {}", agent_id, e);
            } else {
                temporarily_activated_agents.push(agent_id);
            }
        }
    }
    
    // Now recover all transactions
    let mut recovered = 0;
    for transaction in pending {
        // Handle Active state (includes deferred with reasons)
        match transaction.state {
            TransactionState::Active => {
                // Execute the recovery
                if let Err(e) = context.execute_operation(transaction.operation).await {
                    error!("Failed to recover transaction {}: {}", transaction.id, e);
                } else {
                    recovered += 1;
                    // Mark transaction as committed
                    app_state.transaction_coordinator.log.update_transaction_state(
                        &transaction.id,
                        TransactionState::Committed
                    )?;
                }
            }
            _ => {
                // This shouldn't happen - indicates a bug in transaction log
                error!("Found {:?} transaction in recoverable list: {} - this indicates a bug", 
                       transaction.state, transaction.id);
            }
        }
    }
    
    // Close temporarily opened graphs
    for graph_id in temporarily_opened_graphs {
        let mut registry = app_state.graph_registry.write_or_panic("close temp graph").await;
        if let Err(e) = registry.close_graph(&graph_id, true).await {
            error!("Failed to close temporarily opened graph {}: {}", graph_id, e);
        }
    }
    
    // Deactivate temporarily activated agents
    for agent_id in temporarily_activated_agents {
        let mut registry = app_state.agent_registry.write_or_panic("deactivate temp agent").await;
        if let Err(e) = registry.deactivate_agent(&agent_id, true).await {
            error!("Failed to deactivate temporarily activated agent {}: {}", agent_id, e);
        }
    }
    
    if recovered > 0 {
        info!("Successfully recovered {} pending/deferred transactions", recovered);
    }
    
    Ok(recovered)
}

/// Export all data to JSON for debugging/inspection
/// This was previously AppState::export_all_json
pub async fn export_all_json(app_state: &AppState) -> Result<()> {
    // Export registries
    {
        let graph_registry = app_state.graph_registry.read_or_panic("export graph registry").await;
        let path = app_state.data_dir.join("graph_registry.json");
        graph_registry.export_json(&path)?;
    }
    
    {
        let agent_registry = app_state.agent_registry.read_or_panic("export agent registry").await;
        let path = app_state.data_dir.join("agent_registry.json");
        agent_registry.export_json(&path)?;
    }
    
    // Export all graphs (both open and closed)
    // TODO: This is a temporary workaround. Ideally we should export directly from WAL
    // without needing to load graphs into memory. For now, we temporarily open closed
    // graphs to export their data, then close them again to avoid memory bloat.
    {
        // Get all registered graphs from the registry
        let all_graphs = {
            let registry = app_state.graph_registry.read_or_panic("export - get all graphs").await;
            registry.get_all_graphs()
        };
        
        for graph_info in all_graphs {
            let graph_id = graph_info.id;
            
            // Check if this graph already has a manager (is open)
            let was_already_open = {
                let managers = app_state.graph_managers.read().await;
                managers.contains_key(&graph_id)
            };
            
            // If not open, temporarily open it to load data from WAL
            if !was_already_open {
                if let Err(e) = GraphRegistry::open_graph_complete(
                    app_state.graph_registry.clone(),
                    graph_id,
                    true  // skip_wal for temporary export operation
                ).await {
                    error!("Failed to open graph {} for export: {}", graph_id, e);
                    continue;
                }
            }
            
            // Now export the graph (it definitely has a manager now)
            {
                let managers = app_state.graph_managers.read().await;
                if let Some(manager_lock) = managers.get(&graph_id) {
                    let manager = manager_lock.read().await;
                    let graph_dir = app_state.data_dir.join("graphs").join(graph_id.to_string());
                    fs::create_dir_all(&graph_dir)?;
                    let path = graph_dir.join("knowledge_graph.json");
                    manager.export_json(&path)?;
                }
            }
            
            // If we opened it just for export, close it again to free memory
            if !was_already_open {
                let mut registry = app_state.graph_registry.write_or_panic("close graph after export").await;
                if let Err(e) = registry.close_graph(&graph_id, true).await {
                    error!("Failed to close graph {} after export: {}", graph_id, e);
                }
            }
        }
    }
    
    // Export all agents directly
    {
        let agents = app_state.agents.read().await;
        for (agent_id, agent_arc) in agents.iter() {
            let agent = agent_arc.read().await;
            let agent_dir = app_state.data_dir.join("agents").join(agent_id.to_string());
            fs::create_dir_all(&agent_dir)?;
            let path = agent_dir.join("agent.json");
            agent.export_json(&path)?;
        }
    }
    Ok(())
}

/// Rebuild a specific agent from WAL when lazily loading
/// 
/// This is called when activating an inactive agent to reconstruct its state
/// from the transaction log without loading JSON.
pub async fn rebuild_agent_from_wal(
    agent_id: &Uuid,
    coordinator: &TransactionCoordinator,
    context: &RecoveryContext,
) -> Result<()> {
    // Rebuilding agent from WAL
    
    // Get all committed transactions
    let all_transactions = coordinator.log.list_committed_transactions()?;
    
    // Filter for operations affecting this agent
    let mut _replayed = 0;
    for transaction in all_transactions {
        let should_replay = match &transaction.operation {
            // Registry operations for this agent
            Operation::Registry(RegistryOperation::Agent(op)) => {
                match op {
                    // CRITICAL: Skip ActivateAgent/DeactivateAgent operations during rebuild
                    // Same self-referential issue as OpenGraph/CloseGraph - these operations
                    // would modify the agent's loaded state that we're currently using for replay.
                    AgentRegistryOp::ActivateAgent { .. } |
                    AgentRegistryOp::DeactivateAgent { .. } => false,
                    
                    // Replay other operations for this agent
                    AgentRegistryOp::RegisterAgent { agent_id: id, .. } |
                    AgentRegistryOp::AuthorizeAgent { agent_id: id, .. } |
                    AgentRegistryOp::DeauthorizeAgent { agent_id: id, .. } => id == agent_id,
                    _ => false,
                }
            }
            // Agent operations
            Operation::Agent(_) => {
                transaction.operation.extract_agent_id() == Some(*agent_id)
            }
            _ => false,
        };
        
        if should_replay {
            if let Err(e) = context.execute_operation(transaction.operation).await {
                error!("Failed to replay transaction during agent rebuild: {}", e);
            } else {
                _replayed += 1;
            }
        }
    }
    
    // Rebuilt agent from WAL
    
    // Now recover any pending/deferred transactions for this agent
    let recovered = recover_entity_transactions(
        agent_id,
        "agent",
        coordinator,
        context
    ).await?;
    
    if recovered > 0 {
        use tracing::info;
        info!("Recovered {} pending/deferred transactions for agent {}", recovered, agent_id);
    }
    
    Ok(())
}