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
use std::sync::Arc;
use chrono::{DateTime, Utc};
use tracing::{info, error};
use uuid::Uuid;
use serde_json::Value;

use crate::storage::agent_persistence;
use crate::agent::llm::{LLMConfig, LLMBackend, Message, AgentContext, create_llm_backend};
use crate::agent::kg_tools;
use crate::app_state::AppState;
use crate::error::*;

/// Tool argument validation result with detailed error information
#[derive(Debug)]
struct ValidationError {
    field: String,
    issue: ValidationIssue,
}

#[derive(Debug)]
enum ValidationIssue {
    MissingRequired,
    WrongType { expected: String, got: String },
    InvalidUuid(String),
    InvalidValue(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.issue {
            ValidationIssue::MissingRequired => {
                write!(f, "Required field '{}' is missing", self.field)
            }
            ValidationIssue::WrongType { expected, got } => {
                write!(f, "Field '{}' has wrong type: expected {}, got {}", 
                       self.field, expected, got)
            }
            ValidationIssue::InvalidUuid(msg) => {
                write!(f, "Field '{}' has invalid UUID: {}", self.field, msg)
            }
            ValidationIssue::InvalidValue(msg) => {
                write!(f, "Field '{}' has invalid value: {}", self.field, msg)
            }
        }
    }
}

/// Validate tool arguments against the tool's schema
/// 
/// Performs comprehensive validation including:
/// - Required field presence
/// - Type checking
/// - UUID format validation for ID fields
/// - Value range validation where applicable
fn validate_tool_arguments(tool_name: &str, args: &Value) -> std::result::Result<(), String> {
    // Get all tool schemas
    let tools = kg_tools::get_tool_schemas();
    
    // Find the specific tool schema
    let tool = tools.iter()
        .find(|t| t.name == tool_name)
        .ok_or_else(|| format!("Unknown tool: {}", tool_name))?;
    
    // Arguments must be an object
    let args_obj = args.as_object()
        .ok_or_else(|| "Tool arguments must be a JSON object".to_string())?;
    
    let mut errors = Vec::new();
    
    // Check all required fields are present and valid
    for required_field in &tool.parameters.required {
        if let Some(value) = args_obj.get(required_field) {
            // Validate the field type and value
            if let Some(schema) = tool.parameters.properties.get(required_field) {
                if let Err(e) = validate_field_value(required_field, value, &schema.property_type) {
                    errors.push(e);
                }
            }
        } else {
            errors.push(ValidationError {
                field: required_field.clone(),
                issue: ValidationIssue::MissingRequired,
            });
        }
    }
    
    // Check that no unknown fields are present (helps catch typos)
    for (field_name, value) in args_obj {
        if !tool.parameters.properties.contains_key(field_name) {
            // Allow extra fields but log a warning
            tracing::warn!(
                "Unknown field '{}' in arguments for tool '{}' with value: {:?}",
                field_name, tool_name, value
            );
        }
    }
    
    // If there are validation errors, format them nicely
    if !errors.is_empty() {
        let error_messages: Vec<String> = errors.iter()
            .map(|e| e.to_string())
            .collect();
        return Err(error_messages.join("; "));
    }
    
    Ok(())
}

/// Validate a single field value against its expected type
fn validate_field_value(field_name: &str, value: &Value, expected_type: &str) -> std::result::Result<(), ValidationError> {
    match expected_type {
        "string" => {
            if let Some(s) = value.as_str() {
                // Additional validation for UUID fields
                if field_name.ends_with("_id") || field_name == "graph_id" {
                    if Uuid::parse_str(s).is_err() {
                        return Err(ValidationError {
                            field: field_name.to_string(),
                            issue: ValidationIssue::InvalidUuid(format!("'{}' is not a valid UUID", s)),
                        });
                    }
                }
                // Additional validation for non-empty strings
                if field_name == "content" || field_name == "page_name" {
                    if s.trim().is_empty() {
                        return Err(ValidationError {
                            field: field_name.to_string(),
                            issue: ValidationIssue::InvalidValue("Cannot be empty".to_string()),
                        });
                    }
                }
                Ok(())
            } else {
                Err(ValidationError {
                    field: field_name.to_string(),
                    issue: ValidationIssue::WrongType {
                        expected: "string".to_string(),
                        got: value_type_name(value),
                    },
                })
            }
        }
        "number" => {
            if value.is_number() {
                // Additional validation for positive numbers
                if field_name == "max_depth" {
                    if let Some(n) = value.as_u64() {
                        if n == 0 || n > 100 {
                            return Err(ValidationError {
                                field: field_name.to_string(),
                                issue: ValidationIssue::InvalidValue(
                                    "Must be between 1 and 100".to_string()
                                ),
                            });
                        }
                    }
                }
                Ok(())
            } else {
                Err(ValidationError {
                    field: field_name.to_string(),
                    issue: ValidationIssue::WrongType {
                        expected: "number".to_string(),
                        got: value_type_name(value),
                    },
                })
            }
        }
        "object" => {
            if value.is_object() {
                Ok(())
            } else {
                Err(ValidationError {
                    field: field_name.to_string(),
                    issue: ValidationIssue::WrongType {
                        expected: "object".to_string(),
                        got: value_type_name(value),
                    },
                })
            }
        }
        "boolean" => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(ValidationError {
                    field: field_name.to_string(),
                    issue: ValidationIssue::WrongType {
                        expected: "boolean".to_string(),
                        got: value_type_name(value),
                    },
                })
            }
        }
        "array" => {
            if value.is_array() {
                Ok(())
            } else {
                Err(ValidationError {
                    field: field_name.to_string(),
                    issue: ValidationIssue::WrongType {
                        expected: "array".to_string(),
                        got: value_type_name(value),
                    },
                })
            }
        }
        _ => {
            // Unknown type, allow it
            Ok(())
        }
    }
}

