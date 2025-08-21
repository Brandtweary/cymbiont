/**
 * @module agent_commands
 * @description Agent-related WebSocket command handlers
 * 
 * This module implements all agent-related WebSocket commands, providing
 * comprehensive agent management capabilities including chat interactions,
 * agent lifecycle management, and graph authorization control.
 * 
 * ## Command Categories
 * 
 * ### Chat Operations
 * - `AgentChat`: Send messages to agents and receive LLM responses
 * - `AgentHistory`: Retrieve conversation history with optional limits
 * - `AgentReset`: Clear agent conversation history
 * 
 * ### Agent Selection
 * - `AgentSelect`: Switch current agent for connection
 * - `AgentList`: List all agents with active/prime status
 * - `AgentInfo`: Get detailed agent information
 * 
 * ### Agent Administration
 * - `CreateAgent`: Register new agent with MockLLM config
 * - `DeleteAgent`: Archive agent (prime agent protected)
 * - `ActivateAgent`: Load agent into memory
 * - `DeactivateAgent`: Save and unload from memory
 * 
 * ### Authorization Management
 * - `AuthorizeAgent`: Grant agent access to specific graphs
 * - `DeauthorizeAgent`: Revoke agent access from graphs
 * 
 * ## Key Patterns
 * 
 * ### Agent Resolution
 * Commands accept both agent_id (UUID) and agent_name for flexibility.
 * Resolution follows priority: explicit ID > explicit name > current > prime.
 * 
 * ### Prime Agent Protection
 * The prime agent cannot be deleted and serves as the default for all
 * operations when no specific agent is selected.
 * 
 * ### Bidirectional Authorization
 * Authorization updates both agent and graph registries to maintain
 * consistency and enable efficient permission checks.
 * 
 * ### Lock Ordering
 * When both registries need write access (authorization operations),
 * uses `AppState::lock_registries_for_write()` to acquire locks in the
 * canonical order (graph_registry → agent_registry) to prevent deadlocks.
 * 
 * ## Integration
 * 
 * - Uses AgentRegistry for lifecycle and authorization management
 * - Integrates with Agent instances for chat and history operations
 * - Maintains connection state for agent selection persistence
 */

use std::sync::Arc;
use uuid::Uuid;
use crate::AppState;
use crate::server::websocket::Command;
use crate::server::websocket_utils::{send_success_response, send_error_response};

