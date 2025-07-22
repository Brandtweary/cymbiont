# Feature Taskpad: WebSocket Bidirectional Communication

## Feature Description
Implement real-time bidirectional communication between Cymbiont backend and Logseq plugin using WebSockets, enabling the AI agent to instantly create, update, and delete blocks/pages in the user's PKM. This transforms Cymbiont from a read-only knowledge graph mirror into a true bidirectional bridge where AI agents can actively participate in knowledge creation.

## Specifications
- WebSocket server integrated with Axum backend (ws://localhost:3000/ws)
- WebSocket client in Logseq plugin with automatic reconnection
- Command-based protocol for PKM operations (create, update, delete blocks/pages)
- Latency target: <50ms from command to visible change in Logseq
- Error handling with graceful degradation (queue commands during disconnects)
- Authentication token for security (prevent unauthorized PKM modifications)
- Heartbeat/ping mechanism to detect stale connections
- JSON message format for clear, extensible protocol

## Relevant Components

### Backend Infrastructure
- `src/main.rs`: HTTP server setup, will need WebSocket route addition
- `src/api.rs`: Current HTTP endpoints, provides patterns for request handling
- `src/graph_manager.rs`: Knowledge graph operations that will trigger PKM updates
- `AppState`: Shared state structure, needs WebSocket connection tracking
- Current usage: HTTP-only server using Axum

### Logseq Plugin
- `logseq_plugin/api.js`: HTTP client, will add WebSocket client here
- `logseq_plugin/index.js`: Plugin lifecycle, manages initialization and connections
- Logseq API methods needed: `createBlock`, `updateBlock`, `deleteBlock`, `createPage`
- Current usage: HTTP client only, sends data to backend

### New Components Created
- `src/websocket.rs`: WebSocket handler module in Rust backend
- Command protocol definitions in both Rust and JS
- WebSocket client in `logseq_plugin/api.js`
- `logseq_plugin/websocket.js`: Command executor module for Logseq operations

## Development Plan

### 1. Backend WebSocket Infrastructure
- [x] Add WebSocket dependencies to Cargo.toml (axum-ws or tokio-tungstenite)
- [x] Create `src/websocket.rs` module for WebSocket handling
- [x] Define command protocol structs (CreateBlock, UpdateBlock, DeleteBlock, etc.)
- [x] Implement WebSocket upgrade handler in router
- [x] Add WebSocket connections to AppState (HashMap of active connections)
- [x] Create broadcast mechanism for sending commands to specific clients
- [x] Implement heartbeat/ping mechanism (30s intervals)
- [ ] Add authentication token validation on connection

### 2. Plugin WebSocket Client
- [x] Create WebSocket client class in plugin
- [x] Implement connection management with automatic reconnection
- [x] Add command queue for offline resilience
- [x] Create command dispatcher to route to appropriate handlers
- [x] Implement heartbeat/pong responses
- [ ] Store auth token and include in connection headers

### 3. Command Protocol Implementation
- [x] Define TypeScript interfaces matching Rust command structs (using plain objects)
- [x] Implement command handlers for each operation type:
  - [x] HandleCreateBlock: Use logseq.Editor.insertBlock/appendBlockInPage
  - [x] HandleUpdateBlock: Use logseq.Editor.updateBlock with content preservation
  - [x] HandleDeleteBlock: Use logseq.Editor.removeBlock
  - [x] HandleCreatePage: Use logseq.Editor.createPage
- [ ] Add command acknowledgment system (return success/failure with details)
- [ ] Implement error recovery for failed operations
- [ ] Add command deduplication to prevent double-execution

### 4. Integration with Graph Manager
- [ ] Create kg_api module for high-level graph operations
- [ ] Add WebSocket command emission to graph mutations
- [ ] Implement operation batching for efficiency
- [ ] Add transaction-like semantics (all-or-nothing for related operations)
- [ ] Use existing pkm_to_node mapping (pkm_id is the Logseq UUID for blocks)

### 5. Developer Experience
- [x] Add WebSocket connection status to backend logs
- [x] Create temporary CLI command for injecting test WebSocket messages (via --test-websocket flag)
- [ ] Document WebSocket protocol in architecture.md
- [x] Add basic logging for command flow debugging
- [x] Implement deadlock-proof architecture with safe helper functions
- [x] Add bidirectional test command for end-to-end verification

## Development Notes

**Protocol Design Decision**: Using JSON for messages despite minor performance cost because:
- Human-readable for debugging
- Easy to extend with new fields
- Native support in JavaScript
- Negligible overhead for our use case (<1ms serialization)

**Authentication Strategy**: Simple bearer token in WebSocket headers, not full OAuth because:
- Local-only connection (localhost)
- Single user system
- Token can be generated on plugin load and shared via existing HTTP endpoint

**Reconnection Logic**: Exponential backoff (1s, 2s, 4s... max 30s) to prevent thundering herd if backend restarts

**PKM ID Mapping**: The graph manager already has `pkm_to_node: HashMap<String, NodeIndex>` where the key is the Logseq UUID for blocks (or page name for pages). This is sufficient for our needs.

**Deadlock Prevention**: Implemented safe helper functions that encapsulate all lock operations:
- `is_authenticated()`: Read-only check for authentication status
- `set_authenticated()`: Atomic write operation for authentication
- `get_connection_stats()`: Safe stats retrieval
- `get_authenticated_senders()`: Gets senders without holding locks during send
- This architecture makes it impossible to accidentally create deadlocks

**Testing Strategy**: 
- Implemented bidirectional test command that echoes between server and client
- Fixed heartbeat flood issue (was creating infinite ping-pong loop)
- WebSocket authentication completes in ~200ms
- All operations are working with the deadlock-proof architecture

**Known Issue - Redundant Real-time Sync**:
- When WebSocket commands create/update/delete blocks, Logseq's DB.onChanged fires 3-5 times for a single operation
- This is a known Logseq API characteristic (GitHub issue #5662) that the Logseq team considers "correct behavior"
- Impact: Each block operation triggers redundant syncs that send identical data back to Cymbiont
- Decision: NOT implementing throttling or workarounds because:
  - Our sync system already handles duplicate updates gracefully
  - The redundant syncs are harmless (just updating with identical data)
  - Adding complexity to work around Logseq's behavior isn't worth it
  - Even with workarounds, we'd still have multiple events (not reducible to 1)

## Future Tasks
- Comprehensive integration tests with real Logseq API
- Mock Logseq API for unit testing (complex due to browser environment)
- Test reconnection scenarios (kill server, kill plugin, network issues)
- Load testing with rapid command sequences
- Edge case testing (non-existent blocks, permission errors)
- Multi-graph support testing
- Binary protocol optimization (MessagePack) if JSON becomes bottleneck
- Command compression for large content blocks
- WebSocket connection pooling for multiple graph support
- Live collaborative features (see other users' cursors)
- Streaming responses for long-running operations
- WebRTC data channel for P2P sync between devices
- Command history and undo/redo support
- Rate limiting to prevent runaway agents
- Metrics dashboard for monitoring command flow
- Plugin-side command validation before sending
- Connection status UI indicators in plugin

## Implementation Status (Paused for Transaction Log)

### What's Complete
- ✅ WebSocket infrastructure (server and client)
- ✅ Command protocol and handlers
- ✅ Bidirectional communication working
- ✅ Test commands successfully create pages/blocks
- ✅ Deadlock-proof connection management

### What's Missing
- ❌ Command acknowledgment system (blocks return UUIDs)
- ❌ Integration with graph mutations
- ❌ Proper coordination to prevent race conditions
- ❌ kg_api module (deleted - needs transaction log first)

### Why We Paused
Discovered fundamental race condition when LLM agents create content:
1. Can't pre-generate Logseq UUIDs
2. Temporary UUIDs create stranded nodes
3. Real-time sync competes with API-created content
4. Need transaction log for proper distributed coordination

### Next Steps
1. Implement transaction log and WAL
2. Add command acknowledgments with correlation
3. Rebuild kg_api with transaction support
4. Complete graph mutation integration

## Final Implementation
{To be completed when the feature is finished - will contain authoritative summary of what was built}