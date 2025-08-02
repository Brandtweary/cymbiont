# CYMBIONT TEST GUIDE

## Test Structure
- **common/**: Shared test utilities - imported via `#[path = "../common/mod.rs"]`
  - **mod.rs**: Test environment setup (`setup_test_env()`, `cleanup_test_env()`)
  - **test_harness.rs**: `TestServer` lifecycle management
- **integration/**: Integration test suite (single binary for parallelism)
  - **main.rs**: Entry point - imports common utilities and test modules
  - **http_logseq_import.rs**: HTTP API import tests
  - **logseq_import.rs**: CLI import tests
  - **websocket_commands.rs**: WebSocket API tests

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
- `assert_phase(PreShutdown/PostShutdown)`: Document test phase for clarity

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
```