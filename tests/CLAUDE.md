# CYMBIONT TEST GUIDE

⚠️ **MAJOR REFACTOR IN PROGRESS** ⚠️
The test harness is currently outdated due to the CQRS refactor.
- WAL validation needs to be updated to work with the new command log
- Agent chat utilities are deprecated and need CQRS replacements
- Many test helpers still reference the old transaction system
Tests are expected to fail until the harness is updated (Phase 8 of CQRS refactor)

## Test Structure
- **common/**: Shared test utilities - imported via `#[path = "../common/mod.rs"]`
  - **mod.rs**: Test environment setup (`setup_test_env()`, `cleanup_test_env()`)
  - **test_harness.rs**: `TestServer` lifecycle management
  - **wal_validation.rs**: WAL-based state validation for integration tests
- **integration/**: Integration test suite (single binary for parallelism)
  - **main.rs**: Entry point - imports common utilities and test modules
  - **http_logseq_import.rs**: HTTP API import tests
  - **cli_commands.rs**: CLI command tests with build-time contract enforcement (see src/cli.rs header for adding commands)
  - **websocket_commands.rs**: WebSocket API tests
  - **agent_commands.rs**: Agent chat and admin command tests
  - **agent_tools.rs**: Agent tool execution tests for all 15 knowledge graph tools
  - **freeze_mechanism.rs**: Freeze/unfreeze operation tests for deterministic testing
  - **crash_recovery.rs**: Transaction recovery tests for startup and graph switching

## Test Utilities

### common/mod.rs
- Re-exports test utilities from test_harness for convenience

### common/test_harness.rs

#### Test Environment Management
- `setup_test_env() -> TestEnv`: Creates unique test environment (port 8888+, test_data_N/, config.test.N.yaml)
- `cleanup_test_env(TestEnv)`: Removes test data directory and config
- `get_cymbiont_binary() -> PathBuf`: Returns path to compiled binary

#### TestServer Lifecycle
- `TestServer::start(TestEnv) -> Self`: Start server mode, waits for HTTP ready
- `TestServer::start_with_args(TestEnv, Vec<&str>) -> Self`: Start with custom CLI args
- `TestServer::shutdown(self) -> TestEnv`: Send SIGINT and wait for cleanup
- `TestServer::wait_for_completion(self) -> TestEnv`: Wait for natural process exit
- `TestServer::port() -> u16`: Get server port (0 for CLI mode)
- `TestServer::force_kill()`: Force kill process with SIGKILL for crash tests
- `TestServer::send_sigint(&mut self)`: Send SIGINT without waiting (for multi-signal tests)
- `TestServer::pid() -> u32`: Get process ID

#### WebSocket Communication
- `connect_websocket(port) -> WsConnection`: Connect to WebSocket endpoint with retries
- `send_command(ws, cmd) -> Value`: Send command and return response, skipping heartbeats
- `send_command_async(ws, cmd)`: Send command without waiting for response
- `read_pending_response(ws) -> Value`: Read response with timeout handling
- `expect_success(response) -> Option<Value>`: Assert success response and return data
- `authenticate_websocket(ws, token) -> bool`: Authenticate WebSocket connection

#### Agent Chat Utilities (DEPRECATED - Will be replaced by CQRS)
- These functions are deprecated and will be replaced by CQRS-based commands
- See docs/cqrs_refactor_plan.md for the new architecture

#### Graph Setup Helpers
- `import_dummy_graph(env) -> String`: Import dummy graph via CLI and return graph ID
- `import_dummy_graph_http(port, data_dir) -> Result<String>`: Import via HTTP API (async)
- `setup_with_graph(env) -> (TestServer, String)`: Import graph and start server, returning both
- `read_auth_token(data_dir) -> String`: Read and trim auth token from data directory
- `get_single_open_graph_id(data_dir) -> String`: Get ID when exactly one graph is open

#### Freeze Mechanism (Deterministic Testing)
- `freeze_operations(ws) -> bool`: Pause graph operations after transaction creation
- `unfreeze_operations(ws) -> bool`: Resume paused graph operations
- `get_freeze_state(ws) -> bool`: Check if operations are currently frozen

#### Test Phase Documentation
- `assert_phase(PreShutdown/PostShutdown)`: Document test phase for clarity
- `PreShutdown`: Marker type for pre-shutdown phase
- `PostShutdown`: Marker type for post-shutdown phase

#### Utility Functions
- `block_on<F: Future>(future: F) -> F::Output`: Block on async future using tokio runtime

### common/wal_validation.rs

#### Core Types
- `MessagePattern`: Pattern matching for message content validation
  - `Exact(String)`: Match exact content
  - `Contains(String)`: Match substring
  - `matches(&str) -> bool`: Check if pattern matches actual content

#### WalValidator - Main validation fixture
- `WalValidator::new(data_dir: &Path) -> Self`: Create new validator for test data directory

##### Graph Operation Expectations
- `expect_create_page(name: &str, properties: Option<Value>) -> &mut Self`: Expect page creation
- `expect_create_block(block_id: &str, content: &str, page_name: Option<&str>) -> &mut Self`: Expect block creation
- `expect_update_block(block_id: &str, new_content: &str) -> &mut Self`: Expect block content update
- `expect_delete_block(block_id: &str) -> &mut Self`: Expect block deletion
- `expect_delete_page(page_name: &str) -> &mut Self`: Expect page deletion
- `expect_dummy_graph() -> &mut Self`: Add expectations for standard dummy graph import

##### Agent Operation Expectations  
- `expect_agent_created(id: Uuid, name: &str, is_prime: bool) -> &mut Self`: Expect agent registration
- `expect_agent_deleted(id: &Uuid) -> &mut Self`: Expect agent removal
- `expect_agent_activated(id: &Uuid) -> &mut Self`: Expect agent activation
- `expect_agent_deactivated(id: &Uuid) -> &mut Self`: Expect agent deactivation
- `expect_authorization(agent_id: &Uuid, graph_id: &Uuid) -> &mut Self`: Expect graph authorization
- `expect_deauthorization(agent_id: &Uuid, graph_id: &Uuid) -> &mut Self`: Expect graph deauthorization
- `expect_prime_agent(prime_id: Uuid) -> &mut Self`: Helper to expect prime agent creation

##### Message/Conversation Expectations
- `expect_user_message(agent_id: &Uuid, content: MessagePattern) -> &mut Self`: Expect user message
- `expect_assistant_message(agent_id: &Uuid, content: MessagePattern) -> &mut Self`: Expect assistant message
- `expect_tool_message(agent_id: &Uuid, tool: &str, result: MessagePattern) -> &mut Self`: Expect tool result
- `expect_chat_reset(agent_id: &Uuid) -> &mut Self`: Expect chat history clear

##### Validation Methods
- `validate_all() -> Result<(), String>`: Validate all expectations against WAL
- `validate_graph_with_content_checks(graph_id: &str, expected_blocks: &[(&str, Option<&str>)]) -> Result<(), String>`: Validate graph state with specific block content checks

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
Set `KEEP_TEST_DATA=1` to preserve test data directories after tests complete. The test output will show the preserved directory path (e.g., `test_data_0/`) containing graph files and transaction logs.