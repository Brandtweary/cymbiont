//! Agent message queue for async phase-based processing
//! 
//! TODO: This module is DEPRECATED and will be completely replaced by the CQRS refactor.
//! The message queue was a band-aid fix for deadlock issues, but the proper solution
//! is a global command queue that handles ALL mutations, not just AgentChat.
//! See docs/cqrs_refactor_plan.md for the new architecture.
//! 
//! DO NOT extend this module or use it as a pattern for other features.
//! 
//! Implements true async processing by breaking agent interactions into phases:
//! 1. User Message: Add to conversation, return ACK immediately
//! 2. LLM Processing: Call LLM without holding agent locks  
//! 3. Tool Execution: Execute tools without agent locks to prevent deadlocks
//! 4. Response: Add results to conversation, send final WebSocket response

#![allow(deprecated)] // This module itself is deprecated

use tokio::sync::mpsc;
use uuid::Uuid;
use std::sync::Arc;
use once_cell::sync::Lazy;
use crate::{AppState, Result, error::ServerError};
use crate::lock::AsyncRwLockExt;
use crate::agent::agent::{LLMContext, ToolResult};
use crate::agent::llm::{LLMResponse, ToolCall};

/// Original request from WebSocket
#[deprecated(
    since = "0.1.0",
    note = "Will be replaced by CQRS command types. See docs/cqrs_refactor_plan.md"
)]
pub struct AgentRequest {
    pub request_id: Uuid,
    pub connection_id: String,
    pub message: String,
    pub echo: Option<String>,
    pub echo_tool: Option<String>,
}

/// Phase-based tasks for the message queue
#[deprecated(
    since = "0.1.0",
    note = "Will be replaced by CQRS command types. See docs/cqrs_refactor_plan.md"
)]
pub enum QueuedTask {
    /// Phase 1: Add user message to conversation, return ACK
    UserMessage {
        agent_id: Uuid,
        request: AgentRequest,
    },
    /// Phase 2: Get LLM response without holding locks
    LLMProcessing {
        agent_id: Uuid,
        request_id: Uuid,
        connection_id: String,
        context: LLMContext,
    },
    /// Phase 3: Execute tools without agent locks
    ToolExecution {
        agent_id: Uuid,
        request_id: Uuid,
        connection_id: String,
        tool_calls: Vec<ToolCall>,
        llm_response: LLMResponse,
    },
    /// Phase 4: Add results to conversation, send final response
    SendResponse {
        agent_id: Uuid,
        request_id: Uuid,
        connection_id: String,
        llm_response: LLMResponse,
        tool_results: Vec<ToolResult>,
    },
}

/// Final response sent to WebSocket
pub struct AgentResponse {
    pub request_id: Uuid,
    pub response: String,
    pub agent_id: Uuid,
}

/// Global task queue - single worker handles all phases for all agents
static TASK_QUEUE: Lazy<mpsc::UnboundedSender<QueuedTask>> = Lazy::new(|| {
    let (tx, mut rx) = mpsc::unbounded_channel::<QueuedTask>();
    
    // Spawn single worker to process all tasks
    tokio::spawn(async move {
        tracing::info!("Starting global message queue worker");
        
        while let Some(task) = rx.recv().await {
            if let Err(e) = process_queued_task(task).await {
                tracing::error!("Message queue worker error: {:?}", e);
            }
        }
        
        tracing::info!("Message queue worker shutting down");
    });
    
    tx
});

/// Queue a user message for phase-based processing
#[deprecated(
    since = "0.1.0",
    note = "Will be replaced by CQRS command submission. See docs/cqrs_refactor_plan.md"
)]
pub async fn queue_agent_message(
    agent_id: Uuid,
    request: AgentRequest,
) -> Result<()> {
    tracing::debug!("Queueing user message for agent {}, request_id: {}", agent_id, request.request_id);
    
    let task = QueuedTask::UserMessage { agent_id, request };
    
    TASK_QUEUE.send(task)
        .map_err(|_| ServerError::websocket("Message queue worker terminated"))?;
    
    tracing::debug!("User message queued successfully for agent {}", agent_id);
    Ok(())
}

/// Process a queued task based on its phase
async fn process_queued_task(task: QueuedTask) -> Result<()> {
    match task {
        QueuedTask::UserMessage { agent_id, request } => {
            process_user_message(agent_id, request).await
        }
        QueuedTask::LLMProcessing { agent_id, request_id, connection_id, context } => {
            process_llm_response(agent_id, request_id, connection_id, context).await
        }
        QueuedTask::ToolExecution { agent_id, request_id, connection_id, tool_calls, llm_response } => {
            process_tool_execution(agent_id, request_id, connection_id, tool_calls, llm_response).await
        }
        QueuedTask::SendResponse { agent_id, request_id, connection_id, llm_response, tool_results } => {
            process_final_response(agent_id, request_id, connection_id, llm_response, tool_results).await
        }
    }
}

