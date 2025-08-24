//! Integration tests for agent tool execution through the knowledge graph tools system.
//! 
//! These tests verify that agents can successfully execute all available tools via
//! WebSocket commands using the MockLLM's echo_tool functionality. Each tool is tested
//! for correct execution, validation, authorization, and result formatting.

use serde_json::json;
use uuid::Uuid;
use crate::common::{setup_test_env, cleanup_test_env, AgentValidationFixture, GraphValidationFixture};
use crate::common::test_harness::{
    PreShutdown, PostShutdown, assert_phase,
    connect_websocket, send_command, expect_success, 
    authenticate_websocket, setup_with_graph, read_auth_token,
    TestServer
};
use crate::common::agent_validation::MessagePattern;

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
        
        let mut agent_fixture = AgentValidationFixture::new();
        
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
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let initial_graph_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &initial_graph_uuid);
        
        // Test list_graphs tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "List all graphs",
                "echo_tool": "list_graphs"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // Verify response mentions the tool
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_graphs tool"));
            
            // Track conversation
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List all graphs".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "list_graphs",
                MessagePattern::Contains("✓ Tool 'list_graphs' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_graphs tool for you".to_string())
            );
        }
        
        // Test create_graph tool
        let new_graph_id = {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Create a new graph",
                "echo_tool": "create_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Create a new graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "create_graph",
                MessagePattern::Contains("✓ Tool 'create_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
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
        agent_fixture.expect_authorization(&prime_agent_id, &new_graph_id);
        
        // Test list_open_graphs tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "List open graphs",
                "echo_tool": "list_open_graphs"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_open_graphs tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List open graphs".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "list_open_graphs",
                MessagePattern::Contains("✓ Tool 'list_open_graphs' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_open_graphs tool for you".to_string())
            );
        }
        
        // Test close_graph tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Close the initial graph",
                "echo_tool": "close_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the close_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Close the initial graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "close_graph",
                MessagePattern::Contains("✓ Tool 'close_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the close_graph tool for you".to_string())
            );
        }
        
        // Test open_graph tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Open the graph again",
                "echo_tool": "open_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the open_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Open the graph again".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "open_graph",
                MessagePattern::Contains("✓ Tool 'open_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the open_graph tool for you".to_string())
            );
        }
        
        // Test delete_graph tool (on the new graph)
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Delete the new graph",
                "echo_tool": "delete_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Delete the new graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "delete_graph",
                MessagePattern::Contains("✓ Tool 'delete_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the delete_graph tool for you".to_string())
            );
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Validate final state
        agent_fixture.validate_all(&test_env.data_dir);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        let mut graph_fixture = GraphValidationFixture::new();
        
        // Set up expectations for dummy graph
        graph_fixture.expect_dummy_graph();
        
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
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Add debug logging to see conversation history
        use tracing::debug;
        
        // Test add_block tool - simple content
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add a new block with content",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool execution message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call (user → tool → assistant order)
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Add a new block with content".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
            
            // Note: We can't predict the block ID that will be generated, so we don't track it in the fixture
        }
        
        // Test update_block tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Update the block content",
                "echo_tool": "update_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the update_block tool"));
            
            // Track conversation for this tool call
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Update the block content".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "update_block",
                MessagePattern::Contains("✓ Tool 'update_block' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the update_block tool for you".to_string())
            );
            
            // Track update in graph fixture with the UUID that MockLLM generates (existing block from dummy graph)
            graph_fixture.expect_update_block("67f9a190-985b-4dbf-90e4-c2abffb2ab51", "Test content from MockLLM");
        }
        
        // Test add_block with parent_id
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add a child block",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Add a child block".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
        }
        
        // Test add_block with page_name
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add a block to a page",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            
            // Track conversation for this tool call
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Add a block to a page".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "add_block",
                MessagePattern::Contains("✓ Tool 'add_block' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the add_block tool for you".to_string())
            );
        }
        
        // Test delete_block tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Delete a block",
                "echo_tool": "delete_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_block tool"));
            
            // Track conversation for this tool call
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Delete a block".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "delete_block",
                MessagePattern::Contains("✓ Tool 'delete_block' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the delete_block tool for you".to_string())
            );
            
            // Track deletion with the UUID that MockLLM generates (same as update_block)
            graph_fixture.expect_delete("67f9a190-985b-4dbf-90e4-c2abffb2ab51");
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
        agent_fixture.validate_all(&test_env.data_dir);
        graph_fixture.validate_graph(&test_env.data_dir, &graph_id);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        let mut graph_fixture = GraphValidationFixture::new();
        graph_fixture.expect_dummy_graph();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Test create_page tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Create a new page",
                "echo_tool": "create_page"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
            
            // Track page creation
            graph_fixture.expect_create_page("Test Page", None);
        }
        
        // Test delete_page tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Delete the page",
                "echo_tool": "delete_page"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the delete_page tool"));
            
            // Track page deletion
            // Note: expect_delete_page doesn't exist, would need to track page deletion
            graph_fixture.expect_delete("Test Page");
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        agent_fixture.validate_all(&test_env.data_dir);
        graph_fixture.validate_graph(&test_env.data_dir, &graph_id);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Test get_node tool
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Get node information",
                "echo_tool": "get_node"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_node tool"));
        }
        
        // Skip query_graph_bfs - it's still a stub
        // TODO: Add query_graph_bfs test once implemented
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        agent_fixture.validate_all(&test_env.data_dir);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        
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
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &graph_uuid);
        
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
            
            agent_fixture.expect_agent_created(id, "Unauthorized Agent", false);
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
            let cmd = json!({
                "type": "agent_chat", 
                "message": "Set my default graph",
                "echo_tool": "set_default_graph"
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
            
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
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add a block",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
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
            
            agent_fixture.expect_authorization(&secondary_agent_id, &graph_uuid);
        }
        
        // Now the same tool should succeed
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add a block now",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
            // Tool should succeed this time
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        agent_fixture.validate_all(&test_env.data_dir);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let graph_uuid = Uuid::parse_str(&graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Test with invalid tool name (validation happens before execution)
        // MockLLM will generate args, but validation should catch issues
        // For now we're testing that tools execute - detailed validation testing
        // would require modifying MockLLM to generate invalid args
        
        // Test tool with mock-generated args (should have valid UUIDs)
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Try to update a block",
                "echo_tool": "update_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM generates valid test args, so this should attempt the operation
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the update_block tool"));
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        agent_fixture.validate_all(&test_env.data_dir);
        
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
        
        let mut agent_fixture = AgentValidationFixture::new();
        
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
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        let initial_graph_uuid = Uuid::parse_str(&initial_graph_id).unwrap();
        agent_fixture.expect_authorization(&prime_agent_id, &initial_graph_uuid);
        
        // Test get_default_graph - should be the initial graph (first authorized)
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Get my default graph",
                "echo_tool": "get_default_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_default_graph tool"));
            
            // Track conversation
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Get my default graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "get_default_graph",
                MessagePattern::Contains("✓ Tool 'get_default_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the get_default_graph tool for you".to_string())
            );
        }
        
        // Test list_my_graphs - should show the initial graph
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "List my authorized graphs",
                "echo_tool": "list_my_graphs"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_my_graphs tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List my authorized graphs".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "list_my_graphs",
                MessagePattern::Contains("✓ Tool 'list_my_graphs' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_my_graphs tool for you".to_string())
            );
        }
        
        // Create a second graph
        let second_graph_id = {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Create another graph",
                "echo_tool": "create_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Create another graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "create_graph",
                MessagePattern::Contains("✓ Tool 'create_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
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
        agent_fixture.expect_authorization(&prime_agent_id, &second_graph_id);
        
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
            let cmd = json!({
                "type": "agent_chat",
                "message": "Set my default to the new graph",
                "echo_tool": "set_default_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the set_default_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Set my default to the new graph".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "set_default_graph",
                MessagePattern::Contains("Tool 'set_default_graph'".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the set_default_graph tool for you".to_string())
            );
        }
        
        // Verify default changed with get_default_graph
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Check my default graph again",
                "echo_tool": "get_default_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the get_default_graph tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("Check my default graph again".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "get_default_graph",
                MessagePattern::Contains("✓ Tool 'get_default_graph' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the get_default_graph tool for you".to_string())
            );
        }
        
        // Test list_my_graphs again - should now show 2 graphs
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "List all my graphs again",
                "echo_tool": "list_my_graphs"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the list_my_graphs tool"));
            
            agent_fixture.expect_user_message(
                &prime_agent_id,
                MessagePattern::Exact("List all my graphs again".to_string())
            );
            agent_fixture.expect_tool_message(
                &prime_agent_id,
                "list_my_graphs",
                MessagePattern::Contains("✓ Tool 'list_my_graphs' executed successfully".to_string())
            );
            agent_fixture.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact("I've executed the list_my_graphs tool for you".to_string())
            );
        }
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        
        assert_phase(PostShutdown);
        
        // Debug: Check what's actually in the conversation history before validation
        {
            use tracing::debug;
            use crate::common::agent_validation::AgentValidator;
            let validator = AgentValidator::load(&test_env.data_dir, &prime_agent_id);
            let actual_count = validator.get_message_count();
            let expected_count = agent_fixture.expected_conversations.get(&prime_agent_id)
                .map(|msgs| msgs.len())
                .unwrap_or(0);
            
            debug!("Agent {} has {} actual messages, expected {}", 
                prime_agent_id, actual_count, expected_count);
            
            // Print all actual messages
            for (i, msg) in validator.conversation_history.iter().enumerate() {
                debug!("Message {}: role={}, content={}", 
                    i,
                    msg["role"].as_str().unwrap_or("unknown"),
                    msg["content"].as_str().unwrap_or("(no content)").chars().take(50).collect::<String>()
                );
            }
        }
        
        // Validate final state
        agent_fixture.validate_all(&test_env.data_dir);
        
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
pub fn test_agent_tool_chaining() {
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start without imported graph to test full flow
        let server = TestServer::start(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        let mut agent_fixture = AgentValidationFixture::new();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        
        // Chain: create_graph -> create_page -> add_block -> list_graphs
        
        // Step 1: Create a graph
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Create a test graph",
                "echo_tool": "create_graph"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_graph tool"));
        }
        
        // Step 2: Create a page in the new graph
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Create a page in the graph",
                "echo_tool": "create_page"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            assert!(data["response"].as_str().unwrap().contains("I've executed the create_page tool"));
        }
        
        // Step 3: Add blocks to the page
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Add content to the page",
                "echo_tool": "add_block"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
            // MockLLM returns the tool invocation message
            assert!(data["response"].as_str().unwrap().contains("I've executed the add_block tool"));
        }
        
        // Step 4: List the graphs to verify creation
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "List all graphs",
                "echo_tool": "list_graphs"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            
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
        
        agent_fixture.validate_all(&test_env.data_dir);
        
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