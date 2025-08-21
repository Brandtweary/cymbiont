use serde_json::json;
use uuid::Uuid;
use crate::common::{setup_test_env, cleanup_test_env, GraphValidationFixture, AgentValidationFixture};
use crate::common::test_harness::{
    PreShutdown, PostShutdown, assert_phase,
    connect_websocket, send_command, expect_success,
    authenticate_websocket, TestServer, read_auth_token, WsConnection
};

// Include the generated CLI commands list from build script
include!(concat!(env!("OUT_DIR"), "/cli_commands.rs"));

/// List of CLI commands that have been integration tested
/// When adding a new CLI command, add the command name to this list after writing tests
const TESTED_COMMANDS: &[&str] = &[
    "import_logseq",      // ✓ tested in test_all_cli_commands
    "delete_graph",       // ✓ tested in test_all_cli_commands
    "create_agent",       // ✓ tested in test_all_cli_commands
    "delete_agent",       // ✓ tested in test_all_cli_commands
    "activate_agent",     // ✓ tested in test_all_cli_commands
    "deactivate_agent",   // ✓ tested in test_all_cli_commands
    "agent_info",         // ✓ tested in test_all_cli_commands
    "authorize_agent",    // ✓ tested in test_all_cli_commands
    "deauthorize_agent",  // ✓ tested in test_all_cli_commands
    "list_graphs",        // ✓ tested in test_all_cli_commands
];

/// Contract enforcement: Verify we test all CLI commands from macro
#[allow(dead_code)]
fn verify_all_commands_tested() {
    let mut missing = Vec::new();
    for &cmd in ALL_CLI_COMMANDS {
        if !TESTED_COMMANDS.contains(&cmd) {
            missing.push(cmd);
        }
    }
    if !missing.is_empty() {
        panic!("Missing tests for CLI commands: {:?}. Add them to TESTED_COMMANDS after writing tests.", missing);
    }
    
    let mut extra = Vec::new();
    for &cmd in TESTED_COMMANDS {
        if !ALL_CLI_COMMANDS.contains(&cmd) {
            extra.push(cmd);
        }
    }
    if !extra.is_empty() {
        panic!("TESTED_COMMANDS contains commands not in macro: {:?}. Remove them or add to macro.", extra);
    }
}

/// Helper to send a CLI command via WebSocket
fn send_cli_command(ws: &mut WsConnection, command: &str, params: serde_json::Value) -> serde_json::Value {
    let cmd = json!({
        "type": "test_cli_command",
        "command": command,
        "params": params
    });
    send_command(ws, cmd)
}


