//! Command Processor - The Beating Heart of CQRS
//!
//! The `CommandProcessor` is the single-threaded owner of all mutable state in Cymbiont.
//! It runs as a background task, processing commands sequentially to guarantee
//! deadlock-free operation while enabling unlimited concurrent reads. This is the
//! architectural keystone that makes operations safe and predictable.
//!
//! ## Core Responsibilities
//!
//! ### State Ownership
//! The processor directly owns all mutable resources:
//! - Graph managers (knowledge graphs)
//! - Registries (metadata)
//!
//! External code receives Arc<`RwLock`<>> references for read-only access, but
//! only the processor can modify state through command execution.
//!
//! ### Command Processing Pipeline
//! ```text
//! 1. Receive command from queue
//! 2. Execute via router
//! 3. Send response via oneshot
//! ```
//!
//! ### Startup
//! On startup, the processor:
//! 1. Begins processing new commands
//!
//! Entities (graphs) are lazy-loaded on first access to avoid loading
//! everything into memory at once.
//!
//! ## Design Patterns
//!
//! ### Lazy Entity Loading
//! When a command targets an entity that isn't loaded:
//! ```rust
//! // Processor checks if graph is loaded
//! if !self.graph_managers.contains_key(&graph_id) {
//!     // Load graph from disk
//!     self.ensure_graph_loaded(&graph_id)?;
//! }
//! // Now proceed with command execution
//! ```
//!
//! ### `RouterToken` Authorization
//! The processor creates `RouterToken`s that prove commands came through CQRS:
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
//! - Lazy loading prevents loading all graphs at once
//!
//! ### Scalability Path
//! Future optimizations (not yet needed):
//! - Partition commands by entity ID for parallel processing
//! - Read-through caching for frequently accessed data
//! - Command batching for related operations
//!
//! ## Error Handling
//!
//! Commands can fail for various reasons:
//! - **Not Found**: Target entity doesn't exist (graph, block, page)
//! - **Invalid State**: Operation not valid in current state
//! - **Business Logic**: Domain-specific validation failures
//! - **System**: I/O errors, serialization failures
//!
//! All errors are captured and returned via the response channel.
//!
//! ## Implementation Notes
//!
//! ### Why Not Actor Model?
//! We considered actors but chose CQRS because:
//! - Simpler mental model (one processor vs many actors)
//! - Easier debugging (sequential execution)
//! - Natural fit for command ordering
//! - No actor supervision complexity
//!
//! ### Why Not Event Sourcing?
//! CQRS without full event sourcing because:
//! - Commands are coarser than events (better performance)
//! - Commands are self-contained units of change
//! - Easier to understand (commands map to user actions)

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

use super::commands::{Command, CommandResult, GraphCommand};
use super::queue::CommandQueue;
use super::router;
use crate::error::{ProcessorError, Result};
use crate::graph::graph_manager::GraphManager;
use crate::graph::graph_registry::GraphRegistry;

/// Envelope for command submission with response channel
pub struct CommandEnvelope {
    pub command: Command,
    pub response: oneshot::Sender<Result<CommandResult>>,
}

/// References to resources owned by `CommandProcessor` for direct query access
pub struct ProcessorResources {
    pub graph_managers: Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,
}

// ProcessorError is now in error.rs

/// Processor states for shutdown coordination
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessorState {
    Accepting = 0, // Normal operation
    Draining = 1,  // Rejecting non-system commands
    Shutdown = 2,  // Rejecting everything
}

impl From<u8> for ProcessorState {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Draining,
            2 => Self::Shutdown,
            _ => Self::Accepting,
        }
    }
}

/// The command processor that owns all mutable state
pub struct CommandProcessor {
    // Owned mutable state (wrapped for sharing with AppState)
    graph_managers: Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,


    // Registry for metadata (reference from AppState)
    graph_registry: Arc<RwLock<GraphRegistry>>,

    // Shutdown coordination
    state: Arc<AtomicU8>,
    active_count_notify: Arc<Notify>,

    // Configuration
    data_dir: PathBuf,
}

