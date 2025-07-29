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

### 11. Test Data Isolation ✅ COMPLETED
**Status**: ✅ Data directory configuration implemented and committed

**Solution**: Added configurable data directory with CLI override.

- [x] Add `--data-dir <PATH>` CLI argument to main.rs
- [x] Update Config struct to include data_dir field  
- [x] Plumb data_dir through all modules that access disk:
  - [x] `GraphRegistry::load_or_create()` and `save()` 
  - [x] `GraphManager` knowledge graph storage paths
  - [x] `TransactionLog` and saga transaction log paths
  - [x] `SessionManager` for `last_session.json`
  - [x] Archive operations in `graph_manager.rs`
- [x] Add unit tests for `--data-dir` functionality
- [x] Update test harness to use isolated temp directory via config.test.yaml
- [ ] Add lightweight graph registry validation to test harness

**User Benefit**: Users can now store KG data outside the Cymbiont directory.

### 12. Graph Registry Validation (Lightweight)
**Status**: ✅ COMPLETED

Add inline validation to test harness for basic graph registry sanity checks:

- [x] Add `validate_graph_registry_state()` method to `IntegrationTestHarness`
- [x] Validate test_data/graph_registry.json contains 6 test graphs
- [x] Check each graph has valid UUID and expected test graph names
- [x] Verify basic JSON structure and metadata consistency
- [x] Cache graph IDs on first run for persistence validation (implemented)

**Scope**: Lightweight checks only - can't test multi-instance scenarios like session persistence.

### 13. WebSocket Test Refactoring
**Status**: ✅ COMPLETED

- [x] Refactor WebSocket test to use user-simulation approach (HTTP endpoints)
- [x] Add WebSocket verification endpoints to backend:
  - [x] `GET /api/websocket/status` - Connection health and metrics
  - [x] `GET /api/websocket/recent-activity` - Recent commands/confirmations
- [x] Add WebSocket API methods to frontend (api.js)
- [x] Update test harness with WebSocket connection handling
- [x] Fix graph switch confirmation to poll WebSocket messages instead of using hard-coded 3s sleep
- [x] Update documentation in cymbiont_architecture.md
- [x] Delete websocket_test.rs.bak after verification

### 14. WebSocket Test Enhancement (NEEDED)
**Status**: 📋 Planning required

The current websocket_test.rs correctly tests that sync operations don't trigger WebSocket commands. We need to add tests for kg_api operations that DO trigger commands.

- [ ] Create HTTP endpoints for kg_api operations (add_block, update_block, delete_block, create_page)
- [ ] Consider creating a new module for these endpoints (api.rs is already too large)
- [ ] Add test_kg_api_triggers_websocket_commands to websocket_test.rs
- [ ] Test actual WebSocket command flow (commands sent to plugin)
- [ ] Verify acknowledgment flow for create_block operations

**Current state**:
- WebSocket tests are active and test connection health + sync behavior
- Placeholder comment added for future kg_api test

### 15. Test Execution and Verification  
**Status**: 🚀 Ready to execute

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

## Current Status (2025-01-29)
- ✅ Infrastructure complete
- ✅ Test Data Isolation implemented and committed
- ✅ Graph registry validation implemented
- ✅ WebSocket test refactoring completed
- 🚀 Ready for test execution