// tests/integration/main.rs
//
// Integration test suite entry point
// All integration tests are compiled as a single binary to avoid dead code warnings

// Import common utilities once for all integration tests
#[path = "../common/mod.rs"]
mod common;

// Import all integration test modules
mod agent_tools;
mod cli_commands;
mod http_logseq_import;
mod mcp_server;
mod websocket_commands;

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
fn test_agent_tool_validation_errors() {
    agent_tools::test_agent_tool_validation_errors();
}

#[test]
fn test_agent_tool_chaining() {
    agent_tools::test_agent_tool_chaining();
}

// MCP Server tests
#[test]
fn test_mcp_all_tools() {
    mcp_server::test_mcp_all_tools();
}

#[test]
fn test_mcp_initialization() {
    mcp_server::test_mcp_initialization();
}

#[test]
fn test_mcp_protocol_compliance() {
    mcp_server::test_mcp_protocol_compliance();
}

#[test]
fn test_mcp_malformed_requests() {
    mcp_server::test_mcp_malformed_requests();
}

#[test]
fn test_mcp_invalid_tool_arguments() {
    mcp_server::test_mcp_invalid_tool_arguments();
}

#[test]
fn test_mcp_notifications() {
    mcp_server::test_mcp_notifications();
}
