//! Command Queue - The Public Gateway to State Mutations
//!
//! The CommandQueue is the single entry point for all state changes in Cymbiont.
//! It provides a clean, async API that accepts commands and returns futures,
//! allowing callers to submit mutations without blocking while ensuring all
//! changes are processed sequentially by the CommandProcessor.
//!
//! ## Architecture Role
//!
//! The CommandQueue acts as the boundary between the concurrent outer world
//! and the sequential inner command processor:
//!
//! ```text
//! Concurrent World         |  Sequential World
//! -----------------------  |  -----------------
//! Multiple Callers         |
//!   ↓ ↓ ↓                  |
//! CommandQueue             |
//!   - Accept commands      |
//!   - Return futures       |
//!   ↓                      |
//! [mpsc channel] =========>|  CommandProcessor
//!                          |    - Process one at a time
//!                          |    - Modify state safely
//! ```
//!
//! ## Usage Pattern
//!
//! ```rust
//! use cqrs::{Command, GraphCommand};
//!
//! // Submit a command and await the result
//! let result = app_state.command_queue.execute(
//!     Command::Graph(GraphCommand::CreateBlock {
//!         graph_id,
//!         content: "Hello".to_string(),
//!         parent_id: None,
//!         page_name: None,
//!         properties: None,
//!     })
//! ).await?;
//!
//! // Check the result
//! match result.success {
//!     true => println!("Block created: {:?}", result.data),
//!     false => eprintln!("Failed: {}", result.message.unwrap_or_default()),
//! }
//! ```
//!
//! ## Design Benefits
//!
//! ### Async/Await Integration
//! Commands return futures that integrate naturally with Tokio's async runtime.
//! This allows WebSocket handlers, CLI commands, and other async contexts to
//! submit commands without blocking threads.
//!
//! ### Backpressure Handling
//! The underlying mpsc channel provides natural backpressure. If the processor
//! falls behind, the channel will fill up and callers will await, preventing
//! memory exhaustion from unbounded command queuing.
//!
//! ### Clean Error Propagation
//! Errors are propagated through the future's Result type, allowing callers
//! to handle failures appropriately with Rust's standard error handling.
//!
//! ## Implementation Details
//!
//! ### CommandEnvelope
//! Each command is wrapped in an envelope containing:
//! - The command itself
//! - A oneshot channel for the response
//!
//! This allows the processor to send results back to the specific caller
//! without maintaining a complex routing table.
//!
//! ### Channel Lifecycle
//! The mpsc channel is created during processor initialization and lives
//! for the entire application lifetime. The queue holds a Sender clone,
//! allowing it to be cloned freely across threads.
//!
//! ## Error Scenarios
//!
//! The execute method can fail in two ways:
//! 1. **Send failure**: The processor has shut down (channel closed)
//! 2. **Response failure**: The processor crashed before responding
//!
//! Both are converted to CymbiontError for consistent error handling.
//!
//! ## Performance Characteristics
//!
//! - **Submission**: O(1) - just sends to channel
//! - **Memory**: Bounded by channel capacity
//! - **Latency**: Depends on processor queue depth
//! - **Throughput**: Limited by processor speed (intentionally)
//!
//! The single-threaded processor might seem like a bottleneck, but remember:
//! - Most operations are sub-millisecond
//! - Network I/O dominates response time
//! - Reads bypass the queue entirely
//! - Deadlock prevention is worth the trade-off

use tokio::sync::{mpsc, oneshot};

use super::commands::{Command, CommandResult};
use super::processor::CommandEnvelope;
use crate::error::*;

/// Public API for submitting commands to the processor
#[derive(Clone)]
pub struct CommandQueue {
    sender: mpsc::Sender<CommandEnvelope>,
}

impl CommandQueue {
    /// Create a new command queue with external channel management
    ///
    /// This variant is used when the processor and channel need to be
    /// set up separately (e.g., during AppState initialization).
    pub fn new_with_sender(sender: mpsc::Sender<CommandEnvelope>) -> Self {
        CommandQueue { sender }
    }

    /// Execute a command and wait for the result
    pub async fn execute(&self, command: Command) -> Result<CommandResult> {
        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Create envelope
        let envelope = CommandEnvelope {
            command,
            response: tx,
        };

        // Send to processor
        self.sender
            .send(envelope)
            .await
            .map_err(|e| CymbiontError::Other(format!("Failed to send command: {}", e)))?;

        // Wait for response
        rx.await
            .map_err(|e| CymbiontError::Other(format!("Command processor dropped: {}", e)))?
    }
}
