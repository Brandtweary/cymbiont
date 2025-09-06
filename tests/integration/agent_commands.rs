use crate::common::test_harness::{
    agent_chat_sync, assert_phase, authenticate_websocket, connect_websocket, expect_success,
    read_auth_token, send_command, setup_with_graph, PostShutdown, PreShutdown,
};
use crate::common::{cleanup_test_env, setup_test_env, MessagePattern, TestValidator};
use serde_json::json;

/// Test agent chat commands (chat, history, reset, info)
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

        // Initialize the validator
        let mut validator = TestValidator::new(&data_dir);

        // Connect WebSocket client
        let mut ws = connect_websocket(port);

        // Read auth token and authenticate
        let auth_token = read_auth_token(&data_dir);
        assert!(
            authenticate_websocket(&mut ws, &auth_token),
            "Authentication failed with valid token"
        );

        // Test AgentInfo command
        {
            let cmd = json!({
                "type": "agent_info"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_info response");
            assert!(
                data["is_active"].as_bool().unwrap_or(false),
                "Agent should be active"
            );
            assert!(
                data["conversation_stats"].is_object(),
                "Should have conversation stats"
            );
        }

        // Test AgentChat - send a message with echo
        {
            let expected_response = "Hello! I'm here to help with your knowledge graphs.";
            let data = agent_chat_sync(&mut ws, "Hello, agent!", Some(expected_response), None);

            let response_text = data["response"].as_str().expect("No response in data");
            assert_eq!(
                response_text, expected_response,
                "MockLLM should echo the provided response"
            );

            // Record expected message in fixture
            validator.expect_user_message(MessagePattern::Exact("Hello, agent!".to_string()));
            validator
                .expect_assistant_message(MessagePattern::Exact(expected_response.to_string()));
        }

        // Send another message to build conversation
        {
            let expected_response = "2+2 equals 4.";
            let data = agent_chat_sync(&mut ws, "What is 2+2?", Some(expected_response), None);

            let response_text = data["response"].as_str().expect("No response in data");
            assert_eq!(
                response_text, expected_response,
                "MockLLM should echo the provided response"
            );

            validator.expect_user_message(MessagePattern::Exact("What is 2+2?".to_string()));
            validator
                .expect_assistant_message(MessagePattern::Exact(expected_response.to_string()));
        }

        // Test AgentHistory - retrieve conversation
        {
            let cmd = json!({
                "type": "agent_history"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_history response");

            let messages = data["messages"]
                .as_array()
                .expect("messages should be array");
            assert_eq!(
                messages.len(),
                4,
                "Should have 4 messages (2 user, 2 assistant)"
            );

            // Also validate using the validator
            validator.expect_message_count(4, Some(4));

            // Verify message ordering
            assert_eq!(messages[0]["role"].as_str(), Some("user"));
            assert_eq!(messages[0]["content"].as_str(), Some("Hello, agent!"));
            assert_eq!(messages[1]["role"].as_str(), Some("assistant"));
            assert_eq!(
                messages[1]["content"].as_str(),
                Some("Hello! I'm here to help with your knowledge graphs.")
            );
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

            let messages = data["messages"]
                .as_array()
                .expect("messages should be array");
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
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_reset response");

            assert!(data["success"].as_bool().unwrap_or(false));

            // Update fixture expectations
            validator.expect_chat_reset();
        }

        // Verify history is cleared
        {
            let cmd = json!({
                "type": "agent_history"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("No data in agent_history response");

            let messages = data["messages"]
                .as_array()
                .expect("messages should be array");
            assert_eq!(messages.len(), 0, "History should be empty after reset");

            // Validate the reset resulted in empty history
            validator.expect_message_count(0, Some(0));
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
