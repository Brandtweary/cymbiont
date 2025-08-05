use std::thread;
use std::time::Duration;
use std::fs;
use serde_json::{json, Value};
use tracing::debug;
use crate::common::{setup_test_env, cleanup_test_env};
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
        
        // Now validate final state after server has saved everything
        validate_startup_recovery(&test_env.data_dir, &_graph_id);
        
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
        debug!("Importing dummy graph (graph A)");
        let graph_a_id = import_dummy_graph(&test_env);
        
        // Start server
        debug!("Starting server");
        let mut server = TestServer::start(test_env.clone());
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &token), "Authentication failed");
        debug!("Connected and authenticated to server");
        
        // Create a second graph (graph B)
        debug!("Creating second graph (graph B)");
        let create_graph_cmd = json!({
            "type": "create_graph",
            "name": "recovery-test-graph-b",
            "description": "Graph for testing switch recovery"
        });
        
        let response = send_command(&mut ws, create_graph_cmd);
        let graph_b_data = expect_success(response);
        let graph_b_id = graph_b_data.unwrap()["id"].as_str().unwrap().to_string();
        debug!("Created graph B: {}", graph_b_id);
        
        // Switch to graph B
        debug!("Switching to graph B");
        let switch_cmd = json!({
            "type": "switch_graph",
            "graph_id": graph_b_id.clone()
        });
        let response = send_command(&mut ws, switch_cmd);
        expect_success(response);
        debug!("Switched to graph B successfully");
        
        
        // First create the page (not frozen)
        let create_page_cmd = json!({
            "type": "create_page",
            "name": "switch-recovery-page",
            "properties": {"test": "switch-recovery"}
        });
        let response = send_command(&mut ws, create_page_cmd);
        expect_success(response);
        debug!("Created switch-recovery-page on graph B");
        
        // Now freeze and create blocks
        assert!(freeze_operations(&mut ws), "Failed to freeze operations");
        debug!("Operations frozen on graph B - creating pending transactions");
        
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
        for (i, op) in operations.iter().enumerate() {
            debug!("Sending operation {} to graph B: {:?}", i + 1, op);
            send_command_async(&mut ws, op.clone());
        }
        
        // Wait longer to ensure operations reach transaction creation phase
        // The sled database has a default flush interval, so we need to wait for it
        thread::sleep(Duration::from_secs(2));
        
        // Critical: Switch back to graph A before killing server
        // This ensures graph B has pending transactions but is not the active graph
        debug!("Switching back to graph A before crash");
        let switch_back_cmd = json!({
            "type": "switch_graph", 
            "graph_id": graph_a_id.clone()
        });
        
        // Switch graph doesn't use with_active_graph_transaction, so it bypasses freeze
        let response = send_command(&mut ws, switch_back_cmd);
        expect_success(response);
        debug!("Switched back to graph A - graph B now has pending transactions");
        
        // Force kill the server (graph A is active, graph B has pending transactions)
        debug!("Force-killing server with graph A active, graph B having pending transactions");
        server.force_kill();
        thread::sleep(Duration::from_millis(500));
        
        // Start new server instance (will recover graph A on startup, but not graph B)
        debug!("Starting recovered server");
        let server2 = TestServer::start(test_env.clone());
        let port2 = server2.port();
        
        assert_phase(PreShutdown);
        
        // Connect to recovered server
        debug!("Connecting to recovered server");
        let mut ws2 = connect_websocket(port2);
        
        // Read the new token (it rotates on restart)
        let new_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws2, &new_token), "Authentication failed on recovered server");
        
        // Now switch to graph B - this should trigger recovery of pending transactions
        debug!("Switching to graph B to trigger switch-time recovery");
        let switch_to_b_cmd = json!({
            "type": "switch_graph",
            "graph_id": graph_b_id.clone()
        });
        
        let response = send_command(&mut ws2, switch_to_b_cmd);
        expect_success(response);
        debug!("Switched to graph B - recovery should have happened");
        
        // Graceful shutdown
        let test_env = server2.shutdown();
        assert_phase(PostShutdown);
        
        // Now validate final state after server has saved everything
        debug!("Validating switch recovery data in knowledge graph");
        validate_switch_recovery(&test_env.data_dir, &graph_b_id);
        
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

