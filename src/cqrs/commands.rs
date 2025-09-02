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
//! - **RegistryCommand**: Lifecycle operations (create/delete graphs and agents)
//! - **SystemCommand**: Infrastructure control (freeze, shutdown)
//!
//! ## Deterministic Replay
//!
//! Commands must be deterministic for recovery to work correctly. This means the same
//! command must produce the same result every time it's executed. Non-deterministic
//! values like UUIDs and timestamps are captured during command resolution.
//!
//! ### The Resolution Phase
//!
//! Before a command is written to the log, `resolve()` is called to capture any
//! non-deterministic values:
//!
//! ```rust
//! let mut command = Command::Registry(RegistryCommand::Graph(
//!     GraphRegistryCommand::CreateGraph {
//!         name: Some("my-graph".to_string()),
//!         description: None,
//!         resolved_id: None,  // Will be filled by resolve()
//!     }
//! ));
//! 
//! command.resolve();  // Now resolved_id contains a UUID
//! ```
//!
//! During recovery, commands are replayed WITHOUT calling resolve(), using the
//! previously captured values to ensure identical results.
//!
//! ## Command Design Patterns
//!
//! ### Required Fields
//! Most commands require:
//! - `agent_id`: Which agent is performing the action (for authorization)
//! - `graph_id` or target ID: What entity is being modified
//! - Operation-specific data: The actual change being made
//!
//! ### Optional Resolution Fields
//! Commands that generate values include `resolved_*` fields:
//! - `resolved_id`: For entity creation commands
//! - `resolved_timestamp`: For time-sensitive operations (future)
//! - `resolved_hash`: For content-addressed storage (future)
//!
//! ## Adding New Commands
//!
//! When adding a new command:
//!
//! 1. **Choose the right category** - Does it affect graphs, agents, registries, or system?
//! 2. **Define the variant** with all needed parameters:
//!    ```rust
//!    MyNewOperation {
//!        agent_id: Uuid,        // Who's doing this?
//!        target_id: Uuid,       // What's being changed?
//!        new_value: String,     // What's the change?
//!        resolved_id: Option<Uuid>,  // If creating something
//!    }
//!    ```
//! 3. **Implement resolution** if needed in the `resolve()` method
//! 4. **Add handler** in `router.rs` to execute the command
//! 5. **Update GraphOps/tools** if this should be exposed to agents
//!
//! ## Examples
//!
//! ### Simple Mutation Command
//! ```rust
//! Command::Graph(GraphCommand::UpdateBlock {
//!     agent_id: agent_uuid,
//!     graph_id: graph_uuid,
//!     block_id: "block-123".to_string(),
//!     content: "Updated content".to_string(),
//! })
//! ```
//!
//! ### Creation Command with Resolution
//! ```rust
//! Command::Registry(RegistryCommand::Agent(
//!     AgentRegistryCommand::CreateAgent {
//!         name: "Assistant".to_string(),
//!         description: Some("Helper agent".to_string()),
//!         resolved_id: None,  // Will be set by resolve()
//!     }
//! ))
//! ```
//!
//! ### System Control Command
//! ```rust
//! Command::System(SystemCommand::FreezeOperations)
//! ```
//!
//! ## Command Lifecycle
//!
//! 1. **Construction**: Caller creates command with parameters
//! 2. **Resolution**: Non-deterministic values captured (if needed)
//! 3. **Persistence**: Command written to WAL
//! 4. **Execution**: Router handles command, modifying state
//! 5. **Response**: Result returned to caller via future
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
        agent_id: Uuid,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<Value>,
    },
    UpdateBlock {
        graph_id: Uuid,
        agent_id: Uuid,
        block_id: String,
        content: String,
    },
    DeleteBlock {
        graph_id: Uuid,
        agent_id: Uuid,
        block_id: String,
    },
    CreatePage {
        graph_id: Uuid,
        agent_id: Uuid,
        page_name: String,
        properties: Option<Value>,
    },
    DeletePage {
        graph_id: Uuid,
        agent_id: Uuid,
        page_name: String,
    },
}


/// Agent-related mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentCommand {
    AddMessage {
        agent_id: Uuid,
        message: Value, // Full Message struct serialized
    },
    ClearHistory {
        agent_id: Uuid,
    },
    SetLLMConfig {
        agent_id: Uuid,
        config: Value, // LLMConfig serialized
    },
    SetSystemPrompt {
        agent_id: Uuid,
        prompt: String,
    },
    SetDefaultGraph {
        agent_id: Uuid,
        graph_id: Option<Uuid>,
    },
}


/// Registry-related mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryCommand {
    Graph(GraphRegistryCommand),
    Agent(AgentRegistryCommand),
}

/// Graph registry mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphRegistryCommand {
    /// High-level: Create a new graph with prime agent authorization
    /// 
    /// Non-deterministic: Generates UUID during resolution phase
    CreateGraph {
        name: Option<String>,
        description: Option<String>,
        /// UUID generated during resolution, before WAL write
        #[serde(skip_serializing_if = "Option::is_none")]
        resolved_id: Option<Uuid>,
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

/// Agent registry mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRegistryCommand {
    /// High-level: Create a new agent with prime authorization if needed
    /// 
    /// Non-deterministic: Generates UUID during resolution phase
    CreateAgent {
        name: Option<String>,
        description: Option<String>,
        /// UUID generated during resolution, before WAL write
        #[serde(skip_serializing_if = "Option::is_none")]
        resolved_id: Option<Uuid>,
    },
    /// Low-level: Register agent metadata only
    RegisterAgent {
        agent_id: Uuid,
        name: Option<String>,
        description: Option<String>,
    },
    DeleteAgent {
        agent_id: Uuid,
    },
    ActivateAgent {
        agent_id: Uuid,
    },
    DeactivateAgent {
        agent_id: Uuid,
    },
    AuthorizeAgent {
        agent_id: Uuid,
        graph_id: Uuid,
    },
    DeauthorizeAgent {
        agent_id: Uuid,
        graph_id: Uuid,
    },
    SetPrimeAgent {
        agent_id: Uuid,
    },
}