/// Get a human-readable name for a JSON value type
fn value_type_name(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}



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
    
    /// Default graph for tool operations (set to first authorized graph)
    pub default_graph_id: Option<Uuid>,
    
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
            default_graph_id: None,  // Will be set when first authorized for a graph
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
            default_graph_id: loaded.default_graph_id,
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
            self.default_graph_id,
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
    pub fn add_user_message(&mut self, content: String, echo: Option<String>, echo_tool: Option<String>) {
        self.add_message(Message::User {
            content,
            echo,
            echo_tool,
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
    /// Formats the tool result in a way that helps the LLM understand
    /// what happened and continue the conversation appropriately.
    pub fn add_tool_result(&mut self, name: String, args: serde_json::Value, result: AgentContext) {
        // Create a formatted result that's easier for LLMs to understand
        let formatted_result = AgentContext {
            success: result.success,
            message: if result.success {
                format!("✓ Tool '{}' executed successfully: {}", name, result.message)
            } else {
                format!("✗ Tool '{}' failed: {}", name, result.message)
            },
            data: result.data,
        };
        
        self.add_message(Message::Tool {
            name,
            args,
            result: formatted_result,
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
    
    
    /// Get the agent's default graph ID
    pub fn get_default_graph_id(&self) -> Option<Uuid> {
        self.default_graph_id
    }
    
    /// Set the agent's default graph ID
    /// 
    /// This is typically called when the agent is first authorized for a graph,
    /// or when the agent explicitly switches its default using the set_default_graph tool.
    pub fn set_default_graph_id(&mut self, graph_id: Option<Uuid>) {
        self.default_graph_id = graph_id;
        // Auto-save configuration changes
        if let Err(e) = self.save() {
            error!("Failed to save agent after default graph change: {}", e);
        }
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
    
    /// Execute a tool from the registry
    /// 
    /// Validates arguments against the tool schema, then calls the tool with
    /// the agent's ID and provided arguments, converting the result to an
    /// AgentContext for conversation tracking.
    pub async fn execute_tool(&mut self, app_state: &Arc<AppState>, tool_name: &str, args: Value) -> Result<AgentContext> {
        
        // First validate the arguments against the tool schema
        if let Err(validation_error) = validate_tool_arguments(tool_name, &args) {
            return Ok(AgentContext {
                success: false,
                message: format!("Tool argument validation failed: {}", validation_error),
                data: None,
            });
        }
        
        let result = kg_tools::execute_tool(app_state, self, tool_name, args).await?;
        
        // Convert result Value to AgentContext
        let context = if let Some(obj) = result.as_object() {
            AgentContext {
                success: obj.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
                message: obj.get("message")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.get("error").and_then(|v| v.as_str()))
                    .unwrap_or("Tool executed")
                    .to_string(),
                data: obj.get("data").cloned(),
            }
        } else {
            AgentContext {
                success: false,
                message: "Invalid tool response format".to_string(),
                data: None,
            }
        };
        
        Ok(context)
    }
    
    /// Process an incoming message and generate a response
    /// 
    /// This is the main chat interaction method. It:
    /// 1. Adds the user message to history
    /// 2. Calls the LLM backend for a response
    /// 3. Handles any tool calls
    /// 4. Adds the assistant response to history
    /// 5. Returns the response to the user
    pub async fn process_message(&mut self, app_state: &Arc<AppState>, content: String, echo: Option<String>, echo_tool: Option<String>) -> Result<String> {
        
        // Add user message to conversation history
        self.add_user_message(content, echo, echo_tool.clone());
        
        // Get the LLM backend for this agent
        let llm = self.get_llm_backend();
        
        // Get tool schemas
        // TODO: Add tool filtering/selection based on context or agent capabilities
        // For now we pass all tools on every call which works but may overwhelm smaller models
        let tool_schemas = kg_tools::get_tool_schemas();
        
        // Call LLM with conversation history and tools
        let response = llm.complete(&self.conversation_history, &tool_schemas)
            .await
            .map_err(|e| AgentError::llm(format!("LLM backend error: {}", e)))?;
        
        
        // Handle tool calls
        if let Some(tool_call) = response.tool_call {
            // Execute the tool and add result to conversation BEFORE the assistant message
            // This matches the realistic flow: user asks → tool executes → assistant responds
            let result = self.execute_tool(app_state, &tool_call.name, tool_call.arguments.clone()).await?;
            self.add_tool_result(tool_call.name, tool_call.arguments, result);
            
            // Note: With MockLLM, the response still says "I'll use the X tool" even though
            // the tool has already been executed. This is a limitation of the test backend.
            // Real LLMs would generate a response that acknowledges the tool result.
        }
        
        // Add assistant response to conversation history AFTER any tool results
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