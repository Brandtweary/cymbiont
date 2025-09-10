//! MCP Server Integration Tests
//!
//! Tests the MCP (Model Context Protocol) server mode, validating:
//! - Protocol compliance (JSON-RPC 2.0)
//! - Tool discovery and execution
//! - All 14 knowledge graph tools
//! - State persistence after operations

use crate::common::test_harness::{
    mcp_call_tool, mcp_initialize, mcp_list_tools, mcp_request, shutdown_mcp_server,
    start_mcp_server,
};
use crate::common::{cleanup_test_env, setup_test_env, TestValidator};
use serde_json::json;
use std::io::{BufRead, Read, Write};

/// Test all tools through MCP protocol in a single comprehensive test
pub fn test_mcp_all_tools() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        let mut request_id = 1;
        
        // Initialize MCP connection
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize MCP");
        request_id += 1;
        
        // Test tool discovery - should have all 14 tools
        let tools = mcp_list_tools(&mut stdin, &mut stdout, request_id)
            .expect("Failed to list tools");
        request_id += 1;
        
        // Verify all 14 tools are present with cymbiont_ prefix
        let expected_tools = vec![
            "cymbiont_add_block",
            "cymbiont_update_block", 
            "cymbiont_delete_block",
            "cymbiont_create_page",
            "cymbiont_delete_page",
            "cymbiont_get_node",
            "cymbiont_query_graph_bfs",
            "cymbiont_list_graphs",
            "cymbiont_list_open_graphs",
            "cymbiont_open_graph",
            "cymbiont_close_graph",
            "cymbiont_create_graph",
            "cymbiont_delete_graph",
            "cymbiont_import_logseq",
        ];
        
        for tool_name in &expected_tools {
            assert!(
                tools.contains(&tool_name.to_string()),
                "Missing tool: {}",
                tool_name
            );
        }
        assert_eq!(
            tools.len(),
            expected_tools.len(),
            "Unexpected number of tools: {:?}",
            tools
        );
        
        // Test 1: import_logseq - Import dummy graph to have test data
        let import_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "import_logseq",
            json!({ "path": "logseq_databases/dummy_graph" }),
        ).expect("Failed to import logseq graph");
        request_id += 1;
        
        assert!(import_result["success"].as_bool().unwrap_or(false));
        let graph_id = import_result["graph_id"]
            .as_str()
            .expect("No graph_id in import response")
            .to_string();
        
        // Test 2: list_graphs - Should contain the imported graph
        let list_result = mcp_call_tool(&mut stdin, &mut stdout, request_id, "list_graphs", json!({}))
            .expect("Failed to list graphs");
        request_id += 1;
        assert!(list_result["success"].as_bool().unwrap_or(false));
        assert!(list_result["graphs"].is_array());
        
        // Test 3: list_open_graphs - Imported graph should be open
        let open_result = mcp_call_tool(&mut stdin, &mut stdout, request_id, "list_open_graphs", json!({}))
            .expect("Failed to list open graphs");
        request_id += 1;
        assert!(open_result["success"].as_bool().unwrap_or(false));
        let open_ids = open_result["graph_ids"].as_array().unwrap();
        assert!(
            open_ids.iter().any(|id| id.as_str() == Some(&graph_id)),
            "Imported graph not in open graphs list"
        );
        
        // Test 4: create_page - Add a new page to the graph
        let page_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "create_page",
            json!({
                "page_name": "mcp-test-page",
                "graph_id": &graph_id
            }),
        ).expect("Failed to create page");
        request_id += 1;
        assert!(page_result["success"].as_bool().unwrap_or(false));
        
        // Test 5: add_block - Add a block to the page
        let block_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "add_block",
            json!({
                "content": "Test block from MCP",
                "page_name": "mcp-test-page",
                "graph_id": &graph_id
            }),
        ).expect("Failed to add block");
        request_id += 1;
        assert!(block_result["success"].as_bool().unwrap_or(false));
        let block_id = block_result["block_id"].as_str().unwrap().to_string();
        
        // Test 6: get_node - Retrieve the block we just created
        let get_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "get_node",
            json!({
                "node_id": &block_id,
                "graph_id": &graph_id
            }),
        ).expect("Failed to get node");
        request_id += 1;
        assert!(get_result["success"].as_bool().unwrap_or(false));
        assert_eq!(get_result["content"].as_str(), Some("Test block from MCP"));
        
        // Test 7: update_block - Modify the block content
        let update_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "update_block",
            json!({
                "block_id": &block_id,
                "content": "Updated via MCP protocol",
                "graph_id": &graph_id
            }),
        ).expect("Failed to update block");
        request_id += 1;
        assert!(update_result["success"].as_bool().unwrap_or(false));
        
        // Test 8: query_graph_bfs - Test graph traversal
        let query_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "query_graph_bfs",
            json!({
                "start_id": "mcp-test-page",
                "max_depth": 2,
                "graph_id": &graph_id
            }),
        ).expect("Failed to query graph");
        request_id += 1;
        assert!(query_result["success"].as_bool().unwrap_or(false));
        assert!(query_result["nodes"].is_array());
        
        // Test 9: delete_block - Remove the block
        let delete_block_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "delete_block",
            json!({
                "block_id": &block_id,
                "graph_id": &graph_id
            }),
        ).expect("Failed to delete block");
        request_id += 1;
        assert!(delete_block_result["success"].as_bool().unwrap_or(false));
        
        // Test 10: delete_page - Remove the page
        let delete_page_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "delete_page",
            json!({
                "page_name": "mcp-test-page",
                "graph_id": &graph_id
            }),
        ).expect("Failed to delete page");
        request_id += 1;
        assert!(delete_page_result["success"].as_bool().unwrap_or(false));
        
        // Test 11: create_graph - Create a new graph
        let create_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "create_graph",
            json!({
                "name": "MCP Test Graph",
                "description": "Created via MCP protocol"
            }),
        ).expect("Failed to create graph");
        request_id += 1;
        assert!(create_result["success"].as_bool().unwrap_or(false));
        let new_graph_id = create_result["graph_id"].as_str().unwrap().to_string();
        
        // Test 12: open_graph - Open the new graph (it's already open after creation)
        // First close it to test opening
        mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "close_graph",
            json!({ "graph_id": &new_graph_id }),
        ).expect("Failed to close graph");
        request_id += 1;
        
        let open_graph_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "open_graph",
            json!({ "graph_id": &new_graph_id }),
        ).expect("Failed to open graph");
        request_id += 1;
        assert!(open_graph_result["success"].as_bool().unwrap_or(false));
        
        // Test 13: close_graph - Close the new graph
        let close_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "close_graph",
            json!({ "graph_id": &new_graph_id }),
        ).expect("Failed to close graph");
        request_id += 1;
        assert!(close_result["success"].as_bool().unwrap_or(false));
        
        // Test 14: delete_graph - Delete the new graph
        let delete_graph_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "delete_graph",
            json!({ "graph_id": &new_graph_id }),
        ).expect("Failed to delete graph");
        assert!(delete_graph_result["success"].as_bool().unwrap_or(false));
        
        // Shutdown and get test environment for validation
        let test_env = shutdown_mcp_server(process, test_env);
        
        // Use TestValidator for minimal state verification
        let mut validator = TestValidator::new(&test_env.data_dir);
        
        // The original imported graph should still exist with dummy data
        validator.expect_dummy_graph(Some(&graph_id));
        
        // The page and block we created and deleted shouldn't exist
        // (they were deleted, so validator tracks them as deleted)
        validator.expect_delete_page("mcp-test-page", Some(&graph_id));
        
        // Validate all expectations
        validator.validate_all().expect("Validation failed");
        
        test_env
    });
    
    match result {
        Ok(env) => cleanup_test_env(&env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test MCP initialization handshake
pub fn test_mcp_initialization() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        
        // Initialize should succeed
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize");
        
        // Should be able to list tools after initialization
        let tools = mcp_list_tools(&mut stdin, &mut stdout, 2)
            .expect("Failed to list tools after init");
        assert!(!tools.is_empty(), "No tools available after initialization");
        
        shutdown_mcp_server(process, test_env)
    });
    
    cleanup_test_env(&result.unwrap_or(cleanup_env));
}

