//! @module agent_commands
//! @description Agent-related WebSocket command handlers
//! 
//! This module implements all agent-related WebSocket commands, providing
//! comprehensive agent management capabilities including chat interactions,
//! agent lifecycle management, and graph authorization control.
//! 
//! ## Command Categories
//! 
//! ### Chat Operations
//! - `AgentChat`: Send messages to agents and receive LLM responses
//! - `AgentHistory`: Retrieve conversation history with optional limits
//! - `AgentReset`: Clear agent conversation history
//! 
//! ### Agent Selection
//! - `AgentSelect`: Switch current agent for connection
//! - `AgentList`: List all agents with active/prime status
//! - `AgentInfo`: Get detailed agent information
//! 
//! ### Agent Administration
//! - `CreateAgent`: Register new agent with MockLLM config
//! - `DeleteAgent`: Archive agent (prime agent protected)
//! - `ActivateAgent`: Load agent into memory
//! - `DeactivateAgent`: Save and unload from memory
//! 
//! ### Authorization Management
//! - `AuthorizeAgent`: Grant agent access to specific graphs
//! - `DeauthorizeAgent`: Revoke agent access from graphs
//! 
//! ## Key Patterns
//! 
//! ### Agent Resolution
//! Commands accept both agent_id (UUID) and agent_name for flexibility.
//! Resolution follows priority: explicit ID > explicit name > current > prime.
//! 
//! ### Prime Agent Protection
//! The prime agent cannot be deleted and serves as the default for all
//! operations when no specific agent is selected.
//! 
//! ### Bidirectional Authorization
//! Authorization updates both agent and graph registries to maintain
//! consistency and enable efficient permission checks.
//! 
//! ### Lock Ordering
//! When both registries need write access (authorization operations),
//! uses `lock_registries_for_write()` to acquire locks in the
//! canonical order (graph_registry → agent_registry) to prevent deadlocks.
//! 
//! ### Per-Agent Locking Pattern (CRITICAL)
//! Agents use fine-grained locking for parallel operations:
//! 1. **Brief HashMap lock**: Acquire read lock on `agents` HashMap only to get Arc
//! 2. **Clone the Arc**: Get `Arc<RwLock<Agent>>` and immediately drop HashMap lock
//! 3. **Individual agent lock**: Work with the specific agent's lock
//! 
//! **NEVER hold the HashMap lock while calling agent methods!** This would block
//! all other agent operations unnecessarily. Example:
//! ```rust
//! // CORRECT: Brief HashMap access
//! let agent_arc = {
//!     let agents = state.agents.read_or_panic("get agent").await;
//!     agents.get(&agent_id).cloned()
//! };
//! if let Some(agent_arc) = agent_arc {
//!     let mut agent = agent_arc.write_or_panic("operation").await;
//!     // ... work with agent
//! }
//! 
//! // WRONG: Holding HashMap lock during operations
//! let mut agents = state.agents.write_or_panic("...").await;
//! let agent = agents.get_mut(&agent_id)?;
//! agent.process_message(...).await; // BLOCKS ALL OTHER AGENTS!
//! ```
//! 
//! ## Integration
//! 
//! - Uses AgentRegistry for lifecycle and authorization management
//! - Integrates with Agent instances for chat and history operations
//! - Maintains connection state for agent selection persistence

