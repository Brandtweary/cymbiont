use std::fs;
use serde_json::{json, Value};
use crate::common::{setup_test_env, cleanup_test_env, GraphValidationFixture};
use crate::common::test_harness::{
    PreShutdown, PostShutdown, assert_phase,
    WsConnection, connect_websocket, send_command, expect_success, 
    authenticate_websocket, setup_with_graph, read_auth_token, get_single_open_graph_id
};

/// Test creating a new block
fn test_create_block(ws: &mut WsConnection) -> String {
    let cmd = json!({
        "type": "create_block",
        "content": "WebSocket test block with **bold** and *italic* text",
        "page_name": "test-websocket-page"
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

/// Test multiple updates on the same block to ensure edges are preserved
fn test_multiple_block_updates(ws: &mut WsConnection, fixture: &mut GraphValidationFixture) -> String {
    // Create a parent block first
    let parent_cmd = json!({
        "type": "create_block",
        "content": "Parent block for update test",
        "page_name": "test-update-page"
    });
    
    let response = send_command(ws, parent_cmd);
    let data = expect_success(response).expect("No data in create_block response");
    let parent_id = data["block_id"].as_str().expect("No block_id in response").to_string();
    
    // Record parent block expectation - the page will be auto-created
    fixture.expect_create_page("test-update-page", None);
    fixture.expect_create_block(&parent_id, "Parent block for update test", Some("test-update-page"));
    
    // Create a child block
    let child_cmd = json!({
        "type": "create_block",
        "content": "Child block that will be updated multiple times",
        "parent_id": parent_id,
        "page_name": "test-update-page"
    });
    
    let response = send_command(ws, child_cmd);
    let data = expect_success(response).expect("No data in create_block response");
    let child_id = data["block_id"].as_str().expect("No block_id in response").to_string();
    
    // Record child block expectation with parent-child edge
    fixture.expect_create_block(&child_id, "Child block that will be updated multiple times", Some("test-update-page"));
    
    // Add the parent-child edge expectation
    fixture.expect_edge(&parent_id, &child_id, "ParentChild");
    
    // Update 1: Change content
    let update1_cmd = json!({
        "type": "update_block",
        "block_id": child_id,
        "content": "First update - content changed"
    });
    
    let response = send_command(ws, update1_cmd);
    expect_success(response);
    fixture.expect_update_block(&child_id, "First update - content changed");
    
    // Update 2: Change content again
    let update2_cmd = json!({
        "type": "update_block",
        "block_id": child_id,
        "content": "Second update - content changed again"
    });
    
    let response = send_command(ws, update2_cmd);
    expect_success(response);
    fixture.expect_update_block(&child_id, "Second update - content changed again");
    
    // Update 3: Final content change
    let update3_cmd = json!({
        "type": "update_block",
        "block_id": child_id,
        "content": "Final update - this should be the final content"
    });
    
    let response = send_command(ws, update3_cmd);
    expect_success(response);
    fixture.expect_update_block(&child_id, "Final update - this should be the final content");
    
    child_id
}

/// Test graph operations (create, list, and delete)
fn test_graph_operations(ws: &mut WsConnection, data_dir: &std::path::Path) -> String {
    // Get the current open graph ID (should be the imported one)
    let original_graph_id = get_single_open_graph_id(data_dir);
    
    // Test list_graphs with one graph
    let list_cmd = json!({"type": "list_graphs"});
    let response = send_command(ws, list_cmd.clone());
    let data = expect_success(response).expect("No data in list_graphs response");
    let graphs = data["graphs"].as_array().expect("Should have graphs array");
    assert_eq!(graphs.len(), 1, "Should have exactly one graph initially");
    assert_eq!(
        graphs[0]["id"].as_str().unwrap(),
        &original_graph_id,
        "Graph ID from list_graphs should match original"
    );
    
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
    
    // Test list_graphs with two graphs
    let response = send_command(ws, list_cmd.clone());
    let data = expect_success(response).expect("No data in list_graphs response");
    let graphs = data["graphs"].as_array().expect("Should have graphs array");
    assert_eq!(graphs.len(), 2, "Should have two graphs after creation");
    
    // Verify both graphs are present
    let graph_ids: Vec<String> = graphs.iter()
        .map(|g| g["id"].as_str().unwrap().to_string())
        .collect();
    assert!(graph_ids.contains(&original_graph_id), "Original graph should be in list");
    assert!(graph_ids.contains(&new_graph_id), "New graph should be in list");
    
    // Verify the new graph has correct metadata
    let new_graph = graphs.iter()
        .find(|g| g["id"].as_str() == Some(&new_graph_id))
        .expect("New graph should be in list");
    assert_eq!(new_graph["name"].as_str(), Some("test-websocket-graph"));
    assert_eq!(new_graph["description"].as_str(), Some("Created via WebSocket test"));
    
    // Delete the test graph (it was auto-opened on creation)
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
    
    // Test list_graphs after deletion - should be back to one graph
    let response = send_command(ws, list_cmd);
    let data = expect_success(response).expect("No data in list_graphs response");
    let graphs = data["graphs"].as_array().expect("Should have graphs array");
    assert_eq!(graphs.len(), 1, "Should have one graph after deletion");
    assert_eq!(
        graphs[0]["id"].as_str().unwrap(),
        &original_graph_id,
        "Only original graph should remain"
    );
    
    original_graph_id
}

/// Test error cases
fn test_error_cases(ws: &mut WsConnection, port: u16, _original_graph_id: &str) {
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
    
    // Test invalid auth token
    let mut invalid_auth_ws = connect_websocket(port);
    assert!(!authenticate_websocket(&mut invalid_auth_ws, "invalid-token"), "Authentication should fail with invalid token");
}

/// Validate registry state (graph creation and deletion)
fn validate_registry_state(data_dir: &std::path::Path, original_graph_id: &str) {
    // Verify the original graph is still open
    let registry_path = data_dir.join("graph_registry.json");
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse registry");
    
    let open_graphs = registry["open_graphs"].as_array()
        .expect("No open_graphs in registry");
    
    assert!(
        open_graphs.iter().any(|g| g.as_str() == Some(original_graph_id)),
        "Original graph is not open"
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
        // Import dummy graph and start server
        let (server, original_graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // === Phase 1: Server Running (PreShutdown) ===
        assert_phase(PreShutdown);
        
        // Initialize the validation fixture and expect the dummy graph
        let mut fixture = GraphValidationFixture::new();
        fixture.expect_dummy_graph();
        
        // Connect WebSocket client
        let mut ws = connect_websocket(port);
        
        // Read auth token and authenticate
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token), "Authentication failed with valid token");
        
        // Test Create Block  
        let new_block_id = test_create_block(&mut ws);
        // Record expectation in fixture
        fixture.expect_create_block(&new_block_id, "WebSocket test block with **bold** and *italic* text", Some("test-websocket-page"));
        
        // Test Update Block (use a different existing block)
        test_update_block(&mut ws, "67f9a190-985b-4dbf-90e4-c2abffb2ab51");
        // Record expectation in fixture
        fixture.expect_update_block("67f9a190-985b-4dbf-90e4-c2abffb2ab51", "## Types of Knowledge Graphs (Updated via WebSocket)");
        
        // Test Multiple Updates on Same Block (with parent-child and page-to-block edges)
        let _updated_child_id = test_multiple_block_updates(&mut ws, &mut fixture);
        
        // Test Delete Block (use another existing block)
        test_delete_block(&mut ws, "67fbd626-8e4a-485f-ad03-fd1ce5539ebb");
        // Record expectation in fixture
        fixture.expect_delete("67fbd626-8e4a-485f-ad03-fd1ce5539ebb");
        
        // Test Create Page
        test_create_page(&mut ws, "test-websocket-page");
        // Record expectation in fixture
        fixture.expect_create_page("test-websocket-page", Some(json!({
            "test-property": "test-value",
            "created-by": "websocket-test"
        })));
        
        // Test Delete Page (create a new page to delete)
        test_create_page(&mut ws, "page-to-delete");
        fixture.expect_create_page("page-to-delete", Some(json!({
            "test-property": "test-value",
            "created-by": "websocket-test"
        })));
        test_delete_page(&mut ws, "page-to-delete");
        fixture.expect_delete("page-to-delete");
        
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
        
        // Validate graph state using the fixture
        fixture.validate_graph(&test_env.data_dir, &original_graph_id);
        
        // Validate registry state (graph switching behavior)
        validate_registry_state(&test_env.data_dir, &original_graph_id);
        
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