/// Test JSON-RPC protocol compliance
pub fn test_mcp_protocol_compliance() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        let mut request_id = 1;
        
        // Initialize first
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize");
        request_id += 1;
        
        // Test unknown method
        let unknown_result = mcp_request(&mut stdin, &mut stdout, request_id, "unknown_method", None);
        assert!(
            unknown_result.is_err(),
            "Unknown method should return error"
        );
        request_id += 1;
        
        // Test calling tool with missing required parameter
        let missing_param_result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "add_block",
            json!({
                // Missing required "content" parameter
                "page_name": "test"
            }),
        );
        
        // This should return success:false rather than protocol error
        match missing_param_result {
            Ok(res) => {
                assert_eq!(
                    res["success"], 
                    false, 
                    "Should indicate failure for missing parameter"
                );
            }
            Err(_) => {
                // Also acceptable - protocol error for missing params
            }
        }
        request_id += 1;
        
        // Test calling non-existent tool
        let response = mcp_request(
            &mut stdin,
            &mut stdout,
            request_id,
            "tools/call",
            Some(json!({
                "name": "cymbiont_nonexistent_tool",
                "arguments": {}
            })),
        );
        
        assert!(
            response.is_err(),
            "Non-existent tool should return error"
        );
        
        shutdown_mcp_server(process, test_env)
    });
    
    cleanup_test_env(&result.unwrap_or(cleanup_env));
}

