use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;
use serde_json::{json, Value};
use crate::common::{setup_test_env, cleanup_test_env, get_cymbiont_binary};
use crate::common::test_harness::{TestServer, PreShutdown, PostShutdown, assert_phase};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;

/// WebSocket connection type
type WsConnection = WebSocket<MaybeTlsStream<TcpStream>>;


/// Connect to WebSocket endpoint
fn connect_websocket(port: u16) -> WsConnection {
    let url = format!("ws://localhost:{}/ws", port);
    
    // Retry connection a few times as server may still be initializing WebSocket
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 10;
    
    loop {
        attempts += 1;
        match connect(&url) {
            Ok((socket, _)) => return socket,
            Err(e) => {
                if attempts >= MAX_ATTEMPTS {
                    panic!("Failed to connect to WebSocket after {} attempts: {}", MAX_ATTEMPTS, e);
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Send a command and wait for response
fn send_command(ws: &mut WsConnection, command: Value) -> Value {
    // Send command
    let msg = Message::Text(command.to_string());
    ws.send(msg).expect("Failed to send WebSocket message");
    
    // Wait for response
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let response: Value = serde_json::from_str(&text)
                    .expect("Failed to parse WebSocket response");
                
                // Skip heartbeats
                if response["type"] == "heartbeat" {
                    continue;
                }
                
                return response;
            }
            Ok(Message::Close(_)) => {
                panic!("WebSocket connection closed unexpectedly");
            }
            Ok(_) => continue,  // Skip other message types
            Err(e) => panic!("WebSocket read error: {}", e),
        }
    }
}

/// Expect a success response and return the data
fn expect_success(response: Value) -> Option<Value> {
    assert_eq!(
        response["type"], "success",
        "Expected success response, got: {}",
        response
    );
    response.get("data").cloned()
}

/// Authenticate the WebSocket connection
fn authenticate(ws: &mut WsConnection) {
    let auth_cmd = json!({
        "type": "auth",
        "token": "test-token"
    });
    
    let response = send_command(ws, auth_cmd);
    expect_success(response);
}

/// Test creating a new block
fn test_create_block(ws: &mut WsConnection) -> String {
    let cmd = json!({
        "type": "create_block",
        "content": "WebSocket test block with **bold** and *italic* text",
        "page_name": "cyberorganism-test-1"
    });
    
    let response = send_command(ws, cmd);
    let data = expect_success(response).expect("No data in create_block response");
    
    let block_id = data["block_id"].as_str()
        .expect("No block_id in response")
        .to_string();
    
    block_id
}

/// Test updating an existing block
fn test_update_block(ws: &mut WsConnection, block_id: &str) {
    let cmd = json!({
        "type": "update_block",
        "block_id": block_id,
        "content": "## Types of Knowledge Graphs (Updated via WebSocket)"
    });
    
    let response = send_command(ws, cmd);
    let data = expect_success(response);
    
    assert_eq!(
        data.as_ref().and_then(|d| d["block_id"].as_str()),
        Some(block_id),
        "Updated block ID mismatch"
    );
    
}

/// Test deleting a block
fn test_delete_block(ws: &mut WsConnection, block_id: &str) {
    let cmd = json!({
        "type": "delete_block",
        "block_id": block_id
    });
    
    let response = send_command(ws, cmd);
    let data = expect_success(response);
    
    assert_eq!(
        data.as_ref().and_then(|d| d["block_id"].as_str()),
        Some(block_id),
        "Deleted block ID mismatch"
    );
    
}

/// Test creating a new page
fn test_create_page(ws: &mut WsConnection, page_name: &str) {
    let cmd = json!({
        "type": "create_page",
        "name": page_name,
        "properties": {
            "test-property": "test-value",
            "created-by": "websocket-test"
        }
    });
    
    let response = send_command(ws, cmd);
    let data = expect_success(response);
    
    assert_eq!(
        data.as_ref().and_then(|d| d["page_name"].as_str()),
        Some(page_name),
        "Created page name mismatch"
    );
    
}

/// Test deleting a page
fn test_delete_page(ws: &mut WsConnection, page_name: &str) {
    let cmd = json!({
        "type": "delete_page",
        "page_name": page_name
    });
    
    let response = send_command(ws, cmd);
    let data = expect_success(response);
    
    assert_eq!(
        data.as_ref().and_then(|d| d["page_name"].as_str()),
        Some(page_name),
        "Deleted page name mismatch"
    );
    
}

/// Test graph operations (create and switch)
fn test_graph_operations(ws: &mut WsConnection, data_dir: &std::path::Path) -> String {
    // Get the current active graph ID first
    let registry_path = data_dir.join("graph_registry.json");
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read graph registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse graph registry");
    let original_graph_id = registry["active_graph_id"].as_str()
        .expect("No active graph ID")
        .to_string();
    
    // Create a new graph
    let create_cmd = json!({
        "type": "create_graph",
        "name": "test-websocket-graph",
        "description": "Created via WebSocket test"
    });
    
    let response = send_command(ws, create_cmd);
    let data = expect_success(response).expect("No data in create_graph response");
    
    let new_graph_id = data["id"].as_str()
        .expect("No graph ID in response")
        .to_string();
    
    
    // Switch to the new graph
    let switch_cmd = json!({
        "type": "switch_graph",
        "graph_id": new_graph_id
    });
    
    let response = send_command(ws, switch_cmd);
    let data = expect_success(response).expect("No data in switch_graph response");
    
    assert_eq!(
        data["id"].as_str(),
        Some(new_graph_id.as_str()),
        "Switched graph ID mismatch"
    );
    
    // Switch back to original graph
    let switch_back_cmd = json!({
        "type": "switch_graph",
        "graph_id": &original_graph_id
    });
    
    let response = send_command(ws, switch_back_cmd);
    expect_success(response);
    
    // Delete the test graph
    let delete_cmd = json!({
        "type": "delete_graph",
        "graph_id": &new_graph_id
    });
    
    let response = send_command(ws, delete_cmd);
    let data = expect_success(response).expect("No data in delete_graph response");
    
    assert_eq!(
        data["deleted_graph_id"].as_str(),
        Some(new_graph_id.as_str()),
        "Deleted graph ID mismatch"
    );
    
    
    original_graph_id
}

/// Test error cases
fn test_error_cases(ws: &mut WsConnection, port: u16, active_graph_id: &str) {
    // Test invalid block ID for update
    let cmd = json!({
        "type": "update_block",
        "block_id": "non-existent-block-id",
        "content": "This should fail"
    });
    
    let response = send_command(ws, cmd);
    assert_eq!(
        response["type"], "error",
        "Expected error response for invalid block ID"
    );
    assert!(
        response["message"].as_str().unwrap().contains("not found") ||
        response["message"].as_str().unwrap().contains("Failed to update"),
        "Unexpected error message: {}",
        response["message"]
    );
    
    // Test deleting the currently active graph
    let cmd = json!({
        "type": "delete_graph",
        "graph_id": active_graph_id
    });
    
    let response = send_command(ws, cmd);
    assert_eq!(
        response["type"], "error",
        "Expected error response for deleting active graph"
    );
    assert!(
        response["message"].as_str().unwrap().contains("Cannot delete the currently active graph"),
        "Unexpected error message: {}",
        response["message"]
    );
    
    // Test unauthenticated command (need new connection)
    let mut unauth_ws = connect_websocket(port);
    
    let cmd = json!({
        "type": "create_block",
        "content": "Should fail - not authenticated"
    });
    
    let response = send_command(&mut unauth_ws, cmd);
    assert_eq!(
        response["type"], "error",
        "Expected error response for unauthenticated command"
    );
    assert!(
        response["message"].as_str().unwrap().contains("Not authenticated"),
        "Unexpected error message: {}",
        response["message"]
    );
}

/// Validate the final graph state
fn validate_graph_state(
    data_dir: &std::path::Path,
    new_block_id: &str,
    original_graph_id: &str
) {
    // Read the knowledge graph
    let graph_path = data_dir.join("graphs")
        .join(original_graph_id)
        .join("knowledge_graph.json");
    
    let graph_content = fs::read_to_string(&graph_path)
        .expect("Failed to read knowledge graph");
    
    let graph: Value = serde_json::from_str(&graph_content)
        .expect("Failed to parse knowledge graph");
    
    let nodes = graph["graph"]["nodes"].as_array()
        .expect("No nodes in graph");
    
    // Verify new block exists
    let new_block = nodes.iter()
        .find(|n| n["pkm_id"].as_str() == Some(new_block_id))
        .expect("New block not found in graph");
    assert_eq!(
        new_block["content"].as_str(),
        Some("WebSocket test block with **bold** and *italic* text"),
        "New block content mismatch"
    );
    
    // Verify updated block
    let updated_block = nodes.iter()
        .find(|n| n["pkm_id"].as_str() == Some("67f9a190-985b-4dbf-90e4-c2abffb2ab51"))
        .expect("Updated block not found");
    assert_eq!(
        updated_block["content"].as_str(),
        Some("## Types of Knowledge Graphs (Updated via WebSocket)"),
        "Updated block content mismatch"
    );
    
    // Verify deleted block is NOT in main nodes
    let deleted_block = nodes.iter()
        .find(|n| n["pkm_id"].as_str() == Some("67fbd626-8e4a-485f-ad03-fd1ce5539ebb"));
    assert!(
        deleted_block.is_none(),
        "Deleted block should not be in main nodes"
    );
    
    // Verify new page exists
    let new_page = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Page") && 
            n["pkm_id"].as_str() == Some("test-websocket-page")
        })
        .expect("New page not found in graph");
    assert_eq!(
        new_page["node_type"].as_str(),
        Some("Page"),
        "New page node type mismatch"
    );
    
    // Verify page properties
    let page_props = &new_page["properties"];
    assert_eq!(
        page_props["test-property"].as_str(),
        Some("test-value"),
        "Page property not preserved"
    );
    
    // Verify deleted page is NOT in main nodes
    let deleted_page = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Page") && 
            n["pkm_id"].as_str() == Some("page-to-delete")
        });
    assert!(
        deleted_page.is_none(),
        "Deleted page should not be in main nodes"
    );
    
    // Verify we're back on the original graph
    let registry_path = data_dir.join("graph_registry.json");
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse registry");
    
    assert_eq!(
        registry["active_graph_id"].as_str(),
        Some(original_graph_id),
        "Not switched back to original graph"
    );
    
    // Verify the test graph was deleted
    let graphs = registry["graphs"].as_object()
        .expect("No graphs in registry");
    let test_graph = graphs.values()
        .find(|g| g["name"].as_str() == Some("test-websocket-graph"));
    assert!(
        test_graph.is_none(),
        "Test graph should have been deleted from registry"
    );
}