/// Main handler function for agent commands
pub async fn handle(
    command: Command,
    connection_id: &str,
    state: &Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match command {
        // Agent Commands
        Command::AgentChat { message, echo } => {
            
            // Get the current agent for this connection (defaults to prime if none selected)
            let agent_id = {
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read().await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or_else(|| {
                            // Use prime agent if none selected
                            let registry = state.agent_registry.read().ok()?;
                            registry.get_prime_agent_id()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            
            // Ensure we have an agent ID
            let agent_id = agent_id.ok_or_else(|| "No agent available for chat".to_string())?;
            
            // Get the agent and process the message
            let response = {
                let mut agents = state.agents.write().await;
                if let Some(agent) = agents.get_mut(&agent_id) {
                    agent.process_message(message, echo).await
                        .map_err(|e| format!("Failed to process message: {:?}", e))?
                } else {
                    send_error_response(connection_id, state, &format!("Agent {} not found", agent_id)).await?;
                    return Ok(());
                }
            };
            
            // Send response back to client
            let data = serde_json::json!({
                "response": response,
                "agent_id": agent_id.to_string()
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentSelect { agent_id, agent_name } => {
            
            // Resolve the agent using the registry's resolution function
            let selected_id = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                // Parse UUID if provided
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                // Use resolution with smart default (prime agent)
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    true  // Allow smart default to prime agent
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Verify the agent exists and is active
            if !state.is_agent_active(&selected_id) {
                // Try to activate the agent
                state.activate_agent(&selected_id).await
                    .map_err(|e| format!("Failed to activate agent: {}", e))?;
            }
            
            // Update the connection's current agent
            if let Some(ref connections) = state.ws_connections {
                let mut conns = connections.write().await;
                if let Some(conn) = conns.get_mut(connection_id) {
                    conn.current_agent_id = Some(selected_id);
                }
            }
            
            // Get agent info for response
            let agent_info = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                let agent = registry.get_agent(&selected_id)
                    .ok_or_else(|| format!("Agent {} not found", selected_id))?;
                serde_json::json!({
                    "agent_id": selected_id.to_string(),
                    "agent_name": agent.name
                })
            };
            
            send_success_response(connection_id, state, Some(agent_info)).await?;
        }
        
        Command::AgentList => {
            
            let agents_list = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                let agents = registry.get_all_agents();
                let prime_id = registry.get_prime_agent_id();
                
                agents.into_iter().map(|agent| {
                    serde_json::json!({
                        "id": agent.id.to_string(),
                        "name": agent.name,
                        "is_prime": Some(agent.id) == prime_id,
                        "is_active": state.is_agent_active(&agent.id)
                    })
                }).collect::<Vec<_>>()
            };
            
            let data = serde_json::json!({ "agents": agents_list });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AgentHistory { agent_id, agent_name, limit } => {
            
            // Resolve the agent - if none specified, use current connection's agent or prime
            let resolved_id = if agent_id.is_some() || agent_name.is_some() {
                // Explicit agent specified
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default when explicitly specified
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            } else {
                // No agent specified, use current connection's agent or prime
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read().await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or_else(|| {
                            // Use prime agent if none selected
                            let registry = state.agent_registry.read().ok()?;
                            registry.get_prime_agent_id()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }.ok_or_else(|| "No agent available".to_string())?
            };
            
            // Get conversation history
            let history = {
                let agents = state.agents.read().await;
                if let Some(agent) = agents.get(&resolved_id) {
                    let messages = if let Some(limit) = limit {
                        agent.get_recent_messages(limit)
                    } else {
                        &agent.conversation_history
                    };
                    
                    // Convert messages to JSON format
                    messages.iter().map(|msg| {
                        match msg {
                            crate::agent::llm::Message::User { content, timestamp, .. } => {
                                serde_json::json!({
                                    "role": "user",
                                    "content": content,
                                    "timestamp": timestamp.to_rfc3339()
                                })
                            }
                            crate::agent::llm::Message::Assistant { content, timestamp } => {
                                serde_json::json!({
                                    "role": "assistant",
                                    "content": content,
                                    "timestamp": timestamp.to_rfc3339()
                                })
                            }
                            crate::agent::llm::Message::Tool { name, args, result, timestamp } => {
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
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default when explicitly specified
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            } else {
                // No agent specified, use current connection's agent or prime
                if let Some(ref connections) = state.ws_connections {
                    let conns = connections.read().await;
                    if let Some(conn) = conns.get(connection_id) {
                        conn.current_agent_id.or_else(|| {
                            // Use prime agent if none selected
                            let registry = state.agent_registry.read().ok()?;
                            registry.get_prime_agent_id()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }.ok_or_else(|| "No agent selected".to_string())?
            };
            
            // Clear the agent's conversation history
            {
                let mut agents = state.agents.write().await;
                if let Some(agent) = agents.get_mut(&resolved_id) {
                    agent.clear_history();
                    // Save after clearing
                    agent.save()
                        .map_err(|e| format!("Failed to save agent after reset: {:?}", e))?;
                } else {
                    send_error_response(connection_id, state, &format!("Agent {} not found", resolved_id)).await?;
                    return Ok(());
                }
            }
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        // Admin Commands
        Command::CreateAgent { name, description } => {
            
            let agent_info = {
                let mut registry = state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                
                registry.register_agent(
                    None,  // Let it generate a new UUID
                    Some(name.clone()),
                    description.clone(),
                ).map_err(|e| format!("Failed to create agent: {:?}", e))?
            };
            
            // Create the actual Agent instance
            {
                use crate::agent::agent::Agent;
                use crate::agent::llm::LLMConfig;
                
                // Ensure agent directory exists
                std::fs::create_dir_all(&agent_info.data_path)
                    .map_err(|e| format!("Failed to create agent directory: {}", e))?;
                
                // Create agent with default MockLLM config
                let mut agent = Agent::new(
                    agent_info.id,
                    name.clone(),
                    LLMConfig::default(),  // MockLLM by default
                    agent_info.data_path.clone(),
                    description.clone().or(Some("An intelligent assistant".to_string())),
                );
                
                // Save the agent to disk
                agent.save()
                    .map_err(|e| format!("Failed to save agent: {:?}", e))?;
                
                // Add to active agents map
                let mut agents = state.agents.write().await;
                agents.insert(agent_info.id, agent);
            }
            
            // Save the registry after creating agent
            {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            
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
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for delete
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Don't allow deleting the prime agent
            {
                let is_prime = {
                    let registry = state.agent_registry.read()
                        .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                    Some(resolved_id) == registry.get_prime_agent_id()
                };
                
                if is_prime {
                    send_error_response(connection_id, state, "Cannot delete the prime agent").await?;
                    return Ok(());
                }
            }
            
            // Remove agent from memory if loaded
            state.deactivate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to deactivate agent: {:?}", e))?;
            
            // Remove from registry and archive data
            {
                let mut registry = state.agent_registry.write()
                    .map_err(|e| format!("Failed to write agent registry: {}", e))?;
                registry.remove_agent(&resolved_id)
                    .map_err(|e| format!("Failed to remove agent: {:?}", e))?;
            }
            
            // Save registry after deletion
            {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::ActivateAgent { agent_id, agent_name } => {
            
            // Resolve agent (no smart default for explicit operations)
            let resolved_id = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for activate
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Activate the agent
            state.activate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to activate agent: {:?}", e))?;
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::DeactivateAgent { agent_id, agent_name } => {
            
            // Resolve agent (no smart default for explicit operations)
            let resolved_id = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for deactivate
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Deactivate the agent
            state.deactivate_agent(&resolved_id).await
                .map_err(|e| format!("Failed to deactivate agent: {:?}", e))?;
            
            let data = serde_json::json!({
                "agent_id": resolved_id.to_string(),
                "success": true
            });
            
            send_success_response(connection_id, state, Some(data)).await?;
        }
        
        Command::AuthorizeAgent { agent_id, agent_name, graph_id, graph_name } => {
            
            // Resolve agent (must be explicitly specified)
            let resolved_agent_id = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for authorization
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Resolve graph (must be explicitly specified)
            let resolved_graph_id = {
                let registry = state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
                let graph_uuid = graph_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid graph ID: {}", e))?;
                
                registry.resolve_graph_target(
                    graph_uuid.as_ref(),
                    graph_name.as_deref(),
                    false  // No smart default for authorization
                ).map_err(|e| format!("Failed to resolve graph: {:?}", e))?
            };
            
            // Authorize agent for graph
            {
                let (mut graph_registry, mut agent_registry) = state.lock_registries_for_write()
                    .map_err(|e| format!("Failed to lock registries: {}", e))?;
                
                agent_registry.authorize_agent_for_graph(
                    &resolved_agent_id,
                    &resolved_graph_id,
                    &mut graph_registry,
                ).map_err(|e| format!("Failed to authorize agent: {:?}", e))?;
            }
            
            // Save both registries
            {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            {
                let registry = state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save graph registry: {:?}", e))?;
            }
            
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
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    false  // No smart default for deauthorization
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Resolve graph (must be explicitly specified)
            let resolved_graph_id = {
                let registry = state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
                let graph_uuid = graph_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid graph ID: {}", e))?;
                
                registry.resolve_graph_target(
                    graph_uuid.as_ref(),
                    graph_name.as_deref(),
                    false  // No smart default for deauthorization
                ).map_err(|e| format!("Failed to resolve graph: {:?}", e))?
            };
            
            // Deauthorize agent from graph
            {
                let (mut graph_registry, mut agent_registry) = state.lock_registries_for_write()
                    .map_err(|e| format!("Failed to lock registries: {}", e))?;
                
                agent_registry.deauthorize_agent_from_graph(
                    &resolved_agent_id,
                    &resolved_graph_id,
                    &mut graph_registry,
                ).map_err(|e| format!("Failed to deauthorize agent: {:?}", e))?;
            }
            
            // Save both registries
            {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save agent registry: {:?}", e))?;
            }
            {
                let registry = state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                registry.save()
                    .map_err(|e| format!("Failed to save graph registry: {:?}", e))?;
            }
            
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
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let agent_uuid = agent_id.as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| format!("Invalid agent ID: {}", e))?;
                
                registry.resolve_agent_target(
                    agent_uuid.as_ref(),
                    agent_name.as_deref(),
                    true  // Allow smart default (prime agent) for info command
                ).map_err(|e| format!("Failed to resolve agent: {:?}", e))?
            };
            
            // Get agent info from registry
            let (agent_info, is_active) = {
                let registry = state.agent_registry.read()
                    .map_err(|e| format!("Failed to read agent registry: {}", e))?;
                
                let info = registry.get_agent(&resolved_id)
                    .ok_or_else(|| format!("Agent {} not found", resolved_id))?
                    .clone();
                let active = registry.is_agent_active(&resolved_id);
                (info, active)
            };
            
            // Get conversation stats if agent is loaded
            let conversation_stats = if is_active {
                let agents = state.agents.read().await;
                agents.get(&resolved_id).map(|agent| {
                    serde_json::json!({
                        "message_count": agent.conversation_history.len(),
                        "llm_config": agent.llm_config,
                    })
                })
            } else {
                None
            };
            
            // Get authorized graph names
            let authorized_graph_names = {
                let graph_registry = state.graph_registry.read()
                    .map_err(|e| format!("Failed to read graph registry: {}", e))?;
                
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
            return Err("Command routed to wrong handler".into());
        }
    }
    
    Ok(())
}