/// Test all CLI commands
pub fn test_all_cli_commands() {
    // Contract enforcement: Verify we test all commands from the macro
    verify_all_commands_tested();
    
    let test_env = setup_test_env();
    let cleanup_env = test_env.clone();
    
    let result = std::panic::catch_unwind(move || {
        let server = TestServer::start(test_env);
        let port = server.port();
        let data_dir = server.test_env().data_dir.clone();
        
        assert_phase(PreShutdown);
        
        let mut graph_fixture = GraphValidationFixture::new();
        let mut agent_fixture = AgentValidationFixture::new();
        
        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));
        
        // Test import_logseq
        {
            let response = send_cli_command(&mut ws, "import_logseq", json!({
                "path": "logseq_databases/dummy_graph/"
            }));
            let data = expect_success(response).expect("Import should succeed");
            assert!(!data["exit_after"].as_bool().unwrap_or(true), "Import should not exit");
            
            // The fixture validates that the graph was created with expected structure
            graph_fixture.expect_dummy_graph();
        }
        
        // Get graph ID using the list_graphs WebSocket command
        let graph_id = {
            let cmd = json!({"type": "list_graphs"});
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("list_graphs should succeed");
            let graphs = data["graphs"].as_array().expect("Should have graphs array");
            
            // Should have exactly one graph after import
            assert_eq!(graphs.len(), 1, "Should have one graph after import");
            
            // Get the first (and only) graph ID
            graphs[0]["id"].as_str().unwrap().to_string()
        };
        
        // Get prime agent ID
        let prime_agent_id = {
            let cmd = json!({"type": "agent_info"});
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            Uuid::parse_str(data["agent_id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_prime_agent(prime_agent_id);
        agent_fixture.expect_authorization(&prime_agent_id, &Uuid::parse_str(&graph_id).unwrap());
        
        // Test create_agent
        let test_agent_name = "CLI Test Agent";
        {
            let response = send_cli_command(&mut ws, "create_agent", json!({
                "name": test_agent_name,
                "description": "Test agent created via CLI"
            }));
            let data = expect_success(response).expect("Create agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Create agent should exit");
        }
        
        // Get test agent ID
        let test_agent_id = {
            let cmd = json!({"type": "agent_list"});
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            let agents = data["agents"].as_array().unwrap();
            let test_agent = agents.iter()
                .find(|a| a["name"].as_str() == Some(test_agent_name))
                .expect("Test agent should exist");
            Uuid::parse_str(test_agent["id"].as_str().unwrap()).unwrap()
        };
        
        agent_fixture.expect_agent_created(test_agent_id, test_agent_name, false);
        
        // Test agent_info
        {
            let response = send_cli_command(&mut ws, "agent_info", json!({
                "identifier": test_agent_name
            }));
            let data = expect_success(response).expect("Agent info should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Agent info should exit");
        }
        
        // Test authorize_agent
        {
            let response = send_cli_command(&mut ws, "authorize_agent", json!({
                "agent": test_agent_name,
                "for_graph": &graph_id
            }));
            let data = expect_success(response).expect("Authorize agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Authorize should exit");
            agent_fixture.expect_authorization(&test_agent_id, &Uuid::parse_str(&graph_id).unwrap());
        }
        
        // Test deauthorize_agent
        {
            let response = send_cli_command(&mut ws, "deauthorize_agent", json!({
                "agent": test_agent_name,
                "from_graph": &graph_id
            }));
            let data = expect_success(response).expect("Deauthorize agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Deauthorize should exit");
            agent_fixture.expect_deauthorization(&test_agent_id, &Uuid::parse_str(&graph_id).unwrap());
        }
        
        // Test deactivate_agent
        {
            let response = send_cli_command(&mut ws, "deactivate_agent", json!({
                "identifier": test_agent_name
            }));
            let data = expect_success(response).expect("Deactivate agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Deactivate should exit");
            agent_fixture.expect_agent_deactivated(&test_agent_id);
        }
        
        // Test activate_agent
        {
            let response = send_cli_command(&mut ws, "activate_agent", json!({
                "identifier": test_agent_name
            }));
            let data = expect_success(response).expect("Activate agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Activate should exit");
            agent_fixture.expect_agent_activated(&test_agent_id);
        }
        
        // Test delete_agent
        {
            let response = send_cli_command(&mut ws, "delete_agent", json!({
                "identifier": test_agent_name
            }));
            let data = expect_success(response).expect("Delete agent should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "Delete agent should exit");
            agent_fixture.expect_agent_deleted(&test_agent_id);
        }
        
        // Test prime agent deletion protection
        {
            // Try to delete prime agent - should fail
            let response = send_cli_command(&mut ws, "delete_agent", json!({
                "identifier": "Prime Agent"
            }));
            
            // This should be an error response
            assert_eq!(
                response["type"], "error",
                "Should not be able to delete prime agent"
            );
            assert!(
                response["message"].as_str().unwrap_or("").contains("Cannot delete the prime agent"),
                "Error message should mention prime agent protection"
            );
            
            // Verify prime agent still exists
            let cmd = json!({"type": "agent_list"});
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            let agents = data["agents"].as_array().unwrap();
            assert!(
                agents.iter().any(|a| a["name"].as_str() == Some("Prime Agent")),
                "Prime agent should still exist after failed deletion"
            );
        }
        
        // Test delete_graph with a secondary graph (keep the imported one)
        {
            // First create a secondary graph to delete
            let cmd = json!({
                "type": "create_graph",
                "name": "test-graph-to-delete",
                "description": "This graph will be deleted"
            });
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).expect("Create graph should succeed");
            let delete_graph_id = data["id"].as_str().unwrap().to_string();
            
            // Now delete it via CLI
            let response = send_cli_command(&mut ws, "delete_graph", json!({
                "identifier": &delete_graph_id
            }));
            let data = expect_success(response).expect("Delete graph should succeed");
            assert!(!data["exit_after"].as_bool().unwrap_or(true), "Delete graph should not exit");
            
            // Verify it's gone
            let cmd = json!({"type": "list_graphs"});
            let response = send_command(&mut ws, cmd);
            let data = expect_success(response).unwrap();
            let graphs = data["graphs"].as_array().unwrap();
            assert_eq!(graphs.len(), 1, "Should have only the original graph after deletion");
            assert_eq!(graphs[0]["id"].as_str().unwrap(), &graph_id, "Original graph should remain");
        }
        
        // Test list_graphs
        {
            let response = send_cli_command(&mut ws, "list_graphs", json!({}));
            let data = expect_success(response).expect("List graphs should succeed");
            assert!(data["exit_after"].as_bool().unwrap_or(false), "List graphs should exit");
            
            // The actual listing is done via the CLI command, not the WebSocket command
            // So we just verify it executed successfully
        }
        
        
        let _ = ws.close(None);
        let test_env = server.shutdown();
        assert_phase(PostShutdown);
        
        // Now we can validate both the graph and agents
        graph_fixture.validate_graph(&test_env.data_dir, &graph_id);
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