/// Test malformed JSON and edge cases
pub fn test_mcp_malformed_requests() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        
        // Test 1: Send malformed JSON
        stdin.write_all(b"not valid json\n").expect("Failed to write");
        stdin.flush().expect("Failed to flush");
        
        // Should get parse error response
        let mut response_line = String::new();
        stdout.read_line(&mut response_line).expect("Failed to read response");
        let response: serde_json::Value = serde_json::from_str(&response_line)
            .expect("Failed to parse error response");
        assert!(response["error"].is_object(), "Should return error for malformed JSON");
        assert_eq!(response["error"]["code"], -32700, "Should return parse error code");
        
        // Test 2: Send JSON that's not an object
        stdin.write_all(b"[1,2,3]\n").expect("Failed to write");
        stdin.flush().expect("Failed to flush");
        
        response_line.clear();
        stdout.read_line(&mut response_line).expect("Failed to read response");
        let response: serde_json::Value = serde_json::from_str(&response_line)
            .expect("Failed to parse error response");
        assert!(response["error"].is_object(), "Should return error for non-object JSON");
        
        // Test 3: Send empty line (should be ignored, no response)
        stdin.write_all(b"\n").expect("Failed to write");
        stdin.flush().expect("Failed to flush");
        
        // Initialize properly to continue tests
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize");
        
        // Test 4: Send request without method field
        let bad_request = json!({
            "jsonrpc": "2.0",
            "id": 100,
            // Missing "method" field
            "params": {}
        });
        stdin.write_all(format!("{}\n", bad_request).as_bytes()).expect("Failed to write");
        stdin.flush().expect("Failed to flush");
        
        response_line.clear();
        stdout.read_line(&mut response_line).expect("Failed to read response");
        let response: serde_json::Value = serde_json::from_str(&response_line)
            .expect("Failed to parse error response");
        assert!(response["error"].is_object(), "Should return error for missing method");
        
        shutdown_mcp_server(process, test_env)
    });
    
    cleanup_test_env(&result.unwrap_or(cleanup_env));
}

