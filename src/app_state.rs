//! Application State Management - CQRS Resource Container
//!
//! `AppState` is the central resource container for Cymbiont's CQRS-based knowledge
//! graph engine. It provides read-only access to graphs and agent state while routing all mutations
//! through the `CommandQueue` for deadlock-free operation.
//!
//! ## CQRS Architecture
//!
//! ### Command-Query Responsibility Segregation
//! - **Mutations**: All state changes go through `command_queue.execute()`
//! - **Queries**: Direct read access to `graph_managers` and `agent`
//! - **Ownership**: `CommandProcessor` owns mutable state, `AppState` holds Arc references
//! - **Concurrency**: Single-threaded command processing eliminates deadlocks
//!
//! ### Resource Container Pattern
//! `AppState` is a **pure resource container** - all fields are public for direct access.
//! Business logic belongs in domain modules:
//! - Graph operations → `GraphOps` trait (via `CommandQueue`)
//! - Agent operations → Agent methods (via `CommandQueue` for mutations)
//! - Registry operations → Registry methods (via `CommandQueue` for mutations)
//! - Recovery → `CommandProcessor` startup
//!
//! ## Core Resources
//!
//! ### CQRS Command System
//! - **`command_queue`**: Primary interface for all mutations
//! - **`CommandProcessor`**: Single-threaded owner of all mutable state
//! - **`CommandLog`**: CQRS command persistence
//!
//! ### Knowledge Graphs
//! - **`graph_managers`**: `HashMap` of active graph managers (read-only from `AppState`)
//! - **`graph_registry`**: Metadata and persistence for graph lifecycle
//! - **Multi-graph**: Isolated knowledge domains with lazy loading
//!
//! ### Agent System  
//! - **`agent`**: Agent state and conversation management (read-only from `AppState`)
//! - **Persistence**: Automatically loaded/created on startup
//!
//! ### Server Infrastructure
//! - **`ws_connections`**: WebSocket connection tracking (optional)
//! - **`auth_token`**: Token-based authentication
//! - **`operation_freeze`**: Test infrastructure for deterministic timing
//!
//! ## Key Methods
//!
//! ### Initialization
//! ```rust
//! let app_state = AppState::new_with_config(config, data_dir, with_server).await?;
//! ```
//!
//! ### CQRS Operations
//! ```rust
//! // All mutations via CommandQueue
//! let response = app_state.command_queue.execute(
//!     Command::Graph(GraphCommand::CreateBlock {
//!         graph_id, content, parent_id
//!     })
//! ).await?;
//!
//! // Direct read access
//! let graphs = app_state.graph_managers.read().await;
//! let agent = app_state.agent.read().await;
//! ```
//!
//! ### Graceful Shutdown
//! ```rust
//! let active_count = app_state.initiate_graceful_shutdown().await;
//! let completed = app_state.wait_for_transactions(Duration::from_secs(30)).await;
//! app_state.shutdown().await;
//! ```
//!
//! ## Architecture Benefits
//!
//! ### Deadlock Prevention
//! - **Single-threaded mutations**: `CommandProcessor` owns all mutable state
//! - **Lock-free reads**: Direct `HashMap` access for queries
//! - **No lock ordering**: Eliminates complex lock hierarchies
//!
//! ### ACID Guarantees
//! - **Atomicity**: Each command is atomic
//! - **Consistency**: `CommandProcessor` enforces invariants
//! - **Isolation**: Sequential command processing
//! - **Durability**: `CommandLog` persists all mutations

use axum::extract::ws::Message;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tokio::time::sleep;
use tracing::{error, warn};
use uuid::Uuid;

use crate::cqrs::{Command, SystemCommand};
use crate::error::{CymbiontError, Result};
use crate::utils::{save_all_system_data, AsyncRwLockExt};

use crate::{
    config::Config,
    cqrs::{CommandProcessor, CommandQueue},
    graph::graph_manager::GraphManager,
    graph::graph_registry::GraphRegistry,
    http_server::{auth::initialize_auth, websocket::WsConnection},
};

/// Central application state that coordinates all Cymbiont components
///
/// ARCHITECTURAL RULE: `AppState` is a pure resource container.
/// DO NOT add helper methods here. All fields are public.
/// Business logic belongs in domain modules (`GraphOps`, Agent, registries).
pub struct AppState {
    // CQRS command queue - all mutations go through here
    pub command_queue: CommandQueue,

    // Direct read access to graphs (references from CommandProcessor)
    pub graph_managers: Arc<RwLock<HashMap<Uuid, Arc<RwLock<GraphManager>>>>>,

    // Registry for graph metadata and persistence
    pub graph_registry: Arc<RwLock<GraphRegistry>>,

    pub config: Config,
    pub data_dir: PathBuf, // Resolved absolute path

    // Server-specific components (optional)
    pub ws_ready_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub ws_connections: Option<Arc<RwLock<HashMap<String, WsConnection>>>>,
    pub auth_token: Arc<RwLock<Option<String>>>, // Authentication token

    // Shutdown coordination
    pub shutdown_initiated: Arc<AtomicBool>, // Flag to prevent new transactions
}

