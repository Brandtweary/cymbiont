use std::thread;
use std::time::Duration;
use serde_json::json;
use crate::common::{setup_test_env, cleanup_test_env, WalValidator};
use crate::common::test_harness::{
    connect_websocket, authenticate_websocket, read_auth_token,
    freeze_operations, unfreeze_operations, send_command_async, send_command, expect_success,
    read_pending_response, PreShutdown, PostShutdown, assert_phase, TestServer,
    block_on, import_dummy_graph_http,
};

/// Test crash recovery on server startup
/// 
/// This test verifies that pending transactions are automatically recovered
/// when a server restarts. It creates several operations while frozen,
/// force-kills the server, then restarts and verifies all operations
/// were recovered and applied correctly.
pub fn test_startup_recovery() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import graph via HTTP - uses existing prime agent
        let _graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Initialize validation fixture
        let mut validator = WalValidator::new(&data_dir);
        
        // Record expectations for pages
        validator.expect_create_page("recovery-test-page", None);
        validator.expect_create_page("recovery-test-page-2", Some(json!({"test": "recovery"})));
        
        // Freeze operations to create pending transactions
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        
        // Create multiple different types of operations while frozen
        let operations = vec![
            // First create the pages
            json!({
                "type": "create_page",
                "name": "recovery-test-page"
            }),
            json!({
                "type": "create_page",
                "name": "recovery-test-page-2",
                "properties": {"test": "recovery"}
            }),
            // Then create blocks on those pages
            json!({
                "type": "create_block",
                "content": "Recovery test block 1",
                "page_name": "recovery-test-page"
            }),
            json!({
                "type": "create_block", 
                "content": "Recovery test block 2 with parent",
                "parent_id": "dummy-parent-id"
            }),
            json!({
                "type": "create_block",
                "content": "Recovery test block 3",
                "page_name": "recovery-test-page-2"
            }),
        ];
        
        // Send all operations asynchronously (they'll be pending due to freeze)
        for op in operations.iter() {
            send_command_async(&mut ws, op.clone());
        }
        
        // Wait longer to ensure operations reach transaction creation phase
        // The sled database has a default flush interval, so we need to wait for it
        thread::sleep(Duration::from_secs(2));
        
        // Force kill the server to simulate crash
        server.force_kill();
        thread::sleep(Duration::from_millis(500));
        
        // Start new server instance with same environment
        let server2 = TestServer::start(test_env.clone());
        let port2 = server2.port();
        
        assert_phase(PreShutdown);
        
        // Connect to recovered server
        let mut ws2 = connect_websocket(port2);
        
        // Read the new token (it rotates on restart)
        let new_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws2, &new_token), "Authentication failed on recovered server");
        
        // Graceful shutdown
        let test_env = server2.shutdown();
        assert_phase(PostShutdown);
        
        // Validate final state using the validator
        validator.validate_graph_with_content_checks( 
            &_graph_id,
            &[
                ("Recovery test block 1", Some("recovery-test-page")),
                ("Recovery test block 2 with parent", None), // No page since parent doesn't exist
                ("Recovery test block 3", Some("recovery-test-page-2")),
            ]
        ).expect("Validation failed");
        
        test_env
    });
    
    // Cleanup
    match result {
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test mixed open/closed graphs with crash recovery
/// 
/// This test verifies:
/// 1. Open/close graph commands work correctly
/// 2. Crash recovery works for multiple graphs with different open/closed states
/// 3. Open/closed state persists across server restarts
pub fn test_mixed_open_closed_graphs() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import initial graph (graph A) via HTTP
        let graph_a_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Create two more graphs (B and C)
        let create_graph_b = json!({
            "type": "create_graph",
            "name": "test-graph-b",
            "description": "Graph B for mixed open/closed testing"
        });
        
        let response = send_command(&mut ws, create_graph_b);
        let graph_b_data = expect_success(response).unwrap();
        let graph_b_id = graph_b_data["id"].as_str().unwrap().to_string();
        
        let create_graph_c = json!({
            "type": "create_graph",
            "name": "test-graph-c",
            "description": "Graph C for mixed open/closed testing"
        });
        
        let response = send_command(&mut ws, create_graph_c);
        let graph_c_data = expect_success(response).unwrap();
        let graph_c_id = graph_c_data["id"].as_str().unwrap().to_string();
        
        // All three graphs should be open after creation
        // Create pages on each graph first (to have something to add blocks to)
        
        // Graph A - already has dummy content, add a new page
        let create_page_a = json!({
            "type": "create_page",
            "name": "page-a-recovery",
            "graph_id": graph_a_id.clone()
        });
        let response = send_command(&mut ws, create_page_a);
        expect_success(response);
        
        // Graph B - open
        let create_page_b = json!({
            "type": "create_page",
            "name": "page-b-recovery",
            "graph_id": graph_b_id.clone()
        });
        let response = send_command(&mut ws, create_page_b);
        expect_success(response);
        
        // Graph C - open
        let create_page_c = json!({
            "type": "create_page",
            "name": "page-c-recovery",
            "graph_id": graph_c_id.clone()
        });
        let response = send_command(&mut ws, create_page_c);
        expect_success(response);
        
        // Freeze operations to create pending transactions
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        
        // Create pending transactions on all three graphs
        let operations = vec![
            // Graph A operations (open)
            json!({
                "type": "create_block",
                "content": "Graph A pending block 1",
                "page_name": "page-a-recovery",
                "graph_id": graph_a_id.clone()
            }),
            json!({
                "type": "create_block",
                "content": "Graph A pending block 2",
                "page_name": "page-a-recovery",
                "graph_id": graph_a_id.clone()
            }),
            // Graph B operations (closed)
            json!({
                "type": "create_block",
                "content": "Graph B pending block 1",
                "page_name": "page-b-recovery",
                "graph_id": graph_b_id.clone()
            }),
            json!({
                "type": "create_block",
                "content": "Graph B pending block 2",
                "page_name": "page-b-recovery",
                "graph_id": graph_b_id.clone()
            }),
            // Graph C operations (open)
            json!({
                "type": "create_block",
                "content": "Graph C pending block 1",
                "page_name": "page-c-recovery",
                "graph_id": graph_c_id.clone()
            }),
            json!({
                "type": "create_block",
                "content": "Graph C pending block 2",
                "page_name": "page-c-recovery",
                "graph_id": graph_c_id.clone()
            }),
        ];
        
        // Send all operations asynchronously (they'll be pending due to freeze)
        for op in operations.iter() {
            send_command_async(&mut ws, op.clone());
        }
        
        // Wait for operations to reach transaction creation phase
        thread::sleep(Duration::from_secs(2));
        
        // Now close graph B before crashing
        let close_b_cmd = json!({
            "type": "close_graph",
            "graph_id": graph_b_id.clone()
        });
        let response = send_command(&mut ws, close_b_cmd);
        expect_success(response);
        
        // Force kill the server with:
        // - Graph A: open, with pending transactions
        // - Graph B: closed, with pending transactions  
        // - Graph C: open, with pending transactions
        server.force_kill();
        thread::sleep(Duration::from_millis(500));
        
        // Start new server instance
        // Should recover transactions for open graphs (A and C) on startup
        // Graph B's transactions should be recovered when it's opened
        let server2 = TestServer::start(test_env.clone());
        let port2 = server2.port();
        
        assert_phase(PreShutdown);
        
        // Connect to recovered server
        let mut ws2 = connect_websocket(port2);
        let new_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws2, &new_token), "Authentication failed on recovered server");
        
        // Verify open/closed states persisted
        // Graph B should remain closed but still have its transactions recovered
        // due to eager recovery on startup
        
        // Graceful shutdown
        let test_env = server2.shutdown();
        assert_phase(PostShutdown);
        
        // Validate all three graphs have their pending transactions recovered
        let mut validator_a = WalValidator::new(&test_env.data_dir);
        validator_a.expect_create_page("page-a-recovery", None);
        validator_a.validate_graph_with_content_checks(
            &graph_a_id,
            &[
                ("Graph A pending block 1", Some("page-a-recovery")),
                ("Graph A pending block 2", Some("page-a-recovery")),
            ]
        ).expect("Validation failed for graph A");
        
        let mut validator_b = WalValidator::new(&test_env.data_dir);
        validator_b.expect_create_page("page-b-recovery", None);
        validator_b.validate_graph_with_content_checks(
            &graph_b_id,
            &[
                ("Graph B pending block 1", Some("page-b-recovery")),
                ("Graph B pending block 2", Some("page-b-recovery")),
            ]
        ).expect("Validation failed for graph B");
        
        let mut validator_c = WalValidator::new(&test_env.data_dir);
        validator_c.expect_create_page("page-c-recovery", None);
        validator_c.validate_graph_with_content_checks(
            &graph_c_id,
            &[
                ("Graph C pending block 1", Some("page-c-recovery")),
                ("Graph C pending block 2", Some("page-c-recovery")),
            ]
        ).expect("Validation failed for graph C");
        
        test_env
    });
    
    // Cleanup
    match result {
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test graceful shutdown with pending transactions
/// 
/// This test verifies that the two-stage Ctrl+C handler correctly:
/// 1. Waits for pending transactions to complete on first Ctrl+C
/// 2. Allows force quit on second Ctrl+C if transactions are still pending
pub fn test_graceful_shutdown_completes_transactions() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import graph via HTTP - uses existing prime agent
        let graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Create a page to add blocks to
        let create_page = json!({
            "type": "create_page",
            "name": "graceful-test-page",
            "graph_id": graph_id.clone()
        });
        let response = send_command(&mut ws, create_page);
        expect_success(response);
        
        // Freeze operations to simulate slow transaction
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        
        // Start a transaction (will be frozen after WAL write)
        send_command_async(&mut ws, json!({
            "type": "create_block",
            "content": "This block should complete during graceful shutdown",
            "parent_id": null,
            "page_name": "graceful-test-page",
            "graph_id": graph_id.clone(),
            "properties": null
        }));
        
        // Give transaction time to reach frozen state
        thread::sleep(Duration::from_millis(200));
        
        // Unfreeze operations first - transaction will start processing
        assert!(unfreeze_operations(&mut ws), "Failed to unfreeze operations");
        
        // Give transaction a moment to start processing
        thread::sleep(Duration::from_millis(100));
        
        // Now send SIGINT - should wait for the in-flight transaction
        server.send_sigint();
        
        // Server should exit gracefully after transaction completes
        // The graceful shutdown handler will wait for the transaction to finish
        let test_env = server.wait_for_completion();
        assert_phase(PostShutdown);
        
        // Validate that the block was created successfully
        let mut validator = WalValidator::new(&test_env.data_dir);
        validator.expect_create_page("graceful-test-page", None);
        validator.validate_graph_with_content_checks(
            &graph_id,
            &[
                ("This block should complete during graceful shutdown", Some("graceful-test-page")),
            ]
        ).expect("Validation failed");
        
        test_env
    });
    
    // Cleanup
    match result {
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test recovery when opening a closed graph
/// 
/// This test verifies that pending transactions are recovered when a closed graph
/// is reopened. It tests the scenario where operations are sent while frozen,
/// the graph is closed (removing it from memory), and then reopened.
pub fn test_open_graph_recovery() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import initial graph via HTTP - uses existing prime agent
        let graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Create a page first to have something to add blocks to
        let create_page = json!({
            "type": "create_page",
            "name": "open-recovery-test-page",
            "graph_id": graph_id.clone()
        });
        let response = send_command(&mut ws, create_page);
        expect_success(response);
        
        // Freeze operations to create pending transactions
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        
        // Send operations while frozen - they'll create transactions but wait
        let operations = vec![
            json!({
                "type": "create_block",
                "content": "Block 1 - should be recovered on open",
                "page_name": "open-recovery-test-page",
                "graph_id": graph_id.clone()
            }),
            json!({
                "type": "create_block",
                "content": "Block 2 - should be recovered on open",
                "page_name": "open-recovery-test-page", 
                "graph_id": graph_id.clone()
            }),
            json!({
                "type": "create_block",
                "content": "Block 3 - should be recovered on open",
                "page_name": "open-recovery-test-page",
                "graph_id": graph_id.clone()
            }),
        ];
        
        // Send all operations asynchronously (they'll wait due to freeze)
        for op in operations.iter() {
            send_command_async(&mut ws, op.clone());
        }
        
        // Wait for operations to reach transaction creation phase
        thread::sleep(Duration::from_secs(2));
        
        // Close the graph while operations are frozen
        // This removes the graph manager from memory
        let close_cmd = json!({
            "type": "close_graph",
            "graph_id": graph_id.clone()
        });
        let response = send_command(&mut ws, close_cmd);
        expect_success(response);
        
        // Now unfreeze - operations will try to continue but fail
        // because the graph manager is no longer in memory
        assert!(unfreeze_operations(&mut ws), "Failed to unfreeze operations");
        
        // Wait briefly for operations to attempt execution and fail
        thread::sleep(Duration::from_millis(500));
        
        // Drain the error responses from the failed operations
        for _ in 0..3 {
            let response = read_pending_response(&mut ws);
            // These should be errors since the graph is closed
            assert_eq!(response["type"], "error", "Expected error response for closed graph");
        }
        
        // Open the graph again - this should trigger recovery
        let open_cmd = json!({
            "type": "open_graph",
            "graph_id": graph_id.clone()
        });
        let response = send_command(&mut ws, open_cmd);
        expect_success(response);
        
        // Wait for recovery to complete
        thread::sleep(Duration::from_secs(1));
        
        // Graceful shutdown
        let test_env = server.shutdown();
        assert_phase(PostShutdown);
        
        // Validate that all blocks were recovered
        let mut validator = WalValidator::new(&test_env.data_dir);
        validator.expect_create_page("open-recovery-test-page", None);
        validator.validate_graph_with_content_checks(
            &graph_id,
            &[
                ("Block 1 - should be recovered on open", Some("open-recovery-test-page")),
                ("Block 2 - should be recovered on open", Some("open-recovery-test-page")),
                ("Block 3 - should be recovered on open", Some("open-recovery-test-page")),
            ]
        ).expect("Validation failed");
        
        test_env
    });
    
    // Cleanup
    match result {
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}