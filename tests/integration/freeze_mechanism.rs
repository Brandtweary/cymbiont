use std::thread;
use std::time::Duration;
use serde_json::json;
use crate::common::{setup_test_env, cleanup_test_env, WalValidator};
use crate::common::test_harness::{
    connect_websocket, authenticate_websocket, read_auth_token,
    freeze_operations, unfreeze_operations, get_freeze_state,
    send_command_async, read_pending_response, send_command, expect_success,
    PreShutdown, PostShutdown, assert_phase, TestServer,
    block_on, import_dummy_graph_http,
};

/// Test that freeze mechanism works correctly
pub fn test_freeze_mechanism() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let server = TestServer::start(test_env);  // Use default 3s duration
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import graph via HTTP - uses existing prime agent
        let _graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        assert_phase(PreShutdown);
        
        // Initialize WAL validator
        let mut validator = WalValidator::new(&data_dir);
        validator.expect_dummy_graph();
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        
        // Test 1: Verify initial state is unfrozen
        assert!(!get_freeze_state(&mut ws), "Initial state should be unfrozen");
        
        // Test 2: Test normal operation completes quickly
        let start = std::time::Instant::now();
        let create_cmd = json!({
            "type": "create_block",
            "content": "Normal operation test"
        });
        
        let response = send_command(&mut ws, create_cmd);
        let elapsed = start.elapsed();
        expect_success(response);
        assert!(elapsed < Duration::from_secs(1), "Normal operation took too long: {:?}", elapsed);
        
        // Test 3: Test freeze command
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        assert!(get_freeze_state(&mut ws), "Operations should be frozen");
        
        // Test 4: Test operations are blocked when frozen
        let create_cmd = json!({
            "type": "create_block",
            "content": "This should be frozen"
        });
        
        // Send command (won't get response while frozen)
        send_command_async(&mut ws, create_cmd);
        
        // Wait a bit to ensure command is processed
        thread::sleep(Duration::from_millis(200));
        
        // Try to create another block - should also be frozen
        let create_cmd2 = json!({
            "type": "create_block",
            "content": "This should also be frozen"
        });
        send_command_async(&mut ws, create_cmd2);
        
        // Test 5: Unfreeze and verify operations complete
        assert!(unfreeze_operations(&mut ws), "Failed to unfreeze operations");
        assert!(!get_freeze_state(&mut ws), "Operations should be unfrozen");
        
        // Now we should get responses for both frozen operations
        let response1 = read_pending_response(&mut ws);
        let data1 = expect_success(response1);
        assert!(data1.is_some(), "First frozen operation should succeed");
        
        let response2 = read_pending_response(&mut ws);
        let data2 = expect_success(response2);
        assert!(data2.is_some(), "Second frozen operation should succeed");
        
        // Test 6: One simplified freeze/unfreeze cycle
        assert!(freeze_operations(&mut ws), "Failed to freeze in cycle");
        
        // Send operation
        let cmd = json!({
            "type": "create_block",
            "content": "Final cycle block"
        });
        send_command_async(&mut ws, cmd);
        
        // Brief pause
        thread::sleep(Duration::from_millis(50));
        
        // Unfreeze
        assert!(unfreeze_operations(&mut ws), "Failed to unfreeze in cycle");
        
        // Get response
        let response = read_pending_response(&mut ws);
        expect_success(response);
        
        // Shutdown
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Validate all operations
        validator.validate_all().expect("Validation failed");
        
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

/// Test that freeze state is shared across connections
pub fn test_freeze_persistence() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let server = TestServer::start(test_env);  // Use default 3s duration
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import graph via HTTP - uses existing prime agent
        let _graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Initialize WAL validator
        let mut validator = WalValidator::new(&data_dir);
        validator.expect_dummy_graph();
        
        // Connection 1: Set freeze
        let mut ws1 = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws1, &token));
        assert!(freeze_operations(&mut ws1), "Failed to freeze from connection 1");
        
        // Connection 2: Should see frozen state
        let mut ws2 = connect_websocket(port);
        assert!(authenticate_websocket(&mut ws2, &token));
        assert!(get_freeze_state(&mut ws2), "Connection 2 should see frozen state");
        
        // Send operation from connection 2 (should be frozen)
        let cmd = json!({
            "type": "create_block",
            "content": "From connection 2"
        });
        send_command_async(&mut ws2, cmd);
        
        // Wait a bit
        thread::sleep(Duration::from_millis(200));
        
        // Unfreeze from connection 1
        assert!(unfreeze_operations(&mut ws1), "Failed to unfreeze from connection 1");
        
        // Connection 2 should see unfrozen state and get response
        assert!(!get_freeze_state(&mut ws2), "Connection 2 should see unfrozen state");
        let response = read_pending_response(&mut ws2);
        expect_success(response);
        
        // Both connections should agree on state
        assert!(!get_freeze_state(&mut ws1), "Connection 1 should see unfrozen");
        assert!(!get_freeze_state(&mut ws2), "Connection 2 should see unfrozen");
        
        // Shutdown
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Validate all operations
        validator.validate_all().expect("Validation failed");
        
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

/// Test freeze timeout behavior (operations don't block forever)
pub fn test_freeze_timeout() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start server FIRST - creates prime agent
        let server = TestServer::start(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // Import graph via HTTP - uses existing prime agent
        let _graph_id = block_on(import_dummy_graph_http(port, &data_dir))
            .expect("Failed to import dummy graph");
        
        assert_phase(PreShutdown);
        
        // Initialize WAL validator
        let mut validator = WalValidator::new(&data_dir);
        validator.expect_dummy_graph();
        
        // Connect and freeze
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token));
        assert!(freeze_operations(&mut ws));
        
        // Send multiple operations while frozen
        for i in 0..5 {
            let cmd = json!({
                "type": "create_block",
                "content": format!("Frozen block {}", i)
            });
            send_command_async(&mut ws, cmd);
        }
        
        // Wait a reasonable time
        thread::sleep(Duration::from_millis(500));
        
        // Unfreeze and collect all responses
        assert!(unfreeze_operations(&mut ws));
        
        // All operations should complete
        for i in 0..5 {
            let response = read_pending_response(&mut ws);
            let data = expect_success(response);
            assert!(data.is_some(), "Operation {} should succeed", i);
        }
        
        let test_env = server.shutdown();
        assert_phase(PostShutdown);
        test_env
    });
    
    match result {
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}