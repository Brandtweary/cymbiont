# Feature Taskpad: Integration Testing

## Feature Description
Implement comprehensive integration tests for Cymbiont that test the system through its public HTTP and WebSocket APIs without importing internal modules. These tests will validate multi-graph support, transaction coordination, WebSocket communication, and end-to-end sync scenarios using real server processes and actual timing rather than mocks.

## Specifications
- Test only through public APIs (HTTP endpoints and WebSocket)
- No internal module imports (no `use cymbiont::*`)
- Start real `cargo run` processes for each test
- Use proper timing and waits for async operations
- Include graph headers (X-Cymbiont-Graph-*) in all HTTP requests
- Wait for WebSocket confirmations rather than optimistic updates
- Clean process termination and test data cleanup
- Test with existing dummy graphs in `logseq_databases/`
- Validate actual system behavior, not mocked responses

## Relevant Components

### HTTP API Endpoints
- `src/api.rs`: All HTTP endpoints for testing
- Key endpoints: `/data`, `/plugin/initialized`, `/sync/status`, `/api/session/*`
- Current usage: Need to test via reqwest client

### WebSocket Server
- `src/websocket.rs`: WebSocket command protocol
- Key commands: auth, create_block, update_block, GraphSwitchConfirmed
- Current usage: Need WebSocket test client

### Test Graphs
- `logseq_databases/dummy_graph/`: Test graph with sample data
- `logseq_databases/dummy_graph_2/`: Second test graph for switching
- Current usage: Already configured with graph IDs

### Existing Tests
- `tests/graph_registry_test.rs`: Currently uses internal imports
- `tests/session_management_test.rs`: Partially implemented, needs completion
- `tests/integration_test.rs`: Too simple, needs expansion

## Development Plan

### 1. Test Infrastructure
- [ ] Create `tests/common/mod.rs` with shared test utilities
- [ ] Implement `CymbiontTestServer` struct:
  - [ ] `start(args: &[&str]) -> Result<Self>` - Start server with CLI args
  - [ ] `wait_ready() -> Result<()>` - Poll health endpoint
  - [ ] `stop()` - Graceful shutdown
  - [ ] `impl Drop` for automatic cleanup
- [ ] Implement `TestClient` wrapper around reqwest:
  - [ ] `new(graph_name: &str) -> Self` - Create with graph headers
  - [ ] Helper methods for each endpoint (post_data, get_sync_status, etc.)
  - [ ] Automatic base URL and error handling
- [ ] Implement `WebSocketTestClient`:
  - [ ] `connect() -> Result<Self>` - Connect and authenticate
  - [ ] `send_command(cmd: Command) -> Result<()>`
  - [ ] `wait_for_event(matcher, timeout) -> Result<Event>`
  - [ ] Heartbeat handling

### 2. Graph Registry Tests
- [ ] Rename `graph_registry_test.rs` to `api_graph_registry_test.rs`
- [ ] Remove all internal imports
- [ ] Test graph registration via `/plugin/initialized`:
  - [ ] First connection creates new graph
  - [ ] Subsequent connections reuse existing graph
  - [ ] Graph ID persistence across restarts
- [ ] Test graph switching via `/api/session/switch`:
  - [ ] Switch by name
  - [ ] Switch by path
  - [ ] Invalid graph handling
- [ ] Test duplicate graph prevention:
  - [ ] Same name and path returns same ID
  - [ ] Different path creates new graph

### 3. Sync API Tests
- [ ] Create `tests/api_sync_test.rs`
- [ ] Test real-time sync via `/data`:
  - [ ] Send PKMBlockData
  - [ ] Send PKMPageData
  - [ ] Verify sync status updates
- [ ] Test incremental sync flow:
  - [ ] Check sync status
  - [ ] Send data with timestamps
  - [ ] Verify only new data processed
- [ ] Test deletion verification:
  - [ ] Send block IDs to `/sync/verify`
  - [ ] Verify missing blocks archived
- [ ] Test force sync flags:
  - [ ] `--force-incremental-sync`
  - [ ] `--force-full-sync`

