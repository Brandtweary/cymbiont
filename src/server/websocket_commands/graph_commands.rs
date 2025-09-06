//! @module graph_commands
//! @description Graph-related WebSocket command handlers
//!
//! This module implements all graph-related WebSocket commands, providing
//! comprehensive knowledge graph management through the GraphOps trait.
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
//! - `CreateGraph`: Create new graph with specified name
//! - `DeleteGraph`: Archive entire graph to archived_graphs/
//! - `ListGraphs`: Return all registered graphs with metadata
//!
//! ## Graph Targeting
//!
//! Commands support flexible graph targeting through:
//! - `graph_id`: Direct UUID string targeting
//! - `graph_name`: Human-readable name resolution
//! - Smart defaults: Falls back to single open graph when unspecified
//!
//! The graph resolution system provides intelligent defaults: when only one
//! graph is open and no target is specified, operations automatically target
//! that graph. This simplifies common single-graph workflows.
//!
//! ## CQRS Integration
//!
//! All modifying operations route through the CQRS CommandQueue via the
//! GraphOps trait, ensuring sequential processing and consistency. The
//! CommandQueue provides deadlock-free mutations through single-threaded
//! command processing while allowing unlimited concurrent reads.
//!
//! ## Error Handling
//!
//! - Graph resolution failures provide specific error messages
//! - Operation failures are wrapped with descriptive context
//! - Missing graphs return NotFound errors with graph identification
//! - Multiple open graphs without target specification return clear guidance
//!
//! ## Response Format
//!
//! All commands return WebSocket responses with consistent structure:
//! - Success responses include operation-specific data
//! - Error responses include descriptive error messages

use crate::error::*;
use crate::graph::graph_operations::GraphOps;
use crate::server::websocket::Command;
use crate::server::websocket_utils::{resolve_graph_for_command, send_success_response};
use crate::AppState;
use std::sync::Arc;

/// Main handler function for graph commands
pub async fn handle(command: Command, connection_id: &str, state: &Arc<AppState>) -> Result<()> {
    match command {
        Command::CreateBlock {
            content,
            parent_id,
            page_name,
            temp_id: _,
            graph_id,
            graph_name,
        } => {
            // Call kg_api to create the block

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true, // allow smart default
            )
            .await?;

            let block_id = state
                .add_block(
                    None,
                    content,
                    parent_id,
                    page_name,
                    None,
                    None,
                    &resolved_graph_id,
                )
                .await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::UpdateBlock {
            block_id,
            content,
            graph_id,
            graph_name,
        } => {
            // Call kg_api to update the block

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true, // allow smart default
            )
            .await?;

            state
                .update_block(block_id.clone(), content, &resolved_graph_id)
                .await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::DeleteBlock {
            block_id,
            graph_id,
            graph_name,
        } => {
            // Call kg_api to delete the block

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true, // allow smart default
            )
            .await?;

            state
                .delete_block(block_id.clone(), &resolved_graph_id)
                .await?;
            let data = serde_json::json!({ "block_id": block_id });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::CreatePage {
            name,
            properties,
            graph_id,
            graph_name,
        } => {
            // Call kg_api to create the page

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true, // allow smart default
            )
            .await?;

            // Convert HashMap<String, String> to serde_json::Value
            let properties_json = properties.map(|props| {
                serde_json::Value::Object(
                    props
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                        .collect(),
                )
            });

            state
                .create_page(name.clone(), properties_json, &resolved_graph_id)
                .await?;
            let data = serde_json::json!({ "page_name": name });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::DeletePage {
            page_name,
            graph_id,
            graph_name,
        } => {
            // Delete a page

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                true, // allow smart default
            )
            .await?;

            state
                .delete_page(page_name.clone(), &resolved_graph_id)
                .await?;
            let data = serde_json::json!({ "page_name": page_name });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::OpenGraph {
            graph_id,
            graph_name,
        } => {
            // Open a graph

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false, // no smart default - must specify which graph to open
            )
            .await?;

            let graph_info = state.open_graph(resolved_graph_id).await?;
            send_success_response(connection_id, state, Some(graph_info)).await?;
        }
        Command::CloseGraph {
            graph_id,
            graph_name,
        } => {
            // Close a graph

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false, // no smart default - must specify which graph to close
            )
            .await?;

            state.close_graph(resolved_graph_id).await?;
            send_success_response(connection_id, state, None).await?;
        }
        Command::CreateGraph { name, description } => {
            // Create a new graph

            let graph_info = state.create_graph(name, description).await?;
            send_success_response(connection_id, state, Some(graph_info)).await?;
        }
        Command::DeleteGraph {
            graph_id,
            graph_name,
        } => {
            // Delete a graph

            let resolved_graph_id = resolve_graph_for_command(
                state,
                graph_id.as_deref(),
                graph_name.as_deref(),
                false, // no smart default - must specify which graph to delete
            )
            .await?;

            state.delete_graph(&resolved_graph_id).await?;
            let data = serde_json::json!({ "deleted_graph_id": resolved_graph_id.to_string() });
            send_success_response(connection_id, state, Some(data)).await?;
        }
        Command::ListGraphs => {
            // List all graphs

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
