//! @module graph_commands
//! @description Graph-related WebSocket command handlers
//! 
//! This module implements all graph-related WebSocket commands, providing
//! comprehensive knowledge graph management through the GraphOps trait
//! which enforces agent authorization at runtime.
//! 
//! ## Command Categories
//! 
//! ### Block Operations
//! - `CreateBlock`: Create new block with content, parent, and page associations
//! - `UpdateBlock`: Modify block content while preserving edges
//! - `DeleteBlock`: Archive block node (soft delete)
//! 
//! ### Page Operations
//! - `CreatePage`: Create or update page with properties
//! - `DeletePage`: Archive page and associated blocks
//! 
//! ### Graph Lifecycle
//! - `OpenGraph`: Load graph into memory and trigger recovery
//! - `CloseGraph`: Save and unload graph from memory
//! - `CreateGraph`: Create new graph with prime agent authorization
//! - `DeleteGraph`: Archive entire graph to archived_graphs/
//! - `ListGraphs`: Return all registered graphs with metadata
//! 
//! ## Authorization Model
//! 
//! All operations require an authenticated agent with appropriate graph
//! permissions. The current_agent_id from the WebSocket connection is
//! passed to GraphOps methods which perform runtime authorization checks.
//! 
//! ## Graph Targeting
//! 
//! Commands support flexible graph targeting through:
//! - `graph_id`: Direct UUID string targeting
//! - `graph_name`: Human-readable name resolution
//! - Smart defaults: Falls back to single open graph when unspecified
//! 
//! ## Transaction Integration
//! 
//! All modifying operations are wrapped in transactions via the GraphOps
//! trait, ensuring ACID properties and enabling crash recovery through
//! the WAL (Write-Ahead Log).
//! 
//! ## Error Handling
//! 
//! - Authorization failures return clear "not authorized" errors
//! - Missing agent selection returns "no agent selected" errors
//! - Graph resolution failures provide specific error messages
//! - Operation failures are wrapped with descriptive context

use std::sync::Arc;
use uuid::Uuid;
use crate::error::*;
use crate::AppState;
use crate::graph_operations::GraphOps;
use crate::server::websocket::Command;
use crate::server::websocket_utils::{
    send_success_response, resolve_graph_for_command
};

/// Main handler function for graph commands
pub async fn handle(
    command: Command,
    connection_id: &str,
    state: &Arc<AppState>,
    current_agent_id: Option<Uuid>,
) -> Result<()> {
    match command {
        Command::CreateBlock { content, parent_id, page_name, temp_id: _, graph_id, graph_name } => {
            // Call kg_api to create the block
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Get current agent ID
            let agent_id = match current_agent_id {
                Some(id) => id,
                None => {
                    return Err(ServerError::websocket("No agent selected for this operation").into());
                }
            };
            
            let block_id = state.add_block(agent_id, content, parent_id, page_name, None, &resolved_graph_id).await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::UpdateBlock { block_id, content, graph_id, graph_name } => {
            // Call kg_api to update the block
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Get current agent ID
            let agent_id = match current_agent_id {
                Some(id) => id,
                None => {
                    return Err(ServerError::websocket("No agent selected for this operation").into());
                }
            };
            
            state.update_block(agent_id, block_id.clone(), content, &resolved_graph_id).await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::DeleteBlock { block_id, graph_id, graph_name } => {
            // Call kg_api to delete the block
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Get current agent ID
            let agent_id = match current_agent_id {
                Some(id) => id,
                None => {
                    return Err(ServerError::websocket("No agent selected for this operation").into());
                }
            };
            
            state.delete_block(agent_id, block_id.clone(), &resolved_graph_id).await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::CreatePage { name, properties, graph_id, graph_name } => {
            // Call kg_api to create the page
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Get current agent ID
            let agent_id = match current_agent_id {
                Some(id) => id,
                None => {
                    return Err(ServerError::websocket("No agent selected for this operation").into());
                }
            };
            
            // Convert HashMap<String, String> to serde_json::Value
            let properties_json = properties.map(|props| {
                serde_json::Value::Object(
                    props.into_iter()
                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                        .collect()
                )
            });
            
            state.create_page(agent_id, name.clone(), properties_json, &resolved_graph_id).await?;
            let data = serde_json::json!({ "page_name": name });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::DeletePage { page_name, graph_id, graph_name } => {
            // Delete a page
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true  // allow smart default
            ).await?;
            
            // Get current agent ID
            let agent_id = match current_agent_id {
                Some(id) => id,
                None => {
                    return Err(ServerError::websocket("No agent selected for this operation").into());
                }
            };
            
            state.delete_page(agent_id, page_name.clone(), &resolved_graph_id).await?;
            let data = serde_json::json!({ "page_name": page_name });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::OpenGraph { graph_id, graph_name } => {
            // Open a graph
            
            use crate::graph_operations::GraphOps;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to open
            ).await?;
            
            let graph_info = state.open_graph(resolved_graph_id).await?;
            send_success_response(connection_id, state, Some(graph_info)).await?;
        }
        Command::CloseGraph { graph_id, graph_name } => {
            // Close a graph
            
            use crate::graph_operations::GraphOps;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to close
            ).await?;
            
            state.close_graph(resolved_graph_id).await?;
            send_success_response(connection_id, state, None).await?;
        }
        Command::CreateGraph { name, description } => {
            // Create a new graph
            
            use crate::graph_operations::GraphOps;
            
            let graph_info = state.create_graph(name, description).await?;
            send_success_response(connection_id, state, Some(graph_info)).await?;
        }
        Command::DeleteGraph { graph_id, graph_name } => {
            // Delete a graph
            
            use crate::graph_operations::GraphOps;
            
            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false  // no smart default - must specify which graph to delete
            ).await?;
            
            state.delete_graph(&resolved_graph_id).await?;
            let data = serde_json::json!({ "deleted_graph_id": resolved_graph_id.to_string() });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::ListGraphs => {
            // List all graphs
            
            use crate::graph_operations::GraphOps;
            
            let graphs = state.list_graphs().await?;
            let data = serde_json::json!({ "graphs": graphs });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        _ => {
            // This shouldn't happen if routing is correct
            return Err(ServerError::websocket("Command routed to wrong handler").into());
        }
    }
    
    Ok(())
}