use serde_json::json;
use uuid::Uuid;
use crate::common::{setup_test_env, cleanup_test_env, WalValidator, MessagePattern};
use crate::common::test_harness::{
    PreShutdown, PostShutdown, assert_phase,
    connect_websocket, send_command, expect_success, 
    authenticate_websocket, setup_with_graph, read_auth_token
};

/// Test agent chat commands (chat, select, history, reset, list)
pub fn test_agent_chat_commands() {
    // Set up test environment
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone(); // For cleanup after panic
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Import dummy graph and start server
        let (server, _graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        // === Phase 1: Server Running (PreShutdown) ===
        assert_phase(PreShutdown);
        
        // Initialize the WAL validator
        let mut validator = WalValidator::new(&data_dir);
        
        // Connect WebSocket client
        let mut ws = connect_websocket(port);
        
        // Read auth token and authenticate
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token), "Authentication failed with valid token");
        
        // Get prime agent ID from AgentInfo command
        let prime_agent_id = {
            let cmd = json!({
                "type": "agent_info"
                // No agent specified, should default to prime
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_info response");
            assert!(data["is_prime"].as_bool().unwrap_or(false), "Default agent should be prime");
            let id_str = data["agent_id"].as_str().expect("No agent_id in response");
            Uuid::parse_str(id_str).expect("Invalid UUID")
        };
        
        // Set up prime agent expectations
        validator.expect_prime_agent(prime_agent_id);
        
        // Test AgentList - should show prime agent
        {
            let cmd = json!({ "type": "agent_list" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_list response");
            let agents = data["agents"].as_array().expect("agents should be array");
            
            assert_eq!(agents.len(), 1, "Should have exactly one agent (prime)");
            assert_eq!(agents[0]["name"].as_str(), Some("Prime Agent"));
            assert!(agents[0]["is_prime"].as_bool().unwrap_or(false));
            assert!(agents[0]["is_active"].as_bool().unwrap_or(false));
        }
        
        // Test AgentChat - send a message with echo
        {
            let expected_response = "Hello! I'm the Prime Agent, here to help with your knowledge graphs.";
            let cmd = json!({
                "type": "agent_chat",
                "message": "Hello, agent!",
                "echo": expected_response
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_chat response");
            
            let response_text = data["response"].as_str().expect("No response in data");
            assert_eq!(response_text, expected_response, "MockLLM should echo the provided response");
            
            // Record expected message in fixture
            validator.expect_user_message(
                &prime_agent_id, 
                MessagePattern::Exact("Hello, agent!".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact(expected_response.to_string())
            );
        }
        
        // Send another message to build conversation
        {
            let expected_response = "2+2 equals 4.";
            let cmd = json!({
                "type": "agent_chat",
                "message": "What is 2+2?",
                "echo": expected_response
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_chat response");
            
            let response_text = data["response"].as_str().expect("No response in data");
            assert_eq!(response_text, expected_response, 
                   "MockLLM should echo the provided response");
            
            validator.expect_user_message(
                &prime_agent_id, 
                MessagePattern::Exact("What is 2+2?".to_string())
            );
            validator.expect_assistant_message(
                &prime_agent_id,
                MessagePattern::Exact(expected_response.to_string())
            );
        }
        
        // Test AgentHistory - retrieve conversation
        {
            let cmd = json!({
                "type": "agent_history"
                // No agent specified, uses current (prime)
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_history response");
            
            let messages = data["messages"].as_array().expect("messages should be array");
            assert_eq!(messages.len(), 4, "Should have 4 messages (2 user, 2 assistant)");
            
            // Verify message ordering
            assert_eq!(messages[0]["role"].as_str(), Some("user"));
            assert_eq!(messages[0]["content"].as_str(), Some("Hello, agent!"));
            assert_eq!(messages[1]["role"].as_str(), Some("assistant"));
            assert_eq!(messages[1]["content"].as_str(), Some("Hello! I'm the Prime Agent, here to help with your knowledge graphs."));
            assert_eq!(messages[2]["role"].as_str(), Some("user"));
            assert_eq!(messages[2]["content"].as_str(), Some("What is 2+2?"));
            assert_eq!(messages[3]["role"].as_str(), Some("assistant"));
            assert_eq!(messages[3]["content"].as_str(), Some("2+2 equals 4."));
        }
        
        // Test AgentHistory with limit
        {
            let cmd = json!({
                "type": "agent_history",
                "limit": 2
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_history response");
            
            let messages = data["messages"].as_array().expect("messages should be array");
            assert_eq!(messages.len(), 2, "Should have 2 messages with limit");
            // Should get the last 2 messages
            assert_eq!(messages[0]["role"].as_str(), Some("user"));
            assert_eq!(messages[0]["content"].as_str(), Some("What is 2+2?"));
            assert_eq!(messages[1]["role"].as_str(), Some("assistant"));
        }
        
        // Test AgentReset - clear history
        {
            let cmd = json!({
                "type": "agent_reset"
                // No agent specified, uses current (prime)
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_reset response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            // Update fixture expectations
            validator.expect_chat_reset(&prime_agent_id);
        }
        
        // Verify history is cleared
        {
            let cmd = json!({
                "type": "agent_history"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_history response");
            
            let messages = data["messages"].as_array().expect("messages should be array");
            assert_eq!(messages.len(), 0, "History should be empty after reset");
        }
        
        // Close WebSocket connection
        let _ = ws.close(None);
        
        // === Phase 2: Shutdown Server ===
        let test_env = server.shutdown();
        
        // === Phase 3: Server Shutdown (PostShutdown) ===
        assert_phase(PostShutdown);
        
        // Validate agent state using the validator
        validator.validate_all().expect("Validation failed");
        
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

/// Test agent admin commands (create, delete, activate, deactivate, authorize)
pub fn test_agent_admin_commands() {
    // Set up test environment
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone(); // For cleanup after panic
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Import dummy graph and start server
        let (server, graph_id) = setup_with_graph(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        
        // === Phase 1: Server Running (PreShutdown) ===
        assert_phase(PreShutdown);
        
        // Initialize the WAL validator
        let mut validator = WalValidator::new(&data_dir);
        
        // Connect WebSocket client
        let mut ws = connect_websocket(port);
        
        // Read auth token and authenticate
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token), "Authentication failed with valid token");
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({ "type": "agent_info" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_info response");
            let id_str = data["agent_id"].as_str().expect("No agent_id in response");
            Uuid::parse_str(id_str).expect("Invalid UUID")
        };
        
        // Parse graph UUID
        let graph_uuid = Uuid::parse_str(&graph_id).expect("Invalid graph UUID");
        
        // Set up prime agent expectations
        validator.expect_prime_agent(prime_agent_id);
        validator.expect_authorization(&prime_agent_id, &graph_uuid);
        
        // Test CreateAgent - create a secondary agent
        let secondary_agent_id = {
            let cmd = json!({
                "type": "create_agent",
                "name": "Test Agent",
                "description": "An agent for testing"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in create_agent response");
            
            assert_eq!(data["name"].as_str(), Some("Test Agent"));
            assert_eq!(data["description"].as_str(), Some("An agent for testing"));
            
            let id_str = data["agent_id"].as_str().expect("No agent_id in response");
            let id = Uuid::parse_str(id_str).expect("Invalid UUID");
            
            validator.expect_agent_created(id, "Test Agent", false);
            id
        };
        
        // Verify both agents exist in list
        {
            let cmd = json!({ "type": "agent_list" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_list response");
            let agents = data["agents"].as_array().expect("agents should be array");
            
            assert_eq!(agents.len(), 2, "Should have two agents");
            
            // Find agents by name
            let prime = agents.iter().find(|a| a["name"] == "Prime Agent").expect("Prime agent not found");
            let test = agents.iter().find(|a| a["name"] == "Test Agent").expect("Test agent not found");
            
            assert!(prime["is_prime"].as_bool().unwrap_or(false));
            assert!(!test["is_prime"].as_bool().unwrap_or(false));
            assert!(test["is_active"].as_bool().unwrap_or(false), "New agent should start active");
        }
        
        // Test AgentSelect - switch to secondary agent
        {
            let cmd = json!({
                "type": "agent_select",
                "agent_name": "Test Agent"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_select response");
            
            assert_eq!(data["agent_name"].as_str(), Some("Test Agent"));
        }
        
        // Send a message to the secondary agent
        {
            let cmd = json!({
                "type": "agent_chat",
                "message": "Hello from test!",
                "echo": "Hello! I'm Test Agent, ready to assist."
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_chat response");
            
            // Verify it went to the correct agent
            assert_eq!(data["agent_id"].as_str(), Some(secondary_agent_id.to_string().as_str()));
            
            validator.expect_user_message(
                &secondary_agent_id,
                MessagePattern::Exact("Hello from test!".to_string())
            );
            validator.expect_assistant_message(
                &secondary_agent_id,
                MessagePattern::Exact("Hello! I'm Test Agent, ready to assist.".to_string())
            );
        }
        
        // Test AuthorizeAgent - authorize secondary agent for the graph
        {
            let cmd = json!({
                "type": "authorize_agent",
                "agent_id": secondary_agent_id.to_string(),
                "graph_id": graph_id
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in authorize_agent response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_authorization(&secondary_agent_id, &graph_uuid);
        }
        
        // Verify authorization with AgentInfo
        {
            let cmd = json!({
                "type": "agent_info",
                "agent_id": secondary_agent_id.to_string()
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_info response");
            
            let authorized_graphs = data["authorized_graphs"].as_array()
                .expect("authorized_graphs should be array");
            assert_eq!(authorized_graphs.len(), 1, "Should be authorized for one graph");
            assert_eq!(authorized_graphs[0]["id"].as_str(), Some(graph_id.as_str()));
        }
        
        // Test DeauthorizeAgent
        {
            let cmd = json!({
                "type": "deauthorize_agent",
                "agent_id": secondary_agent_id.to_string(),
                "graph_id": graph_id
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in deauthorize_agent response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_deauthorization(&secondary_agent_id, &graph_uuid);
        }
        
        // Test DeactivateAgent
        {
            let cmd = json!({
                "type": "deactivate_agent",
                "agent_name": "Test Agent"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in deactivate_agent response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_agent_deactivated(&secondary_agent_id);
        }
        
        // Verify agent is deactivated in list
        {
            let cmd = json!({ "type": "agent_list" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_list response");
            let agents = data["agents"].as_array().expect("agents should be array");
            
            let test = agents.iter().find(|a| a["name"] == "Test Agent").expect("Test agent not found");
            assert!(!test["is_active"].as_bool().unwrap_or(true), "Agent should be deactivated");
        }
        
        // Test ActivateAgent
        {
            let cmd = json!({
                "type": "activate_agent",
                "agent_name": "Test Agent"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in activate_agent response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_agent_activated(&secondary_agent_id);
        }
        
        // Test that we CAN deactivate the prime agent (no protection)
        {
            let cmd = json!({
                "type": "deactivate_agent",
                "agent_id": prime_agent_id.to_string()
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("Should be able to deactivate prime agent");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_agent_deactivated(&prime_agent_id);
        }
        
        // Reactivate prime agent for deletion test
        {
            let cmd = json!({
                "type": "activate_agent",
                "agent_id": prime_agent_id.to_string()
            });
            let response = send_command(&mut ws, cmd);
            expect_success(response);
            
            validator.expect_agent_activated(&prime_agent_id);
        }
        
        // Test DeleteAgent - should work for secondary agent
        {
            let cmd = json!({
                "type": "delete_agent",
                "agent_name": "Test Agent"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in delete_agent response");
            
            assert!(data["success"].as_bool().unwrap_or(false));
            
            validator.expect_agent_deleted(&secondary_agent_id);
        }
        
        // Test DeleteAgent - should FAIL for prime agent (protection)
        {
            let cmd = json!({
                "type": "delete_agent",
                "agent_id": prime_agent_id.to_string()
            });
            let response = send_command(&mut ws, cmd);
            
            // This should be an error response
            assert_eq!(response["type"], "error", "Should not be able to delete prime agent");
            assert!(response["message"].as_str().unwrap().contains("Cannot delete the prime agent"),
                   "Error message should mention prime agent protection");
        }
        
        // Verify only prime agent remains
        {
            let cmd = json!({ "type": "agent_list" });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_list response");
            let agents = data["agents"].as_array().expect("agents should be array");
            
            assert_eq!(agents.len(), 1, "Should only have prime agent left");
            assert_eq!(agents[0]["name"].as_str(), Some("Prime Agent"));
        }
        
        // Test AgentInfo with detailed stats
        {
            let cmd = json!({
                "type": "agent_info",
                "agent_id": prime_agent_id.to_string()
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_info response");
            
            // Verify all expected fields
            assert_eq!(data["agent_id"].as_str(), Some(prime_agent_id.to_string().as_str()));
            assert_eq!(data["name"].as_str(), Some("Prime Agent"));
            assert!(data["is_prime"].as_bool().unwrap_or(false));
            assert!(data["is_active"].as_bool().unwrap_or(false));
            assert!(data["created"].is_string());
            assert!(data["last_active"].is_string());
            assert!(data["authorized_graphs"].is_array());
            assert!(data["conversation_stats"].is_object());
            
            // Check conversation stats
            let stats = &data["conversation_stats"];
            assert_eq!(stats["message_count"].as_u64(), Some(0), "Prime agent history was reset");
        }
        
        // Close WebSocket connection
        let _ = ws.close(None);
        
        // === Phase 2: Shutdown Server ===
        let test_env = server.shutdown();
        
        // === Phase 3: Server Shutdown (PostShutdown) ===
        assert_phase(PostShutdown);
        
        // Validate agent state using the validator
        validator.validate_all().expect("Validation failed");
        
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