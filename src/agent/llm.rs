//! LLM Backend Abstraction Layer
//!
//! This module provides the trait and configuration for different LLM backends.
//! It enables agents to use various LLM providers (Mock, Ollama, OpenAI, etc.)
//! while maintaining a consistent interface.
//!
//! ## Design Philosophy
//!
//! Each agent owns its LLM configuration, which is persisted as part of the agent state.
//! This allows different agents to use different models or providers, enabling:
//! - Testing with MockLLM while production uses real providers
//! - Specialized agents with different models for different tasks
//! - Seamless migration between providers without code changes
//!
//! ## Backend Types
//!
//! ### MockLLM
//! The default backend for testing and development. Provides deterministic responses
//! through an echo mechanism where tests can specify exact responses. This ensures
//! full testability of agent interactions without external dependencies.
//!
//! ### Future Backends
//! The architecture supports additional backends through the LLMBackend trait.
//! Each backend implementation handles its own connection management, error handling,
//! and response formatting while maintaining the common interface.
//!
//! ## Conversation Management
//!
//! The module defines a comprehensive Message enum that captures the full context
//! of agent conversations. Messages include User inputs with optional echo responses
//! for testing, Assistant completions with LLM-generated content, and Tool execution
//! records with parameters and results. This enables complete conversation replay
//! and context preservation across agent sessions.
//!
//! ## Tool Integration
//!
//! The LLM backend interfaces with Cymbiont's tool system through ToolDefinition
//! schemas that describe available graph operations. Future implementations will
//! support function calling where LLMs can request specific tool executions based
//! on conversation context, enabling autonomous agent behavior with knowledge graph
//! manipulation capabilities.
//!
//! ## Persistence Integration
//!
//! Agent configurations and conversation histories are automatically persisted
//! through the storage layer. The LLMConfig enum serializes cleanly to JSON,
//! allowing agents to maintain their model preferences across restarts. Auto-save
//! thresholds ensure conversation data is preserved without excessive disk I/O.
//!
//! ## Testing Strategy
//!
//! The MockLLM implementation provides deterministic behavior for integration tests.
//! Tests can specify exact responses through the echo mechanism, ensuring predictable
//! agent behavior without external LLM dependencies. This enables comprehensive
//! testing of agent workflows, conversation management, and tool integration.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::schemas::ToolDefinition;
use crate::error::*;



/// Configuration for different LLM backends
/// 
/// This enum is serialized as part of agent persistence, allowing
/// each agent to maintain its own model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LLMConfig {
    /// Mock backend for testing
    Mock {
        /// Default response when no echo is provided
        #[serde(default = "default_mock_response")]
        default_response: String,
    },
    
    /// Ollama backend for local inference
    Ollama {
        /// Model name (e.g., "llama3.2", "mistral")
        model: String,
        /// API endpoint (e.g., "http://localhost:11434")
        endpoint: String,
        /// Temperature for response generation (0.0 to 1.0)
        #[serde(default = "default_temperature")]
        temperature: f32,
        /// Maximum tokens to generate
        #[serde(default = "default_max_tokens")]
        max_tokens: usize,
    },
    
    // Future backends can be added here:
    // OpenAI { api_key: String, model: String, ... }
    // Anthropic { api_key: String, model: String, ... }
}

