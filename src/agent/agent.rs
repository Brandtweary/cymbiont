//! Agent Core Implementation
//!
//! This module provides the main Agent struct that manages conversation state,
//! LLM configuration, and interaction with the knowledge graph. Each agent
//! maintains its own conversation history, model configuration, and graph
//! authorizations.
//!
//! ## Architecture
//!
//! The Agent struct owns its full state and handles persistence through the
//! agent_persistence module. It acts as a context manager, bundling:
//! - Conversation history with tool results
//! - LLM configuration (model, parameters)
//! - System prompts and instructions
//! - Context window management
//!
//! Agents do not directly track graph authorizations - this is managed by the
//! AgentRegistry as the single source of truth to prevent synchronization issues.
//!
//! ## Lifecycle
//!
//! Agents follow a clear lifecycle pattern:
//! 1. **Creation**: AgentRegistry creates metadata, Agent struct instantiated
//! 2. **Activation**: Agent loaded into memory from disk
//! 3. **Interaction**: Chat messages processed, history maintained
//! 4. **Auto-save**: Triggered by time (5min) or message count (10)
//! 5. **Deactivation**: Agent saved to disk and removed from memory
//! 6. **Deletion**: Agent archived to `archived_agents/` directory
//!
//! The prime agent is auto-created on first run and cannot be deleted,
//! ensuring a seamless user experience with always-available assistance.
//!
//! ## Conversation Management
//!
//! The agent maintains a sliding context window of conversation history.
//! When the context limit is reached, older messages are trimmed while
//! preserving conversation continuity. Each message in the history includes:
//! - Role (User, Assistant, Tool)
//! - Content (text or structured data)
//! - Timestamp
//! - Optional metadata
//!
//! ## Persistence
//!
//! Agent state is persisted to `{data_dir}/agents/{agent-id}/agent.json`.
//! This includes the full conversation history, LLM configuration, and
//! system prompts. Auto-save ensures data durability without manual intervention.

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