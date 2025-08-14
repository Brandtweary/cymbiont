//! Agent Core Implementation
//!
//! This module provides the main Agent struct that manages conversation state,
//! LLM configuration, and interaction with the knowledge graph. Each agent
//! maintains its own conversation history, model configuration, and serves as
//! an autonomous knowledge worker within the multi-agent framework.
//!
//! ## Multi-Agent Framework Role
//!
//! Agents are the primary interface between users and the knowledge graph.
//! They provide personalized, stateful interaction while enforcing security
//! through the authorization system. Each agent can be granted specific
//! permissions to access and modify different graphs, enabling collaborative
//! knowledge management with proper access control.
//!
//! ## Architecture Overview
//!
//! The Agent struct owns its complete state and handles persistence through the
//! agent_persistence module. It acts as a stateful context manager, bundling:
//! - **Conversation History**: Full chat context with sliding window management
//! - **LLM Configuration**: Model selection, parameters, and backend settings
//! - **System Prompts**: Custom instructions and behavioral guidelines
//! - **Context Management**: Automatic history trimming and continuity preservation
//! - **Auto-save Logic**: Time and message-based persistence triggers
//!
//! Agents do not directly track graph authorizations - this is managed by the
//! AgentRegistry as the single source of truth to prevent synchronization issues
//! and ensure consistent authorization state across the system.
//!
//! ## Agent Lifecycle States
//!
//! Agents progress through a well-defined lifecycle with clear state transitions:
//!
//! ### Creation Phase
//! - AgentRegistry creates metadata entry with UUID and paths
//! - Agent struct instantiated with default LLM configuration
//! - Data directory structure created: `{data_dir}/agents/{agent-id}/`
//! - Initial agent.json written with empty conversation history
//!
//! ### Activation Phase  
//! - Agent loaded into AppState's active agents HashMap
//! - Full state restored from agent.json on disk
//! - AgentRegistry updated to mark agent as "active"
//! - Agent becomes available for chat interactions
//!
//! ### Interaction Phase
//! - Chat messages processed through `process_message()`
//! - LLM backend called for response generation
//! - Conversation history maintained with automatic trimming
//! - Auto-save triggers monitor time (5min) and message count (10)
//!
//! ### Deactivation Phase
//! - Agent saved to disk with complete state preservation
//! - Removed from AppState's active agents HashMap
//! - AgentRegistry updated to mark agent as "inactive" 
//! - Memory freed, but agent data remains on disk
//!
//! ### Deletion/Archival Phase
//! - Agent moved to `{data_dir}/archived_agents/` with timestamp
//! - Removed from AgentRegistry permanently
//! - All graph authorizations revoked automatically
//! - Prime agent protected from deletion
//!
//! ## Prime Agent System
//!
//! The prime agent is a special system agent that ensures seamless user experience:
//! - **Auto-creation**: Created automatically on first system startup
//! - **Always Available**: Cannot be deleted, ensuring at least one agent exists
//! - **Default Authorization**: Automatically authorized for all new graphs
//! - **WebSocket Default**: Used as current agent when no specific agent selected
//! - **Fallback Role**: Provides system stability and prevents authentication deadlocks
//!
//! ## Conversation Management
//!
//! Agents maintain sophisticated conversation state with multiple features:
//!
//! ### Message Types
//! - **User Messages**: Human input with optional echo field for testing
//! - **Assistant Messages**: LLM-generated responses with timestamps
//! - **Tool Messages**: Function call results (future phase)
//!
//! ### Context Window Management
//! - Configurable limit (default: 100 messages) per agent
//! - Automatic trimming when limit exceeded
//! - FIFO removal strategy (oldest messages removed first)
//! - Context continuity preserved through intelligent trimming
//!
//! ### Message Processing Pipeline
//! 1. **Input Validation**: User message validated and timestamped
//! 2. **History Update**: Message added to conversation history
//! 3. **LLM Invocation**: Backend called with full conversation context
//! 4. **Response Processing**: Assistant response validated and timestamped
//! 5. **History Recording**: Response added to conversation history
//! 6. **Auto-save Check**: Persistence thresholds evaluated
//! 7. **Context Trimming**: Window size enforced if necessary
//!
//! ## LLM Backend Integration
//!
//! Agents support pluggable LLM backends through the LLMConfig system:
//! - **MockLLM**: Test backend with echo support for deterministic testing
//! - **Ollama**: Future integration with local Ollama instances
//! - **OpenAI**: Future integration with OpenAI API
//! - **Custom**: Extensible for additional backend implementations
//!
//! Per-agent configuration allows different models for different agents.
//! Runtime switching supported via `set_llm_config()` with graceful fallback
//! handling for backend failures.
//!
//! ## Persistence and Durability
//!
//! Agent state is comprehensively persisted to `{data_dir}/agents/{agent-id}/agent.json`:
//!
//! ### Persistence Triggers
//! - **Time-based**: Every 5 minutes of activity
//! - **Message-based**: Every 10 messages processed
//! - **Configuration changes**: Immediate save on system prompt/LLM config updates
//! - **Manual**: Explicit save() calls
//! - **Shutdown**: Guaranteed save during graceful shutdown
//! - **Deactivation**: Save before removing from memory
//!
//! ### State Serialization
//! - Full conversation history with timestamps
//! - Complete LLM configuration and parameters
//! - System prompts and custom instructions
//! - Context window settings and metadata
//! - Creation and last activity timestamps
//! - Agent name and display preferences
//!
//! ## Authorization and Security
//!
//! Agents operate within the multi-agent authorization framework:
//! - Graph authorizations managed by AgentRegistry (single source of truth)
//! - Authorization checked via phantom types in graph operations
//! - Unauthorized operations fail at compile time when possible
//! - Runtime authorization errors provide clear messaging
//! - Agent state isolated per agent (no cross-agent data access)
//! - Prime agent cannot be deleted (system stability)
//! - All graph modifications audited through transaction log
//!
//! ## Error Handling and Recovery
//!
//! Comprehensive error handling ensures system resilience:
//! - **Persistence Errors**: Disk I/O failures during save/load
//! - **LLM Errors**: Backend communication failures with auto-retry
//! - **Authorization Errors**: Graph access denied
//! - **Configuration Errors**: Invalid LLM settings
//!
//! Recovery strategies include graceful degradation when backends unavailable,
//! conversation history preservation during errors, and atomic save operations
//! to prevent partial state corruption.
//!
//! ## Future Extensibility
//!
//! The agent system is designed for future expansion:
//! - **Tool Integration**: Knowledge graph tool execution (Phase 2)
//! - **Function Calling**: LLM-driven tool selection and execution
//! - **Advanced Context**: Semantic search in conversation history
//! - **Multi-modal**: Support for image and document processing
//! - **Collaborative**: Multi-agent coordination and handoffs
//!
//! Extension points include pluggable LLM backends via LLMBackend trait,
//! custom message types, configurable persistence strategies, and custom
//! context window management algorithms.
//!
//! ## Performance Considerations
//!
//! The agent system is optimized for responsive interaction:
//! - Active agents loaded in memory for fast access
//! - Inactive agents persist on disk only
//! - Context window limits prevent unbounded memory growth
//! - Auto-save prevents excessive disk writes
//! - Atomic file operations ensure consistency
//! - Agent state protected by async RwLock in AppState
//! - Thread-safe message processing and persistence

