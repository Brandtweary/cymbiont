/**
 * @module auth
 * @description Token-based authentication system with auto-generation and rotation
 * 
 * This module implements a simple but effective authentication system for Cymbiont's
 * HTTP and WebSocket APIs. It generates cryptographically secure tokens on startup,
 * saves them to the filesystem with restricted permissions, and validates incoming
 * requests against the stored token.
 * 
 * ## Key Features
 * 
 * - **Auto-generation**: Creates UUID v4 tokens on server startup
 * - **Token rotation**: New token generated on each restart for security
 * - **File persistence**: Token saved to `{data_dir}/auth_token` 
 * - **Restricted permissions**: Unix mode 0600 (owner read/write only)
 * - **HTTP middleware**: Protects sensitive endpoints via Authorization header
 * - **WebSocket integration**: Validates tokens in Auth command
 * - **Configuration options**: Fixed token or disable auth via config.yaml
 * 
 * ## Usage
 * 
 * ### HTTP Authentication
 * ```
 * Authorization: Bearer <token>
 * Authorization: <token>
 * ```
 * 
 * ### WebSocket Authentication
 * ```json
 * {"type": "auth", "token": "<token>"}
 * ```
 * 
 * ## Security Model
 * 
 * The authentication system is designed for local/trusted environments:
 * - Token is readable by any process with filesystem access
 * - No token expiration or refresh mechanism
 * - No user management or role-based access control
 * - Suitable for single-user or trusted multi-user scenarios
 * 
 * For production deployments, consider using a reverse proxy with
 * proper TLS termination and more robust authentication.
 */

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::error::*;
use crate::lock::AsyncRwLockExt;
use crate::AppState;

/// Generate a new cryptographically secure auth token
pub fn generate_auth_token() -> String {
    Uuid::new_v4().to_string()
}

/// Save auth token to file for external access with restricted permissions
pub async fn save_auth_token(data_dir: &Path, token: &str) -> std::io::Result<()> {
    let token_path = data_dir.join("auth_token");
    tokio::fs::write(&token_path, token).await?;
    
    // Set restrictive permissions (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&token_path, permissions).await?;
    }
    
    Ok(())
}

/// Initialize authentication based on configuration
pub async fn initialize_auth(
    app_state: &AppState,
) -> Result<String> {
    // Check if auth is disabled
    if app_state.config.auth.disabled {
        warn!("⚠️  Authentication is DISABLED - all endpoints are open!");
        return Ok(String::new());
    }
    
    // Check for configured token
    if let Some(configured_token) = &app_state.config.auth.token {
        info!("🔐 Using configured authentication token (token rotation disabled)");
        save_auth_token(&app_state.data_dir, configured_token).await
            .map_err(|e| ServerError::startup(format!("Failed to save configured auth token: {}", e)))?;
        info!("");
        info!("Warning: Token rotation is disabled when using a configured token");
        info!("For better security, remove the 'auth.token' config and use auto-generated tokens");
        return Ok(configured_token.clone());
    }
    
    // Always generate new token on startup for security (token rotation)
    // Only skip if user has explicitly configured a token
    let new_token = generate_auth_token();
    save_auth_token(&app_state.data_dir, &new_token).await
        .map_err(|e| ServerError::startup(format!("Failed to save generated auth token: {}", e)))?;
    
    info!("🔐 Auth token: {} (saved to {}/auth_token)", new_token, app_state.data_dir.display());
    
    Ok(new_token)
}

/// Axum middleware for HTTP authentication
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // Check if auth is disabled
    if state.config.auth.disabled {
        return Ok(next.run(request).await);
    }
    
    // Get the stored auth token
    let token_guard = state.auth_token.read_or_panic("auth middleware - read token").await;
    let expected_token = match &*token_guard {
        Some(token) => token,
        None => {
            error!("No auth token configured but auth is enabled");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    
    // Check Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    
    if let Some(auth_value) = auth_header {
        // Support both "Bearer TOKEN" and just "TOKEN"
        let token = if auth_value.starts_with("Bearer ") {
            &auth_value[7..]
        } else {
            auth_value
        };
        
        if token == expected_token {
            return Ok(next.run(request).await);
        }
    }
    
    // Unauthorized
    Err(StatusCode::UNAUTHORIZED)
}

/// Validate a token against the configured auth token
pub async fn validate_token(app_state: &AppState, token: &str) -> bool {
    // Check if auth is disabled
    if app_state.config.auth.disabled {
        return true;
    }
    
    let token_guard = app_state.auth_token.read_or_panic("verify websocket auth - read token").await;
    match &*token_guard {
        Some(expected_token) => token == expected_token,
        None => {
            error!("No auth token configured but auth is enabled");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_auth_token() {
        let token1 = generate_auth_token();
        let token2 = generate_auth_token();
        
        // Should be UUIDs
        assert_eq!(token1.len(), 36);
        assert_eq!(token2.len(), 36);
        
        // Should be different
        assert_ne!(token1, token2);
    }
}