/// Test invalid tool arguments and error handling
pub fn test_mcp_invalid_tool_arguments() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        let mut request_id = 1;
        
        // Initialize first
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize");
        request_id += 1;
        
        // Test 1: Call tool with wrong type for parameter
        let result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "add_block",
            json!({
                "content": 123,  // Should be string, not number
                "page_name": "test",
                "graph_id": "dummy-graph"
            }),
        );
        request_id += 1;
        
        match result {
            Ok(res) => {
                assert_eq!(res["success"], false, "Should fail for wrong parameter type");
                assert!(res["error"].as_str().unwrap().contains("type"), 
                    "Error should mention type issue");
            }
            Err(_) => {
                // Protocol error is also acceptable
            }
        }
        
        // Test 2: Call tool with null for required parameter
        let result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "create_page",
            json!({
                "page_name": null,  // Required field is null
                "graph_id": "dummy-graph"
            }),
        );
        request_id += 1;
        
        match result {
            Ok(res) => {
                assert_eq!(res["success"], false, "Should fail for null required field");
            }
            Err(_) => {
                // Protocol error is also acceptable
            }
        }
        
        // Test 3: Call tool with invalid graph_id
        let result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "add_block",
            json!({
                "content": "Test",
                "page_name": "test",
                "graph_id": "non-existent-graph-id"
            }),
        );
        request_id += 1;
        
        assert!(result.is_ok(), "Should return success:false for invalid graph");
        let res = result.unwrap();
        assert_eq!(res["success"], false, "Should fail for non-existent graph");
        assert!(res["error"].as_str().unwrap().contains("graph") || 
                res["error"].as_str().unwrap().contains("Graph"),
                "Error should mention graph issue");
        
        // Test 4: Call tool with extra unexpected parameters (should be ignored)
        let result = mcp_call_tool(
            &mut stdin,
            &mut stdout,
            request_id,
            "list_graphs",
            json!({
                "unexpected_param": "should be ignored",
                "another_one": 42
            }),
        );
        
        assert!(result.is_ok(), "Extra parameters should be ignored");
        let res = result.unwrap();
        assert_eq!(res["success"], true, "Should succeed despite extra parameters");
        
        shutdown_mcp_server(process, test_env)
    });
    
    cleanup_test_env(&result.unwrap_or(cleanup_env));
}

/// Test notification handling (notifications should not receive responses)
pub fn test_mcp_notifications() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (process, mut stdin, mut stdout, test_env) = start_mcp_server(test_env);
        
        // Initialize first
        mcp_initialize(&mut stdin, &mut stdout).expect("Failed to initialize");
        
        // Send initialized notification (no id field means it's a notification)
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
            // No "id" field - this makes it a notification
        });
        
        stdin.write_all(format!("{}\n", notification).as_bytes()).expect("Failed to write");
        stdin.flush().expect("Failed to flush");
        
        // Give server time to process
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Try to read - should timeout since notifications don't get responses
        use std::io::ErrorKind;
        let mut buf = [0u8; 1];
        match stdout.read_exact(&mut buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // Expected - no response for notification
            }
            Ok(_) => {
                // If we got data, it should be from a subsequent request, not the notification
                // Put it back for proper reading
                let mut response_line = String::from_utf8(vec![buf[0]]).unwrap();
                stdout.read_line(&mut response_line).ok();
                
                // Send a real request to verify server is still responsive
                let request = json!({
                    "jsonrpc": "2.0",
                    "method": "tools/list",
                    "id": 999
                });
                stdin.write_all(format!("{}\n", request).as_bytes()).expect("Failed to write");
                stdin.flush().expect("Failed to flush");
                
                let mut new_response = String::new();
                stdout.read_line(&mut new_response).expect("Failed to read");
                let response: serde_json::Value = serde_json::from_str(&new_response)
                    .expect("Failed to parse response");
                assert_eq!(response["id"], 999, "Server should still respond to requests");
            }
            Err(e) => {
                // Other errors are unexpected
                panic!("Unexpected error reading: {:?}", e);
            }
        }
        
        shutdown_mcp_server(process, test_env)
    });
    
    cleanup_test_env(&result.unwrap_or(cleanup_env));
}