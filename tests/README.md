# Cymbiont Integration Tests

This directory contains integration tests for Cymbiont's multi-graph functionality.

TODO: Integration tests are currently broken after removing lib.rs. They need to be redesigned to test via HTTP API/WebSocket instead of internal module access.

## Test Files

### `graph_registry_test.rs`
Unit-style integration tests that verify graph registration and switching logic without requiring Logseq to be installed. These tests:
- Verify multiple graph registration
- Test graph switching by name and path
- Validate duplicate graph handling
- Test session manager launch targets

### `session_management_test.rs`
Full end-to-end integration tests that require Logseq to be installed. These tests:
- Launch Cymbiont with a specific graph
- Switch between graphs using the session API
- Verify graph isolation and proper tracking
- Test session persistence across restarts

### `integration_test.rs`
Legacy test file for basic HTTP endpoint testing.

## Running Tests

### Quick Tests (No Logseq Required)
```bash
# Run only the graph registry tests
cargo test --test graph_registry_test
```

### Full Integration Tests (Requires Logseq)
```bash
# Ensure Logseq is installed and URL handler is registered
# Run session management tests
cargo test --test session_management_test

# Run with output to see what's happening
cargo test --test session_management_test -- --nocapture
```

### All Tests
```bash
# Run all tests
cargo test

# Run tests with logging
RUST_LOG=info cargo test -- --nocapture
```

## Test Data

The tests use two pre-configured test databases:
- `logseq_databases/dummy_graph/` - Contains sample pages and content
- `logseq_databases/dummy_graph_2/` - Nearly empty graph for testing switches

## Prerequisites

For full integration tests:
1. Logseq must be installed (AppImage or other)
2. The `logseq://` URL handler must be registered (Cymbiont does this automatically on first run)
3. The test graphs must exist at the expected paths
4. **IMPORTANT**: Each test graph must be manually linked in Logseq before running tests:
   - Open Logseq
   - Click "Add graph" 
   - Navigate to `logseq_databases/dummy_graph/` and link it
   - Repeat for `logseq_databases/dummy_graph_2/`
   - This is a one-time setup requirement due to Logseq's security model

## CI/CD Considerations

The `graph_registry_test.rs` tests are suitable for CI/CD as they don't require external dependencies.

The `session_management_test.rs` tests require a display server and Logseq installation, so they may need to be run separately or in a container with xvfb.