/// Phase 1: Add user message to conversation, return ACK, queue Phase 2
async fn process_user_message(agent_id: Uuid, request: AgentRequest) -> Result<()> {
    tracing::debug!("Phase 1: Processing user message for agent {}, request_id: {}", agent_id, request.request_id);
    
    // Get AppState reference
    let app_state = get_app_state()?;
    
    // Add user message and get LLM context (brief agent lock)
    let context = {
        let agents = app_state.agents.read_or_panic("process user message").await;
        match agents.get(&agent_id) {
            Some(agent_arc) => {
                let mut agent = agent_arc.write_or_panic("add user message").await;
                let result = agent.add_user_message_and_get_context(
                    request.message,
                    request.echo,
                    request.echo_tool
                ).await?;
                result
            }
            None => {
                return Err(ServerError::websocket(format!("Agent {} not found", agent_id)).into());
            }
        }
    };
    
    // Send ACK immediately to WebSocket
    send_agent_ack(&request.connection_id, &app_state, request.request_id).await?;
    
    // Queue Phase 2 (LLM processing)
    let llm_task = QueuedTask::LLMProcessing {
        agent_id,
        request_id: request.request_id,
        connection_id: request.connection_id,
        context,
    };
    
    TASK_QUEUE.send(llm_task)
        .map_err(|_| ServerError::websocket("Failed to queue LLM processing"))?;
    
    tracing::debug!("Phase 1: User message processed, ACK sent, LLM processing queued");
    Ok(())
}

/// Phase 2: Get LLM response without holding any locks
async fn process_llm_response(agent_id: Uuid, request_id: Uuid, connection_id: String, context: LLMContext) -> Result<()> {
    use crate::agent::agent::Agent;
    
    tracing::debug!("Phase 2: Processing LLM response for agent {}, request_id: {}", agent_id, request_id);
    
    // Call LLM without holding any locks  
    let llm_response = Agent::get_llm_response(context).await?;
    
    // Determine next phase based on tool calls
    if let Some(tool_call) = &llm_response.tool_call {
        tracing::debug!("Phase 2: LLM requested tool '{}', queueing tool execution", tool_call.name);
        
        // Queue Phase 3 (tool execution)
        let tool_task = QueuedTask::ToolExecution {
            agent_id,
            request_id,
            connection_id,
            tool_calls: vec![tool_call.clone()],
            llm_response,
        };
        
        TASK_QUEUE.send(tool_task)
            .map_err(|_| ServerError::websocket("Failed to queue tool execution"))?;
    } else {
        tracing::debug!("Phase 2: No tools requested, queueing final response");
        
        // No tools needed, queue Phase 4 (final response)
        let response_task = QueuedTask::SendResponse {
            agent_id,
            request_id,
            connection_id,
            llm_response,
            tool_results: vec![],
        };
        
        TASK_QUEUE.send(response_task)
            .map_err(|_| ServerError::websocket("Failed to queue final response"))?;
    }
    
    tracing::debug!("Phase 2: LLM response processed, next phase queued");
    Ok(())
}

/// Phase 3: Execute tools without holding agent locks
async fn process_tool_execution(
    agent_id: Uuid,
    request_id: Uuid,
    connection_id: String,
    tool_calls: Vec<ToolCall>,
    llm_response: LLMResponse,
) -> Result<()> {
    use crate::agent::kg_tools;
    
    tracing::debug!("Phase 3: Processing tool execution for agent {}, request_id: {}", agent_id, request_id);
    
    let app_state = get_app_state()?;
    
    // Execute tools without holding agent locks - this prevents deadlocks!
    let tool_results = kg_tools::execute_tools_stateless(&app_state, agent_id, tool_calls).await?;
    
    tracing::debug!("Phase 3: {} tools executed successfully", tool_results.len());
    
    // Queue Phase 4 (final response)
    let response_task = QueuedTask::SendResponse {
        agent_id,
        request_id,
        connection_id,
        llm_response,
        tool_results,
    };
    
    TASK_QUEUE.send(response_task)
        .map_err(|_| ServerError::websocket("Failed to queue final response"))?;
    
    tracing::debug!("Phase 3: Tool execution completed, final response queued");
    Ok(())
}

