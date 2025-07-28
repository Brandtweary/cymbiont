//! Integration tests for session management and multi-graph support
//!
//! These tests verify that Cymbiont can:
//! - Launch with a specific graph
//! - Switch between graphs
//! - Maintain proper graph isolation
//! - Track active graphs correctly

use tokio::time::{sleep, Duration};
use reqwest;
use serde_json::json;

/// Helper to wait for server to be ready
async fn wait_for_server(base_url: &str, max_attempts: u32) -> Result<(), String> {
    for attempt in 1..=max_attempts {
        match reqwest::get(format!("{}/", base_url)).await {
            Ok(response) if response.status().is_success() => {
                println!("Server ready after {} attempts", attempt);
                return Ok(());
            }
            _ => {
                if attempt < max_attempts {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    Err("Server failed to start".to_string())
}

// Removed wait_for_plugin_init - this test doesn't simulate plugin connection
// It tests session management at the server level without a real plugin

// TODO: This test is disabled because it cannot properly verify graph switches
// without either:
// 1. A real Logseq instance that responds to logseq:// URL scheme
// 2. Plugin connecting and confirming the switch
// 3. Mocking the platform URL handler
// 
// The test would always pass even if switching failed because we're just
// checking what the server *thinks* happened, not what actually happened.
#[ignore]
#[tokio::test]
async fn test_session_management_graph_switching() {
    // Copy test config to working directory
    std::fs::copy("config.test.yaml", "config.yaml")
        .expect("Failed to copy test config");
    
    // Start Cymbiont server with dummy_graph
    let mut server_process = std::process::Command::new("cargo")
        .args(&["run", "--", "--graph", "dummy_graph", "--duration", "15"])
        .env("RUST_LOG", "debug")
        .spawn()
        .expect("Failed to start Cymbiont server");
        
    // Give server time to start
    sleep(Duration::from_secs(3)).await;
    
    let base_url = "http://127.0.0.1:3000";
    
    // Wait for server to be ready
    if let Err(e) = wait_for_server(base_url, 20).await {
        server_process.kill().ok();
        panic!("Server startup failed: {}", e);
    }
    
    // Note: This test doesn't simulate plugin initialization
    // It relies on the --graph CLI argument to set the initial graph
    
    // Get initial session state
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/api/session/current", base_url))
        .send()
        .await
        .expect("Failed to get session");
        
    assert!(response.status().is_success(), "Failed to get current session");
    
    let session_data: serde_json::Value = response.json().await.unwrap();
    println!("Initial session: {:?}", session_data);
    
    // The session should show dummy_graph as active (or starting)
    let active_name = session_data["active_graph_name"].as_str();
    if let Some(name) = active_name {
        assert_eq!(name, "dummy_graph", "Initial graph should be dummy_graph");
    }
    
    // Wait a bit for Logseq to fully initialize
    sleep(Duration::from_secs(5)).await;
    
    // Now switch to dummy_graph_2
    let switch_response = client
        .post(format!("{}/api/session/switch", base_url))
        .json(&json!({
            "path": "logseq_databases/dummy_graph_2"
        }))
        .send()
        .await
        .expect("Failed to switch database");
        
    assert!(switch_response.status().is_success(), "Failed to switch to dummy_graph_2");
    
    // Give time for the switch to complete
    sleep(Duration::from_secs(3)).await;
    
    // Verify the switch by checking session again
    let verify_response = client
        .get(format!("{}/api/session/current", base_url))
        .send()
        .await
        .expect("Failed to get session after switch");
        
    let new_session_data: serde_json::Value = verify_response.json().await.unwrap();
    println!("Session after switch: {:?}", new_session_data);
    
    // Should now show dummy_graph_2 as active
    let new_active_path = new_session_data["active_graph_path"].as_str();
    if let Some(path) = new_active_path {
        assert!(path.contains("dummy_graph_2"), "Should have switched to dummy_graph_2");
    }
    
    // List all databases to verify both are registered
    let list_response = client
        .get(format!("{}/api/session/databases", base_url))
        .send()
        .await
        .expect("Failed to list databases");
        
    let databases: Vec<serde_json::Value> = list_response.json().await.unwrap();
    println!("Registered databases: {:?}", databases);
    
    // Should have at least 2 databases registered
    assert!(databases.len() >= 2, "Should have at least 2 databases registered");
    
    // Verify both dummy graphs are present
    let has_dummy_graph = databases.iter().any(|db| {
        db["path"].as_str()
            .map(|p| p.contains("dummy_graph") && !p.contains("dummy_graph_2"))
            .unwrap_or(false)
    });
    
    let has_dummy_graph_2 = databases.iter().any(|db| {
        db["path"].as_str()
            .map(|p| p.contains("dummy_graph_2"))
            .unwrap_or(false)
    });
    
    assert!(has_dummy_graph, "dummy_graph should be registered");
    assert!(has_dummy_graph_2, "dummy_graph_2 should be registered");
    
    // Clean up
    server_process.kill().ok();
    
    // Give time for graceful shutdown
    sleep(Duration::from_secs(2)).await;
}

// TODO: This test is also disabled for similar reasons - it can't verify
// that Logseq actually opens with the persisted graph
#[ignore]
#[tokio::test]
async fn test_session_persistence() {
    // This test verifies that session state persists across restarts
    
    // First, start server and let it create a session file
    let mut server1 = std::process::Command::new("cargo")
        .args(&["run", "--", "--graph", "dummy_graph", "--duration", "10"])
        .env("RUST_LOG", "info")
        .spawn()
        .expect("Failed to start first server instance");
        
    sleep(Duration::from_secs(5)).await;
    server1.kill().ok();
    sleep(Duration::from_secs(2)).await;
    
    // Now start server without specifying a graph - it should use last active
    let mut server2 = std::process::Command::new("cargo")
        .args(&["run", "--", "--duration", "10"])
        .env("RUST_LOG", "info")
        .spawn()
        .expect("Failed to start second server instance");
        
    sleep(Duration::from_secs(3)).await;
    
    let base_url = "http://127.0.0.1:3000";
    let client = reqwest::Client::new();
    
    // Check if it remembers the last active graph
    if let Ok(response) = client.get(format!("{}/api/session/current", base_url)).send().await {
        if let Ok(session_data) = response.json::<serde_json::Value>().await {
            println!("Restored session: {:?}", session_data);
            // Should have some active graph from previous session
            assert!(session_data["active_graph_id"].as_str().is_some(), "Should have restored active graph");
        }
    }
    
    server2.kill().ok();
    sleep(Duration::from_secs(1)).await;
}