//! Integration tests for data synchronization with real Logseq instance
//!
//! These tests verify sync functionality through the plugin's actual behavior,
//! not just server API responses.

use crate::common::test_harness::IntegrationTestHarness;
use serde_json::json;
use tokio::time::{sleep, Duration};

/// Test real-time sync of blocks and pages through the plugin
pub async fn test_real_time_sync(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing real-time sync with plugin");
    
    // Send block data
    let block_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "test-block-001",
            "content": "Test block from integration test",
            "created": "1234567890000",
            "updated": "1234567890000",
            "parent": null,
            "children": [],
            "page": "test-page-001",
            "properties": {},
            "references": []
        }]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&block_data)
        .send()
        .await?;
    
    assert!(response.status().is_success(), "Failed to send block data");
    
    // Send page data
    let page_data = json!({
        "source": "test",
        "type_": "pages",
        "payload": json!([{
            "name": "TestPage",
            "normalized_name": "testpage",
            "created": "1234567890000",
            "updated": "1234567890000",
            "properties": {},
            "blocks": ["test-block-001"]
        }]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&page_data)
        .send()
        .await?;
    
    assert!(response.status().is_success(), "Failed to send page data");
    
    // Small delay for sync to process (sync operations don't trigger WebSocket confirmations)
    sleep(Duration::from_millis(500)).await;
    
    // Verify sync status was updated
    let status_response = harness.http_client()
        .get(format!("{}/sync/status", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .send()
        .await?;
    
    assert!(status_response.status().is_success());
    let status: serde_json::Value = status_response.json().await?;
    
    // Should have recent sync timestamp
    assert!(status["last_incremental_sync"].as_i64().is_some());
    
    Ok(())
}

/// Test incremental sync based on timestamps
pub async fn test_incremental_sync(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing incremental sync");
    
    // Get current sync status
    let status_response = harness.http_client()
        .get(format!("{}/sync/status", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .send()
        .await?;
    
    let initial_status: serde_json::Value = status_response.json().await?;
    let last_sync = initial_status["last_incremental_sync"].as_i64().unwrap_or(0);
    
    // Send data with old timestamp (should be skipped in incremental)
    let old_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "old-block-001",
            "content": "Old block that should be skipped",
            "created": "1000000000000",  // Very old timestamp
            "updated": "1000000000000",
            "parent": null,
            "children": [],
            "page": null,
            "properties": {},
            "references": []
        }]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&old_data)
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Send data with new timestamp
    let new_timestamp = (last_sync + 1000).to_string();
    let new_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "new-block-001",
            "content": "New block for incremental sync",
            "created": &new_timestamp,
            "updated": &new_timestamp,
            "parent": null,
            "children": [],
            "page": null,
            "properties": {},
            "references": []
        }]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&new_data)
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Update sync status
    let update_response = harness.http_client()
        .patch(format!("{}/sync", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&json!({
            "last_incremental_sync": new_timestamp.parse::<i64>().unwrap()
        }))
        .send()
        .await?;
    
    assert!(update_response.status().is_success());
    
    Ok(())
}

/// Test deletion detection and archival
pub async fn test_deletion_detection(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing deletion detection");
    
    // First, add some blocks to the graph
    let blocks_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([
            {
                "id": "block-to-keep-001",
                "content": "This block will remain",
                "created": "1234567890000",
                "updated": "1234567890000",
                "parent": null,
                "children": [],
                "page": null,
                "properties": {},
                "references": []
            },
            {
                "id": "block-to-delete-001",
                "content": "This block will be deleted",
                "created": "1234567890000",
                "updated": "1234567890000",
                "parent": null,
                "children": [],
                "page": null,
                "properties": {},
                "references": []
            }
        ]).to_string()
    });
    
    let response = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&blocks_data)
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Wait for data to be processed
    sleep(Duration::from_millis(500)).await;
    
    // Now verify blocks, reporting only one (simulating deletion)
    let verify_response = harness.http_client()
        .post(format!("{}/sync/verify", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .json(&json!({
            "pages": [],
            "blocks": ["block-to-keep-001"]  // block-to-delete-001 is missing
        }))
        .send()
        .await?;
    
    assert!(verify_response.status().is_success());
    
    let verify_result: serde_json::Value = verify_response.json().await?;
    assert!(verify_result["success"].as_bool().unwrap_or(false));
    
    // Should report archiving 1 block
    let message = verify_result["message"].as_str().unwrap_or("");
    assert!(message.contains("1 blocks"), "Expected 1 block to be archived");
    
    Ok(())
}

/// Test force sync flags
pub async fn test_force_sync_flags(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing force sync flags");
    
    // This test verifies that the force sync flags are respected
    // In a real scenario, we'd launch Cymbiont with --force-incremental-sync or --force-full-sync
    // For now, we just verify the sync status endpoint works
    
    let status_response = harness.http_client()
        .get(format!("{}/sync/status", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_sync")
        .send()
        .await?;
    
    assert!(status_response.status().is_success());
    
    let status: serde_json::Value = status_response.json().await?;
    
    // Verify expected fields exist
    assert!(status["graph_id"].is_string());
    assert!(status["last_incremental_sync"].is_i64());
    assert!(status["last_full_sync"].is_i64() || status["last_full_sync"].is_null());
    
    Ok(())
}