# Cymbiont Integration Tests

This directory contains integration tests that verify Cymbiont's behavior through its public APIs with real Logseq instances.

## Single-Instance Test Architecture

The integration tests use a single long-running Cymbiont+Logseq instance for the entire test suite. This approach:
- Avoids the 4.5s startup penalty per test
- Tests realistic long-running server behavior
- Provides graph-based isolation for side effects
- Verifies actual Logseq plugin behavior (not mocks)

## Prerequisites

### 1. Manual Graph Linking (One-Time Setup)

**IMPORTANT**: Before running the integration tests, you must manually link all test graphs in Logseq:

1. Open Logseq
2. For each of the following test graphs, add them to Logseq:
   - `logseq_databases/test_graph_empty`
   - `logseq_databases/test_graph_switching`
   - `logseq_databases/test_graph_sync`
   - `logseq_databases/test_graph_websocket`
   - `logseq_databases/test_graph_multi_1`
   - `logseq_databases/test_graph_multi_2`

3. To add a graph in Logseq:
   - Click the graph name dropdown (top-left)
   - Select "Add graph"
   - Choose "Open local directory"
   - Navigate to the test graph folder
   - Click "Open"

**Note**: Only link the main test graphs, NOT the backup folders in `test_backups/`. The backup folders are automatically managed by the test harness.

### 2. Other Requirements

- Rust toolchain installed
- Logseq installed and accessible
- `config.test.yaml` file in project root
- Write permissions in project directory

## Test Organization

### Main Test Suite (`integration_test_suite.rs`)

The main test runner that:
1. Launches Cymbiont+Logseq once
2. Runs all tests sequentially with graph isolation
3. Automatically restores graphs between tests
4. Shuts down gracefully after all tests

### Test Categories

- **`sync_test.rs`** - Data synchronization:
  - Real-time sync through Logseq plugin
  - Incremental sync with timestamps
  - Deletion detection via plugin
  - Force sync flags

- **`websocket_test.rs`** - WebSocket communication:
  - Plugin connection and authentication
  - Command execution through plugin
  - Graph switch notifications
  - Real-time acknowledgments

- **`multi_graph_test.rs`** - Multi-graph functionality:
  - Graph switching with plugin confirmation
  - Data isolation between graphs
  - Session persistence
  - Concurrent operations

### Separate Tests

- **`api_graph_registry_test.rs`** - Graph registry (runs separately):
  - Requires multiple server instances
  - Tests persistence across restarts
  - CLI launch options
  - Run with: `cargo test api_graph_registry_test`

## Running Tests

### Run the main integration test suite:
```bash
cargo test integration_test_suite --test integration_test_suite -- --test-threads=1
```

**Note**: The `--test-threads=1` flag is required to ensure tests run sequentially.

### Run graph registry tests separately:
```bash
cargo test api_graph_registry_test
```

### Run with debug logging:
```bash
RUST_LOG=debug cargo test integration_test_suite --test integration_test_suite -- --test-threads=1
```

## Test Infrastructure

### Graph Backup/Restore

Each test graph has a backup in `logseq_databases/test_backups/`:
- `test_graph_sync` → `test_backups/test_graph_sync_backup`
- `test_graph_websocket` → `test_backups/test_graph_websocket_backup`
- etc.

The test harness automatically:
1. Creates backups if missing
2. Switches to the appropriate graph for each test
3. Restores from backup after each test completes
4. Ensures test isolation

### Test Harness (`common/test_harness.rs`)

The `IntegrationTestHarness` manages:
- Single Cymbiont+Logseq instance lifecycle
- Graph switching with WebSocket confirmation
- Automatic backup/restore between tests
- HTTP and WebSocket clients
- Graceful shutdown

## Debugging Failed Tests

1. **Check Logseq is properly linked**: Ensure all test graphs show up in Logseq's graph switcher
2. **Check server logs**: Tests run with debug logging enabled
3. **Check WebSocket connection**: Plugin must be connected for tests to work
4. **Verify graph state**: Check if test graphs were properly restored from backups
5. **Manual cleanup**: If tests fail catastrophically, manually restore graphs from `test_backups/`

## Adding New Tests

1. Create a new test graph if needed:
   ```bash
   cp -r logseq_databases/dummy_graph logseq_databases/test_graph_new
   cp -r logseq_databases/test_graph_new logseq_databases/test_backups/test_graph_new_backup
   ```

2. Manually link the new graph in Logseq

3. Add the graph to the test harness in `ensure_backups()`

4. Add test functions to appropriate test file or create new file

5. Add test execution to `integration_test_suite.rs`

## Common Issues

### "Graph not found" errors
- Ensure the test graph is linked in Logseq
- Check that Logseq is running before starting tests
- Verify graph names match exactly

### WebSocket connection failures
- Check that the Cymbiont plugin is installed in Logseq
- Verify the plugin is enabled for all test graphs
- Check for port conflicts (3000-3010)

### Test pollution
- Verify backups exist in `test_backups/`
- Check restore_graph() is working correctly
- Ensure tests switch to test_graph_empty before restore

### Slow test execution
- The first run creates backups (one-time cost)
- Logseq startup takes ~4.5s (unavoidable)
- Graph switching takes ~3s (WebSocket confirmation)

## Future Improvements

- Automated graph linking via browser automation
- Parallel test execution with multiple Logseq instances
- Performance benchmarks as separate suite
- Visual test reporter showing graph changes
- CI/CD integration with headless Logseq