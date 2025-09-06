//! CQRS (Command Query Responsibility Segregation) System
//!
//! This module implements a CQRS architecture that eliminates deadlocks by routing all
//! mutations through a single command processor while maintaining fast concurrent reads.
//! This is the core architectural pattern that ensures Cymbiont can handle complex 
//! multi-agent operations without lock contention.
//!
//! ## Why CQRS?
//!
//! The previous lock-based architecture suffered from deadlock issues when multiple
//! agents tried to access graphs and registries simultaneously. CQRS solves this by:
//! - Serializing all mutations through a single processor (no lock contention)
//! - Allowing unlimited concurrent reads (no read locks needed)
//! - Providing a clear audit trail of all changes (command log)
//!
//! ## Architecture Overview
//!
//! ```text
//! External Callers (CLI, WebSocket, etc.)
//!         |
//!         v
//!   CommandQueue (Public API)
//!         |
//!    [Channel]
//!         |
//!         v
//!  CommandProcessor (Background Task)
//!    - Owns all mutable state
//!    - Processes commands sequentially
//!    - Routes through RouterToken system
//!         |
//!         v
//!    Domain Logic (via RouterToken)
//! ```
//!
//! ## Core Components
//!
//! - **CommandQueue**: Thread-safe public API for command submission
//! - **CommandProcessor**: Single-threaded owner of all mutable state
//! - **Command**: Enum representing all possible mutations
//! - **RouterToken**: Compile-time enforcement of CQRS routing
//! - **CommandLog**: Command persistence
//!
//! ## Usage Examples
//!
//! ### Submitting Commands
//! ```rust
//! use cqrs::{Command, GraphCommand};
//! 
//! // All mutations go through CommandQueue
//! let response = app_state.command_queue.execute(
//!     Command::Graph(GraphCommand::CreateBlock {
//!         agent_id,
//!         graph_id,
//!         content: "Hello, world!".to_string(),
//!         parent_id: None,
//!         page_name: Some("index".to_string()),
//!         properties: None,
//!     })
//! ).await?;
//! ```
//!
//! ### Direct Reads (Queries)
//! ```rust
//! // Queries bypass the command queue for performance
//! let graphs = app_state.graph_managers.read().await;
//! if let Some(manager) = graphs.get(&graph_id) {
//!     let graph = manager.read().await;
//!     // Read operations directly on graph...
//! }
//! ```
//!
//! ## Key Design Decisions
//!
//! ### Single Processor Thread
//! All mutations are processed by a single background task, eliminating the possibility
//! of deadlocks. This might seem like a bottleneck, but in practice:
//! - Graph operations are typically millisecond-scale
//! - Network I/O dominates response time anyway
//! - Reads (the majority of operations) are unlimited concurrent
//!
//! ### RouterToken Pattern
//! The `RouterToken` is a zero-sized type that can only be constructed within the
//! router module. All domain logic methods require a RouterToken, making it impossible
//! to bypass CQRS at compile time. This is Rust's type system working as architecture
//! enforcement.
//!
//! ### Command Resolution
//! Some commands need to capture non-deterministic values (UUIDs, timestamps) before
//! being written to the log. The `resolve()` method handles this for consistency.
//!
//! ## Migration Path
//!
//! When migrating code to use CQRS:
//! 1. Identify the mutation (what changes?)
//! 2. Define a Command variant with all needed parameters
//! 3. Convert direct calls to `command_queue.execute()`
//! 4. Move business logic to router handlers
//! 5. Require RouterToken in helper functions
//!
//! ## Future Simplifications
//!

mod commands;
mod processor;
mod queue;
pub mod router;

pub use commands::{Command, SystemCommand};
pub use queue::CommandQueue;
pub use processor::CommandProcessor;

// Re-export command types for convenience
pub use commands::{GraphCommand, AgentCommand, RegistryCommand,
                   GraphRegistryCommand};