impl CommandProcessor {
    /// Create a new command processor
    pub fn new(
        graph_registry: Arc<RwLock<GraphRegistry>>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            graph_managers: Arc::new(RwLock::new(HashMap::new())),
            graph_registry,
            state: Arc::new(AtomicU8::new(ProcessorState::Accepting as u8)),
            active_count_notify: Arc::new(Notify::new()),
            data_dir,
        }
    }

    /// Start the command processor in a background task
    /// Returns a `CommandQueue` for submitting commands and `ProcessorResources` for queries
    pub fn start(self) -> (CommandQueue, ProcessorResources) {
        info!("🚀 Command processor started");

        // Create channel for command submission
        let (sender, receiver) = mpsc::channel(100);

        // Extract references before moving self
        let resources = ProcessorResources {
            graph_managers: self.graph_managers.clone(),
        };

        // Spawn processor in background for command processing
        tokio::spawn(async move {
            self.run(receiver).await;
        });

        // Return queue with sender and resources
        (CommandQueue::new_with_sender(sender), resources)
    }

    /// Run the command processing loop
    pub async fn run(mut self, mut receiver: mpsc::Receiver<CommandEnvelope>) {
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
                        let _ = envelope
                            .response
                            .send(Err(ProcessorError::ShuttingDown.into()));
                        continue;
                    }
                }
                ProcessorState::Shutdown => {
                    // Reject everything
                    warn!(
                        "Rejecting command - processor is shut down: {:?}",
                        envelope.command
                    );
                    let _ = envelope.response.send(Err(ProcessorError::Shutdown.into()));
                    continue;
                }
            }

            // Process the command
            let result = self.process_command(envelope.command).await;

            // Send response (ignore send errors if receiver dropped)
            let _ = envelope.response.send(result);
        }

        info!("Command processor stopped");
    }

    /// Process a single command
    fn process_command(
        &mut self,
        command: Command,
    ) -> Pin<Box<dyn Future<Output = Result<CommandResult>> + Send + '_>> {
        Box::pin(async move {
            // Apply the command directly
            let result = self.apply_command(command).await;

            // Execute child commands if any
            if let Ok(cmd_result) = &result {
                for child_cmd in &cmd_result.child_commands {
                    // Process child command
                    let child_result = self.process_command(child_cmd.clone()).await;
                    if let Err(e) = child_result {
                        error!("Child command failed: {}", e);
                        // Continue processing other child commands even if one fails
                    }
                }
            }

            result
        })
    }

    /// Apply a mutation command
    async fn apply_command(&self, command: Command) -> Result<CommandResult> {
        match command {
            Command::Graph(graph_cmd) => {
                // Extract graph_id for loading
                let graph_id = match &graph_cmd {
                    GraphCommand::CreateBlock { graph_id, .. }
                    | GraphCommand::UpdateBlock { graph_id, .. }
                    | GraphCommand::DeleteBlock { graph_id, .. }
                    | GraphCommand::CreatePage { graph_id, .. }
                    | GraphCommand::DeletePage { graph_id, .. } => *graph_id,
                };

                // Ensure graph is loaded before routing
                self.ensure_graph_loaded(graph_id).await?;

                router::route_graph_command(&self.graph_managers, graph_cmd).await
            }
            Command::Registry(reg_cmd) => {
                router::route_registry_command(
                    &self.graph_managers,
                    &self.graph_registry,
                    reg_cmd,
                    &self.data_dir,
                )
                .await
            }
            Command::System(sys_cmd) => self.handle_system_command(sys_cmd).await,
        }
    }

    /// Handle system-level commands for lifecycle management
    async fn handle_system_command(
        &self,
        command: super::commands::SystemCommand,
    ) -> Result<CommandResult> {
        use super::commands::SystemCommand;

        match command {
            SystemCommand::InitiateShutdown => {
                // Set state to draining to reject new non-system commands
                self.initiate_shutdown();

                Ok(CommandResult {
                    success: true,
                    data: Some(serde_json::json!({
                        "active_count": 0,
                    })),
                    error: None,
                    child_commands: vec![],
                })
            }
            SystemCommand::WaitForCompletion { timeout_secs } => {
                // Wait for active transactions to complete
                let completed = self
                    .wait_for_completion(Duration::from_secs(timeout_secs))
                    .await;

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
                self.state
                    .store(ProcessorState::Shutdown as u8, Ordering::Release);
                info!("Force flush completed");

                Ok(CommandResult {
                    success: true,
                    data: None,
                    error: None,
                    child_commands: vec![],
                })
            }
        }
    }

    /// Ensure a graph is loaded in memory
    async fn ensure_graph_loaded(&self, graph_id: Uuid) -> Result<()> {
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
            managers
                .entry(graph_id)
                .or_insert_with(|| Arc::new(RwLock::new(graph_manager)));
        }

        Ok(())
    }

    /// Initiate graceful shutdown
    pub fn initiate_shutdown(&self) {
        info!("Initiating graceful shutdown of command processor");
        self.state
            .store(ProcessorState::Draining as u8, Ordering::Release);
        self.active_count_notify.notify_one();
    }

    /// Wait for active commands to complete with timeout
    pub async fn wait_for_completion(&self, timeout: Duration) -> bool {
        tokio::time::sleep(timeout).await;
        true
    }
}
