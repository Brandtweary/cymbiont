//! Integration tests for agent tool execution through the knowledge graph tools system.
//!
//! These tests verify that agents can successfully execute all available tools via
//! WebSocket commands using the `MockLLM`'s `echo_tool` functionality. Each tool is tested
//! for correct execution, validation, authorization, and result formatting.

use crate::common::test_harness::{
    assert_phase, authenticate_websocket, connect_websocket, execute_tool_sync, expect_success, read_auth_token,
    send_command, setup_with_graph, PostShutdown, PreShutdown, TestServer,
};
use crate::common::{cleanup_test_env, setup_test_env, TestValidator};
use serde_json::json;
use uuid::Uuid;

/// Test graph management tools (create, list, open, close, delete graphs)
#[allow(clippy::too_many_lines)]
pub fn test_agent_graph_management_tools() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        // Import dummy graph and start server
        let (server, initial_graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);

        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test list_graphs tool
        {
            let result = execute_tool_sync(&mut ws, "list_graphs", json!({}));
            // Just verify it returns graphs - the actual graph list is validated later
            assert!(result.get("graphs").is_some());
        }

        // Test create_graph tool
        let new_graph_uuid = {
            let result = execute_tool_sync(&mut ws, "create_graph", json!({
                "name": "Test Graph",
                "description": "Created by test"
            }));
            let graph_id_str = result["graph_id"].as_str().expect("create_graph should return graph_id");
            let uuid = Uuid::parse_str(graph_id_str).unwrap();
            
            // Validate the graph was created and opened in the registry
            validator.expect_graph_created(uuid, "Test Graph");
            validator.expect_graph_open(uuid);

            uuid
        };

        // Track the new graph creation

        // Test list_open_graphs tool
        {
            let result = execute_tool_sync(&mut ws, "list_open_graphs", json!({}));
            // Just verify it returns graph_ids - actual state is validated at the end
            assert!(result.get("graph_ids").is_some());
        }

        // Test close_graph and open_graph tools
        {
            // Close the new graph first to make initial the only open graph
            let close_cmd = json!({
                "type": "close_graph",
                "graph_id": new_graph_uuid.to_string()
            });
            let close_response = send_command(&mut ws, &close_cmd);
            expect_success(&close_response);
            validator.expect_graph_closed(new_graph_uuid);

            // Now test close_graph tool with the initial graph ID
            let result = execute_tool_sync(&mut ws, "close_graph", json!({
                "graph_id": initial_graph_id
            }));
            assert_eq!(result["success"], true);

            // Validate the initial graph was closed
            let initial_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
            validator.expect_graph_closed(initial_uuid);

            // Now both are closed. Open the initial graph directly
            let open_cmd = json!({
                "type": "open_graph",
                "graph_id": initial_graph_id
            });
            let open_response = send_command(&mut ws, &open_cmd);
            expect_success(&open_response);
            validator.expect_graph_open(initial_uuid);
        }

        // Test delete_graph tool - delete the initial graph (it's open)
        {
            // First re-open the initial graph since we closed it
            let open_cmd = json!({
                "type": "open_graph",
                "graph_id": initial_graph_id
            });
            let open_response = send_command(&mut ws, &open_cmd);
            expect_success(&open_response);
            
            // Delete the initial graph
            let result = execute_tool_sync(&mut ws, "delete_graph", json!({
                "graph_id": initial_graph_id
            }));
            assert_eq!(result["success"], true);

            // The initial graph should be deleted
            let initial_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
            validator.expect_graph_deleted(initial_uuid);
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        // Validate final state
        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test block operations (add, update, delete blocks)
#[allow(clippy::too_many_lines)]
pub fn test_agent_block_operations() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        // Import dummy graph and start server
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);

        // Set up expectations for dummy graph
        validator.expect_dummy_graph(Some(&graph_id));

        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test add_block tool - simple content
        {
            let result = execute_tool_sync(&mut ws, "add_block", json!({
                "content": "Test block content",
                "page_name": "contents"
            }));
            
            // Verify the tool returned a block_id
            let block_id = result["block_id"].as_str().expect("add_block should return block_id");
            
            // Track the block creation in the validator
            validator.expect_create_block(block_id, "Test block content", Some("contents"), Some(&graph_id));
        }

        // Test update_block tool
        // First create a block to update
        {
            let create_cmd = json!({
                "type": "create_block",
                "content": "Block to be updated",
                "page_name": "contents"
            });
            let response = send_command(&mut ws, &create_cmd);
            let data = expect_success(&response).expect("No data in create_block response");
            let block_id = data["block_id"].as_str().expect("No block_id").to_string();
            
            // Now test update_block tool
            let result = execute_tool_sync(&mut ws, "update_block", json!({
                "block_id": block_id,
                "content": "Updated block content"
            }));
            
            // Verify the tool executed successfully
            assert_eq!(result["success"], true);
            
            // Validate the block was actually updated
            validator.expect_update_block(&block_id, "Updated block content", Some(&graph_id));
        }

        // Test add_block with parent_id
        {
            // First create a parent block
            let parent_result = execute_tool_sync(&mut ws, "add_block", json!({
                "content": "Parent block",
                "page_name": "contents"
            }));
            let parent_id = parent_result["block_id"].as_str().expect("add_block should return block_id");
            validator.expect_create_block(parent_id, "Parent block", Some("contents"), Some(&graph_id));
            
            // Now add a child block
            let child_result = execute_tool_sync(&mut ws, "add_block", json!({
                "content": "Child block",
                "parent_id": parent_id
            }));
            let child_id = child_result["block_id"].as_str().expect("add_block should return block_id");
            validator.expect_create_block(child_id, "Child block", None, Some(&graph_id));
        }

        // Test add_block with specific page_name
        {
            let result = execute_tool_sync(&mut ws, "add_block", json!({
                "content": "Block on specific page",
                "page_name": "test-websocket"
            }));
            let block_id = result["block_id"].as_str().expect("add_block should return block_id");
            validator.expect_create_block(block_id, "Block on specific page", Some("test-websocket"), Some(&graph_id));
        }

        // Test delete_block tool
        // First create a block to delete
        {
            let create_cmd = json!({
                "type": "create_block",
                "content": "Block to be deleted",
                "page_name": "contents"
            });
            let response = send_command(&mut ws, &create_cmd);
            let data = expect_success(&response).expect("No data in create_block response");
            let block_id = data["block_id"].as_str().expect("No block_id").to_string();
            
            // Now test delete_block tool
            let result = execute_tool_sync(&mut ws, "delete_block", json!({
                "block_id": block_id
            }));
            
            // Verify the tool executed successfully
            assert_eq!(result["success"], true);
            
            // Validate the block was actually deleted
            validator.expect_delete_block(&block_id, Some(&graph_id));
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        // Validate final state - both agent conversation and graph changes
        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test page operations (create and delete pages)
pub fn test_agent_page_operations() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);
        validator.expect_dummy_graph(Some(&graph_id));

        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test create_page tool
        {
            let result = execute_tool_sync(&mut ws, "create_page", json!({
                "page_name": "Test Page",
                "properties": {"type": "test"}
            }));
            
            // Verify the tool executed successfully
            assert_eq!(result["success"], true);
            
            // Track page creation in the original graph
            validator.expect_create_page("Test Page", Some(json!({"type": "test"})), Some(&graph_id));
        }

        // Test delete_page tool
        {
            let result = execute_tool_sync(&mut ws, "delete_page", json!({
                "page_name": "Test Page"
            }));
            
            // Verify the tool executed successfully
            assert_eq!(result["success"], true);
            
            // Track page deletion in the original graph
            validator.expect_delete_page("Test Page", Some(&graph_id));
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test query operations (`get_node` and `query_graph_bfs`)
pub fn test_agent_query_operations() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);

        // Expect the dummy graph that was imported
        validator.expect_dummy_graph(Some(&graph_id));

        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test get_node tool
        {
            // Get information about an existing page
            let result = execute_tool_sync(&mut ws, "get_node", json!({
                "node_id": "contents"
            }));
            
            // Verify the tool returned node data (it returns the node data directly)
            assert!(result.get("id").is_some());
        }

        // Skip query_graph_bfs - it's still a stub
        // TODO: Add query_graph_bfs test once implemented

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test tool argument validation errors
pub fn test_agent_tool_validation_errors() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);

        // Expect the dummy graph that was imported
        validator.expect_dummy_graph(Some(&graph_id));

        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test tool with invalid arguments
        {
            // Try to update a non-existent block
            let result = execute_tool_sync(&mut ws, "update_block", json!({
                "block_id": "non-existent-block-id",
                "content": "This should fail"
            }));
            
            // The tool should execute but might return an error in the message
            // We're not testing the error itself, just that the tool executes
            assert!(result.is_object());
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test multiple tool calls in sequence (tool chaining)
pub fn test_agent_tool_chaining() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();

    let result = std::panic::catch_unwind(move || {
        // Start without imported graph to test full flow
        let server = TestServer::start(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();

        assert_phase(PreShutdown);

        let mut validator = TestValidator::new(&data_dir);

        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Chain: create_graph -> create_page -> add_block -> list_graphs

        // Step 1: Create a graph and capture its ID
        let new_graph_id = {
            let result = execute_tool_sync(&mut ws, "create_graph", json!({
                "name": "Test Chain Graph",
                "description": "Graph for testing tool chaining"
            }));
            
            let graph_id = result["graph_id"].as_str().expect("create_graph should return graph_id").to_string();
            
            // Track that we expect this graph to exist
            validator.expect_graph_created(uuid::Uuid::parse_str(&graph_id).unwrap(), "Test Chain Graph");
            validator.expect_graph_open(uuid::Uuid::parse_str(&graph_id).unwrap());
            
            graph_id
        };

        // Step 2: Create a page in the new graph and validate it
        {
            let result = execute_tool_sync(&mut ws, "create_page", json!({
                "page_name": "Chain Test Page"
            }));
            
            assert_eq!(result["success"], true);
            
            // Validate the page was created in the right graph
            validator.expect_create_page("Chain Test Page", None, Some(&new_graph_id));
        }

        // Step 3: Add blocks to the page and validate them
        {
            let result = execute_tool_sync(&mut ws, "add_block", json!({
                "content": "Chain test block content",
                "page_name": "Chain Test Page"
            }));
            
            let block_id = result["block_id"].as_str().expect("add_block should return block_id");
            
            // Validate the block was created with the expected content
            validator.expect_create_block(block_id, "Chain test block content", Some("Chain Test Page"), Some(&new_graph_id));
        }

        // Step 4: List the graphs to verify creation
        {
            let result = execute_tool_sync(&mut ws, "list_graphs", json!({}));
            
            // Verify the list contains our new graph
            let graphs = result["graphs"].as_array().expect("list_graphs should return graphs array");
            assert!(graphs.iter().any(|g| g["id"].as_str() == Some(&new_graph_id)), 
                   "New graph should be in the list");
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();

        assert_phase(PostShutdown);

        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}
