//! @module misc_commands
//! @description System, authentication, and test command handlers
//!
//! This module implements system-level WebSocket commands including
//! authentication, testing utilities, and operation control mechanisms
//! for deterministic testing scenarios.
//!
//! ## Command Categories
//!
//! ### Authentication
//! - `Auth`: Authenticate connection with token
//!
//! ### Testing
//! - `Test`: Echo test with connection statistics
//! - `Heartbeat`: Client keep-alive (acknowledged but no response)
//! - `TestCliCommand`: CLI command bridge (debug builds only)
//!

//! ## Authentication Flow
//!
//! 1. Client connects via WebSocket (no auth required)
//! 2. Client sends Auth command with token
//! 3. Token validated against stored auth_token
//! 4. Connection marked as authenticated
//! 5. First auth triggers ws_ready signal
//!
//! ## Heartbeat Design
//!
//! Client heartbeats are acknowledged internally but don't generate
//! responses to prevent infinite loops. The server sends its own
//! heartbeats every 30 seconds independently.
//!
//! ## Integration
//!
//! - Uses auth module for token validation
//! - Updates connection state for authentication
//! - Bridges to CLI module for command testing (debug only)

use crate::error::*;
use crate::server::auth::validate_token;
use crate::server::websocket::Command;
use crate::server::websocket_utils::{
    get_connection_stats, send_success_response, set_authenticated,
};
use crate::AppState;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Main handler function for miscellaneous commands
pub async fn handle(command: Command, connection_id: &str, state: &Arc<AppState>) -> Result<()> {
    match command {
        Command::Auth { token } => {
            // Validate token against configured auth token
            if !validate_token(state, &token).await {
                warn!(
                    "🔐 WebSocket authentication failed for {}: invalid token",
                    connection_id
                );
                return Err(
                    ServerError::authentication("Failed to authenticate: invalid token").into(),
                );
            }

            // Set authenticated (atomic operation)
            match set_authenticated(connection_id, state).await {
                Ok(is_first) => {
                    // Send success response
                    send_success_response(connection_id, state, None).await?;
                    info!("🔐 WebSocket authenticated: {}", connection_id);

                    // Signal that WebSocket is ready if this is the first authenticated connection
                    if is_first {
                        if let Ok(mut tx_guard) = state.ws_ready_tx.lock() {
                            if let Some(tx) = tx_guard.take() {
                                let _ = tx.send(());
                                info!("📡 WebSocket ready signal sent");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to authenticate connection {}: {}", connection_id, e);
                    return Err(ServerError::authentication(format!(
                        "Failed to authenticate: {}",
                        e
                    ))
                    .into());
                }
            }
        }
        Command::Test { message } => {
            // Test command - just echo back the message with some stats

            let (total, authenticated) = get_connection_stats(state).await;
            let response_data = serde_json::json!({
                "echo": message,
                "connection_id": connection_id,
                "total_connections": total,
                "authenticated_connections": authenticated,
            });

            send_success_response(connection_id, state, Some(response_data)).await?;
        }
        Command::Heartbeat => {
            // Client sent a heartbeat/pong - just acknowledge receipt, don't respond
            // This prevents infinite heartbeat loops
        }

        // Command for CLI integration testing (only available in debug builds)
        #[cfg(debug_assertions)]
        Command::TestCliCommand { command, params } => {
            // Dispatch to CLI module
            let exit_after = match crate::cli::dispatch_cli_command(state, &command, &params).await
            {
                Ok(exit) => exit,
                Err(e) => {
                    error!("CLI command failed: {}", e);
                    return Err(ServerError::websocket(format!("CLI command failed: {}", e)).into());
                }
            };

            // Return result
            let data = serde_json::json!({
                "exit_after": exit_after,
                "command": command
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