impl AppState {
    /// Create new `AppState` with pre-loaded config (avoids duplicate config loading)
    pub async fn new_with_config(
        mut config: Config,
        data_dir_override: Option<String>,
        with_server: bool,
    ) -> Result<Arc<Self>> {
        // Apply data_dir override if provided
        if let Some(cli_data_dir) = &data_dir_override {
            config.data_dir.clone_from(cli_data_dir);
        }

        Self::new_internal_with_config(config, with_server).await
    }

    async fn new_internal_with_config(config: Config, with_server: bool) -> Result<Arc<Self>> {
        // Initialize data directory
        let data_dir = if Path::new(&config.data_dir).is_absolute() {
            PathBuf::from(&config.data_dir)
        } else {
            env::current_dir()
                .map_err(|e| CymbiontError::Other(format!("Failed to get current directory: {e}")))?
                .join(&config.data_dir)
        };
        fs::create_dir_all(&data_dir)
            .map_err(|e| CymbiontError::Other(format!("Failed to create data directory: {e}")))?;

        // Load existing registry or create new
        let registry_path = data_dir.join("graph_registry.json");
        let mut graph_registry_inner = GraphRegistry::load(&registry_path).unwrap_or_else(|e| {
            warn!("Failed to load graph registry: {}, starting fresh", e);
            GraphRegistry::new()
        });
        graph_registry_inner.set_data_dir(&data_dir);
        let graph_registry = Arc::new(RwLock::new(graph_registry_inner));

        // Create the command processor with registry reference
        let processor =
            CommandProcessor::new(graph_registry.clone(), data_dir.clone());

        // Start the processor
        let (command_queue, resources) = processor.start();

        // Create WebSocket connections if server mode
        let ws_connections = if with_server {
            Some(Arc::new(RwLock::new(HashMap::new())))
        } else {
            None
        };

        let app_state = Arc::new(Self {
            command_queue,
            graph_managers: resources.graph_managers,
            graph_registry,
            config,
            data_dir: data_dir.clone(),
            ws_ready_tx: Mutex::new(None),
            ws_connections,
            auth_token: Arc::new(RwLock::new(None)),
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
        });

        // Note: Graphs and agents will be loaded from JSON during startup in main.rs

        // Initialize authentication if in server mode
        if with_server {
            let token = initialize_auth(&app_state).await?;
            if !app_state.config.auth.disabled {
                let mut token_guard = app_state
                    .auth_token
                    .write_or_panic("initialize auth token")
                    .await;
                *token_guard = Some(token);
            }
        }

        Ok(app_state)
    }
    // NOTE: All business logic methods have been removed from AppState.
    // AppState is now a pure resource container. Access the public fields directly.
    // For operations:
    // - Graph operations: Use GraphOps trait (implemented on Arc<AppState>)
    // - Agent operations: Access agent through app_state.agent
    // - Registry operations: Access registry through app_state.graph_registry

    /// Shutdown - cleanup and export all data
    pub async fn shutdown(self: &Arc<Self>) {
        // Close all WebSocket connections
        if let Some(ref connections) = self.ws_connections {
            let mut conn_map = connections.write_or_panic("cleanup connections").await;
            let connection_count = conn_map.len();

            if connection_count > 0 {
                // Send Close frame to all connections before shutting down
                for (_, conn) in conn_map.iter() {
                    // Send WebSocket Close frame
                    let close_msg = Message::Close(None);
                    let _ = conn.sender.send(close_msg);

                    // Then send shutdown signal
                    let _ = conn.shutdown_tx.send(true);
                }

                // Clear the connections
                conn_map.clear();
                drop(conn_map);

                // Give tasks a moment to shut down gracefully
                sleep(Duration::from_millis(100)).await;
            }
        }

        // Save all system data to JSON
        if let Err(e) = save_all_system_data(self).await {
            error!("Failed to save system data: {}", e);
        }
    }

    /// Initiate graceful shutdown on the CQRS command queue
    pub async fn initiate_graceful_shutdown(&self) -> usize {
        // Set the local shutdown flag to prevent new operations
        self.shutdown_initiated.store(true, Ordering::Release);

        // Send shutdown command to the processor
        if let Ok(result) = self
            .command_queue
            .execute(Command::System(SystemCommand::InitiateShutdown))
            .await
        {
            if let Some(data) = result.data {
                if let Some(count) = data.get("active_count").and_then(serde_json::Value::as_u64) {
                    #[allow(clippy::cast_possible_truncation)] // active command count is small
                    return count as usize;
                }
            }
        }

        0
    }

    /// Wait for active commands to complete
    pub async fn wait_for_transactions(&self, timeout: Duration) -> bool {
        if let Ok(result) = self
            .command_queue
            .execute(Command::System(SystemCommand::WaitForCompletion {
                timeout_secs: timeout.as_secs(),
            }))
            .await
        {
            if let Some(data) = result.data {
                if let Some(completed) = data.get("completed").and_then(serde_json::Value::as_bool)
                {
                    return completed;
                }
            }
        }

        false
    }

    /// Force flush transactions for immediate shutdown
    pub async fn force_flush_transactions(&self) {
        let _ = self
            .command_queue
            .execute(Command::System(SystemCommand::ForceFlush))
            .await;
    }
}
