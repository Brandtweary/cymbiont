//! Command Processor - The Beating Heart of CQRS
//!
//! The CommandProcessor is the single-threaded owner of all mutable state in Cymbiont.
//! It runs as a background task, processing commands sequentially to guarantee
//! deadlock-free operation while enabling unlimited concurrent reads. This is the
//! architectural keystone that makes multi-agent operations safe and predictable.
//!
//! ## Core Responsibilities
//!
//! ### State Ownership
//! The processor directly owns all mutable resources:
//! - Graph managers (knowledge graphs)
//! - Active agents (AI assistants)
//! - Registries (metadata)
//!
//! External code receives Arc<RwLock<>> references for read-only access, but
//! only the processor can modify state through command execution.
//!
//! ### Command Processing Pipeline
//! ```text
//! 1. Receive command from queue
//! 2. Resolve non-deterministic values
//! 3. Write to WAL (persistence)
//! 4. Execute via router
//! 5. Send response via oneshot
//! ```
//!
//! ### Recovery and Startup
//! On startup, the processor:
//! 1. Reads the WAL to rebuild registry state
//! 2. Ensures the prime agent exists
//! 3. Begins processing new commands
//!
//! Entities (graphs/agents) are lazy-loaded on first access to avoid loading
//! everything into memory at once.
//!
//! ## Design Patterns
//!
//! ### Lazy Entity Loading
//! When a command targets an entity that isn't loaded:
//! ```rust
//! // Processor checks if graph is loaded
//! if !self.graph_managers.contains_key(&graph_id) {
//!     // Replay filtered WAL to rebuild graph state
//!     self.ensure_graph_loaded(&graph_id)?;
//! }
//! // Now proceed with command execution
//! ```
//!
//! ### RouterToken Authorization
//! The processor creates RouterTokens that prove commands came through CQRS:
//! ```rust
//! let token = router::create_router_token();
//! router::apply_graph_command(token, command, &mut self.graph_managers)?;
//! ```
//!
//! This makes it impossible for domain logic to bypass the command queue.
//!
//! ### Graceful Shutdown
//! The processor tracks active commands and provides graceful shutdown:
//! ```rust
//! // Initiate shutdown - stops accepting new commands
//! processor.initiate_shutdown();
//! 
//! // Wait up to 30 seconds for active commands
//! let completed = processor.wait_for_completion(Duration::from_secs(30));
//! ```
//!
//! ## Performance Characteristics
//!
//! ### Throughput
//! Single-threaded processing might seem limiting, but consider:
//! - Most graph operations complete in microseconds
//! - Network I/O (WebSocket, HTTP) dominates response time
//! - Reads (the majority) bypass the processor entirely
//! - Deadlock prevention is worth the trade-off
//!
//! ### Memory Management
//! - Lazy loading keeps memory usage bounded
//! - Entities can be unloaded when not in use
//! - WAL filtering prevents loading irrelevant commands
//!
//! ### Scalability Path
//! Future optimizations (not yet needed):
//! - Partition commands by entity ID for parallel processing
//! - Read-through caching for frequently accessed data
//! - Command batching for related operations
//!
//! ## Error Handling
//!
//! Commands can fail at several stages:
//! 1. **Authorization**: Agent lacks permission
//! 2. **Validation**: Invalid parameters
//! 3. **Execution**: Business logic failure
//! 4. **System**: WAL write failure, out of memory
//!
//! All errors are captured and returned via the response channel.
//!
//! ## Test Infrastructure
//!
//! ### Operation Freeze
//! For testing crash recovery, operations can be frozen after WAL write:
//! ```rust
//! // Freeze after writing to WAL
//! processor.set_freeze_state(Arc::new(RwLock::new(true)));
//! 
//! // Command will be written but not executed
//! // Simulates crash during processing
//! ```
//!
//! ## Implementation Notes
//!
//! ### Why Not Actor Model?
//! We considered actors but chose CQRS because:
//! - Simpler mental model (one processor vs many actors)
//! - Easier debugging (sequential execution)
//! - Natural fit for WAL (total ordering of commands)
//! - No actor supervision complexity
//!
//! ### Why Not Event Sourcing?
//! CQRS without full event sourcing because:
//! - Commands are coarser than events (better performance)
//! - Simpler replay logic (commands are self-contained)
//! - Easier to understand (commands map to user actions)
//!
//! The current WAL approach will be simplified to JSON snapshots,
//! but the CQRS architecture will remain unchanged.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock, Notify};
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::error::*;
use crate::graph::graph_manager::GraphManager;
use crate::graph::graph_registry::GraphRegistry;
use crate::agent::agent::Agent;
use crate::agent::agent_registry::AgentRegistry;
use super::commands::{Command, CommandResult, GraphCommand, AgentCommand, RegistryCommand, 
                       GraphRegistryCommand, AgentRegistryCommand};