use std::sync::Arc;
use uuid::Uuid;
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
            
            // Get the current agent for this connection (defaults to prime if none selected)
            // Get prime agent ID first
            let prime_agent_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                registry.get_prime_agent_id()
            };
            
            let agent_id = {
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read_or_panic("agent chat - read connections").await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or(prime_agent_id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            
            // Ensure we have an agent ID
            let agent_id = agent_id.ok_or_else(|| ServerError::websocket("No agent available for chat"))?;
            
            // Send immediate ACK response
            send_success_response(connection_id, state, Some(json!({
                "status": "processing",
                "agent_id": agent_id.to_string()
            }))).await?;
            
            // Spawn background task for LLM processing
            let state_clone = state.clone();
            let conn_id = connection_id.to_string();
            tokio::spawn(async move {
                // Process the message (all 4 phases encapsulated)
                match process_agent_message(&state_clone, agent_id, message, echo, echo_tool).await {
                    Ok(response) => {
                        // Send final response via WebSocket
                        let response_data = json!({
                            "agent_id": agent_id.to_string(),
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
        
        Command::AgentSelect { agent_id, agent_name } => {
            
            // Resolve the agent using the registry's resolution function
            let selected_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                // Parse UUID if provided
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                // Use resolution with smart default (prime agent)
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    true  // Allow smart default to prime agent
                )?
            };
            
            // Verify the agent exists and is active
            {
                let registry = state.agent_registry.read_or_panic("check agent active").await;
                if !registry.is_agent_active(&selected_id) {
                    drop(registry);
                    // Submit command to activate the agent
                    let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                        crate::cqrs::AgentRegistryCommand::ActivateAgent { agent_id: selected_id }
                    ));
                    state.command_queue.execute(command).await?;
                }
            }
            
            // Update the connection's current agent
            if let Some(ref connections) = state.ws_connections {
                let mut conns = connections.write_or_panic("agent select - write connections").await;
                if let Some(conn) = conns.get_mut(connection_id) {
                    conn.current_agent_id = Some(selected_id);
                }
            }
            
            // Get agent info for response
            let agent_info = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                let agent = registry.get_agent(&selected_id)
                    .ok_or_else(|| ServerError::websocket(format!("Agent {} not found", selected_id)))?;
                serde_json::json!({
                    "agent_id": selected_id.to_string(),
                    "agent_name": agent.name
                })
            };
            
            send_success_response(connection_id, state, Some(agent_info)).await?;
        }
        
        Command::AgentList => {
            
            let agents_list = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                let agents = registry.get_all_agents();
                let prime_id = registry.get_prime_agent_id();
                
                let mut agent_list = Vec::new();
                for agent in agents {
                    let is_active = registry.is_agent_active(&agent.id);
                    agent_list.push(serde_json::json!({
                        "id": agent.id.to_string(),
                        "name": agent.name,
                        "is_prime": Some(agent.id) == prime_id,
                        "is_active": is_active
                    }));
                }
                agent_list
            };
            
            let data = serde_json::json!({ "agents": agents_list });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentHistory { agent_id, agent_name, limit } => {
            
            // Resolve the agent - if none specified, use current connection's agent or prime
            let resolved_id = if agent_id.is_some() || agent_name.is_some() {
                // Explicit agent specified
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default when explicitly specified
                )?
            } else {
                // Get prime agent ID first
                let prime_agent_id = {
                    let registry = state.agent_registry.read_or_panic("read agent registry").await;
                    registry.get_prime_agent_id()
                };
                
                // No agent specified, use current connection's agent or prime
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read_or_panic("agent chat - read connections").await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or(prime_agent_id)
                    } else {
                        None
                    }
                } else {
                    None
                }.ok_or_else(|| ServerError::websocket("No agent available"))?
            };
            
            // Get conversation history
            let history = {
                let agents = state.agents.read_or_panic("agent history - read agents").await;
                if let Some(agent_arc) = agents.get(&resolved_id) {
                    let agent = agent_arc.read_or_panic("agent history - read agent").await;
                    let messages = if let Some(limit) = limit {
                        agent.get_recent_messages(limit)
                    } else {
                        &agent.conversation_history
                    };
                    
                    // Convert messages to JSON format
                    messages.iter().map(|msg| {
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
                    }).collect::<Vec<_>>()
                } else {
                    vec![]
                }
            };
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "messages": history
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentReset { agent_id, agent_name } => {
            
            // Resolve the agent - if none specified, use current connection's agent or prime
            let resolved_id = if agent_id.is_some() || agent_name.is_some() {
                // Explicit agent specified
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default when explicitly specified
                )?
            } else {
                // Get prime agent ID first  
                let prime_agent_id = {
                    let registry = state.agent_registry.read_or_panic("read agent registry").await;
                    registry.get_prime_agent_id()
                };
                
                // No agent specified, use current connection's agent or prime
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read_or_panic("agent chat - read connections").await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or(prime_agent_id)
                    } else {
                        None
                    }
                } else {
                    None
                }.ok_or_else(|| ServerError::websocket("No agent selected"))?
            };
            
            // Submit command to clear conversation history
            let command = CqrsCommand::Agent(AgentCommand::ClearHistory {
                agent_id: resolved_id,
            });
            state.command_queue.execute(command).await?;
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        // Admin Commands
        Command::CreateAgent { name, description } => {
            
            // Submit command to create agent
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::CreateAgent {
                    name: Some(name.clone()),
                    description: description.clone().or(Some("An intelligent assistant".to_string())),
                    resolved_id: None,  // Will be resolved before WAL write
                }
            ));
            let result = state.command_queue.execute(command).await?;
            let agent_info: crate::agent::agent_registry::AgentInfo = serde_json::from_value(result.data.unwrap())?;
            
            // Save the registry after creating agent
            // Registry persisted through WAL, no need for JSON save
            
            let data = serde_json::json!({
                "agent_id": agent_info.id.to_string(),
                "name": agent_info.name,
                "description": agent_info.description,
                "created": agent_info.created,
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::DeleteAgent { agent_id, agent_name } => {
            
            // Resolve agent (no smart default for destructive operations)
            let resolved_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for delete
                )?
            };
            
            // Don't allow deleting the prime agent
            {
                let is_prime = {
                    let registry = state.agent_registry.read_or_panic("read agent registry").await;
                    Some(resolved_id) == registry.get_prime_agent_id()
                };
                
                if is_prime {
                    send_error_response(connection_id, state, "Cannot delete the prime agent").await?;
                    return Ok(());
                }
            }
            
            // Submit command to remove agent (handles deactivation and archival)
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::DeleteAgent { agent_id: resolved_id }
            ));
            state.command_queue.execute(command).await?;
            
            // Save registry after deletion
            // Registry persisted through WAL, no need for JSON save
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::ActivateAgent { agent_id, agent_name } => {
            
            // Resolve agent (no smart default for explicit operations)
            let resolved_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for activate
                )?
            };
            
            // Submit command to activate agent
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::ActivateAgent { agent_id: resolved_id }
            ));
            state.command_queue.execute(command).await?;
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::DeactivateAgent { agent_id, agent_name } => {
            
            // Resolve agent (no smart default for explicit operations)
            let resolved_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for deactivate
                )?
            };
            
            // Submit command to deactivate agent
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::DeactivateAgent { agent_id: resolved_id }
            ));
            state.command_queue.execute(command).await?;
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AuthorizeAgent { agent_id, agent_name, graph_id, graph_name } => {
            
            // Resolve agent (must be explicitly specified)
            let resolved_agent_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for authorization
                )?
            };
            
            // Resolve graph (must be explicitly specified)
            let resolved_graph_id = {
                let registry = state.graph_registry.read_or_panic("read graph registry").await;
                
                let graph_uuid = graph_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid graph ID: {}", e)))?;
                
                registry.resolve_graph_target(
                    graph_uuid.as_ref(),
                    graph_name.as_deref(),
                    false  // No smart default for authorization
                )?
            };
            
            // Submit command to authorize agent for graph
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::AuthorizeAgent {
                    agent_id: resolved_agent_id,
                    graph_id: resolved_graph_id,
                }
            ));
            state.command_queue.execute(command).await?;
            
            let data = serde_json::json!({
                "agent_id": resolved_agent_id.to_string(),
                "graph_id": resolved_graph_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::DeauthorizeAgent { agent_id, agent_name, graph_id, graph_name } => {
            
            // Resolve agent (must be explicitly specified)
            let resolved_agent_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for deauthorization
                )?
            };
            
            // Resolve graph (must be explicitly specified)
            let resolved_graph_id = {
                let registry = state.graph_registry.read_or_panic("read graph registry").await;
                
                let graph_uuid = graph_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid graph ID: {}", e)))?;
                
                registry.resolve_graph_target(
                    graph_uuid.as_ref(),
                    graph_name.as_deref(),
                    false  // No smart default for deauthorization
                )?
            };
            
            // Submit command to deauthorize agent from graph
            let command = CqrsCommand::Registry(crate::cqrs::RegistryCommand::Agent(
                crate::cqrs::AgentRegistryCommand::DeauthorizeAgent { 
                    agent_id: resolved_agent_id,
                    graph_id: resolved_graph_id,
                }
            ));
            state.command_queue.execute(command).await?;
            
            // Save both registries
            // Registry persisted through WAL, no need for JSON save
            // Registry persisted through WAL, no need for JSON save
            
            let data = serde_json::json!({
                "agent_id": resolved_agent_id.to_string(),
                "graph_id": resolved_graph_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentInfo { agent_id, agent_name } => {
            
            // Resolve agent (defaults to prime if not specified)
            let resolved_id = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| ServerError::invalid_request(format!("Invalid agent ID: {}", e)))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    true  // Allow smart default (prime agent) for info command
                )?
            };
            
            // Get agent info from registry
            let (agent_info, is_active) = {
                let registry = state.agent_registry.read_or_panic("read agent registry").await;
                
                let info = registry.get_agent(&resolved_id)
                    .ok_or_else(|| ServerError::websocket(format!("Agent {} not found", resolved_id)))?
                    .clone();
                let active = registry.is_agent_active(&resolved_id);
                (info, active)
            };
            
            // Get conversation stats if agent is loaded
            let conversation_stats = if is_active {
                let agents_map = state.agents.read_or_panic("agent info - read agents").await;
                match agents_map.get(&resolved_id) {
                    Some(agent_arc) => {
                        let agent = agent_arc.read_or_panic("agent info - read agent").await;
                        Some(serde_json::json!({
                            "message_count": agent.conversation_history.len(),
                            "llm_config": agent.llm_config,
                        }))
                    },
                    None => None
                }
            } else {
                None
            };
            
            // Get authorized graph names
            let authorized_graph_names = {
                let graph_registry = state.graph_registry.read_or_panic("read graph registry").await;
                
                agent_info.authorized_graphs.iter()
                    .filter_map(|graph_id| {
                        graph_registry.get_graph(graph_id)
                            .map(|g| serde_json::json!({
                                "id": graph_id.to_string(),
                                "name": g.name.clone()
                            }))
                    })
                    .collect::<Vec<_>>()
            };
            
            let data = serde_json::json!({
                "agent_id": agent_info.id.to_string(),
                "name": agent_info.name,
                "description": agent_info.description,
                "is_prime": agent_info.is_prime,
                "is_active": is_active,
                "created": agent_info.created,
                "last_active": agent_info.last_active,
                "authorized_graphs": authorized_graph_names,
                "conversation_stats": conversation_stats,
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        _ => {
            // This shouldn't happen if routing is correct
            return Err(ServerError::websocket("Command routed to wrong handler").into());
        }
    }
    
    Ok(())
}