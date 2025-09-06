# CYMBIONT TEST GUIDE

## Test Structure
- **common/**: Shared test utilities - imported via `#[path = "../common/mod.rs"]`
  - **mod.rs**: Test environment setup and re-exports
  - **test_harness.rs**: `TestServer` lifecycle management and WebSocket utilities
  - **test_validator.rs**: JSON-based state validation for single-agent architecture
- **integration/**: Integration test suite (single binary for parallelism)
  - **main.rs**: Entry point - imports common utilities and test modules
  - **http_logseq_import.rs**: HTTP API import tests
  - **cli_commands.rs**: CLI command tests
  - **websocket_commands.rs**: WebSocket API tests
  - **agent_commands.rs**: Agent chat command tests
  - **agent_tools.rs**: Agent tool execution tests for knowledge graph tools
- **deployment_manual_doctests.rs**: Validates commands in CYMBIONT_EMERGENCY_DEPLOYMENT.md

## Test Utilities

### common/test_harness.rs
- `setup_test_env() -> TestEnv`: Creates unique test environment (port 8888+, test_data_N/, config.test.N.yaml)
- `cleanup_test_env(TestEnv)`: Removes test data directory and config
- `get_cymbiont_binary() -> PathBuf`: Returns path to compiled binary
- `TestServer::start(TestEnv) -> Self`: Start server mode, waits for HTTP ready
- `TestServer::start_with_args(TestEnv, Vec<&str>) -> Self`: Start with custom CLI args
- `TestServer::shutdown(self) -> TestEnv`: Send SIGINT and wait for cleanup
- `TestServer::port() -> u16`: Get server port (0 for CLI mode)
- `TestServer::test_env() -> &TestEnv`: Get test environment reference
- `connect_websocket(port) -> WsConnection`: Connect to WebSocket endpoint with retries
- `send_command(ws, cmd) -> Value`: Send command and return response, skipping heartbeats
- `send_command_async(ws, cmd)`: Send command without waiting for response
- `read_pending_response(ws) -> Value`: Read response with timeout handling
- `expect_success(response) -> Option<Value>`: Assert success response and return data
- `expect_error(response) -> Option<Value>`: Assert error response and return message
- `authenticate_websocket(ws, token) -> bool`: Authenticate WebSocket connection
- `agent_chat(ws, message, echo, echo_tool) -> Value`: Send AgentChat command
- `agent_chat_sync(ws, message, echo, echo_tool) -> Value`: Agent chat with response wait
- `send_cli_command(ws, command, args) -> Value`: Execute CLI command via WebSocket
- `import_dummy_graph(env) -> String`: Import dummy graph via CLI and return graph ID
- `import_dummy_graph_http(port, data_dir) -> Result<String>`: Import via HTTP API (async)
- `setup_with_graph(env) -> (TestServer, String)`: Import graph and start server, returning both
- `read_auth_token(data_dir) -> String`: Read and trim auth token from data directory
- `get_single_open_graph_id(data_dir) -> String`: Get ID when exactly one graph is open
- `make_http_request(port, method, path, body, auth_token) -> Result<Value>`: Make HTTP request
- `make_import_request(port, path, name, auth_token) -> Result<Value>`: Import Logseq graph via HTTP
- `assert_phase(PreShutdown/PostShutdown)`: Document test phase for clarity
- `block_on<F: Future>(future: F) -> F::Output`: Block on async future using tokio runtime

### common/test_validator.rs
- `TestValidator::new(data_dir: &Path) -> Self`: Create new validator for test data directory
- `expect_create_page(name, properties, graph_id: Option<&str>) -> &mut Self`: Expect page creation
- `expect_create_block(block_id, content, page_name, graph_id: Option<&str>) -> &mut Self`: Expect block creation
- `expect_update_block(block_id, new_content, graph_id: Option<&str>) -> &mut Self`: Expect block content update
- `expect_delete_block(block_id, graph_id: Option<&str>) -> &mut Self`: Expect block deletion
- `expect_delete_page(page_name, graph_id: Option<&str>) -> &mut Self`: Expect page deletion
- `expect_dummy_graph(graph_id: Option<&str>) -> &mut Self`: Add expectations for standard dummy graph import
- `expect_user_message(content: MessagePattern) -> &mut Self`: Expect user message
- `expect_assistant_message(content: MessagePattern) -> &mut Self`: Expect assistant message
- `expect_tool_message(tool_name: &str, result: MessagePattern) -> &mut Self`: Expect tool result
- `expect_chat_reset() -> &mut Self`: Expect chat history clear
- `expect_message_count(min: usize, max: Option<usize>) -> &mut Self`: Expect message count range
- `expect_graph_created(id: Uuid, name: &str) -> &mut Self`: Expect graph creation in registry
- `expect_graph_open(id: Uuid) -> &mut Self`: Expect graph to be opened
- `expect_graph_closed(id: Uuid) -> &mut Self`: Expect graph to be closed
- `expect_graph_deleted(id: Uuid) -> &mut Self`: Expect graph removal from registry
- `validate_all() -> Result<(), String>`: Validate all expectations against JSON files (agent, registry, graphs)

## MockLLM Testing
MockLLM is the test LLM backend with two control mechanisms:
- `echo`: Pass text in AgentChat commands to control assistant responses
- `echo_tool`: Pass tool name and args to trigger deterministic tool execution
Without echo/echo_tool, MockLLM returns a default response. These fields flow through Message::User to MockLLM::complete().

## Key Concepts
- Each test gets unique port (8888+), data directory, and config file via atomic counter
- Tests run in parallel within the integration binary - ensure no shared state
- Always use panic handler + cleanup pattern for proper teardown
- Use tracing macros (debug!, trace!) for logging in tests, never println!/eprintln!
- See `test_harness.rs` header docs for full usage example

## Running Tests
```bash
cargo test                          # Run all tests
cargo test --test integration       # Run only integration tests
cargo test --test integration test_http_logseq_import  # Run specific test
RUST_LOG=debug cargo test -- --nocapture  # Debug with full output  
RUST_LOG=cymbiont::storage=trace cargo test -- --nocapture  # Trace specific module
KEEP_TEST_DATA=1 cargo test         # Preserve test data directories for manual inspection
```

## Debugging Test Data
Set `KEEP_TEST_DATA=1` to preserve test data directories after tests complete. The test output will show the preserved directory path (e.g., `test_data_0/`) containing:
- `agent.json`: Single agent state and conversation history
- `graph_registry.json`: Graph metadata and open/closed state
- `graphs/{graph-id}/knowledge_graph.json`: Individual graph data
- `auth_token`: Authentication token for the test session
- `config.test.N.yaml`: Test-specific configurationclaud