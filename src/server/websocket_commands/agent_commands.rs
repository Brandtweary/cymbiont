//! @module agent_commands
//! @description Agent-related WebSocket command handlers
//! 
//! This module implements agent-related WebSocket commands for single-agent mode.
//! 
//! ## Command Categories
//! 
//! ### Chat Operations
//! - `AgentChat`: Send messages to the agent and receive LLM responses
//! - `AgentHistory`: Retrieve conversation history with optional limits
//! - `AgentReset`: Clear agent conversation history
//! - `AgentInfo`: Get detailed information about the current agent
//! 
//! ## Key Patterns
//! 
//! ### Single Agent Operations
//! All operations work with the single agent stored in `state.agent` as
//! `Arc<RwLock<Option<Agent>>>`.

use std::sync::Arc;
use crate::error::*;
use crate::AppState;
use crate::agent::llm::Message as LlmMessage;
use crate::agent::agent::process_agent_message;
use crate::cqrs::{Command as CqrsCommand, AgentCommand};
use crate::server::websocket::Command;
use crate::server::websocket_utils::{send_success_response, send_error_response};
use crate::utils::AsyncRwLockExt;
use serde_json::json;

/// Main handler function for agent commands
pub async fn handle(
    command: Command,
    connection_id: &str,
    state: &Arc<AppState>,
) -> Result<()> {
    match command {
        // Agent Commands
        Command::AgentChat { message, echo, echo_tool } => {
            
            // Check if the single agent exists
            let agent_guard = state.agent.read_or_panic("read agent").await;
            if agent_guard.is_none() {
                drop(agent_guard);
                send_error_response(connection_id, state, "No agent available for chat").await?;
                return Ok(());
            }
            drop(agent_guard);
            
            // Send immediate ACK response
            send_success_response(connection_id, state, Some(json!({
                "status": "processing"
            }))).await?;
            
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
                        if let Err(e) = send_success_response(&conn_id, &state_clone, Some(response_data)).await {
                            tracing::error!("Failed to send agent response: {}", e);
                        }
                    }
                    Err(e) => {
                        // Send error response
                        if let Err(send_err) = send_error_response(&conn_id, &state_clone, &e.to_string()).await {
                            tracing::error!("Failed to send error response: {}", send_err);
                        }
                    }
                }
            });
            
            // Return immediately after spawning background task
        }
        
        Command::AgentHistory { limit } => {
            
            // Get conversation history from the single agent
            let history = {
                let agent_guard = state.agent.read_or_panic("agent history - read agent").await;
                if let Some(ref agent) = *agent_guard {
                    let messages = if let Some(limit) = limit {
                        agent.get_recent_messages(limit)
                    } else {
                        &agent.conversation_history
                    };
                    
                    // Convert messages to JSON format
                    let history = messages.iter().map(|msg| {
                        match msg {
                            LlmMessage::User { content, timestamp, .. } => {
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
                            LlmMessage::Tool { name, args, result, timestamp } => {
                                serde_json::json!({
                                    "role": "tool",
                                    "name": name,
                                    "args": args,
                                    "result": result,
                                    "timestamp": timestamp.to_rfc3339()
                                })
                            }
                        }
                    }).collect::<Vec<_>>();
                    
                    history
                } else {
                    send_error_response(connection_id, state, "No agent available").await?;
                    return Ok(());
                }
            };
            
            let data = serde_json::json!({
                "messages": history
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentReset => {
            // Clear conversation history
            let command = CqrsCommand::Agent(AgentCommand::ClearHistory);
            state.command_queue.execute(command).await?;
            
            let data = serde_json::json!({
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentInfo => {
            
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
                
                send_success_response(connection_id, state, Some(data)).await?;
            } else {
                send_error_response(connection_id, state, "No agent available").await?;
            }
        }
        
        _ => {
            // This shouldn't happen if routing is correct
            return Err(ServerError::websocket("Command routed to wrong handler").into());
        }
    }
    
    Ok(())
}