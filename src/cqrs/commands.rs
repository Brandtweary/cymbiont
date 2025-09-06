//! Command Definitions - The Vocabulary of State Changes
//!
//! This module defines all possible mutations in Cymbiont as strongly-typed commands.
//! Commands are the atomic units of change that flow through the CQRS system, providing
//! a complete vocabulary for how the system can evolve over time.
//!
//! ## Command Categories
//!
//! Commands are organized into logical groups based on what they affect:
//!
//! - **GraphCommand**: Mutations to knowledge graph content (blocks, pages)
//! - **AgentCommand**: Changes to agent state (messages, configuration)
//! - **RegistryCommand**: Lifecycle operations (create/delete graphs)
//! - **SystemCommand**: Infrastructure control (shutdown)
//!
//! ## Command Design Patterns
//!
//! ### Required Fields
//! Most commands require:
//! - `graph_id` or target ID: What entity is being modified
//! - Operation-specific data: The actual change being made
//!
//! ## Adding New Commands
//!
//! When adding a new command:
//!
//! 1. **Choose the right category** - Does it affect graphs, agents, registries, or system?
//! 2. **Define the variant** with all needed parameters:
//!    ```rust
//!    MyNewOperation {
//!        target_id: Uuid,       // What's being changed?
//!        new_value: String,     // What's the change?
//!    }
//!    ```
//! 3. **Add handler** in `router.rs` to execute the command
//! 4. **Update GraphOps/tools** if this should be exposed to agents
//!
//! ## Examples
//!
//! ### Simple Mutation Command
//! ```rust
//! Command::Graph(GraphCommand::UpdateBlock {
//!     graph_id: graph_uuid,
//!     block_id: "block-123".to_string(),
//!     content: "Updated content".to_string(),
//! })
//! ```
//!
//! ### Creation Command
//! ```rust
//! Command::Registry(RegistryCommand::Graph(
//!     GraphRegistryCommand::CreateGraph {
//!         name: Some("my-graph".to_string()),
//!         description: Some("A new knowledge graph".to_string()),
//!     }
//! ))
//! ```
//!
//! ### System Control Command
//! ```rust
//! Command::System(SystemCommand::InitiateShutdown)
//! ```
//!
//! ## Command Lifecycle
//!
//! 1. **Construction**: Caller creates command with parameters
//! 2. **Execution**: Router handles command, modifying state
//! 3. **Response**: Result returned to caller via future
//!
//! ## Design Philosophy
//!
//! Commands should be:
//! - **Self-contained**: Include all data needed to execute
//! - **Atomic**: Represent a single logical change
//! - **Idempotent**: When possible, repeated execution should be safe
//! - **Descriptive**: The command name should clearly indicate what happens

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use serde_json::Value;

/// All possible operations in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Graph(GraphCommand),
    Agent(AgentCommand),
    Registry(RegistryCommand),
    System(SystemCommand),
}

/// Graph-related mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphCommand {
    CreateBlock {
        graph_id: Uuid,
        block_id: Option<String>,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<Value>,
        reference_content: Option<String>,
    },
    UpdateBlock {
        graph_id: Uuid,
        block_id: String,
        content: String,
    },
    DeleteBlock {
        graph_id: Uuid,
        block_id: String,
    },
    CreatePage {
        graph_id: Uuid,
        page_name: String,
        properties: Option<Value>,
    },
    DeletePage {
        graph_id: Uuid,
        page_name: String,
    },
}


/// Agent-related mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentCommand {
    AddMessage {
        message: Value, // Full Message struct serialized
    },
    ClearHistory,
    SetLLMConfig {
        config: Value, // LLMConfig serialized
    },
    SetSystemPrompt {
        prompt: String,
    },
}


/// Registry-related mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryCommand {
    Graph(GraphRegistryCommand),
}

/// Graph registry mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphRegistryCommand {
    /// Create a new graph
    CreateGraph {
        name: Option<String>,
        description: Option<String>,
    },
    /// Low-level: Register graph metadata only
    RegisterGraph {
        graph_id: Uuid,
        name: Option<String>,
        description: Option<String>,
    },
    /// Remove a graph (archive to archived_graphs/)
    RemoveGraph {
        graph_id: Uuid,
    },
    OpenGraph {
        graph_id: Uuid,
    },
    CloseGraph {
        graph_id: Uuid,
    },
}


/// System-level commands for lifecycle management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemCommand {
    /// Initiate graceful shutdown - returns active transaction count
    InitiateShutdown,
    /// Wait for active transactions to complete - returns true if all completed
    WaitForCompletion { timeout_secs: u64 },
    /// Force flush for immediate shutdown
    ForceFlush,
}


/// Result of command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    /// Child commands to execute after this command completes
    #[serde(default)]
    pub child_commands: Vec<Command>,
}