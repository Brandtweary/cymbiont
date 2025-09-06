//! Integration tests for agent tool execution through the knowledge graph tools system.
//! 
//! These tests verify that agents can successfully execute all available tools via
//! WebSocket commands using the MockLLM's echo_tool functionality. Each tool is tested
//! for correct execution, validation, authorization, and result formatting.

use serde_json::json;
use uuid::Uuid;
use crate::common::{setup_test_env, cleanup_test_env, TestValidator, MessagePattern};
use crate::common::test_harness::{
    PreShutdown, PostShutdown, assert_phase,
    connect_websocket, send_command, expect_success, 
    authenticate_websocket, setup_with_graph, read_auth_token,
    TestServer
};

use crate::common::test_harness::agent_chat_sync;

/// Test graph management tools (create, list, open, close, delete graphs)
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
            let data = agent_chat_sync(&mut ws, "List all graphs", None, Some("list_graphs"));
            
            // Verify response mentions the tool
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_graphs tool"));
            
            // Track conversation
            validator.expect_user_message(
                MessagePattern::Exact("List all graphs".to_string())
            );
            validator.expect_tool_message(
                "list_graphs",
                MessagePattern::Contains("✓ Tool 'list_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the list_graphs tool for you".to_string())
            );
        }
        
        // Test create_graph tool
        let _new_graph_uuid = {
            let data = agent_chat_sync(&mut ws, "Create a new graph", None, Some("create_graph"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            validator.expect_user_message(
                MessagePattern::Exact("Create a new graph".to_string())
            );
            validator.expect_tool_message(
                "create_graph",
                MessagePattern::Contains("✓ Tool 'create_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the create_graph tool for you".to_string())
            );
            
            // Get the new graph ID by listing all graphs
            let list_cmd = json!({ "type": "list_graphs" });
            let list_response = send_command(&mut ws, list_cmd);
            let list_data = expect_success(list_response).unwrap();
            let graphs = list_data["graphs"].as_array().unwrap();
            
            // Find the graph that isn't the initial one
            let mut new_graph_id = None;
            for graph in graphs {
                let id_str = graph["id"].as_str().unwrap();
                if id_str != initial_graph_id {
                    new_graph_id = Some(Uuid::parse_str(id_str).unwrap());
                    break;
                }
            }
            
            let uuid = new_graph_id.expect("Should have found the new graph");
            
            // Validate the graph was created and opened in the registry
            // MockLLM doesn't provide a name, so it gets auto-generated as "Graph {id}"
            let expected_name = format!("Graph {}", &uuid.to_string()[..8]);
            validator.expect_graph_created(uuid, &expected_name);
            validator.expect_graph_open(uuid);
            
            uuid
        };
        
        // Track the new graph creation
        
        // Test list_open_graphs tool
        {
            let data = agent_chat_sync(&mut ws, "List open graphs", None, Some("list_open_graphs"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_open_graphs tool"));
            
            validator.expect_user_message(
                MessagePattern::Exact("List open graphs".to_string())
            );
            validator.expect_tool_message(
                "list_open_graphs",
                MessagePattern::Contains("✓ Tool 'list_open_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the list_open_graphs tool for you".to_string())
            );
        }
        
        // Test close_graph and open_graph tools
        {
            // Close the new graph first to make initial the only open graph
            let close_cmd = json!({ 
                "type": "close_graph",
                "graph_id": _new_graph_uuid.to_string()
            });
            let close_response = send_command(&mut ws, close_cmd);
            expect_success(close_response);
            validator.expect_graph_closed(_new_graph_uuid);
            
            // Now test agent close_graph with smart default (only one graph open)
            let data = agent_chat_sync(&mut ws, "Close the open graph", None, Some("close_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the close_graph tool"));
            
            validator.expect_user_message(
                MessagePattern::Exact("Close the open graph".to_string())
            );
            validator.expect_tool_message(
                "close_graph",
                MessagePattern::Contains("✓ Tool 'close_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the close_graph tool for you".to_string())
            );
            
            // Validate the initial graph was closed
            let initial_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
            validator.expect_graph_closed(initial_uuid);
            
            // Now both are closed. Open the initial graph directly
            let open_cmd = json!({ 
                "type": "open_graph",
                "graph_id": initial_graph_id
            });
            let open_response = send_command(&mut ws, open_cmd);
            expect_success(open_response);
            validator.expect_graph_open(initial_uuid);
        }
        
        // Test delete_graph tool - delete the initial graph (it's open)
        {
            // Delete the initial graph which is the only open graph
            // This should work with smart default
            let data = agent_chat_sync(&mut ws, "Delete the open graph", None, Some("delete_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_graph tool"));
            
            validator.expect_user_message(
                MessagePattern::Exact("Delete the open graph".to_string())
            );
            validator.expect_tool_message(
                "delete_graph",
                MessagePattern::Contains("✓ Tool 'delete_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the delete_graph tool for you".to_string())
            );
            
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
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test block operations (add, update, delete blocks)
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
        
        
        // Add debug logging to see conversation history
        use tracing::debug;
        
        // Test add_block tool - simple content
        {
            let data = agent_chat_sync(&mut ws, "Add a new block with content", None, Some("add_block"));
            
            // MockLLM returns the tool execution message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call (user → tool → assistant order)
            validator.expect_user_message(
                MessagePattern::Exact("Add a new block with content".to_string())
            );
            validator.expect_tool_message(
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
            
            // Note: We can't predict the block ID that will be generated, so we don't track it in the fixture
        }
        
        // Test update_block tool
        // First create a block to update
        {
            let create_cmd = json!({
                "type": "create_block",
                "content": "Block to be updated by agent",
                "page_name": "agent-test-page"
            });
            let response = send_command(&mut ws, create_cmd);
            let data = expect_success(response).expect("No data in create_block response");
            let block_id = data["block_id"].as_str().expect("No block_id").to_string();
            // Don't track the create since we're testing update - the final state matters
            
            // Now test update - include the block_id in the message
            // MockLLM extracts the UUID from the message content
            let message = format!("Update block {} with new content", block_id);
            let data = agent_chat_sync(&mut ws, &message, None, Some("update_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the update_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                MessagePattern::Exact(message.clone())
            );
            validator.expect_tool_message(
                "update_block",
                MessagePattern::Contains("✓ Tool 'update_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the update_block tool for you".to_string())
            );
            
            // Validate the block was actually updated (this overrides the initial content expectation)
            validator.expect_update_block(&block_id, "Test content from MockLLM", Some(&graph_id));
        }
        
        // Test add_block with parent_id
        {
            let data = agent_chat_sync(&mut ws, "Add a child block", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                MessagePattern::Exact("Add a child block".to_string())
            );
            validator.expect_tool_message(
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
        }
        
        // Test add_block with page_name
        {
            let data = agent_chat_sync(&mut ws, "Add a block to a page", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                MessagePattern::Exact("Add a block to a page".to_string())
            );
            validator.expect_tool_message(
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
        }
        
        // Test delete_block tool
        // First create a block to delete
        {
            let create_cmd = json!({
                "type": "create_block",
                "content": "Block to be deleted by agent",
                "page_name": "agent-test-page"
            });
            let response = send_command(&mut ws, create_cmd);
            let data = expect_success(response).expect("No data in create_block response");
            let block_id = data["block_id"].as_str().expect("No block_id").to_string();
            // Don't track the create since we're testing delete
            
            // Now test delete - include the block_id in the message
            // MockLLM extracts the UUID from the message content
            let message = format!("Delete block {}", block_id);
            let data = agent_chat_sync(&mut ws, &message, None, Some("delete_block"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                MessagePattern::Exact(message.clone())
            );
            validator.expect_tool_message(
                "delete_block",
                MessagePattern::Contains("✓ Tool 'delete_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                MessagePattern::Exact("I've executed the delete_block tool for you".to_string())
            );
            
            // Validate the block was actually deleted
            validator.expect_delete_block(&block_id, Some(&graph_id));
        }
        
        // Debug: print conversation history before validation
        {
            let cmd = json!({
                "type": "agent_history"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            let messages = data["messages"].as_array().unwrap();
            for (i, msg) in messages.iter().enumerate() {
                debug!("Message {}: role={}, content={}", 
                    i, 
                    msg["role"].as_str().unwrap_or("unknown"),
                    msg["content"].as_str().unwrap_or("(not a string)")
                );
            }
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Validate final state - both agent conversation and graph changes
        validator.validate_all().expect("Validation failed");
        
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
            let data = agent_chat_sync(&mut ws, "Create a new page", None, Some("create_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
            
            // Track page creation in the original graph
            validator.expect_create_page("Test Page", None, Some(&graph_id));
        }
        
        // Test delete_page tool
        {
            let data = agent_chat_sync(&mut ws, "Delete the page", None, Some("delete_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_page tool"));
            
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
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}

/// Test query operations (get_node and query_graph_bfs)
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
            let data = agent_chat_sync(&mut ws, "Get node information", None, Some("get_node"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_node tool"));
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
        Ok(test_env) => cleanup_test_env(test_env),
        Err(panic) => {
            cleanup_test_env(cleanup_env);
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
        
        
        // Test with invalid tool name (validation happens before execution)
        // MockLLM will generate args, but validation should catch issues
        // For now we're testing that tools execute - detailed validation testing
        // would require modifying MockLLM to generate invalid args
        
        // Test tool with mock-generated args (should have valid UUIDs)
        {
            let data = agent_chat_sync(&mut ws, "Try to update a block", None, Some("update_block"));
            
            // MockLLM generates valid test args, so this should attempt the operation
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the update_block tool"));
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        validator.validate_all().expect("Validation failed");
        
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

/// Test multiple tool calls in sequence (tool chaining)
#[allow(deprecated)] // Using deprecated agent_chat_sync until CQRS refactor
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
            let data = agent_chat_sync(&mut ws, "Create a test graph", None, Some("create_graph"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            // Get the new graph ID by listing graphs
            let list_cmd = json!({ "type": "list_graphs" });
            let list_response = send_command(&mut ws, list_cmd);
            let list_data = expect_success(list_response).unwrap();
            let graphs = list_data["graphs"].as_array().unwrap();
            
            // Should have exactly one graph (the new one we just created)
            assert_eq!(graphs.len(), 1, "Should have exactly one graph");
            let graph_id = graphs[0]["id"].as_str().unwrap().to_string();
            
            // Track that we expect this graph to exist
            // MockLLM doesn't provide a name, so it gets a default name like "Graph {id}"
            let expected_name = format!("Graph {}", &graph_id[..8]);
            validator.expect_graph_created(
                uuid::Uuid::parse_str(&graph_id).unwrap(),
                &expected_name
            );
            
            graph_id
        };
        
        // Step 2: Create a page in the new graph and validate it
        // The created graph is automatically opened, so smart default should work
        {
            let data = agent_chat_sync(&mut ws, "Create a page in the graph", None, Some("create_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
            
            // MockLLM creates a page called "Test Page" - validate it's in the RIGHT graph
            validator.expect_create_page("Test Page", None, Some(&new_graph_id));
        }
        
        // Step 3: Add blocks to the page and validate them
        {
            let data = agent_chat_sync(&mut ws, "Add content to the page", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // MockLLM creates a block with a random UUID and content "Test content from MockLLM"
            // We can't validate the specific block ID since it's random, but we can validate
            // that a block with the expected content was created
            // TODO: The validator needs a way to check for blocks by content, not just ID
        }
        
        // Step 4: List the graphs to verify creation
        {
            let data = agent_chat_sync(&mut ws, "List all graphs", None, Some("list_graphs"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_graphs tool"));
        }
        
        // Verify conversation has all the tool calls tracked
        {
            let cmd = json!({
                "type": "agent_history"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            let messages = data["messages"].as_array().unwrap();
            // Should have 4 user messages and 4 assistant responses minimum
            assert!(messages.len() >= 8, "Should have tracked all tool interactions");
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        validator.validate_all().expect("Validation failed");
        
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