### 4. WebSocket Integration Tests
- [ ] Create `tests/websocket_integration_test.rs`
- [ ] Test authentication flow:
  - [ ] Connect without auth (should fail)
  - [ ] Send auth command
  - [ ] Verify authenticated state
- [ ] Test command broadcasting:
  - [ ] Send create_block from server
  - [ ] Verify acknowledgment received
  - [ ] Check correlation IDs match
- [ ] Test graph switch confirmation:
  - [ ] Trigger graph switch
  - [ ] Wait for GraphSwitchRequested
  - [ ] Send GraphSwitchConfirmed
  - [ ] Verify state updated
- [ ] Test heartbeat mechanism:
  - [ ] Verify ping/pong exchange
  - [ ] Test connection timeout

### 5. Multi-Graph E2E Tests
- [ ] Rename `session_management_test.rs` to `e2e_multi_graph_test.rs`
- [ ] Complete existing test implementations
- [ ] Test data isolation between graphs:
  - [ ] Create data in graph 1
  - [ ] Switch to graph 2
  - [ ] Verify data not visible
  - [ ] Switch back to graph 1
  - [ ] Verify data still present
- [ ] Test concurrent operations:
  - [ ] Run two servers with different graphs
  - [ ] Send data to both simultaneously
  - [ ] Verify no cross-contamination
- [ ] Test session persistence:
  - [ ] Start with graph 1
  - [ ] Restart server
  - [ ] Verify returns to graph 1

### 6. Transaction Coordination Tests
- [ ] Create `tests/transaction_test.rs`
- [ ] Test content hash deduplication:
  - [ ] Send same content via WebSocket and HTTP
  - [ ] Verify only processed once
- [ ] Test acknowledgment flow:
  - [ ] Create block via kg_api
  - [ ] Verify WebSocket command sent
  - [ ] Send acknowledgment
  - [ ] Verify transaction committed
- [ ] Test timeout scenarios:
  - [ ] Create block but don't acknowledge
  - [ ] Verify timeout after 30s
  - [ ] Check transaction rolled back
- [ ] Test crash recovery:
  - [ ] Start transaction
  - [ ] Kill process
  - [ ] Restart and verify recovery

### 7. Performance and Load Tests
- [ ] Create `tests/performance_test.rs`
- [ ] Benchmark sync performance:
  - [ ] Time to sync 1000 blocks
  - [ ] Memory usage during sync
  - [ ] Verify <5ms transaction overhead
- [ ] Load test concurrent operations:
  - [ ] 10 simultaneous WebSocket connections
  - [ ] Rapid command sequences
  - [ ] Verify no deadlocks
- [ ] Stress test graph switching:
  - [ ] Rapid switches between graphs
  - [ ] Verify data integrity maintained

### 8. Test Documentation
- [ ] Update `tests/README.md`:
  - [ ] New test structure explanation
  - [ ] How to run specific test suites
  - [ ] Prerequisites (Logseq not required)
  - [ ] CI/CD considerations
- [ ] Add inline documentation to test utilities
- [ ] Create example test as template

## Development Notes
**Architecture Decision**: All tests must use public APIs to ensure we're testing the actual user-facing behavior. This catches issues that unit tests miss, like header parsing, middleware behavior, and timing issues.

**Test Data Strategy**: Use the existing dummy graphs rather than creating new test data. This ensures tests run against realistic data structures.

**Process Management**: Each test spawns its own server process to ensure isolation. The test utilities handle cleanup even if tests panic.

**Timing Considerations**: Use proper async waits rather than sleep(). Poll endpoints for readiness rather than assuming fixed startup times.

## Future Tasks
- Mock Logseq plugin for full E2E testing including plugin-side behavior
- Fuzzing tests for API endpoints to find edge cases
- Network failure simulation (connection drops, high latency)
- Property-based testing for graph operations
- Visual test reporter showing graph state changes
- Integration with GitHub Actions for CI/CD
- Benchmarking suite comparing different sync strategies
- Chaos testing framework (random kills, resource limits)
- Test coverage reporting and gap analysis
- Contract testing between backend and plugin

## Final Implementation
{To be completed when the integration tests are fully implemented}