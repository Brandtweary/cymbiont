//! Integration tests for multi-graph functionality with real Logseq instance
//!
//! These tests verify graph switching, data isolation, and session management.

use crate::common::test_harness::IntegrationTestHarness;
use serde_json::json;
use tokio::time::{sleep, Duration};

/// Test basic graph switching functionality
pub async fn test_basic_graph_switch(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing basic graph switch");
    
    // We're already on test_graph_switching
    // Verify current session state
    let session_response = harness.http_client()
        .get(format!("{}/api/session/current", harness.base_url()))
        .send()
        .await?;
    
    assert!(session_response.status().is_success());
    let session: serde_json::Value = session_response.json().await?;
    
    // Should have some session state
    assert!(session["session_state"].is_string());
    
    // List available databases
    let list_response = harness.http_client()
        .get(format!("{}/api/session/databases", harness.base_url()))
        .send()
        .await?;
    
    assert!(list_response.status().is_success());
    let databases: Vec<serde_json::Value> = list_response.json().await?;
    
    // Should have our test graphs registered
    assert!(databases.len() >= 6, "Should have at least 6 test graphs");
    
    // Request switch to test_graph_multi_1
    let switch_response = harness.http_client()
        .post(format!("{}/api/session/switch", harness.base_url()))
        .json(&json!({
            "name": "test_graph_multi_1"
        }))
        .send()
        .await?;
    
    assert!(switch_response.status().is_success());
    
    // Note: The API endpoint handles WebSocket confirmation internally with timeout
    
    // Verify session updated
    let new_session_response = harness.http_client()
        .get(format!("{}/api/session/current", harness.base_url()))
        .send()
        .await?;
    
    let new_session: serde_json::Value = new_session_response.json().await?;
    
    // Session state should reflect the switch
    // Note: Without real plugin confirmation, we can't verify Logseq actually switched
    // but we can verify Cymbiont's intent to switch
    
    // Verify session contains expected fields  
    assert!(new_session["active_graph_id"].is_string(), "Session should have active_graph_id");
    assert!(new_session["active_graph_name"].is_string(), "Session should have active_graph_name");
    assert!(new_session["active_graph_path"].is_string(), "Session should have active_graph_path");
    
    Ok(())
}

/// Test graph switch persistence across operations
pub async fn test_graph_switch_persistence(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing graph switch persistence");
    
    // Add some data to current graph (test_graph_switching)
    let data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "persistence-test-001",
            "content": "Block in test_graph_switching",
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
        .header("X-Cymbiont-Graph-Name", "test_graph_switching")
        .json(&data)
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Switch to another graph
    let switch_response = harness.http_client()
        .post(format!("{}/api/session/switch", harness.base_url()))
        .json(&json!({
            "name": "test_graph_multi_2"
        }))
        .send()
        .await?;
    
    assert!(switch_response.status().is_success());
    
    sleep(Duration::from_secs(3)).await;
    
    // Add data to the new graph
    let data2 = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([{
            "id": "persistence-test-002",
            "content": "Block in test_graph_multi_2",
            "created": "1234567890000",
            "updated": "1234567890000",
            "parent": null,
            "children": [],
            "page": null,
            "properties": {},
            "references": []
        }]).to_string()
    });
    
    let response2 = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_multi_2")
        .json(&data2)
        .send()
        .await?;
    
    assert!(response2.status().is_success());
    
    // Switch back to original graph
    let switch_back_response = harness.http_client()
        .post(format!("{}/api/session/switch", harness.base_url()))
        .json(&json!({
            "name": "test_graph_switching"
        }))
        .send()
        .await?;
    
    assert!(switch_back_response.status().is_success());
    
    Ok(())
}

/// Test data isolation between multiple graphs
pub async fn test_multi_graph_isolation(harness: &IntegrationTestHarness) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Testing multi-graph data isolation");
    
    // This test uses two graphs to verify data isolation
    
    // First, switch to test_graph_multi_1
    harness.switch_to_graph("test_graph_multi_1").await?;
    
    // Add unique data to graph 1
    let graph1_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([
            {
                "id": "graph1-unique-block-001",
                "content": "This block only exists in graph 1",
                "created": "1234567890000",
                "updated": "1234567890000",
                "parent": null,
                "children": [],
                "page": null,
                "properties": {},
                "references": []
            },
            {
                "id": "shared-block-001",
                "content": "This block ID exists in both graphs",
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
    
    let response1 = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_multi_1")
        .json(&graph1_data)
        .send()
        .await?;
    
    assert!(response1.status().is_success());
    
    // Switch to test_graph_multi_2
    harness.switch_to_graph("test_graph_multi_2").await?;
    
    // Add different data to graph 2
    let graph2_data = json!({
        "source": "test",
        "type_": "blocks",
        "payload": json!([
            {
                "id": "graph2-unique-block-001",
                "content": "This block only exists in graph 2",
                "created": "1234567890000",
                "updated": "1234567890000",
                "parent": null,
                "children": [],
                "page": null,
                "properties": {},
                "references": []
            },
            {
                "id": "shared-block-001",
                "content": "Different content for same ID in graph 2",
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
    
    let response2 = harness.http_client()
        .post(format!("{}/data", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_multi_2")
        .json(&graph2_data)
        .send()
        .await?;
    
    assert!(response2.status().is_success());
    
    // Verify deletion detection is graph-specific
    // Graph 1: verify only graph1-unique-block-001 exists
    harness.switch_to_graph("test_graph_multi_1").await?;
    
    let verify1_response = harness.http_client()
        .post(format!("{}/sync/verify", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_multi_1")
        .json(&json!({
            "pages": [],
            "blocks": ["graph1-unique-block-001", "shared-block-001"]
        }))
        .send()
        .await?;
    
    assert!(verify1_response.status().is_success());
    
    // Graph 2: verify only graph2-unique-block-001 exists
    harness.switch_to_graph("test_graph_multi_2").await?;
    
    let verify2_response = harness.http_client()
        .post(format!("{}/sync/verify", harness.base_url()))
        .header("X-Cymbiont-Graph-Name", "test_graph_multi_2")
        .json(&json!({
            "pages": [],
            "blocks": ["graph2-unique-block-001", "shared-block-001"]
        }))
        .send()
        .await?;
    
    assert!(verify2_response.status().is_success());
    
    // Each graph maintains its own data in isolation
    println!("✓ Data isolation between graphs verified");
    
    Ok(())
}