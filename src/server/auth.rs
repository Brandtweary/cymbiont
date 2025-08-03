/**
 * Authentication Module
 * 
 * Provides token-based authentication for both HTTP and WebSocket endpoints.
 * Generates cryptographically secure tokens and manages token persistence.
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

use crate::AppState;

/// Generate a new cryptographically secure auth token
pub fn generate_auth_token() -> String {
    Uuid::new_v4().to_string()
}

/// Save auth token to file for external access with restricted permissions
pub async fn save_auth_token(data_dir: &Path, token: &str) -> Result<(), std::io::Error> {
    let token_path = data_dir.join("auth_token");
    tokio::fs::write(&token_path, token).await?;
    
    // Set restrictive permissions (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&token_path, permissions).await?;
    }
    
    info!("🔐 Authentication token saved to: {}", token_path.display());
    Ok(())
}

/// Initialize authentication based on configuration
pub async fn initialize_auth(
    app_state: &AppState,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Check if auth is disabled
    if app_state.config.auth.disabled {
        warn!("⚠️  Authentication is DISABLED - all endpoints are open!");
        return Ok(String::new());
    }
    
    // Check for configured token
    if let Some(configured_token) = &app_state.config.auth.token {
        info!("🔐 Using configured authentication token (token rotation disabled)");
        save_auth_token(&app_state.data_dir, configured_token).await?;
        info!("");
        info!("Warning: Token rotation is disabled when using a configured token");
        info!("For better security, remove the 'auth.token' config and use auto-generated tokens");
        return Ok(configured_token.clone());
    }
    
    // Always generate new token on startup for security (token rotation)
    // Only skip if user has explicitly configured a token
    info!("🔐 Generating new authentication token (token rotation enabled)");
    
    // Generate new token
    let new_token = generate_auth_token();
    save_auth_token(&app_state.data_dir, &new_token).await?;
    
    info!("🔐 Authentication token: {}", new_token);
    info!("📁 Token saved to: {}/auth_token", app_state.data_dir.display());
    info!("");
    info!("To connect:");
    info!("- Automated tools: Read token from {}/auth_token", app_state.data_dir.display());
    info!("- Manual: Use the token above");
    info!("");
    info!("Note: Token rotates on each server restart for security");
    
    Ok(new_token)
}

/// Axum middleware for HTTP authentication
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check if auth is disabled
    if state.config.auth.disabled {
        return Ok(next.run(request).await);
    }
    
    // Get the stored auth token
    let token_guard = state.auth_token.read().await;
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
    
    let token_guard = app_state.auth_token.read().await;
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