use super::wal::{CommandLog, CommandTransaction, CommandState};
use super::queue::CommandQueue;
use super::router;

/// Envelope for command submission with response channel
pub struct CommandEnvelope {
    pub command: Command,
    pub response: oneshot::Sender<Result<CommandResult>>,
}

/// References to resources owned by CommandProcessor for direct query access
pub struct ProcessorResources {
    pub graph_managers: Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    pub agents: Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
}

/// Error type for processor operations
#[derive(Debug)]
pub struct ProcessorError(pub String);

impl From<ProcessorError> for CymbiontError {
    fn from(e: ProcessorError) -> Self {
        CymbiontError::Other(e.0)
    }
}

/// Processor states for shutdown coordination
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessorState {
    Accepting = 0,  // Normal operation
    Draining = 1,   // Rejecting non-system commands
    Shutdown = 2,   // Rejecting everything
}

impl From<u8> for ProcessorState {
    fn from(value: u8) -> Self {
        match value {
            1 => ProcessorState::Draining,
            2 => ProcessorState::Shutdown,
            _ => ProcessorState::Accepting,
        }
    }
}

/// The command processor that owns all mutable state
pub struct CommandProcessor {
    // Owned mutable state (wrapped for sharing with AppState)
    graph_managers: Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
    agents: Arc<RwLock<HashMap<Uuid, Arc<RwLock<Agent>>>>>,
    
    // Registries for metadata (references from AppState)
    graph_registry: Arc<RwLock<GraphRegistry>>,
    agent_registry: Arc<RwLock<AgentRegistry>>,
    
    // WAL integration
    wal: Arc<CommandLog>,
    pending_operations: HashMap<String, String>, // content_hash -> tx_id
    active_transactions: HashSet<String>,
    
    // Recovery state
    is_recovering: bool,
    
    // Shutdown coordination
    state: Arc<AtomicU8>,
    active_count_notify: Arc<Notify>,
    
    // Test infrastructure
    operation_freeze: Option<Arc<RwLock<bool>>>,
    
    // Configuration
    data_dir: PathBuf,
}

