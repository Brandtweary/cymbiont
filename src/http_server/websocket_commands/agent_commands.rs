//! Agent-related WebSocket command handlers
//!
//! This module implements WebSocket commands for agent interactions including:
//! - Chat messaging with the agent
//! - Conversation history retrieval  
//! - Agent reset functionality
//! - Agent information queries
//!
//! ## Command Flow
//!
//! All agent commands follow a similar pattern:
//! 1. Validate authentication status of the WebSocket connection
//! 2. Extract and validate command parameters
//! 3. Execute operation via CQRS command queue or direct agent methods
//! 4. Format and send response back through WebSocket channel
//!
//! ## AgentChat Command
//!
//! The main interface for agent interaction. Supports both normal chat and test modes:
//! - Normal mode: Sends message to configured LLM backend for processing
//! - Echo mode: Forces specific text response (for testing)
//! - Echo tool mode: Forces tool execution with name and args (MockLLM only)
//!
//! Example:
//! ```json
//! {
//!   "type": "AgentChat",
//!   "message": "What graphs are available?",
//!   "echo": null,
//!   "echo_tool": null
//! }
//! ```
//!
//! ## AgentHistory Command
//!
//! Retrieves conversation history with optional limit:
//! ```json
//! {
//!   "type": "AgentHistory",
//!   "limit": 10
//! }
//! ```
//!
//! Returns array of Message objects with User, Assistant, and Tool roles.
//!
//! ## AgentReset Command
//!
//! Clears the conversation history while preserving agent configuration:
//! ```json
//! {
//!   "type": "AgentReset"
//! }
//! ```
//!
//! ## AgentInfo Command
//!
//! Returns detailed agent information including:
//! - Agent ID and name
//! - Creation and update timestamps
//! - Message count statistics
//! - Current LLM configuration
//!
//! ## Error Handling
//!
//! All commands return standardized error responses for:
//! - Unauthorized access (connection not authenticated)
//! - Missing or invalid parameters
//! - Agent operation failures
//! - Serialization errors

use crate::agent::agent::process_agent_message;
use crate::agent::llm::Message as LlmMessage;
use crate::cqrs::{AgentCommand, Command as CqrsCommand};
use crate::error::{Result, ServerError};
use crate::http_server::websocket::Command;
use crate::http_server::websocket_utils::{send_error_response, send_success_response};
use crate::utils::AsyncRwLockExt;
use crate::AppState;
use serde_json::json;
use std::sync::Arc;
use tracing::error;

// ===== Individual Command Handlers =====

async fn handle_agent_chat(
    connection_id: &str,
    state: &Arc<AppState>,
    message: String,
    echo: Option<String>,
    echo_tool: Option<String>,
) -> Result<()> {
    // Check if the single agent exists
    let agent_guard = state.agent.read_or_panic("read agent").await;
    if agent_guard.is_none() {
        drop(agent_guard);
        send_error_response(connection_id, state, "No agent available for chat").await?;
        return Ok(());
    }
    drop(agent_guard);

    // Send immediate ACK response
    send_success_response(
        connection_id,
        state,
        Some(json!({
            "status": "processing"
        })),
    )
    .await?;

    // Spawn background task for LLM processing
    let state_clone = state.clone();
    let conn_id = connection_id.to_string();
    tokio::spawn(async move {
        // Process the message (all 4 phases encapsulated)
        match process_agent_message(&state_clone, message, echo, echo_tool).await {
            Ok(response) => {
                // Send final response via WebSocket
                let response_data = json!({
                    "response": response
                });
                if let Err(e) =
                    send_success_response(&conn_id, &state_clone, Some(response_data)).await
                {
                    error!("Failed to send agent response: {}", e);
                }
            }
            Err(e) => {
                // Send error response
                if let Err(send_err) =
                    send_error_response(&conn_id, &state_clone, &e.to_string()).await
                {
                    error!("Failed to send error response: {}", send_err);
                }
            }
        }
    });

    Ok(())
}

async fn handle_agent_history(
    connection_id: &str,
    state: &Arc<AppState>,
    limit: Option<usize>,
) -> Result<()> {
    // Get conversation history from the single agent
    let history = {
        let agent_guard = state
            .agent
            .read_or_panic("agent history - read agent")
            .await;
        if let Some(ref agent) = *agent_guard {
            let messages = limit.map_or(agent.conversation_history.as_slice(), |limit| {
                agent.get_recent_messages(limit)
            });

            // Convert messages to JSON format
            let history = messages
                .iter()
                .map(|msg| match msg {
                    LlmMessage::User {
                        content, timestamp, ..
                    } => {
                        serde_json::json!({
                            "role": "user",
                            "content": content,
                            "timestamp": timestamp.to_rfc3339()
                        })
                    }
                    LlmMessage::Assistant { content, timestamp } => {
                        serde_json::json!({
                            "role": "assistant",
                            "content": content,
                            "timestamp": timestamp.to_rfc3339()
                        })
                    }
                    LlmMessage::Tool {
                        name,
                        args,
                        result,
                        timestamp,
                    } => {
                        serde_json::json!({
                            "role": "tool",
                            "name": name,
                            "args": args,
                            "result": result,
                            "timestamp": timestamp.to_rfc3339()
                        })
                    }
                })
                .collect::<Vec<_>>();

            history
        } else {
            send_error_response(connection_id, state, "No agent available").await?;
            return Ok(());
        }
    };

    let data = serde_json::json!({
        "messages": history
    });

    send_success_response(connection_id, state, Some(data)).await
}

async fn handle_agent_reset(connection_id: &str, state: &Arc<AppState>) -> Result<()> {
    // Clear conversation history
    let command = CqrsCommand::Agent(AgentCommand::ClearHistory);
    state.command_queue.execute(command).await?;

    let data = serde_json::json!({
        "success": true
    });

    send_success_response(connection_id, state, Some(data)).await
}

async fn handle_agent_info(connection_id: &str, state: &Arc<AppState>) -> Result<()> {
    // Get info about the single agent
    let agent_guard = state.agent.read_or_panic("agent info - read agent").await;
    if let Some(ref agent) = *agent_guard {
        let conversation_stats = serde_json::json!({
            "message_count": agent.conversation_history.len(),
            "llm_config": agent.llm_config,
        });

        let data = serde_json::json!({
            "name": agent.name,
            "is_active": true,
            "created": agent.created,
            "conversation_stats": conversation_stats,
        });

        send_success_response(connection_id, state, Some(data)).await
    } else {
        send_error_response(connection_id, state, "No agent available").await
    }
}

/// Main handler function for agent commands - routes to individual handlers
pub async fn handle(command: Command, connection_id: &str, state: &Arc<AppState>) -> Result<()> {
    match command {
        Command::AgentChat {
            message,
            echo,
            echo_tool,
        } => handle_agent_chat(connection_id, state, message, echo, echo_tool).await,

        Command::AgentHistory { limit } => handle_agent_history(connection_id, state, limit).await,

        Command::AgentReset => handle_agent_reset(connection_id, state).await,

        Command::AgentInfo => handle_agent_info(connection_id, state).await,

        _ => {
            // This shouldn't happen if routing is correct
            Err(ServerError::websocket("Command routed to wrong handler").into())
        }
    }
}
