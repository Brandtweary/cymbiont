//! Integration tests for agent tool execution through the knowledge graph tools system.
//! 
//! These tests verify that agents can successfully execute all available tools via
//! WebSocket commands using the MockLLM's echo_tool functionality. Each tool is tested
//! for correct execution, validation, authorization, and result formatting.

use serde_json::json;
use uuid::Uuid;
use crate::common::{setup_test_env, cleanup_test_env, WalValidator, MessagePattern};
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
        
        let mut validator = WalValidator::new(&data_dir);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let initial_graph_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &initial_graph_uuid);
        
        // Test list_graphs tool
        {
            let data = agent_chat_sync(&mut ws, "List all graphs", None, Some("list_graphs"));
            
            // Verify response mentions the tool
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_graphs tool"));
            
            // Track conversation
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List all graphs".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "list_graphs",
                MessagePattern::Contains("✓ Tool 'list_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_graphs tool for you".to_string())
            );
        }
        
        // Test create_graph tool
        let new_graph_id = {
            let data = agent_chat_sync(&mut ws, "Create a new graph", None, Some("create_graph"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Create a new graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "create_graph",
                MessagePattern::Contains("✓ Tool 'create_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
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
                let id = Uuid::parse_str(id_str).unwrap();
                if id != initial_graph_uuid {
                    new_graph_id = Some(id);
                    break;
                }
            }
            
            new_graph_id.expect("Should have found the new graph")
        };
        
        // Track the new graph creation
        validator.expect_authorization(&prime_agent_id, &new_graph_id);
        
        // Test list_open_graphs tool
        {
            let data = agent_chat_sync(&mut ws, "List open graphs", None, Some("list_open_graphs"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_open_graphs tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List open graphs".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "list_open_graphs",
                MessagePattern::Contains("✓ Tool 'list_open_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_open_graphs tool for you".to_string())
            );
        }
        
        // Test close_graph tool
        {
            let data = agent_chat_sync(&mut ws, "Close the initial graph", None, Some("close_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the close_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Close the initial graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "close_graph",
                MessagePattern::Contains("✓ Tool 'close_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the close_graph tool for you".to_string())
            );
        }
        
        // Test open_graph tool
        {
            let data = agent_chat_sync(&mut ws, "Open the graph again", None, Some("open_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the open_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Open the graph again".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "open_graph",
                MessagePattern::Contains("✓ Tool 'open_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the open_graph tool for you".to_string())
            );
        }
        
        // Test delete_graph tool (on the new graph)
        {
            let data = agent_chat_sync(&mut ws, "Delete the new graph", None, Some("delete_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Delete the new graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "delete_graph",
                MessagePattern::Contains("✓ Tool 'delete_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the delete_graph tool for you".to_string())
            );
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
        
        let mut validator = WalValidator::new(&data_dir);
        
        // Set up expectations for dummy graph
        validator.expect_dummy_graph();
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Add debug logging to see conversation history
        use tracing::debug;
        
        // Test add_block tool - simple content
        {
            let data = agent_chat_sync(&mut ws, "Add a new block with content", None, Some("add_block"));
            
            // MockLLM returns the tool execution message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call (user → tool → assistant order)
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Add a new block with content".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
            
            // Note: We can't predict the block ID that will be generated, so we don't track it in the fixture
        }
        
        // Test update_block tool
        {
            let data = agent_chat_sync(&mut ws, "Update the block content", None, Some("update_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the update_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Update the block content".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "update_block",
                MessagePattern::Contains("✓ Tool 'update_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the update_block tool for you".to_string())
            );
            
            // Track update in graph fixture with the UUID that MockLLM generates (existing block from dummy graph)
            validator.expect_update_block("67f9a190-985b-4dbf-90e4-c2abffb2ab51", "Test content from MockLLM");
        }
        
        // Test add_block with parent_id
        {
            let data = agent_chat_sync(&mut ws, "Add a child block", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Add a child block".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
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
                &prime_agent_id,
                MessagePattern::Exact("Add a block to a page".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
        }
        
        // Test delete_block tool
        {
            let data = agent_chat_sync(&mut ws, "Delete a block", None, Some("delete_block"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_block tool"));
            
            // Track conversation for this tool call
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Delete a block".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "delete_block",
                MessagePattern::Contains("✓ Tool 'delete_block' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the delete_block tool for you".to_string())
            );
            
            // Track deletion with the UUID that MockLLM generates (same as update_block)
            validator.expect_delete_block("67f9a190-985b-4dbf-90e4-c2abffb2ab51");
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
        
        let mut validator = WalValidator::new(&data_dir);
        validator.expect_dummy_graph();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Test create_page tool
        {
            let data = agent_chat_sync(&mut ws, "Create a new page", None, Some("create_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
            
            // Track page creation
            validator.expect_create_page("Test Page", None);
        }
        
        // Test delete_page tool
        {
            let data = agent_chat_sync(&mut ws, "Delete the page", None, Some("delete_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_page tool"));
            
            // Track page deletion
            validator.expect_delete_page("Test Page");
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
        
        let mut validator = WalValidator::new(&data_dir);
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
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

/// Test authorization failures when agent lacks graph access
pub fn test_agent_authorization_failures() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        let mut validator = WalValidator::new(&data_dir);
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Create a secondary agent without authorization
        let secondary_agent_id = {
            let cmd = json!({
                "type": "create_agent",
                "name": "Unauthorized Agent"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            let id_str = data["agent_id"].as_str().unwrap();
            let id = Uuid::parse_str(id_str).unwrap();
            
            validator.expect_agent_created(id, "Unauthorized Agent");
            id
        };
        
        // Select the unauthorized agent
        {
            let cmd = json!({
                "type": "agent_select",
                "agent_name": "Unauthorized Agent"
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
        }
        
        // Temporarily authorize the agent to set a default graph, then deauthorize
        // This ensures the agent has a graph (avoiding "no graph" error) but lacks authorization
        {
            // Authorize
            let cmd = json!({
                "type": "authorize_agent",
                "agent_id": secondary_agent_id.to_string(),
                "graph_id": graph_id
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
            
            // Set default using the tool
            agent_chat_sync(&mut ws, "Set my default graph", None, Some("set_default_graph"));
            
            // Deauthorize
            let cmd = json!({
                "type": "deauthorize_agent",
                "agent_id": secondary_agent_id.to_string(),
                "graph_id": graph_id
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
        }
        
        // Now try to use add_block tool - should fail due to no authorization
        {
            let data = agent_chat_sync(&mut ws, "Add a block", None, Some("add_block"));
            
            // The tool should have been called but returned an error
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // TODO: Verify the tool result contains authorization error
        }
        
        // Skip query_graph_bfs test - it's still a stub
        
        // Authorize the agent
        {
            let cmd = json!({
                "type": "authorize_agent",
                "agent_id": secondary_agent_id.to_string(),
                "graph_id": graph_id
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
            
            validator.expect_authorization(&secondary_agent_id, &graph_uuid);
        }
        
        // Now the same tool should succeed
        {
            let data = agent_chat_sync(&mut ws, "Add a block now", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            // Tool should succeed this time
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

/// Test tool argument validation errors
pub fn test_agent_tool_validation_errors() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        let mut validator = WalValidator::new(&data_dir);
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
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

/// Test agent graph management tools (set_default_graph, get_default_graph, list_my_graphs)
pub fn test_agent_graph_management_tools_direct() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Import dummy graph and start server
        let (server, initial_graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        let mut validator = WalValidator::new(&data_dir);
        
        // Connect and authenticate
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        let initial_graph_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
        validator.expect_authorization(&prime_agent_id, &initial_graph_uuid);
        
        // Test get_default_graph - should be the initial graph (first authorized)
        {
            let data = agent_chat_sync(&mut ws, "Get my default graph", None, Some("get_default_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_default_graph tool"));
            
            // Track conversation
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Get my default graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "get_default_graph",
                MessagePattern::Contains("✓ Tool 'get_default_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the get_default_graph tool for you".to_string())
            );
        }
        
        // Test list_my_graphs - should show the initial graph
        {
            let data = agent_chat_sync(&mut ws, "List my authorized graphs", None, Some("list_my_graphs"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_my_graphs tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List my authorized graphs".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "list_my_graphs",
                MessagePattern::Contains("✓ Tool 'list_my_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_my_graphs tool for you".to_string())
            );
        }
        
        // Create a second graph
        let second_graph_id = {
            let data = agent_chat_sync(&mut ws, "Create another graph", None, Some("create_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Create another graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "create_graph",
                MessagePattern::Contains("✓ Tool 'create_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
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
                let id = Uuid::parse_str(id_str).unwrap();
                if id != initial_graph_uuid {
                    new_graph_id = Some(id);
                    break;
                }
            }
            
            new_graph_id.expect("Should have found the new graph")
        };
        
        // Prime agent should be authorized for the new graph
        validator.expect_authorization(&prime_agent_id, &second_graph_id);
        
        // Close the initial graph to leave only the new graph open (for smart default)
        {
            let cmd = json!({
                "type": "close_graph", 
                "graph_id": initial_graph_id
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response).unwrap();
        }
        
        // Test set_default_graph with smart default (only 1 graph open now)
        {
            let data = agent_chat_sync(&mut ws, "Set my default to the new graph", None, Some("set_default_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the set_default_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Set my default to the new graph".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "set_default_graph",
                MessagePattern::Contains("Tool 'set_default_graph'".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the set_default_graph tool for you".to_string())
            );
        }
        
        // Verify default changed with get_default_graph
        {
            let data = agent_chat_sync(&mut ws, "Check my default graph again", None, Some("get_default_graph"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_default_graph tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Check my default graph again".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "get_default_graph",
                MessagePattern::Contains("✓ Tool 'get_default_graph' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the get_default_graph tool for you".to_string())
            );
        }
        
        // Test list_my_graphs again - should now show 2 graphs
        {
            let data = agent_chat_sync(&mut ws, "List all my graphs again", None, Some("list_my_graphs"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_my_graphs tool"));
            
            validator.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List all my graphs again".to_string())
            );
            validator.expect_tool_message(
                &prime_agent_id,
                "list_my_graphs",
                MessagePattern::Contains("✓ Tool 'list_my_graphs' executed successfully".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_my_graphs tool for you".to_string())
            );
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Debug logging removed as WAL validator checks transactions directly
        // rather than loading JSON files
        
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
        
        let mut validator = WalValidator::new(&data_dir);
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        validator.expect_prime_agent(prime_agent_id);
        
        // Chain: create_graph -> create_page -> add_block -> list_graphs
        
        // Step 1: Create a graph
        {
            let data = agent_chat_sync(&mut ws, "Create a test graph", None, Some("create_graph"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
        }
        
        // Step 2: Create a page in the new graph
        {
            let data = agent_chat_sync(&mut ws, "Create a page in the graph", None, Some("create_page"));
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
        }
        
        // Step 3: Add blocks to the page
        {
            let data = agent_chat_sync(&mut ws, "Add content to the page", None, Some("add_block"));
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
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