pub fn test_websocket_commands() {
    // Set up test environment
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone(); // For cleanup after panic
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Import dummy_graph via CLI
        let output = Command::new(get_cymbiont_binary())
            .env("CYMBIONT_TEST_MODE", "1")
            .args(&["--config", test_env.config_path.to_str().unwrap(),
                "--import-logseq", "logseq_databases/dummy_graph/"])
            .output()
            .expect("Failed to run cymbiont import");
        
        assert!(output.status.success(), 
            "Import failed with exit code: {:?}", 
            output.status.code());
        
        // Get the imported graph ID
        let registry_path = test_env.data_dir.join("graph_registry.json");
        let registry_content = fs::read_to_string(&registry_path)
            .expect("Failed to read registry");
        let registry: Value = serde_json::from_str(&registry_content)
            .expect("Failed to parse registry");
        let original_graph_id = registry["active_graph_id"].as_str()
            .expect("No active graph")
            .to_string();
        
        // Start server with WebSocket support
        let server = TestServer::start(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // === Phase 1: Server Running (PreShutdown) ===
        assert_phase(PreShutdown);
        
        // Connect WebSocket client
        let mut ws = connect_websocket(port);
        
        // Authenticate
        authenticate(&mut ws);
        
        // Test Create Block
        let new_block_id = test_create_block(&mut ws);
        
        // Test Update Block (use a different existing block)
        test_update_block(&mut ws, "67f9a190-985b-4dbf-90e4-c2abffb2ab51");
        
        // Test Delete Block (use another existing block)
        test_delete_block(&mut ws, "67fbd626-8e4a-485f-ad03-fd1ce5539ebb");
        
        // Test Create Page
        test_create_page(&mut ws, "test-websocket-page");
        
        // Test Delete Page (create a new page to delete)
        test_create_page(&mut ws, "page-to-delete");
        test_delete_page(&mut ws, "page-to-delete");
        
        // Test Graph Operations
        let final_graph_id = test_graph_operations(&mut ws, &data_dir);
        assert_eq!(final_graph_id, original_graph_id, "Graph ID changed unexpectedly");
        
        // Test Error Cases
        test_error_cases(&mut ws, port, &original_graph_id);
        
        // Close WebSocket connection
        let _ = ws.close(None);
        
        // === Phase 2: Shutdown Server ===
        let test_env = server.shutdown();
        
        // === Phase 3: Server Shutdown (PostShutdown) ===
        assert_phase(PostShutdown);
        
        // NOW validate final state after server has saved everything
        validate_graph_state(&test_env.data_dir, &new_block_id, &original_graph_id);
        
        // Return test_env for cleanup
        test_env
    });
    
    // Always clean up, even if test failed
    match result {
        Ok(test_env) => {
            cleanup_test_env(test_env);
        }
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}