impl CommandProcessor {
    /// Create a new command processor
    pub fn new(
        wal: Arc<CommandLog>,
        graph_registry: Arc<RwLock<GraphRegistry>>,
        agent_registry: Arc<RwLock<AgentRegistry>>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            graph_managers: Arc::new(RwLock::new(HashMap::new())),
            agents: Arc::new(RwLock::new(HashMap::new())),
            graph_registry,
            agent_registry,
            wal,
            pending_operations: HashMap::new(),
            active_transactions: HashSet::new(),
            is_recovering: false,
            state: Arc::new(AtomicU8::new(ProcessorState::Accepting as u8)),
            active_count_notify: Arc::new(Notify::new()),
            operation_freeze: None,
            data_dir,
        }
    }
    
    /// Set the freeze state for testing crash recovery
    pub fn set_freeze_state(&mut self, freeze: Arc<RwLock<bool>>) {
        self.operation_freeze = Some(freeze);
    }
    
    /// Start the command processor in a background task
    /// Returns a CommandQueue for submitting commands and ProcessorResources for queries
    /// 
    /// This method runs recovery synchronously BEFORE spawning the background task,
    /// ensuring that WAL replay completes before any business logic runs.
    pub async fn start(mut self) -> Result<(CommandQueue, ProcessorResources)> {
        info!("🚀 Command processor started");
        
        // Run recovery BEFORE spawning background task
        self.startup_recovery().await?;
        
        // Ensure prime agent exists after recovery
        self.ensure_prime_agent().await?;
        
        // Create channel for command submission
        let (sender, receiver) = mpsc::channel(100);
        
        // Extract references before moving self
        let resources = ProcessorResources {
            graph_managers: self.graph_managers.clone(),
            agents: self.agents.clone(),
        };
        
        // NOW spawn processor in background for command processing
        tokio::spawn(async move {
            self.run(receiver).await;
        });
        
        // Return queue with sender and resources
        Ok((CommandQueue::new_with_sender(sender), resources))
    }
    
    /// Run the command processing loop
    pub async fn run(mut self, mut receiver: mpsc::Receiver<CommandEnvelope>) {
        // Recovery and prime agent check already done in start()
        // This loop just processes commands
        
        while let Some(envelope) = receiver.recv().await {
            // Check processor state and handle accordingly
            let state = ProcessorState::from(self.state.load(Ordering::Acquire));
            match state {
                ProcessorState::Accepting => {
                    // Normal operation - process all commands
                }
                ProcessorState::Draining => {
                    // Only allow System commands through
                    if !matches!(envelope.command, Command::System(_)) {
                        warn!("Rejecting command during shutdown: {:?}", envelope.command);
                        let _ = envelope.response.send(Err(
                            ProcessorError("Graceful shutdown in progress, rejecting new commands".to_string()).into()
                        ));
                        continue;
                    }
                }
                ProcessorState::Shutdown => {
                    // Reject everything
                    warn!("Rejecting command - processor is shut down: {:?}", envelope.command);
                    let _ = envelope.response.send(Err(
                        ProcessorError("Processor is shut down".to_string()).into()
                    ));
                    continue;
                }
            }
            
            // Process the command
            let result = self.process_command(envelope.command).await;
            
            // Send response (ignore send errors if receiver dropped)
            let _ = envelope.response.send(result);
            
            // Notify shutdown mechanism if active transactions changed
            if self.active_transactions.is_empty() {
                self.active_count_notify.notify_one();
            }
        }
        
        info!("Command processor stopped");
    }
    
    /// Process a single command
    fn process_command(&mut self, mut command: Command) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CommandResult>> + Send + '_>> {
        Box::pin(async move {
            // Skip WAL during recovery (we're replaying from WAL)
            if self.is_recovering {
                return self.apply_command(command).await;
            }
        
        // Resolve non-deterministic values BEFORE writing to WAL
        command.resolve();
        
        // For mutations, begin a WAL transaction
        let tx_id = self.begin_transaction(&command).await?;
        
        // Check freeze state AFTER writing to WAL (for test crash recovery)
        if let Some(freeze) = &self.operation_freeze {
            let bypass_freeze = matches!(command, 
                Command::Registry(RegistryCommand::Graph(
                    GraphRegistryCommand::OpenGraph { .. } | 
                    GraphRegistryCommand::CloseGraph { .. }
                )) |
                Command::Registry(RegistryCommand::Agent(
                    AgentRegistryCommand::ActivateAgent { .. } | 
                    AgentRegistryCommand::DeactivateAgent { .. }
                ))
            );
            
            if !bypass_freeze {
                let is_frozen = *freeze.read().await;
                if is_frozen {
                    // Defer the command - it stays Active in WAL but won't execute
                    self.wal.update_command_deferred(&tx_id, "Operations frozen for testing")?;
                    
                    // Remove from active transactions since we're not actually executing
                    self.active_transactions.remove(&tx_id);
                    
                    // Return success with a flag indicating deferral
                    return Ok(CommandResult {
                        success: true,
                        data: Some(serde_json::json!({
                            "deferred": true,
                            "reason": "Operations frozen for testing"
                        })),
                        error: None,
                        child_commands: vec![],
                    });
                }
            }
        }
        
        // Apply the command
        let result = self.apply_command(command).await;
        
        // Complete the transaction based on result
        match &result {
            Ok(cmd_result) => {
                self.commit_transaction(&tx_id).await?;
                
                // Execute child commands if any (not during recovery)
                // Child commands get their own WAL entries, so they'll be replayed separately
                for child_cmd in &cmd_result.child_commands {
                    // Process child command (this will create its own WAL entry)
                    let child_result = Box::pin(self.process_command(child_cmd.clone())).await;
                    if let Err(e) = child_result {
                        error!("Child command failed: {}", e);
                        // Continue processing other child commands even if one fails
                    }
                }
            }
            Err(e) => {
                self.rollback_transaction(&tx_id).await?;
                error!("Command failed, transaction rolled back: {}", e);
            }
        }
        
        result
        })
    }
    
    
    
    /// Apply a mutation command
    async fn apply_command(&mut self, command: Command) -> Result<CommandResult> {
        match command {
            Command::Graph(graph_cmd) => {
                // Extract graph_id for loading
                let graph_id = match &graph_cmd {
                    GraphCommand::CreateBlock { graph_id, .. } |
                    GraphCommand::UpdateBlock { graph_id, .. } |
                    GraphCommand::DeleteBlock { graph_id, .. } |
                    GraphCommand::CreatePage { graph_id, .. } |
                    GraphCommand::DeletePage { graph_id, .. } => *graph_id,
                };
                
                // Ensure graph is loaded before routing
                self.ensure_graph_loaded(graph_id).await?;
                
                router::route_graph_command(
                    &self.graph_managers,
                    &self.agent_registry,
                    graph_cmd,
                ).await
            }
            Command::Agent(agent_cmd) => {
                // Extract agent_id for loading
                let agent_id = match &agent_cmd {
                    AgentCommand::AddMessage { agent_id, .. } |
                    AgentCommand::ClearHistory { agent_id } |
                    AgentCommand::SetLLMConfig { agent_id, .. } |
                    AgentCommand::SetSystemPrompt { agent_id, .. } |
                    AgentCommand::SetDefaultGraph { agent_id, .. } => *agent_id,
                };
                
                // Ensure agent is loaded before routing
                self.ensure_agent_loaded(agent_id).await?;
                
                router::route_agent_command(
                    &self.agents,
                    agent_cmd,
                ).await
            }
            Command::Registry(reg_cmd) => {
                router::route_registry_command(
                    &self.graph_managers,
                    &self.agents,
                    &self.graph_registry,
                    &self.agent_registry,
                    reg_cmd,
                    &self.data_dir,
                ).await
            }
            Command::System(sys_cmd) => {
                self.handle_system_command(sys_cmd).await
            }
        }
    }
    
    /// Handle system-level commands for lifecycle management
    async fn handle_system_command(&mut self, command: super::commands::SystemCommand) -> Result<CommandResult> {
        use super::commands::SystemCommand;
        
        match command {
            SystemCommand::InitiateShutdown => {
                // Set state to draining to reject new non-system commands
                self.initiate_shutdown();
                
                // Get count of active transactions  
                let active_count = self.active_transactions.len();
                
                Ok(CommandResult {
                    success: true,
                    data: Some(serde_json::json!({
                        "active_count": active_count,
                    })),
                    error: None,
                    child_commands: vec![],
                })
            }
            SystemCommand::WaitForCompletion { timeout_secs } => {
                // Wait for active transactions to complete
                let completed = self.wait_for_completion(Duration::from_secs(timeout_secs)).await;
                
                Ok(CommandResult {
                    success: true,
                    data: Some(serde_json::json!({
                        "completed": completed,
                    })),
                    error: None,
                    child_commands: vec![],
                })
            }
            SystemCommand::ForceFlush => {
                // Set state to fully shutdown
                self.state.store(ProcessorState::Shutdown as u8, Ordering::Release);
                
                // Force WAL flush by closing it
                self.wal.close().await?;
                info!("Force flush completed - WAL closed");
                
                Ok(CommandResult {
                    success: true,
                    data: None,
                    error: None,
                    child_commands: vec![],
                })
            }
        }
    }
    
    /// Ensure a graph is loaded in memory, loading from WAL if necessary
    async fn ensure_graph_loaded(&mut self, graph_id: Uuid) -> Result<()> {
        // Check if already loaded
        {
            let managers = self.graph_managers.read().await;
            if managers.contains_key(&graph_id) {
                return Ok(());
            }
        }
        
        // Not loaded, so create and insert
        let graph_path = self.data_dir.join("graphs").join(graph_id.to_string());
        let graph_manager = GraphManager::new(&graph_path)?;
        
        {
            let mut managers = self.graph_managers.write().await;
            // Double-check in case another task loaded it
            if !managers.contains_key(&graph_id) {
                managers.insert(graph_id, Arc::new(RwLock::new(graph_manager)));
            }
        }
        
        // Rebuild from WAL by replaying all commands for this graph
        self.rebuild_entity_from_wal(Some(graph_id), None).await?;
        
        Ok(())
    }
    
    /// Ensure an agent is loaded in memory, loading from WAL if necessary
    async fn ensure_agent_loaded(&mut self, agent_id: Uuid) -> Result<()> {
        // Check if already loaded
        {
            let agents = self.agents.read().await;
            if agents.contains_key(&agent_id) {
                return Ok(());
            }
        }
        
        // Not loaded, so create and insert
        let agent = Agent::new(
            agent_id, 
            "Agent".to_string(), 
            crate::agent::llm::LLMConfig::default(),
            None
        );
        
        {
            let mut agents = self.agents.write().await;
            // Double-check in case another task loaded it
            if !agents.contains_key(&agent_id) {
                agents.insert(agent_id, Arc::new(RwLock::new(agent)));
            }
        }
        
        // Rebuild from WAL by replaying all commands for this agent
        self.rebuild_entity_from_wal(None, Some(agent_id)).await?;
        
        Ok(())
    }
    
    /// Begin a WAL transaction
    async fn begin_transaction(&mut self, command: &Command) -> Result<String> {
        let transaction = CommandTransaction::new(command.clone());
        let tx_id = transaction.id.clone();
        
        // Check for content deduplication
        if let Some(content_hash) = &transaction.content_hash {
            if let Ok(is_pending) = self.wal.is_content_pending(content_hash) {
                if is_pending {
                    return Err(ProcessorError(format!(
                        "Content already pending with hash: {}", content_hash
                    )).into());
                }
            }
            self.pending_operations.insert(content_hash.clone(), tx_id.clone());
        }
        
        self.active_transactions.insert(tx_id.clone());
        self.wal.append_command(transaction)?;
        Ok(tx_id)
    }
    
    /// Commit a WAL transaction
    async fn commit_transaction(&mut self, tx_id: &str) -> Result<()> {
        // Remove from pending operations if it has a content hash
        if let Ok(transaction) = self.wal.get_command(tx_id) {
            if let Some(content_hash) = &transaction.content_hash {
                self.pending_operations.remove(content_hash);
            }
        }
        
        self.active_transactions.remove(tx_id);
        self.wal.update_command_state(tx_id, CommandState::Committed)?;
        
        // Notify shutdown mechanism
        if self.active_transactions.is_empty() {
            self.active_count_notify.notify_one();
        }
        
        Ok(())
    }
    
    /// Rollback a WAL transaction
    async fn rollback_transaction(&mut self, tx_id: &str) -> Result<()> {
        // Remove from pending operations if it has a content hash
        if let Ok(transaction) = self.wal.get_command(tx_id) {
            if let Some(content_hash) = &transaction.content_hash {
                self.pending_operations.remove(content_hash);
            }
        }
        
        self.active_transactions.remove(tx_id);
        self.wal.update_command_state(tx_id, CommandState::Aborted)?;
        
        // Notify shutdown mechanism
        if self.active_transactions.is_empty() {
            self.active_count_notify.notify_one();
        }
        
        Ok(())
    }
    
    /// Initiate graceful shutdown
    pub fn initiate_shutdown(&self) {
        info!("Initiating graceful shutdown of command processor");
        self.state.store(ProcessorState::Draining as u8, Ordering::Release);
        self.active_count_notify.notify_one();
    }
    
    /// Check if a command is self-referential (would affect its own replay)
    fn is_self_referential(&self, command: &Command) -> bool {
        matches!(command,
            Command::Registry(RegistryCommand::Graph(
                GraphRegistryCommand::OpenGraph { .. } |
                GraphRegistryCommand::CloseGraph { .. }
            )) |
            Command::Registry(RegistryCommand::Agent(
                AgentRegistryCommand::ActivateAgent { .. } |
                AgentRegistryCommand::DeactivateAgent { .. }
            ))
        )
    }
    
    /// Ensure prime agent exists after recovery
    async fn ensure_prime_agent(&mut self) -> Result<()> {
        // Check if prime agent already exists
        let needs_prime_agent = {
            let registry = self.agent_registry.read().await;
            registry.ensure_prime_agent()
        };
        
        if needs_prime_agent {
            info!("👑 Creating prime agent on first run");
            
            // Create the prime agent command
            let mut create_cmd = Command::Registry(RegistryCommand::Agent(
                AgentRegistryCommand::CreateAgent {
                    name: Some("Prime Agent".to_string()),
                    description: Some("Primary assistant with full graph access".to_string()),
                    resolved_id: None,
                }
            ));
            
            // Resolve the command to generate UUID
            create_cmd.resolve();
            
            // Extract the resolved agent ID
            let agent_id = if let Command::Registry(RegistryCommand::Agent(
                AgentRegistryCommand::CreateAgent { resolved_id: Some(id), .. }
            )) = &create_cmd {
                *id
            } else {
                return Err(ProcessorError("Failed to resolve prime agent ID".to_string()).into());
            };
            
            // Process the create command
            self.process_command(create_cmd).await?;
            
            // Set as prime agent
            let set_prime_cmd = Command::Registry(RegistryCommand::Agent(
                AgentRegistryCommand::SetPrimeAgent { agent_id }
            ));
            self.process_command(set_prime_cmd).await?;
            
            info!("👑 Prime agent created and designated: {}", agent_id);
        }
        
        Ok(())
    }
    
    /// Startup recovery - rebuild state from WAL and handle pending commands
    pub async fn startup_recovery(&mut self) -> Result<()> {
        info!("Starting WAL recovery process");
        
        // Mark as recovering to skip WAL writes
        self.is_recovering = true;
        
        // First rebuild state from committed commands
        let rebuilt = self.rebuild_from_wal().await?;
        info!("Rebuilt {} commands from WAL", rebuilt);
        
        // Restore runtime state (active agents, open graphs)
        self.restore_runtime_state().await?;
        
        // Now we can mark recovery as complete
        self.is_recovering = false;
        
        // Then handle any pending commands from crash
        let recovered = self.recover_pending_commands().await?;
        if recovered > 0 {
            info!("Recovered {} pending commands from previous session", recovered);
        }
        
        Ok(())
    }
    
    /// Rebuild system state from committed commands in WAL
    async fn rebuild_from_wal(&mut self) -> Result<usize> {
        // is_recovering is already set by startup_recovery
        
        // Get all committed commands
        let commands = self.wal.list_committed_commands()?;
        let total = commands.len();
        
        if total == 0 {
            return Ok(0);
        }
        
        info!("Replaying {} committed commands from WAL", total);
        
        let mut replayed = 0;
        for cmd_tx in commands {
            // Skip self-referential operations that would affect replay
            if self.is_self_referential(&cmd_tx.command) {
                continue;
            }
            
            // Apply the command (process_command will skip WAL since is_recovering=true)
            match self.process_command(cmd_tx.command).await {
                Ok(_) => {
                    replayed += 1;
                }
                Err(e) => {
                    error!("Failed to replay command {} during rebuild: {}", cmd_tx.id, e);
                    // Continue with other commands even if one fails
                }
            }
        }
        
        // Don't set is_recovering = false yet, restore_runtime_state needs it
        Ok(replayed)
    }
    
    /// Restore runtime state (active agents, open graphs) after WAL rebuild
    async fn restore_runtime_state(&mut self) -> Result<()> {
        info!("Restoring runtime state");
        
        // Restore active agents
        let agents_to_activate = {
            let registry = self.agent_registry.read().await;
            registry.get_active_agents()
        };
        
        for agent_id in agents_to_activate {
            info!("Reactivating agent {}", agent_id);
            let mut registry = self.agent_registry.write().await;
            let mut agents_map = self.agents.write().await;
            let token = super::router::RouterToken::new_internal();
            if let Err(e) = registry.activate_agent_complete(&token, agent_id, &mut *agents_map).await {
                error!("Failed to reactivate agent {}: {}", agent_id, e);
            }
        }
        
        // Restore open graphs
        let graphs_to_open = {
            let registry = self.graph_registry.read().await;
            registry.get_open_graphs()
        };
        
        for graph_id in graphs_to_open {
            info!("Reopening graph {}", graph_id);
            let mut registry = self.graph_registry.write().await;
            let mut managers = self.graph_managers.write().await;
            let token = super::router::RouterToken::new_internal();
            if let Err(e) = registry.open_graph_complete(
                &token,
                graph_id,
                &mut *managers,
                &self.data_dir,
            ).await {
                error!("Failed to reopen graph {}: {}", graph_id, e);
            }
        }
        
        Ok(())
    }
    
    /// Rebuild a specific entity from WAL (for lazy loading)
    async fn rebuild_entity_from_wal(&mut self, graph_id: Option<Uuid>, agent_id: Option<Uuid>) -> Result<()> {
        self.is_recovering = true;
        
        // First, replay all committed commands for this entity
        let commands = self.wal.list_committed_commands()?;
        
        for cmd_tx in commands {
            // Check if this command affects the entity we're rebuilding
            let should_replay = match (&graph_id, &agent_id) {
                (Some(gid), _) => {
                    // For graphs, we need to check both graph operations and registry operations
                    match &cmd_tx.command {
                        // Registry operations for this specific graph
                        Command::Registry(RegistryCommand::Graph(op)) => {
                            match op {
                                // Skip meta-operations that would affect the loaded state
                                GraphRegistryCommand::OpenGraph { .. } |
                                GraphRegistryCommand::CloseGraph { .. } => false,
                                // CreateGraph doesn't apply to existing graphs
                                GraphRegistryCommand::CreateGraph { .. } => false,
                                // Include RegisterGraph for this graph
                                GraphRegistryCommand::RegisterGraph { graph_id: id, .. } => id == gid,
                                GraphRegistryCommand::RemoveGraph { graph_id: id } => id == gid,
                            }
                        }
                        // Include all graph operations for this graph
                        Command::Graph(_) => cmd_tx.command.extract_graph_id() == Some(*gid),
                        // Include agent registry operations that affect this graph
                        Command::Registry(RegistryCommand::Agent(op)) => {
                            match op {
                                AgentRegistryCommand::AuthorizeAgent { graph_id: id, .. } |
                                AgentRegistryCommand::DeauthorizeAgent { graph_id: id, .. } => id == gid,
                                _ => false,
                            }
                        }
                        _ => false,
                    }
                }
                (_, Some(aid)) => {
                    // For agents, check agent operations and registry operations
                    match &cmd_tx.command {
                        // Registry operations for this specific agent
                        Command::Registry(RegistryCommand::Agent(op)) => {
                            match op {
                                // Skip meta-operations
                                AgentRegistryCommand::ActivateAgent { .. } |
                                AgentRegistryCommand::DeactivateAgent { .. } => false,
                                // Include other operations for this agent
                                AgentRegistryCommand::RegisterAgent { agent_id: id, .. } |
                                AgentRegistryCommand::AuthorizeAgent { agent_id: id, .. } |
                                AgentRegistryCommand::DeauthorizeAgent { agent_id: id, .. } => id == aid,
                                _ => false,
                            }
                        }
                        // Include all agent operations for this agent
                        Command::Agent(_) => cmd_tx.command.extract_agent_id() == Some(*aid),
                        _ => false,
                    }
                }
                _ => false,
            };
            
            if should_replay {
                // Apply the command
                debug!("Replaying command {} (seq {}): {:?}", cmd_tx.id, cmd_tx.sequence, cmd_tx.command);
                if let Err(e) = self.process_command(cmd_tx.command.clone()).await {
                    error!("Failed to replay command during entity rebuild: {}", e);
                    debug!("  Failed command was: {:?}", cmd_tx.command);
                    debug!("  Command ID: {}, Sequence: {}", cmd_tx.id, cmd_tx.sequence);
                }
            }
        }
        
        // Now recover any pending commands for this entity
        let pending = self.wal.list_pending_commands()?;
        for cmd_tx in pending {
            let is_for_entity = match (&graph_id, &agent_id) {
                (Some(gid), _) => cmd_tx.command.extract_graph_id() == Some(*gid),
                (_, Some(aid)) => cmd_tx.command.extract_agent_id() == Some(*aid),
                _ => false,
            };
            
            if is_for_entity {
                let tx_id = cmd_tx.id.clone();
                match self.process_command(cmd_tx.command).await {
                    Ok(_) => {
                        self.wal.update_command_state(&tx_id, CommandState::Committed)?;
                    }
                    Err(e) => {
                        error!("Failed to recover pending command {} for entity: {}", tx_id, e);
                    }
                }
            }
        }
        
        self.is_recovering = false;
        Ok(())
    }
    
    /// Recover pending commands from a previous crash
    async fn recover_pending_commands(&mut self) -> Result<usize> {
        let pending = self.wal.list_pending_commands()?;
        
        if pending.is_empty() {
            return Ok(0);
        }
        
        info!("Found {} pending commands from previous session", pending.len());
        
        // Track which entities we temporarily load for recovery
        let mut temp_loaded_graphs = HashSet::new();
        let mut temp_loaded_agents = HashSet::new();
        
        // First pass: identify what needs to be loaded
        for cmd_tx in &pending {
            if let Some(graph_id) = cmd_tx.command.extract_graph_id() {
                {
                    let managers = self.graph_managers.read().await;
                    if !managers.contains_key(&graph_id) {
                        temp_loaded_graphs.insert(graph_id);
                    }
                }
            }
            if let Some(agent_id) = cmd_tx.command.extract_agent_id() {
                {
                    let agents = self.agents.read().await;
                    if !agents.contains_key(&agent_id) {
                        temp_loaded_agents.insert(agent_id);
                    }
                }
            }
        }
        
        // Note: The lazy loading in ensure_graph_loaded and ensure_agent_loaded
        // will handle loading these entities when we apply commands
        
        self.is_recovering = true;
        let mut recovered = 0;
        
        for cmd_tx in pending {
            let tx_id = cmd_tx.id.clone();
            
            match self.process_command(cmd_tx.command.clone()).await {
                Ok(_) => {
                    // Mark as committed in WAL
                    self.wal.update_command_state(&tx_id, CommandState::Committed)?;
                    recovered += 1;
                }
                Err(e) => {
                    error!("Failed to recover pending command {}: {}", tx_id, e);
                    // Could mark as Aborted or leave for manual intervention
                    // For now, leave it pending for manual resolution
                }
            }
        }
        
        self.is_recovering = false;
        
        // Clean up temporarily loaded entities
        for graph_id in temp_loaded_graphs {
            let mut managers = self.graph_managers.write().await;
            managers.remove(&graph_id);
        }
        for agent_id in temp_loaded_agents {
            let mut agents = self.agents.write().await;
            agents.remove(&agent_id);
        }
        
        Ok(recovered)
    }
    
    /// Wait for active commands to complete with timeout
    pub async fn wait_for_completion(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        
        while !self.active_transactions.is_empty() {
            if start.elapsed() > timeout {
                warn!("Timeout waiting for active commands to complete");
                return false;
            }
            
            // Wait for notification or timeout
            tokio::select! {
                _ = self.active_count_notify.notified() => {
                    // Check if we're done
                    if self.active_transactions.is_empty() {
                        info!("All active commands completed");
                        return true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    // Continue checking
                }
            }
        }
        
        true
    }
}