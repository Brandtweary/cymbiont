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
//! The Agent struct owns its complete state. It acts as a stateful context manager, bundling:
//! - **Conversation History**: Full chat context with sliding window management
//! - **LLM Configuration**: Model selection, parameters, and backend settings
//! - **System Prompts**: Custom instructions and behavioral guidelines
//! - **Context Management**: Automatic history trimming and continuity preservation
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
//!
//! ### Deactivation Phase
//! - Removed from AppState's active agents HashMap
//! - AgentRegistry updated to mark agent as "inactive" 
//! - Memory freed
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
//! 6. **Context Trimming**: Window size enforced if necessary
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
//! - **LLM Errors**: Backend communication failures with auto-retry
//! - **Authorization Errors**: Graph access denied
//! - **Configuration Errors**: Invalid LLM settings
//!
//! Recovery strategies include graceful degradation when backends unavailable
//! and conversation history preservation during errors.
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
//! custom message types, and custom context window management algorithms.
//!
//! ## Performance Considerations
//!
//! The agent system is optimized for responsive interaction:
//! - Active agents loaded in memory for fast access
//! - Context window limits prevent unbounded memory growth
//! - Agent state protected by async RwLock in AppState
//! - Thread-safe message processing

use std::path::Path;
use std::fs;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;
use serde_json::Value;

use crate::agent::llm::{LLMConfig, LLMBackend, LLMResponse, Message, AgentContext, create_llm_backend};
use crate::agent::kg_tools;
use crate::app_state::AppState;
use crate::error::*;
use crate::storage::{TransactionCoordinator, Operation};
use crate::storage::transaction_log::AgentOperation;

/// Context for LLM processing without holding agent locks
pub struct LLMContext {
    pub conversation_history: Vec<Message>,
    pub llm_backend: Box<dyn LLMBackend>,
}

/// Result of tool execution for Phase 4 processing
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub name: String,
    pub arguments: Value,
    pub result: AgentContext,
}

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
    
    /// Transaction coordinator for WAL operations
    pub transaction_coordinator: Arc<TransactionCoordinator>,
}

