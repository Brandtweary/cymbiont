# Feature Taskpad: Integration Testing

## Feature Description
Implement comprehensive integration tests for Cymbiont that test the system through its public HTTP and WebSocket APIs with real Logseq instances. Uses a single-instance test paradigm where all tests share one long-running Cymbiont+Logseq instance with graph-based isolation.

## Specifications
- Test through public APIs only (no internal imports)
- Single Cymbiont+Logseq instance for entire test suite
- Graph-based isolation with automatic backup/restore
- Real Logseq behavior verification (no mocks)
- WebSocket confirmations for graph switches
- Separate graph registry tests (require multiple instances)

## Development Plan

### 1-8. [OBSOLETE] Old Test Infrastructure
Initial sections 1-8 created tests that only hit server APIs without Logseq. These have been superseded by section 9.

### 9. Single-Instance Test Paradigm 
**Status**: ✅ Infrastructure complete, awaiting test execution

- [x] Infrastructure setup:
  - [x] Add `--shutdown-server` CLI command
  - [x] Create test graphs with backups in `test_backups/`
  - [x] Modify `config.test.yaml` for indefinite running
- [x] Test harness implementation (`tests/common/test_harness.rs`)
- [x] New test files created:
  - [x] `sync_test.rs`
  - [x] `websocket_test.rs`
  - [x] `multi_graph_test.rs`
  - [x] `integration_test_suite.rs` (main entry point)
- [x] Documentation updates (tests/README.md)
- [x] Delete old test files

### 10. Graph Registry Tests (Separate)
- [x] Keep `api_graph_registry_test.rs` standalone
- [ ] Run with `cargo test api_graph_registry_test`

### 11. Test Data Isolation (CRITICAL)
**Status**: 🚨 Required before any test execution - currently mixing test/production data

**Problem**: Tests currently write to production `data/` directory, polluting user data and making deterministic assertions impossible.

**Solution**: Add `--data-dir` CLI flag to redirect all data storage for tests.

- [ ] Add `--data-dir <PATH>` CLI argument to main.rs
- [ ] Update Config struct to include data_dir field
- [ ] Plumb data_dir through all modules that access disk:
  - [ ] `GraphRegistry::load_or_create()` and `save()` 
  - [ ] `GraphManager` knowledge graph storage paths
  - [ ] `TransactionLog` and saga transaction log paths
  - [ ] `SessionManager` for `last_session.json`
  - [ ] Archive operations in `graph_manager.rs`
- [ ] Add unit tests for `--data-dir` functionality
- [ ] Update test harness to use isolated temp directory
- [ ] Add graph registry validation with deterministic assertions

**User Benefit**: This also enables users to store KG data outside the Cymbiont directory.

### 12. Test Execution and Verification  
**Status**: ⏸️ Blocked on Section 11 (Test Data Isolation)

- [ ] Delete standalone graph registry tests (will be covered by main suite)
- [ ] Manually link all test graphs in Logseq
- [ ] Run `cargo test integration_test_suite --test integration_test_suite -- --test-threads=1`
- [ ] Fix any failing tests
- [ ] Verify test side effects (diff graph folders)
- [ ] If clean, consolidate into single `test_graph`

## Critical API Details

### PKMData Format
```json
{
  "source": "test",
  "type_": "blocks|pages",
  "payload": "JSON string"  // stringified JSON!
}
```

### Key Field Names
- PKMBlockData: `id`, `created`/`updated` as strings
- PKMPageData: `name`, `normalized_name`
- Headers: `X-Cymbiont-Graph-Name`, `X-Cymbiont-Graph-Path`

## Current Status (2025-01-28)
- ✅ Infrastructure complete
- 🚨 **CRITICAL**: Section 11 (Test Data Isolation) must be completed before any test execution
- ⏸️ Test execution blocked until data isolation is implemented