fn default_mock_response() -> String {
    "I'll help you with that task.".to_string()
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> usize {
    2048
}

impl Default for LLMConfig {
    fn default() -> Self {
        // Default to mock for testing
        LLMConfig::Mock {
            default_response: default_mock_response(),
        }
    }
}

/// Message types in conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    User {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        echo: Option<String>,  // Test-only: force MockLLM to echo this response
        #[serde(with = "chrono::serde::ts_seconds")]
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Assistant {
        content: String,
        #[serde(with = "chrono::serde::ts_seconds")]
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Tool {
        name: String,
        args: Value,
        result: AgentContext,
        #[serde(with = "chrono::serde::ts_seconds")]
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

/// Context returned from tool executions
/// 
/// This structure provides a consistent format for tool results,
/// making it easy for the LLM to understand what happened and
/// for the conversation history to track operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Whether the operation succeeded
    pub success: bool,
    /// Human-readable message about the operation
    pub message: String,
    /// Optional data returned by queries
    pub data: Option<Value>,
}

/// Response from LLM completion
#[derive(Debug, Clone)]
pub struct LLMResponse {
    /// The text response from the model
    pub content: String,
    /// Optional tool call request
    /// 
    /// TODO: Will be used in Phase 1 when LLMs can request tool executions.
    /// The agent will parse this and execute the requested graph operation.
    #[allow(dead_code)]
    pub tool_call: Option<ToolCall>,
}

/// Tool call request from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call
    pub name: String,
    /// Arguments for the tool
    pub arguments: Value,
}

/// Trait for LLM backend implementations
/// 
/// All LLM providers must implement this trait to be usable by agents.
/// The trait is async to support network calls to remote models.
#[async_trait]
pub trait LLMBackend: Send + Sync {
    /// Generate a completion given conversation history and available tools
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LLMResponse>;
    
    /// Check if the backend is available and responsive
    /// 
    /// TODO: Will be used to verify LLM connectivity before operations
    /// in Phase 1 when we integrate real LLM backends.
    #[allow(dead_code)]
    async fn health_check(&self) -> Result<bool>;
}

/// Create an LLM backend from configuration
/// 
/// This factory function creates the appropriate backend implementation
/// based on the provided configuration.
pub fn create_llm_backend(config: &LLMConfig) -> Box<dyn LLMBackend> {
    match config {
        LLMConfig::Mock { default_response } => {
            Box::new(MockLLM {
                default_response: default_response.clone(),
            })
        }
        LLMConfig::Ollama { .. } => {
            // For now, return mock since Ollama is deferred
            // TODO: Implement OllamaLLM when ready
            Box::new(MockLLM {
                default_response: "Ollama backend not yet implemented, using mock".to_string(),
            })
        }
    }
}

/// Mock LLM implementation for testing
/// 
/// Provides deterministic responses for testing agent functionality
/// without requiring a real LLM connection.
pub struct MockLLM {
    default_response: String,
}

// Note: MockLLM instances are created via create_llm_backend() using LLMConfig

#[async_trait]
impl LLMBackend for MockLLM {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LLMResponse> {
        // For testing: look at the last user message
        if let Some(Message::User { content, echo, .. }) = messages.last() {
            // First priority: if echo is provided, use it
            if let Some(echo_response) = echo {
                return Ok(LLMResponse {
                    content: echo_response.clone(),
                    tool_call: None,
                });
            }
            
            // Second priority: check if any tool name is mentioned
            for tool in tools {
                if content.to_lowercase().contains(&tool.name) {
                    // Return a tool call response
                    return Ok(LLMResponse {
                        content: format!("I'll use the {} tool for you.", tool.name),
                        tool_call: Some(ToolCall {
                            name: tool.name.clone(),
                            arguments: serde_json::json!({}), // Mock arguments
                        }),
                    });
                }
            }
            
            // No more predefined responses - just echo or default
        }
        
        // Default: return default response
        Ok(LLMResponse {
            content: self.default_response.clone(),
            tool_call: None,
        })
    }
    
    async fn health_check(&self) -> Result<bool> {
        // Mock is always healthy
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_mock_llm_default_response() {
        let config = LLMConfig::Mock {
            default_response: "Test response".to_string(),
        };
        let llm = create_llm_backend(&config);
        let messages = vec![
            Message::User {
                content: "Hello".to_string(),
                echo: None,
                timestamp: Utc::now(),
            }
        ];
        
        let response = llm.complete(&messages, &[]).await.unwrap();
        assert_eq!(response.content, "Test response");
        assert!(response.tool_call.is_none());
    }

    #[tokio::test]
    async fn test_mock_llm_tool_detection() {
        let config = LLMConfig::Mock {
            default_response: "Default".to_string(),
        };
        let llm = create_llm_backend(&config);
        let messages = vec![
            Message::User {
                content: "Please add_block with content 'test'".to_string(),
                echo: None,
                timestamp: Utc::now(),
            }
        ];
        
        let tools = vec![
            ToolDefinition {
                name: "add_block".to_string(),
                description: "Add a block".to_string(),
                parameters: crate::agent::schemas::ParameterSchema {
                    schema_type: "object".to_string(),
                    properties: HashMap::new(),
                    required: vec![],
                },
            }
        ];
        
        let response = llm.complete(&messages, &tools).await.unwrap();
        assert!(response.content.contains("add_block"));
        assert!(response.tool_call.is_some());
        assert_eq!(response.tool_call.unwrap().name, "add_block");
    }

    #[test]
    fn test_llm_config_serialization() {
        let config = LLMConfig::Mock {
            default_response: "Default".to_string(),
        };
        
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: LLMConfig = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            LLMConfig::Mock { default_response } => {
                assert_eq!(default_response, "Default");
            }
            _ => panic!("Wrong config type"),
        }
    }

    #[test]
    fn test_message_serialization() {
        // Test User message
        let user_msg = Message::User {
            content: "Hello".to_string(),
            echo: None,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&user_msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        match deserialized {
            Message::User { content, .. } => assert_eq!(content, "Hello"),
            _ => panic!("Wrong message type"),
        }

        // Test Assistant message
        let assistant_msg = Message::Assistant {
            content: "Hi there".to_string(),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&assistant_msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        match deserialized {
            Message::Assistant { content, .. } => assert_eq!(content, "Hi there"),
            _ => panic!("Wrong message type"),
        }

        // Test Tool message
        let tool_msg = Message::Tool {
            name: "add_block".to_string(),
            args: serde_json::json!({"content": "test"}),
            result: AgentContext {
                success: true,
                message: "Created".to_string(),
                data: Some(serde_json::json!({"id": "123"})),
            },
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&tool_msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        match deserialized {
            Message::Tool { name, result, .. } => {
                assert_eq!(name, "add_block");
                assert!(result.success);
                assert_eq!(result.message, "Created");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_agent_context_serialization() {
        let context = AgentContext {
            success: true,
            message: "Operation completed".to_string(),
            data: Some(serde_json::json!({"result": "test"})),
        };
        
        let json = serde_json::to_string(&context).unwrap();
        let deserialized: AgentContext = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.success, true);
        assert_eq!(deserialized.message, "Operation completed");
        assert!(deserialized.data.is_some());
    }
}