/// Phase 4: Add results to conversation and send final WebSocket response
async fn process_final_response(
    agent_id: Uuid,
    request_id: Uuid,
    connection_id: String,
    llm_response: LLMResponse,
    tool_results: Vec<ToolResult>,
) -> Result<()> {
    tracing::debug!("Phase 4: Processing final response for agent {}, request_id: {}", agent_id, request_id);
    
    let app_state = get_app_state()?;
    
    // Add response to conversation (brief agent lock)
    let final_response = {
        let agents = app_state.agents.read_or_panic("process final response").await;
        match agents.get(&agent_id) {
            Some(agent_arc) => {
                let mut agent = agent_arc.write_or_panic("add response to conversation").await;
                agent.add_response_to_conversation(llm_response, tool_results).await?
            }
            None => {
                return Err(ServerError::websocket(format!("Agent {} not found", agent_id)).into());
            }
        }
    };
    
    // Send final response to WebSocket
    let response_msg = AgentResponse {
        request_id,
        response: final_response,
        agent_id,
    };
    
    send_agent_response(&connection_id, &app_state, response_msg).await?;
    
    tracing::debug!("Phase 4: Final response sent successfully");
    Ok(())
}

/// Global AppState weak reference for message queue worker
static APP_STATE_REF: once_cell::sync::OnceCell<std::sync::Weak<AppState>> = once_cell::sync::OnceCell::new();

/// Initialize message queue with AppState reference (called during AppState creation)
#[deprecated(
    since = "0.1.0",
    note = "Will be replaced by CQRS command processor initialization. See docs/cqrs_refactor_plan.md"
)]
pub fn initialize_message_queue(app_state: &Arc<AppState>) {
    let _ = APP_STATE_REF.set(Arc::downgrade(app_state));
}

/// Get AppState reference for message queue operations
fn get_app_state() -> Result<Arc<AppState>> {
    let weak_ref = APP_STATE_REF.get()
        .ok_or_else(|| ServerError::websocket("Message queue not initialized with AppState"))?;
    
    weak_ref.upgrade()
        .ok_or_else(|| ServerError::websocket("AppState reference is no longer valid").into())
}

/// Send immediate ACK response to WebSocket
async fn send_agent_ack(connection_id: &str, app_state: &Arc<AppState>, request_id: Uuid) -> Result<()> {
    use crate::server::websocket::Response;
    use axum::extract::ws::Message;
    
    tracing::debug!("Sending ACK for connection {}, request_id: {}", connection_id, request_id);
    
    let ack_response = Response::AgentChatAck {
        request_id: request_id.to_string(),
    };
    
    // Send via the connection's channel
    if let Some(ref connections) = app_state.ws_connections {
        let conns = connections.read_or_panic("send agent ack").await;
        if let Some(conn) = conns.get(connection_id) {
            let msg = Message::Text(serde_json::to_string(&ack_response)?);
            let sender = conn.sender.clone();
            drop(conns); // Release lock before sending
            
            sender.send(msg)
                .map_err(|e| ServerError::websocket(format!("Failed to send ACK: {}", e)))?;
            tracing::debug!("ACK sent successfully for request_id: {}", request_id);
        } else {
            tracing::error!("Connection {} not found when sending ACK", connection_id);
        }
    } else {
        tracing::error!("No WebSocket connections available");
    }
    
    Ok(())
}

/// Send async response to WebSocket connection
async fn send_agent_response(
    connection_id: &str,
    app_state: &Arc<AppState>,
    response: AgentResponse,
) -> Result<()> {
    use crate::server::websocket::Response;
    use axum::extract::ws::Message;
    
    let request_id = response.request_id; // Save for logging
    tracing::debug!("Preparing to send response for connection {}, request_id: {}", connection_id, request_id);
    
    let data = serde_json::json!({
        "request_id": response.request_id.to_string(),
        "response": response.response,
        "agent_id": response.agent_id.to_string(),
    });
    
    // New response type for async agent responses
    let ws_response = Response::AgentChatResponse { data };
    
    // Send via the connection's channel
    if let Some(ref connections) = app_state.ws_connections {
        let conns = connections.read_or_panic("send agent response").await;
        if let Some(conn) = conns.get(connection_id) {
            let msg = Message::Text(serde_json::to_string(&ws_response)?);
            let sender = conn.sender.clone();
            drop(conns); // Release lock before sending
            
            tracing::debug!("Sending WebSocket message for request_id: {}", request_id);
            sender.send(msg)
                .map_err(|e| ServerError::websocket(format!("Failed to send agent response: {}", e)))?;
            tracing::debug!("Successfully sent response for request_id: {}", request_id);
        } else {
            tracing::error!("Connection {} not found when sending response", connection_id);
        }
    } else {
        tracing::error!("No WebSocket connections available");
    }
    
    Ok(())
}

/// Shutdown the message queue worker gracefully
#[deprecated(
    since = "0.1.0",
    note = "Will be replaced by CQRS processor shutdown. See docs/cqrs_refactor_plan.md"
)]
pub async fn shutdown_workers() {
    tracing::info!("Shutting down message queue worker");
    // The worker will stop automatically when TASK_QUEUE is dropped
    // In a production system, we might want to send a shutdown signal
}