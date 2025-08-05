# CYMBIONT TEST GUIDE

## Test Structure
- **common/**: Shared test utilities - imported via `#[path = "../common/mod.rs"]`
  - **mod.rs**: Test environment setup (`setup_test_env()`, `cleanup_test_env()`)
  - **test_harness.rs**: `TestServer` lifecycle management
  - **graph_validation.rs**: Automated graph state validation for integration tests
- **integration/**: Integration test suite (single binary for parallelism)
  - **main.rs**: Entry point - imports common utilities and test modules
  - **http_logseq_import.rs**: HTTP API import tests
  - **logseq_import.rs**: CLI import tests
  - **websocket_commands.rs**: WebSocket API tests
  - **freeze_mechanism.rs**: Freeze/unfreeze operation tests for deterministic testing
  - **crash_recovery.rs**: Transaction recovery tests for startup and graph switching

## Test Utilities

### common/mod.rs
- Re-exports test utilities from test_harness for convenience

### common/test_harness.rs
- `setup_test_env() -> TestEnv`: Creates unique test environment (port 8888+, test_data_N/, config.test.N.yaml)
- `cleanup_test_env(TestEnv)`: Removes test data directory and config
- `get_cymbiont_binary() -> PathBuf`: Returns path to compiled binary
- `TestServer::start(TestEnv) -> Self`: Start server mode, waits for HTTP ready
- `TestServer::start_with_args(TestEnv, Vec<&str>) -> Self`: Start with custom CLI args
- `TestServer::shutdown(self) -> TestEnv`: Send SIGINT and wait for cleanup
- `TestServer::wait_for_completion(self) -> TestEnv`: Wait for natural process exit
- `TestServer::port() -> u16`: Get server port (0 for CLI mode)
- `TestServer::force_kill()`: Force kill process with SIGKILL for crash tests
- `TestServer::pid() -> u32`: Get process ID
- `assert_phase(PreShutdown/PostShutdown)`: Document test phase for clarity
- `connect_websocket(port) -> WsConnection`: Connect to WebSocket endpoint with retries
- `send_command(ws, cmd) -> Value`: Send command and return response, skipping heartbeats
- `expect_success(response) -> Option<Value>`: Assert success response and return data
- `authenticate_websocket(ws, token) -> bool`: Authenticate WebSocket connection
- `import_dummy_graph(env) -> String`: Import dummy graph via CLI and return graph ID
- `read_auth_token(data_dir) -> String`: Read and trim auth token from data directory
- `get_active_graph_id(data_dir) -> String`: Get active graph ID from registry
- `setup_with_graph(env) -> (TestServer, String)`: Import graph and start server, returning both
- `freeze_operations(ws) -> bool`: Pause graph operations after transaction creation
- `unfreeze_operations(ws) -> bool`: Resume paused graph operations
- `get_freeze_state(ws) -> bool`: Check if operations are currently frozen
- `send_command_async(ws, cmd)`: Send command without waiting for response
- `read_pending_response(ws) -> Value`: Read response with timeout handling

### common/graph_validation.rs
- `GraphValidationFixture::new()`: Create fixture to track expected graph transformations
- `expect_dummy_graph()`: Set up expectations for imported dummy_graph test data
- `expect_create_block(id, content, page_name)`: Track block creation expectations
- `expect_update_block(id, new_content)`: Track block content updates (preserves edges)
- `expect_delete(id)`: Track node deletion expectations
- `expect_edge(source_id, target_id, edge_type)`: Track custom edge expectations (ParentChild, PageToBlock, etc.)
- `validate_graph(data_dir, graph_id)`: Validate all expectations against persisted graph state

## Key Concepts
- Each test gets unique port (8888+), data directory, and config file via atomic counter
- Tests run in parallel within the integration binary - ensure no shared state
- Always use panic handler + cleanup pattern for proper teardown
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
Set `KEEP_TEST_DATA=1` to preserve test data directories after tests complete. The test output will show the preserved directory path (e.g., `test_data_0/`) containing graph files and transaction logs.