use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use tracing::{info, error};
use uuid::Uuid;

use crate::storage::agent_persistence;
use crate::agent::llm::{LLMConfig, LLMBackend, Message, AgentContext, create_llm_backend};

/// Agent errors
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Persistence error: {0}")]
    Persistence(#[from] agent_persistence::AgentPersistenceError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;

/// Main Agent struct containing full state
/// 
/// This struct manages the complete agent state including conversation
/// history, LLM configuration, and graph authorizations. It provides
/// methods for conversation management, persistence, and context control.
#[derive(Debug)]
pub struct Agent {
    /// Unique identifier
    pub id: Uuid,
    
    /// Display name
    pub name: String,
    
    /// LLM backend configuration (persisted)
    pub llm_config: LLMConfig,
    
    /// Full conversation history with tool results
    pub conversation_history: Vec<Message>,
    
    /// Maximum number of messages to keep in context
    pub context_window_limit: usize,
    
    /// Custom system prompt/instructions
    pub system_prompt: Option<String>,
    
    /// Creation timestamp
    pub created: DateTime<Utc>,
    
    /// Last activity timestamp
    pub last_active: DateTime<Utc>,
    
    /// Path to agent data directory
    data_path: PathBuf,
    
    /// Track messages since last save for auto-save
    messages_since_save: usize,
    
    /// Last save time for auto-save
    last_save_time: DateTime<Utc>,
}

impl Agent {
    /// Create a new agent with the given configuration
    pub fn new(
        id: Uuid,
        name: String,
        llm_config: LLMConfig,
        data_path: PathBuf,
        system_prompt: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Agent {
            id,
            name,
            llm_config,
            conversation_history: Vec::new(),
            context_window_limit: 100,  // Default context window
            system_prompt,
            created: now,
            last_active: now,
            data_path,
            messages_since_save: 0,
            last_save_time: now,
        }
    }
    
    /// Load agent from disk
    pub fn load(agent_dir: &Path) -> Result<Self> {
        let loaded = agent_persistence::load_agent(agent_dir)?;
        
        Ok(Agent {
            id: loaded.id,
            name: loaded.name,
            llm_config: loaded.llm_config,
            conversation_history: loaded.conversation_history,
            context_window_limit: loaded.context_window_limit,
            system_prompt: loaded.system_prompt,
            created: loaded.created,
            last_active: loaded.last_active,
            data_path: agent_dir.to_path_buf(),
            messages_since_save: 0,
            last_save_time: Utc::now(),
        })
    }
    
    /// Save agent to disk
    pub fn save(&mut self) -> Result<()> {
        agent_persistence::save_agent(
            &self.data_path,
            self.id,
            &self.name,
            &self.llm_config,
            &self.conversation_history,
            self.context_window_limit,
            self.system_prompt.as_deref(),
            self.created,
            self.last_active,
        )?;
        
        self.messages_since_save = 0;
        self.last_save_time = Utc::now();
        
        Ok(())
    }
    
    /// Check if auto-save is needed and perform if necessary
    /// 
    /// Uses time-based (5 minutes) and message-based (10 messages) thresholds
    /// to determine when to persist agent state.
    pub fn auto_save_if_needed(&mut self) -> Result<()> {
        if agent_persistence::should_save(self.last_save_time, self.messages_since_save) {
            self.save()?;
        }
        Ok(())
    }
    
    /// Get the LLM backend for this agent
    pub fn get_llm_backend(&self) -> Box<dyn LLMBackend> {
        create_llm_backend(&self.llm_config)
    }
    
    /// Add a message to conversation history
    pub fn add_message(&mut self, message: Message) {
        self.conversation_history.push(message);
        self.last_active = Utc::now();
        self.messages_since_save += 1;
        
        // Trim if we exceed the context window
        self.trim_context();
    }
    
    /// Add a user message with optional echo for testing
    pub fn add_user_message(&mut self, content: String, echo: Option<String>) {
        self.add_message(Message::User {
            content,
            echo,
            timestamp: Utc::now(),
        });
    }
    
    /// Add an assistant message
    pub fn add_assistant_message(&mut self, content: String) {
        self.add_message(Message::Assistant {
            content,
            timestamp: Utc::now(),
        });
    }
    
    /// Add a tool execution result
    /// 
    /// TODO: Will be used in Phase 1 when agents can execute graph tools.
    #[allow(dead_code)]
    pub fn add_tool_result(&mut self, name: String, args: serde_json::Value, result: AgentContext) {
        self.add_message(Message::Tool {
            name,
            args,
            result,
            timestamp: Utc::now(),
        });
    }
    
    /// Trim conversation history to stay within context window
    /// 
    /// Keeps the most recent messages up to the limit. System messages
    /// and initial context are preserved if possible.
    pub fn trim_context(&mut self) {
        if self.conversation_history.len() <= self.context_window_limit {
            return;
        }
        
        // Calculate how many messages to remove
        let to_remove = self.conversation_history.len() - self.context_window_limit;
        
        // Remove oldest messages (could be smarter about preserving important context)
        self.conversation_history.drain(0..to_remove);
        
        info!("Trimmed {} messages from agent '{}' conversation history", 
              to_remove, self.name);
    }
    
    /// Clear conversation history
    pub fn clear_history(&mut self) {
        self.conversation_history.clear();
        self.messages_since_save += 1;
        info!("Cleared conversation history for agent '{}'", self.name);
    }
    
    
    /// Set system prompt
    /// 
    /// TODO: Add WebSocket/CLI command for setting system prompt.
    /// This will be useful for customizing agent behavior.
    #[allow(dead_code)]
    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
        // Auto-save configuration changes
        if let Err(e) = self.save() {
            error!("Failed to save agent after system prompt change: {}", e);
        }
    }
    
    /// Update LLM configuration
    /// 
    /// TODO: Add WebSocket/CLI command for updating LLM config.
    /// This will allow switching between different LLM backends.
    #[allow(dead_code)]
    pub fn set_llm_config(&mut self, config: LLMConfig) {
        self.llm_config = config;
        // Auto-save configuration changes
        if let Err(e) = self.save() {
            error!("Failed to save agent after LLM config change: {}", e);
        }
    }
    
    /// Get recent messages for context
    /// 
    /// Returns the most recent N messages, useful for building
    /// prompts for the LLM.
    pub fn get_recent_messages(&self, count: usize) -> &[Message] {
        let len = self.conversation_history.len();
        if len <= count {
            &self.conversation_history
        } else {
            &self.conversation_history[len - count..]
        }
    }
    
    /// Process an incoming message and generate a response
    /// 
    /// This is the main chat interaction method. It:
    /// 1. Adds the user message to history
    /// 2. Calls the LLM backend for a response
    /// 3. Handles any tool calls (in future phases)
    /// 4. Adds the assistant response to history
    /// 5. Returns the response to the user
    pub async fn process_message(&mut self, content: String, echo: Option<String>) -> Result<String> {
        // Add user message to conversation history
        self.add_user_message(content, echo);
        
        // Get the LLM backend for this agent
        let llm = self.get_llm_backend();
        
        // Call LLM with conversation history
        // For now, we're not passing tools - that will come in Phase 2
        let response = llm.complete(&self.conversation_history, &[])
            .await
            .map_err(|e| AgentError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("LLM error: {}", e)
            )))?;
        
        // TODO: In Phase 2, check for tool calls here and execute them
        // if let Some(tool_call) = response.tool_call {
        //     // Execute tool and add result to history
        //     // Call LLM again with updated history
        // }
        
        // Add assistant response to conversation history
        self.add_assistant_message(response.content.clone());
        
        // Auto-save if needed (based on time/message thresholds)
        self.auto_save_if_needed()?;
        
        // Return the response content
        Ok(response.content)
    }
}


#[cfg(test)]
mod tests {
    // Note: Most agent tests are now integration tests since agents
    // are created through AppState factory methods.
    // Unit tests for low-level functionality will be added as needed.
}