/// Validate that the startup recovery actually recovered the expected data
fn validate_startup_recovery(data_dir: &std::path::Path, graph_id: &str) {
    // Read the knowledge graph
    let graph_path = data_dir.join("graphs")
        .join(graph_id)
        .join("knowledge_graph.json");
    
    let graph_content = fs::read_to_string(&graph_path)
        .expect("Failed to read knowledge graph");
    
    let graph: Value = serde_json::from_str(&graph_content)
        .expect("Failed to parse knowledge graph");
    
    let nodes = graph["graph"]["nodes"].as_array()
        .expect("No nodes in graph");
    
    // Verify recovery-test-page exists
    let recovery_page = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Page") && 
            n["pkm_id"].as_str() == Some("recovery-test-page")
        })
        .expect("recovery-test-page not found - recovery failed!");
    
    assert_eq!(
        recovery_page["node_type"].as_str(),
        Some("Page"),
        "recovery-test-page should be a Page node"
    );
    
    // Verify recovery-test-page-2 exists with properties
    let recovery_page_2 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Page") && 
            n["pkm_id"].as_str() == Some("recovery-test-page-2")
        })
        .expect("recovery-test-page-2 not found - recovery failed!");
    
    assert_eq!(
        recovery_page_2["properties"]["test"].as_str(),
        Some("recovery"),
        "recovery-test-page-2 should have test property"
    );
    
    // Verify the specific blocks were recovered
    let block1 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Block") && 
            n["content"].as_str() == Some("Recovery test block 1")
        })
        .expect("'Recovery test block 1' not found - recovery failed!");
    
    // Verify it's connected to recovery-test-page
    // First, we need to find the node indices
    let nodes_array = graph["graph"]["nodes"].as_array()
        .expect("No nodes array in graph");
    
    let page_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == Some("recovery-test-page")
    }).expect("Could not find index for recovery-test-page");
    
    let block1_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == block1["pkm_id"].as_str()
    }).expect("Could not find index for block 1");
    
    // Now check edges (stored as [source_index, target_index, data])
    let edges = graph["graph"]["edges"].as_array()
        .expect("No edges in graph");
    
    let page_to_block_edge = edges.iter()
        .find(|e| {
            if let (Some(source), Some(target)) = (e[0].as_u64(), e[1].as_u64()) {
                source as usize == page_index && target as usize == block1_index
            } else {
                false
            }
        })
        .expect("Block 1 not connected to recovery-test-page");
    
    assert_eq!(
        page_to_block_edge[2]["edge_type"].as_str(),
        Some("PageToBlock"),
        "Wrong edge type for page to block connection"
    );
    
    // Verify block 2 with parent
    let _block2 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Block") && 
            n["content"].as_str() == Some("Recovery test block 2 with parent")
        })
        .expect("'Recovery test block 2 with parent' not found - recovery failed!");
    
    // Note: We used "dummy-parent-id" which doesn't exist, so this block might be orphaned
    // That's OK - the important thing is that it was recovered
    
    // Verify block 3 on page 2
    let block3 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Block") && 
            n["content"].as_str() == Some("Recovery test block 3")
        })
        .expect("'Recovery test block 3' not found - recovery failed!");
    
    // Find indices for page 2 and block 3
    let page2_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == Some("recovery-test-page-2")
    }).expect("Could not find index for recovery-test-page-2");
    
    let block3_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == block3["pkm_id"].as_str()
    }).expect("Could not find index for block 3");
    
    let page2_to_block3_edge = edges.iter()
        .find(|e| {
            if let (Some(source), Some(target)) = (e[0].as_u64(), e[1].as_u64()) {
                source as usize == page2_index && target as usize == block3_index
            } else {
                false
            }
        })
        .expect("Block 3 not connected to recovery-test-page-2");
    
    assert_eq!(
        page2_to_block3_edge[2]["edge_type"].as_str(),
        Some("PageToBlock"),
        "Wrong edge type for page 2 to block 3 connection"
    );
    
    debug!("All recovery data validated successfully!");
}

/// Validate that the switch recovery actually recovered the expected data
fn validate_switch_recovery(data_dir: &std::path::Path, graph_id: &str) {
    // Read the knowledge graph for graph B
    let graph_path = data_dir.join("graphs")
        .join(graph_id)
        .join("knowledge_graph.json");
    
    let graph_content = fs::read_to_string(&graph_path)
        .expect("Failed to read knowledge graph");
    
    let graph: Value = serde_json::from_str(&graph_content)
        .expect("Failed to parse knowledge graph");
    
    let nodes = graph["graph"]["nodes"].as_array()
        .expect("No nodes in graph");
    
    // Verify switch-recovery-page exists with properties
    let switch_page = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Page") && 
            n["pkm_id"].as_str() == Some("switch-recovery-page")
        })
        .expect("switch-recovery-page not found - switch recovery failed!");
    
    assert_eq!(
        switch_page["properties"]["test"].as_str(),
        Some("switch-recovery"),
        "switch-recovery-page should have test property"
    );
    
    // Verify the specific blocks were recovered
    let block1 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Block") && 
            n["content"].as_str() == Some("Switch recovery block 1")
        })
        .expect("'Switch recovery block 1' not found - switch recovery failed!");
    
    let block2 = nodes.iter()
        .find(|n| {
            n["node_type"].as_str() == Some("Block") && 
            n["content"].as_str() == Some("Switch recovery block 2")
        })
        .expect("'Switch recovery block 2' not found - switch recovery failed!");
    
    // Verify both blocks are connected to the page
    let nodes_array = nodes;
    let edges = graph["graph"]["edges"].as_array()
        .expect("No edges in graph");
    
    // Find indices
    let page_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == Some("switch-recovery-page")
    }).expect("Could not find index for switch-recovery-page");
    
    let block1_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == block1["pkm_id"].as_str()
    }).expect("Could not find index for block 1");
    
    let block2_index = nodes_array.iter().position(|n| {
        n["pkm_id"].as_str() == block2["pkm_id"].as_str()
    }).expect("Could not find index for block 2");
    
    let page_to_block1_edge = edges.iter()
        .find(|e| {
            if let (Some(source), Some(target)) = (e[0].as_u64(), e[1].as_u64()) {
                source as usize == page_index && target as usize == block1_index
            } else {
                false
            }
        })
        .expect("Block 1 not connected to switch-recovery-page");
    
    let page_to_block2_edge = edges.iter()
        .find(|e| {
            if let (Some(source), Some(target)) = (e[0].as_u64(), e[1].as_u64()) {
                source as usize == page_index && target as usize == block2_index
            } else {
                false
            }
        })
        .expect("Block 2 not connected to switch-recovery-page");
    
    assert_eq!(
        page_to_block1_edge[2]["edge_type"].as_str(),
        Some("PageToBlock"),
        "Wrong edge type for page to block 1 connection"
    );
    
    assert_eq!(
        page_to_block2_edge[2]["edge_type"].as_str(),
        Some("PageToBlock"),
        "Wrong edge type for page to block 2 connection"
    );
    
    debug!("All switch recovery data validated successfully!");
}