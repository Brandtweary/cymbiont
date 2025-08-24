// tests/integration/main.rs
//
// Integration test suite entry point
// All integration tests are compiled as a single binary to avoid dead code warnings

// Import common utilities once for all integration tests
#[path = "../common/mod.rs"]
mod common;

// Import all integration test modules
mod http_logseq_import;
mod websocket_commands;
mod freeze_mechanism;
mod crash_recovery;
mod agent_commands;
mod agent_tools;
mod cli_commands;

// Re-export test functions with #[test] attribute
#[test]
fn test_http_logseq_import() {
    http_logseq_import::test_http_logseq_import();
}

#[test]
fn test_http_import_error_cases() {
    http_logseq_import::test_http_import_error_cases();
}

#[test]
fn test_websocket_commands() {
    websocket_commands::test_websocket_commands();
}

#[test]
fn test_freeze_mechanism() {
    freeze_mechanism::test_freeze_mechanism();
}

#[test]
fn test_freeze_persistence() {
    freeze_mechanism::test_freeze_persistence();
}

#[test]
fn test_freeze_timeout() {
    freeze_mechanism::test_freeze_timeout();
}

#[test]
fn test_startup_recovery() {
    crash_recovery::test_startup_recovery();
}

#[test]
fn test_mixed_open_closed_graphs() {
    crash_recovery::test_mixed_open_closed_graphs();
}

#[test]
fn test_open_graph_recovery() {
    crash_recovery::test_open_graph_recovery();
}

#[test]
fn test_graceful_shutdown_completes_transactions() {
    crash_recovery::test_graceful_shutdown_completes_transactions();
}

#[test]
fn test_agent_chat_commands() {
    agent_commands::test_agent_chat_commands();
}

#[test]
fn test_agent_admin_commands() {
    agent_commands::test_agent_admin_commands();
}

#[test]
fn test_all_cli_commands() {
    cli_commands::test_all_cli_commands();
}

#[test]
fn test_agent_graph_management_tools() {
    agent_tools::test_agent_graph_management_tools();
}

#[test]
fn test_agent_block_operations() {
    agent_tools::test_agent_block_operations();
}

#[test]
fn test_agent_page_operations() {
    agent_tools::test_agent_page_operations();
}

#[test]
fn test_agent_query_operations() {
    agent_tools::test_agent_query_operations();
}

#[test]
fn test_agent_authorization_failures() {
    agent_tools::test_agent_authorization_failures();
}

#[test]
fn test_agent_tool_validation_errors() {
    agent_tools::test_agent_tool_validation_errors();
}

#[test]
fn test_agent_tool_chaining() {
    agent_tools::test_agent_tool_chaining();
}

#[test]
fn test_agent_graph_management_tools_direct() {
    agent_tools::test_agent_graph_management_tools_direct();
}