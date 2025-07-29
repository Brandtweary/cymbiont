//! Integration tests for WebSocket functionality with real Logseq instance
//!
//! These tests verify WebSocket communication through user-level actions:
//! HTTP API calls that should trigger WebSocket notifications, and verification
//! that the WebSocket connection is healthy and receiving expected messages.

use crate::common::test_harness::IntegrationTestHarness;
use serde_json::json;
use tokio::time::{sleep, Duration};

/// Test that WebSocket connection is established and healthy
pub async fn test_websocket_connection(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing WebSocket connection health");
    
    // Verify WebSocket is connected
    let is_connected = harness.verify_websocket_connected().await?;
    assert!(is_connected, "WebSocket should be connected during test setup");
    
    // Clear any existing messages for clean test
    harness.clear_websocket_messages().await?;
    
    println!("✓ WebSocket connection verified");
    Ok(())
}

/// Test that sync operations don't trigger WebSocket commands
pub async fn test_sync_operations_no_websocket_commands(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing that sync operations don't trigger WebSocket commands");
    
    // Clear messages for clean test
    harness.clear_websocket_messages().await?;
    
    // NOTE: This test verifies that regular sync operations via /data endpoint
    // do NOT generate WebSocket commands. Only kg_api operations should trigger
    // WebSocket commands to the plugin.
    
    let sync_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "websocket-test-block-001",
            "content": "Block created via sync API",
            "created": "1234567890000",
            "updated": "1234567890000",
            "parent": null,
            "children": [],
            "page": null,
            "properties": {},
            "references": []
        }]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_websocket")
        .json(&sync_data)
        .send()
        .await?;
    
    assert!(response.status().is_success(), "Failed to send sync data");
    
    // Wait briefly for any potential WebSocket activity
    sleep(Duration::from_millis(500)).await;
    
    // Verify we didn't capture any WebSocket commands (sync doesn't trigger them)
    let messages = harness.get_recent_websocket_messages().await?;
    assert!(messages.is_empty(), "Sync operations should not trigger WebSocket commands");
    
    // Verify the connection is still healthy
    let is_connected = harness.verify_websocket_connected().await?;
    assert!(is_connected, "WebSocket should remain connected after API operations");
    
    println!("✓ Verified sync operations don't trigger WebSocket commands");
    Ok(())
}

// Future test: test_kg_api_triggers_websocket_commands
// Once kg_api endpoints are exposed via HTTP, add a test that:
// 1. Calls kg_api add_block endpoint
// 2. Verifies WebSocket sends create_block command to plugin
// 3. Tests the acknowledgment flow (temp_id -> real UUID mapping)

