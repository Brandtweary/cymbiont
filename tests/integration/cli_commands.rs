use crate::common::test_harness::{
    assert_phase, authenticate_websocket, connect_websocket, expect_success, read_auth_token,
    send_command, PostShutdown, PreShutdown, TestServer, WsConnection,
};
use crate::common::{cleanup_test_env, setup_test_env, TestValidator};
use serde_json::json;
use uuid::Uuid;

// Include the generated CLI commands list from build script
include!(concat!(env!("OUT_DIR"), "/cli_commands.rs"));

/// List of CLI commands that have been integration tested
/// When adding a new CLI command, add the command name to this list after writing tests
const TESTED_COMMANDS: &[&str] = &[
    "import_logseq", // ✓ tested in test_all_cli_commands
    "delete_graph",  // ✓ tested in test_all_cli_commands
    "list_graphs",   // ✓ tested in test_all_cli_commands
    "create_graph",  // ✓ tested in test_all_cli_commands
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
    assert!(missing.is_empty(), "Missing tests for CLI commands: {missing:?}. Add them to TESTED_COMMANDS after writing tests.");

    let mut extra = Vec::new();
    for &cmd in TESTED_COMMANDS {
        if !ALL_CLI_COMMANDS.contains(&cmd) {
            extra.push(cmd);
        }
    }
    assert!(
        extra.is_empty(),
        "TESTED_COMMANDS contains commands not in macro: {extra:?}. Remove them or add to macro."
    );
}

/// Helper to send a CLI command via WebSocket
fn send_cli_command(
    ws: &mut WsConnection,
    command: &str,
    params: &serde_json::Value,
) -> serde_json::Value {
    let cmd = json!({
        "type": "test_cli_command",
        "command": command,
        "params": params
    });
    send_command(ws, &cmd)
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

        // Initialize the validator
        let mut validator = TestValidator::new(&data_dir);

        let mut ws = connect_websocket(port);
        let auth_token = read_auth_token(&data_dir);
        assert!(authenticate_websocket(&mut ws, &auth_token));

        // Test import_logseq
        let (graph_id, graph_name) = {
            let response = send_cli_command(
                &mut ws,
                "import_logseq",
                &json!({
                    "path": "logseq_databases/dummy_graph/"
                }),
            );
            expect_success(&response).expect("Import should succeed");

            // Get graph info using list_graphs to get ID and name
            let cmd = json!({"type": "list_graphs"});
            let response = send_command(&mut ws, &cmd);
            let data = expect_success(&response).expect("list_graphs should succeed");
            let graphs = data["graphs"].as_array().expect("Should have graphs array");

            // Should have exactly one graph after import
            assert_eq!(graphs.len(), 1, "Should have one graph after import");

            // Get the graph ID and name
            let graph = &graphs[0];
            let id = graph["id"].as_str().unwrap().to_string();
            let name = graph["name"].as_str().unwrap().to_string();
            (id, name)
        };

        // Set up expectations for the imported graph
        validator.expect_dummy_graph(Some(&graph_id));

        // Add registry expectations
        let imported_uuid = Uuid::parse_str(&graph_id).expect("Invalid UUID");
        validator.expect_graph_created(imported_uuid, &graph_name);
        validator.expect_graph_open(imported_uuid);

        // Test delete_graph with a secondary graph (keep the imported one)
        {
            // First create a secondary graph to delete (via WebSocket)
            let cmd = json!({
                "type": "create_graph",
                "name": "test-graph-to-delete",
                "description": "This graph will be deleted"
            });
            let response = send_command(&mut ws, &cmd);
            let data = expect_success(&response).expect("Create graph should succeed");
            let delete_graph_id = data["id"].as_str().unwrap().to_string();

            // Parse UUID and add expectations for creation
            let delete_graph_uuid = Uuid::parse_str(&delete_graph_id).expect("Invalid UUID");
            validator.expect_graph_created(delete_graph_uuid, "test-graph-to-delete");
            validator.expect_graph_open(delete_graph_uuid);

            // Now delete it via CLI
            let response = send_cli_command(
                &mut ws,
                "delete_graph",
                &json!({
                    "identifier": &delete_graph_id
                }),
            );
            expect_success(&response).expect("Delete graph should succeed");

            // Add expectation for deletion
            validator.expect_graph_deleted(delete_graph_uuid);
        }

        // Test create_graph CLI command
        {
            let response = send_cli_command(
                &mut ws,
                "create_graph",
                &json!({
                    "name": "test-created-graph",
                    "description": "Created via CLI command"
                }),
            );
            expect_success(&response).expect("Create graph should succeed");

            // Get the created graph ID from listing
            let cmd = json!({"type": "list_graphs"});
            let response = send_command(&mut ws, &cmd);
            let data = expect_success(&response).unwrap();
            let graphs = data["graphs"].as_array().unwrap();

            // Find the newly created graph
            let created_graph = graphs
                .iter()
                .find(|g| g["name"].as_str() == Some("test-created-graph"))
                .expect("Should find the created graph");
            let created_graph_id = created_graph["id"].as_str().unwrap();

            // Parse UUID and add expectations
            let created_graph_uuid = Uuid::parse_str(created_graph_id).expect("Invalid UUID");
            validator.expect_graph_created(created_graph_uuid, "test-created-graph");
            validator.expect_graph_open(created_graph_uuid);
        }

        // Test list_graphs
        {
            let response = send_cli_command(&mut ws, "list_graphs", &json!({}));
            expect_success(&response).expect("List graphs should succeed");

            // The actual listing is done via the CLI command, not the WebSocket command
            // So we just verify it executed successfully
        }

        let _ = ws.close(None);
        let test_env = server.shutdown();
        assert_phase(PostShutdown);

        // Now we can validate both the graph and agents
        // Validate both graph and agent state
        validator.validate_all().expect("Validation failed");

        test_env
    });

    match result {
        Ok(test_env) => cleanup_test_env(&test_env),
        Err(panic) => {
            cleanup_test_env(&cleanup_env);
            std::panic::resume_unwind(panic);
        }
    }
}
