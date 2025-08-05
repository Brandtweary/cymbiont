use std::thread;
use std::time::Duration;
use serde_json::json;
use crate::common::{setup_test_env, cleanup_test_env, GraphValidationFixture};
use crate::common::test_harness::{
    connect_websocket, authenticate_websocket, read_auth_token,
    freeze_operations, send_command_async, send_command, expect_success,
    PreShutdown, PostShutdown, assert_phase, import_dummy_graph, TestServer,
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
        // Setup: Import graph first
        let _graph_id = import_dummy_graph(&test_env);
        
        // Start server
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Initialize validation fixture
        let mut fixture = GraphValidationFixture::new();
        
        // Record expectations for pages
        fixture.expect_create_page("recovery-test-page", None);
        fixture.expect_create_page("recovery-test-page-2", Some(json!({"test": "recovery"})));
        
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
        
        // Validate final state using the fixture
        fixture.validate_graph_with_content_checks(
            &test_env.data_dir, 
            &_graph_id,
            &[
                ("Recovery test block 1", Some("recovery-test-page")),
                ("Recovery test block 2 with parent", None), // No page since parent doesn't exist
                ("Recovery test block 3", Some("recovery-test-page-2")),
            ]
        );
        
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

/// Test crash recovery during graph switching
/// 
/// This test verifies that pending transactions are recovered when switching
/// to a graph (not just on startup). It creates pending transactions on graph B,
/// switches back to graph A, crashes, then restarts and switches to graph B
/// to trigger the switch-time recovery.
pub fn test_graph_switch_recovery() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Setup: Import initial graph (graph A)
        let graph_a_id = import_dummy_graph(&test_env);
        
        // Start server
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Create a second graph (graph B)
        let create_graph_cmd = json!({
            "type": "create_graph",
            "name": "recovery-test-graph-b",
            "description": "Graph for testing switch recovery"
        });
        
        let response = send_command(&mut ws, create_graph_cmd);
        let graph_b_data = expect_success(response);
        let graph_b_id = graph_b_data.unwrap()["id"].as_str().unwrap().to_string();
        
        // Switch to graph B
        let switch_cmd = json!({
            "type": "switch_graph",
            "graph_id": graph_b_id.clone()
        });
        let response = send_command(&mut ws, switch_cmd);
        expect_success(response);
        
        
        // First create the page (not frozen)
        let create_page_cmd = json!({
            "type": "create_page",
            "name": "switch-recovery-page",
            "properties": {"test": "switch-recovery"}
        });
        let response = send_command(&mut ws, create_page_cmd);
        expect_success(response);
        
        // Now freeze and create blocks
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        
        let operations = vec![
            json!({
                "type": "create_block",
                "content": "Switch recovery block 1", 
                "page_name": "switch-recovery-page"
            }),
            json!({
                "type": "create_block",
                "content": "Switch recovery block 2",
                "page_name": "switch-recovery-page"
            }),
        ];
        
        // Send operations (they'll be pending due to freeze)
        for op in operations.iter() {
            send_command_async(&mut ws, op.clone());
        }
        
        // Wait longer to ensure operations reach transaction creation phase
        // The sled database has a default flush interval, so we need to wait for it
        thread::sleep(Duration::from_secs(2));
        
        // Critical: Switch back to graph A before killing server
        // This ensures graph B has pending transactions but is not the active graph
        let switch_back_cmd = json!({
            "type": "switch_graph", 
            "graph_id": graph_a_id.clone()
        });
        
        // Switch graph doesn't use with_active_graph_transaction, so it bypasses freeze
        let response = send_command(&mut ws, switch_back_cmd);
        expect_success(response);
        
        // Force kill the server (graph A is active, graph B has pending transactions)
        server.force_kill();
        thread::sleep(Duration::from_millis(500));
        
        // Start new server instance (will recover graph A on startup, but not graph B)
        let server2 = TestServer::start(test_env.clone());
        let port2 = server2.port();
        
        assert_phase(PreShutdown);
        
        // Connect to recovered server
        let mut ws2 = connect_websocket(port2);
        
        // Read the new token (it rotates on restart)
        let new_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws2, &new_token), "Authentication failed on recovered server");
        
        // Now switch to graph B - this should trigger recovery of pending transactions
        let switch_to_b_cmd = json!({
            "type": "switch_graph",
            "graph_id": graph_b_id.clone()
        });
        
        let response = send_command(&mut ws2, switch_to_b_cmd);
        expect_success(response);
        
        // Graceful shutdown
        let test_env = server2.shutdown();
        assert_phase(PostShutdown);
        
        // Validate final state using fixture
        let mut fixture = GraphValidationFixture::new();
        fixture.expect_create_page("switch-recovery-page", Some(json!({"test": "switch-recovery"})));
        
        fixture.validate_graph_with_content_checks(
            &test_env.data_dir, 
            &graph_b_id,
            &[
                ("Switch recovery block 1", Some("switch-recovery-page")),
                ("Switch recovery block 2", Some("switch-recovery-page")),
            ]
        );
        
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