/// System-level commands for lifecycle management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemCommand {
    /// Initiate graceful shutdown - returns active transaction count
    InitiateShutdown,
    /// Wait for active transactions to complete - returns true if all completed
    WaitForCompletion { timeout_secs: u64 },
    /// Force flush WAL for immediate shutdown
    ForceFlush,
}

impl Command {
    /// Resolve non-deterministic values before WAL write.
    /// 
    /// This method captures any non-deterministic values (UUIDs, timestamps, etc.)
    /// that would otherwise be generated during command execution. This ensures
    /// deterministic replay from the WAL.
    /// 
    /// IMPORTANT: This method modifies the command in-place and MUST be called
    /// exactly once before writing to WAL. It must NEVER be called during WAL replay.
    pub fn resolve(&mut self) {
        match self {
            Command::Registry(RegistryCommand::Graph(GraphRegistryCommand::CreateGraph { 
                resolved_id, .. 
            })) => {
                if resolved_id.is_none() {
                    *resolved_id = Some(Uuid::new_v4());
                }
            }
            Command::Registry(RegistryCommand::Agent(AgentRegistryCommand::CreateAgent { 
                resolved_id, .. 
            })) => {
                if resolved_id.is_none() {
                    *resolved_id = Some(Uuid::new_v4());
                }
            }
            _ => {
                // Most commands are already deterministic and need no resolution
            }
        }
    }
    
    /// Extract the graph ID from a command if it involves a specific graph
    pub fn extract_graph_id(&self) -> Option<Uuid> {
        match self {
            Command::Graph(cmd) => match cmd {
                GraphCommand::CreateBlock { graph_id, .. } |
                GraphCommand::UpdateBlock { graph_id, .. } |
                GraphCommand::DeleteBlock { graph_id, .. } |
                GraphCommand::CreatePage { graph_id, .. } |
                GraphCommand::DeletePage { graph_id, .. } => Some(*graph_id),
            },
            Command::Agent(_) => None, // Agent commands don't directly affect graphs
            Command::Registry(RegistryCommand::Graph(cmd)) => match cmd {
                GraphRegistryCommand::CreateGraph { resolved_id, .. } => *resolved_id, // May have resolved ID
                GraphRegistryCommand::RegisterGraph { graph_id, .. } |
                GraphRegistryCommand::RemoveGraph { graph_id } |
                GraphRegistryCommand::OpenGraph { graph_id } |
                GraphRegistryCommand::CloseGraph { graph_id } => Some(*graph_id),
            },
            Command::Registry(RegistryCommand::Agent(cmd)) => match cmd {
                AgentRegistryCommand::AuthorizeAgent { graph_id, .. } |
                AgentRegistryCommand::DeauthorizeAgent { graph_id, .. } => Some(*graph_id),
                _ => None,
            },
            Command::System(_) => None,
        }
    }
    
    /// Extract the agent ID from a command if it involves a specific agent
    pub fn extract_agent_id(&self) -> Option<Uuid> {
        match self {
            Command::Graph(cmd) => match cmd {
                GraphCommand::CreateBlock { agent_id, .. } |
                GraphCommand::UpdateBlock { agent_id, .. } |
                GraphCommand::DeleteBlock { agent_id, .. } |
                GraphCommand::CreatePage { agent_id, .. } |
                GraphCommand::DeletePage { agent_id, .. } => Some(*agent_id),
            },
            Command::Agent(cmd) => match cmd {
                AgentCommand::AddMessage { agent_id, .. } |
                AgentCommand::ClearHistory { agent_id } |
                AgentCommand::SetLLMConfig { agent_id, .. } |
                AgentCommand::SetSystemPrompt { agent_id, .. } |
                AgentCommand::SetDefaultGraph { agent_id, .. } => Some(*agent_id),
            },
            Command::Registry(RegistryCommand::Agent(cmd)) => match cmd {
                AgentRegistryCommand::CreateAgent { resolved_id, .. } => *resolved_id, // May have resolved ID
                AgentRegistryCommand::RegisterAgent { agent_id, .. } |
                AgentRegistryCommand::DeleteAgent { agent_id } |
                AgentRegistryCommand::ActivateAgent { agent_id } |
                AgentRegistryCommand::DeactivateAgent { agent_id } |
                AgentRegistryCommand::AuthorizeAgent { agent_id, .. } |
                AgentRegistryCommand::DeauthorizeAgent { agent_id, .. } |
                AgentRegistryCommand::SetPrimeAgent { agent_id } => Some(*agent_id),
            },
            Command::Registry(RegistryCommand::Graph(_)) => None, // Graph registry commands don't have agent IDs
            Command::System(_) => None,
        }
    }
}

/// Result of command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    /// Child commands to execute after this command completes
    /// These are ignored during recovery since they'll have their own WAL entries
    #[serde(default)]
    pub child_commands: Vec<Command>,
}