impl Agent {
    /// Create a new agent with the given configuration
    pub fn new(
        id: Uuid,
        name: String,
        llm_config: LLMConfig,
        system_prompt: Option<String>,
        transaction_coordinator: Arc<TransactionCoordinator>,
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
            transaction_coordinator,
        }
    }
    
    /// Create empty agent for WAL rebuild
    /// 
    /// This creates a minimal agent that will be populated through WAL replay.
    /// Used during system startup to rebuild from transaction log.
    pub fn new_empty(
        id: Uuid, 
        name: String, 
        transaction_coordinator: Arc<TransactionCoordinator>
    ) -> Self {
        let now = Utc::now();
        
        Agent {
            id,
            name,
            llm_config: LLMConfig::default(),
            conversation_history: Vec::new(),
            context_window_limit: 100,
            system_prompt: None,
            default_graph_id: None,
            created: now,
            last_active: now,
            transaction_coordinator,
        }
    }
    
    /// Load agent from JSON (legacy path, will be removed)
    /// 
    /// Agent state will be rebuilt from WAL replay.
    pub fn load(_agent_dir: &Path, transaction_coordinator: Arc<TransactionCoordinator>) -> Result<Self> {
        // Create minimal agent - state rebuilt from WAL
        Ok(Agent {
            id: Uuid::new_v4(), // Overwritten during recovery
            name: String::new(),
            llm_config: LLMConfig::default(),
            conversation_history: Vec::new(),
            context_window_limit: 100,
            system_prompt: None,
            default_graph_id: None,
            created: Utc::now(),
            last_active: Utc::now(),
            transaction_coordinator,
        })
    }
    
    
    /// Get the LLM backend for this agent
    pub fn get_llm_backend(&self) -> Box<dyn LLMBackend> {
        create_llm_backend(&self.llm_config)
    }
    
    /// Add a message to conversation history
    pub async fn add_message(&mut self, message: Message, skip_wal: bool) -> Result<()> {
        // Create WAL operation conditionally
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Agent(AgentOperation::AddMessage {
                agent_id: self.id,
                message: serde_json::to_value(&message)?,
            }))
        };
        
        // Execute with transaction
        let coordinator = self.transaction_coordinator.clone();
        let tx = coordinator.begin(operation).await?;
        
        // Do the actual work
        self.conversation_history.push(message);
        self.last_active = Utc::now();
        
        // Trim if we exceed the context window
        self.trim_context();
        
        // Commit the transaction
        tx.commit().await
    }
    
    /// Add a user message with optional echo for testing
    pub async fn add_user_message(&mut self, content: String, echo: Option<String>, echo_tool: Option<String>) -> Result<()> {
        self.add_message(Message::User {
            content,
            echo,
            echo_tool,
            timestamp: Utc::now(),
        }, false).await
    }
    
    /// Add an assistant message
    pub async fn add_assistant_message(&mut self, content: String) -> Result<()> {
        self.add_message(Message::Assistant {
            content,
            timestamp: Utc::now(),
        }, false).await
    }
    
    /// Add a tool execution result
    /// 
    /// Formats the tool result in a way that helps the LLM understand
    /// what happened and continue the conversation appropriately.
    pub async fn add_tool_result(&mut self, name: String, args: serde_json::Value, result: AgentContext) -> Result<()> {
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
        }, false).await
    }
    
    /// Trim conversation history to stay within context window (internal helper)
    /// 
    /// Keeps the most recent messages up to the limit. System messages
    /// and initial context are preserved if possible.
    /// This is called automatically by add_message when needed.
    fn trim_context(&mut self) {
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
    
    /// Explicitly trim conversation history to stay within context window
    
    /// Clear conversation history
    pub async fn clear_history(&mut self, skip_wal: bool) -> Result<()> {
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Agent(AgentOperation::ClearHistory {
                agent_id: self.id,
            }))
        };
        
        let coordinator = self.transaction_coordinator.clone();
        let tx = coordinator.begin(operation).await?;
        
        // Do the actual work
        self.conversation_history.clear();
        info!("Cleared conversation history for agent '{}'", self.name);
        
        // Commit the transaction
        tx.commit().await
    }
    
    
    /// Get the agent's default graph ID
    pub fn get_default_graph_id(&self) -> Option<Uuid> {
        self.default_graph_id
    }
    
    /// Set the agent's default graph ID
    /// 
    /// This is typically called when the agent is first authorized for a graph,
    /// or when the agent explicitly switches its default using the set_default_graph tool.
    pub async fn set_default_graph_id(&mut self, graph_id: Option<Uuid>, skip_wal: bool) -> Result<()> {
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Agent(AgentOperation::SetDefaultGraph {
                agent_id: self.id,
                graph_id,
            }))
        };
        
        let coordinator = self.transaction_coordinator.clone();
        let tx = coordinator.begin(operation).await?;
        
        // Do the actual work
        self.default_graph_id = graph_id;
        
        // Commit the transaction
        tx.commit().await
    }
    
    /// Set system prompt
    /// 
    /// TODO: Add WebSocket/CLI command for setting system prompt.
    /// This will be useful for customizing agent behavior.
    #[allow(dead_code)]
    pub async fn set_system_prompt(&mut self, prompt: String, skip_wal: bool) -> Result<()> {
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Agent(AgentOperation::SetSystemPrompt {
                agent_id: self.id,
                prompt: prompt.clone(),
            }))
        };
        
        let coordinator = self.transaction_coordinator.clone();
        let tx = coordinator.begin(operation).await?;
        
        // Do the actual work
        self.system_prompt = Some(prompt);
        
        // Commit the transaction
        tx.commit().await
    }
    
    /// Update LLM configuration
    /// 
    /// TODO: Add WebSocket/CLI command for updating LLM config.
    /// This will allow switching between different LLM backends.
    #[allow(dead_code)]
    pub async fn set_llm_config(&mut self, config: LLMConfig, skip_wal: bool) -> Result<()> {
        let operation = if skip_wal {
            None
        } else {
            Some(Operation::Agent(AgentOperation::SetLLMConfig {
                agent_id: self.id,
                config: serde_json::to_value(&config)?,
            }))
        };
        
        let coordinator = self.transaction_coordinator.clone();
        let tx = coordinator.begin(operation).await?;
        
        // Do the actual work
        self.llm_config = config;
        
        // Commit the transaction
        tx.commit().await
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
    
    /// Phase 1: Add user message and prepare context (brief agent lock)
    /// 
    /// Returns LLM context needed for Phase 2 processing without holding locks
    pub async fn add_user_message_and_get_context(&mut self, content: String, echo: Option<String>, echo_tool: Option<String>) -> Result<LLMContext> {
        // Add user message to conversation history
        self.add_user_message(content, echo, echo_tool.clone()).await?;
        
        // Get LLM backend while we have the agent lock
        let llm_backend = self.get_llm_backend();
        
        // Return context needed for LLM processing (no locks held)
        Ok(LLMContext {
            conversation_history: self.conversation_history.clone(),
            llm_backend,
        })
    }
    
    /// Phase 2: Get LLM response (no agent locks held)
    /// 
    /// Static method that doesn't require agent access - prevents lock contention
    pub async fn get_llm_response(context: LLMContext) -> Result<LLMResponse> {
        // Use the LLM backend from context (obtained during Phase 1 while holding agent lock)
        let llm = context.llm_backend;
        
        // Get tool schemas
        let tool_schemas = kg_tools::get_tool_schemas();
        
        // Call LLM with conversation history and tools
        let response = llm.complete(&context.conversation_history, &tool_schemas)
            .await
            .map_err(|e| AgentError::llm(format!("LLM backend error: {}", e)))?;
        
        Ok(response)
    }
    
    /// Phase 4: Add response to conversation and return final content (brief agent lock)
    /// 
    /// Adds assistant response and any tool results to conversation history.
    /// Also handles setting default graph if agent authorized a new graph.
    pub async fn add_response_to_conversation(&mut self, response: LLMResponse, tool_results: Vec<ToolResult>) -> Result<String> {
        // Check if any tool authorized a graph and this agent needs a default
        // This was moved here from agent_registry.rs to avoid deadlock
        if self.default_graph_id.is_none() {
            for tool_result in &tool_results {
                if tool_result.name == "create_graph" || tool_result.name == "authorize_agent" {
                    // Check if result contains a graph_id
                    if let Some(data) = &tool_result.result.data {
                        if let Some(graph_id) = data.get("graph_id").and_then(|v| v.as_str()) {
                            if let Ok(graph_uuid) = uuid::Uuid::parse_str(graph_id) {
                                // Set as default since agent has no default yet
                                self.set_default_graph_id(Some(graph_uuid), false).await?;
                                break; // Only set the first one
                            }
                        }
                    }
                }
            }
        }
        
        // Add tool results to conversation BEFORE assistant message
        for tool_result in tool_results {
            self.add_tool_result(tool_result.name, tool_result.arguments, tool_result.result).await?;
        }
        
        // Add assistant response to conversation history AFTER any tool results
        self.add_assistant_message(response.content.clone()).await?;
        
        // Return the response content
        Ok(response.content)
    }
    
    /// Export the agent to JSON for debugging/inspection
    /// 
    /// Note: This is NOT for persistence - WAL is the source of truth
    /// The test harness (tests/common/agent_validation.rs) reads this file for validation
    pub fn export_json(&self, path: &Path) -> Result<()> {
        // Create a serializable version without transaction_coordinator
        let data = serde_json::json!({
            "version": 1,
            "id": self.id,
            "name": self.name,
            "llm_config": self.llm_config,
            "conversation_history": self.conversation_history,
            "context_window_limit": self.context_window_limit,
            "system_prompt": self.system_prompt,
            "default_graph_id": self.default_graph_id,
            "created": self.created,
            "last_active": self.last_active,
        });
        
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| AgentError::serialization(format!("Failed to serialize agent: {}", e)))?;
        
        fs::write(path, json)
            .map_err(|e| AgentError::serialization(format!("Failed to write agent JSON: {}", e)))?;
        
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    // Note: Most agent tests are now integration tests since agents
    // are created through AppState factory methods.
    // Unit tests for low-level functionality will be added as needed.
}