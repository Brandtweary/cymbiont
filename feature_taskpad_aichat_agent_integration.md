# Feature Taskpad: AIChat-Agent Integration

## Feature Description
Integrate the aichat-agent library as a git submodule in Cymbiont to provide LLM-powered chat agents with native knowledge graph capabilities. This will enable Cymbiont to leverage AIChat's battle-tested REPL and agent infrastructure while extending it with custom Rust functions that interact with the PKM knowledge graph. Users will be able to query their personal knowledge graph through natural language conversations, with the "knowledge-graph-agent" having direct access to graph traversal, semantic search, and knowledge synthesis capabilities.

## Specifications
- Import aichat-agent as a git submodule for transparent source access
- Create native Rust functions for knowledge graph operations (query, traverse, analyze)
- Build a specialized "knowledge-graph-agent" with KG-aware context and tools
- Integrate KG retrieval with AIChat's existing RAG capabilities for comparison
- Provide programmatic agent configuration without touching ~/.config/aichat
- Enable both standalone REPL mode and potential future API endpoints
- Maintain clean separation: aichat-agent handles LLM/chat, cymbiont adds KG layer

## Relevant Components

### AIChat-Agent Library (External)
- `aichat-agent-lib/`: Complete library with REPL, agents, and function integration
- Key APIs: `TempConfigBuilder`, `AgentDefinitionBuilder`, `FunctionRegistry`, `ReplSession`
- Current usage: To be imported as git submodule

### Cymbiont Graph Manager
- `src/graph_manager.rs`: Core knowledge graph implementation using petgraph
- Key methods: `query_nodes()`, `get_connected_nodes()`, `semantic_search()`
- Current usage: HTTP API only, needs public API for direct access

### Cymbiont Data Structures
- `src/import/pkm_data.rs`: Shared data types (`PKMBlockData`, `PKMPageData`)
- Current usage: Serialization for HTTP API, will be used in function signatures

### Agent Functions Directory
- `functions/agents/knowledge-graph-agent/`: Placeholder for KG tool implementations
- Current usage: Empty directory structure awaiting implementation

## Development Plan

### 1. Logseq Plugin API Research
- [x] Research Logseq plugin API documentation for block manipulation capabilities
- [x] Investigate block creation API (createBlock, insertBlock methods)
- [x] Study block update/edit API functions and their limitations
- [x] Understand page creation and appending mechanisms
- [x] Test API quirks and edge cases (async behavior, rate limits, error handling)
- [x] Document findings and create examples for each operation type
- [x] Identify any plugin API limitations that might affect our implementation
  - ✅ Completed in `docs/logseq_plugin_api_research.md`

### 2. Graph Manager Public API with PKM Sync
**✅ COMPLETED - kg_api module ready for consumption**
- [x] Created `src/kg_api.rs` module as the public API layer
- [x] Implemented all core operations with transaction support:
  - [x] `add_block()` - Creates blocks with saga workflow and temp_id → UUID mapping
  - [x] `update_block()` - Updates with transaction boundaries  
  - [x] `delete_block()` - Archives nodes with full transaction support
  - [x] `create_page()` - Page creation with properties
  - [x] `get_node()` - Query operations (read-only)
  - [x] `query_graph_bfs()` - Placeholder for BFS traversal
- [x] WebSocket sync integrated for all write operations
- [x] Correlation ID support for create_block (acknowledgment flow complete)
- [x] Transaction consistency via saga pattern
- [x] **Transaction Log Foundation**: Complete WAL system with ACID guarantees

**Status**: READY FOR INTEGRATION - The kg_api module provides a complete, transaction-safe public API for knowledge graph operations. It's currently marked with `#![allow(dead_code)]` to prevent warnings, but all functionality is implemented and tested. This is the foundation layer that the AIChat-Agent native functions will consume.

### 3. Multi-Graph Support Completion (from feature_taskpad_multi_graph_support.md)
- [ ] Recovery happens per-graph on startup
- [ ] Test with both file and DB graph types
- [ ] Update archive recovery to filter by graph_id when needed
- [ ] Memory management considerations:
  - [ ] Document that we keep all graphs in RAM (modern machines can handle it)
  - [ ] Future optimization: LRU cache for inactive graphs
- [ ] CLI Commands:
  - [ ] `cymbiont switch-graph <name>` - Switch active graph by Logseq graph name (via API)
  - [ ] `cymbiont switch-graph --path <path>` - Alternative: switch by path (will resolve to name)
  - [ ] `cymbiont list-graphs` - Show all graphs with names and paths (via API)
  - [ ] `cymbiont current-graph` - Show current active graph name and path (via API)
- [ ] Migration and Documentation:
  - [ ] Create migration guide for existing users
  - [ ] Update CLAUDE.md with multi-graph instructions
  - [ ] Document session management and low-click workflow
  - [ ] Document graph identifier best practices
  - [ ] Add examples for common multi-graph setups
  - [ ] Update architecture documentation

### 4. Transaction Log Completion (from feature_taskpad_transaction_log_wal.md)
- [ ] Test crash recovery scenarios with kill -9
- [ ] Add timeout handling for missing acknowledgments
- [ ] Add correlation tracking for update operations
- [ ] Add correlation tracking for delete operations
- [ ] Add correlation tracking for page operations
- [ ] Add metrics for deduplication effectiveness
- [ ] Test recovery with various failure scenarios
- [ ] Configuration and Management:
  - [ ] Add transaction log settings to config.yaml
  - [ ] Implement log rotation and compaction
  - [ ] Create CLI commands for log inspection
  - [ ] Add metrics and health checks
  - [ ] Document operational procedures

### 5. WebSocket Bidirectional Completion (from feature_taskpad_websocket_bidirectional.md)
- [ ] WebSocket Multi-Graph Support:
  - [ ] Modify `ConnectionState` struct to include `graph_id: Option<String>` field
  - [ ] Update `auth` command to accept graph identification from plugin headers
  - [ ] Modify `broadcast_command()` to filter by graph_id before sending
  - [ ] Add `get_authenticated_senders_for_graph(graph_id)` helper function
  - [ ] Update plugin WebSocket client to send graph context during authentication
  - [ ] Update WebSocket messages to include graph context
  - [ ] Coordinate with SessionManager when graphs are switched
  - [ ] Maintain WebSocket connections across graph switches (don't disconnect)
  - [ ] Update active graph context for existing connections when user switches graphs
  - [ ] Handle scenario where connection was established for Graph A but user switches to Graph B
  - [ ] Track which connections belong to which graph
  - [ ] Only broadcast commands to relevant connections
  - [ ] Update WebSocket connections with new graph context on session switch
- [ ] Timeout and Correlation:
  - [ ] Add acknowledgment timeouts (30s default)
  - [ ] Extend correlation tracking to update_block operations
  - [ ] Extend correlation tracking to delete_block operations  
  - [ ] Extend correlation tracking to create_page operations
- [ ] Implement error recovery for failed operations
- [ ] Add command deduplication to prevent double-execution
- [ ] Implement operation batching for efficiency
- [ ] Document WebSocket protocol in architecture.md

### 6. Git Submodule Setup (PRIORITY #5 - Final Integration)
- [ ] Add aichat-agent as git submodule at `cymbiont/aichat-agent/`
- [ ] Configure Cargo.toml with path dependency to submodule
- [ ] Set up workspace configuration if needed
- [ ] Test basic compilation with the library dependency
- [ ] Document submodule update workflow in README

**Status**: WAIT FOR INFRASTRUCTURE COMPLETION
- **Implementation Priority**: #5 in the development sequence
- **Why Last**: Should only begin when everything else is complete and stable
- **Dependencies**: Multi-Graph Support Completion, Transaction Log Completion, WebSocket Completion, Integration Testing

### 7. Knowledge Graph Function Implementation
- [ ] Create `src/kg_functions.rs` module for native function implementations
- [ ] Implement core KG tool functions:
  - [ ] `kg_query_nodes(query: String) -> Vec<NodeInfo>` - BFS traversal with search
  - [ ] `kg_add_note(content: String, metadata: Value) -> NodeInfo` - Add note to KG and PKM
  - [ ] `kg_edit_note(node_id: String, updates: Value) -> Result<NodeInfo>`
  - [ ] `kg_delete_note(node_id: String) -> Result<String>`
  - [ ] `kg_get_connections(node_id: String, depth: u32) -> ConnectionGraph`
- [ ] Ensure PKM sync for add/edit/delete operations
- [ ] Create JSON serialization for function inputs/outputs
- [ ] Add error handling and validation
- [ ] Write unit tests for each function

### 8. Function Registry Integration
- [ ] Create `FunctionRegistry` instance with KG functions
- [ ] Map Rust functions to AIChat's function calling format
- [ ] Handle async operations if GraphManager requires them
- [ ] Test function execution through the registry
- [ ] Add function documentation and examples

### 9. Knowledge Graph Agent Definition
- [ ] Create agent definition using `AgentDefinitionBuilder`
- [ ] Write agent instructions for KG-aware responses
- [ ] Configure agent with available KG functions
- [ ] Set appropriate model and temperature settings
- [ ] Save agent definition to appropriate directory structure

### 10. REPL Integration and Testing
- [ ] Create main entry point for Cymbiont REPL mode
- [ ] Initialize `TempConfigBuilder` with API keys from config
- [ ] Set up `ReplSession` with KG functions and agent
- [ ] Add command-line arguments for REPL vs server mode
- [ ] Test full conversation flow with KG queries

### 11. Configuration and Deployment
- [ ] Update `config.yaml` with AIChat-related settings
- [ ] Create example configurations for different use cases
- [ ] Update Cymbiont CLI to support agent commands
- [ ] Document configuration options in README

### 12. Integration Testing
- [ ] Create integration tests for agent + KG scenarios
- [ ] Test error handling and edge cases
- [ ] Verify agent maintains conversation context
- [ ] Test with different LLM providers (Claude, GPT-4, etc.)
- [ ] Ensure KG modifications are reflected in agent responses

### 13. Documentation
- [ ] Write user guide for knowledge graph agent usage
- [ ] Document available KG functions and examples
- [ ] Create architecture diagram showing integration
- [ ] Add inline code documentation for new modules
- [ ] Update README with agent capabilities

### 14. Final Polish
- [ ] Clean up any debugging code or logs
- [ ] Remove temporary test code
- [ ] Ensure all error messages are user-friendly

## Development Notes

**Architecture Decision**: The Graph Manager public API with PKM sync (Phase 2) must be implemented before the KG native functions (Phase 4) to ensure clean separation of concerns. The Logseq plugin API research (Phase 1) is critical to inform the PKM sync implementation. The native functions will call the public API rather than directly manipulating the graph internals. This provides:
- Clear abstraction boundaries
- Easier testing of both layers independently  
- Potential for future API consumers beyond the agent functions
- Consistent error handling and validation

## Future Tasks

### AIChat-Agent Integration Future Tasks
- Implement automatic context injection from recent KG queries
- Add relevance scoring for KG results
- Create context windowing to stay within token limits
- Compare KG retrieval with AIChat's RAG for effectiveness
- Add metrics/logging for retrieval performance
- Profile performance of KG function calls
- Optimize frequently used graph queries
- Add caching for repeated KG lookups
- Implement graceful degradation if LLM is unavailable
- Benchmark KG retrieval vs RAG for different query types
- Add streaming responses for long KG analyses
- Implement agent memory persistence between sessions
- Create specialized agents for different PKM workflows (research, writing, learning)
- Add multi-modal support for diagrams and knowledge visualization
- Build collaborative features for shared knowledge graphs
- Implement incremental indexing for large knowledge bases
- Add support for external knowledge sources beyond Logseq

### Multi-Graph Support Future Tasks (from feature_taskpad_multi_graph_support.md)
- Enhanced session management (multiple simultaneous Logseq instances)
- Graph preloading and caching for instant switching
- Graph-specific plugin configurations
- Cross-graph query capabilities (with explicit permission)
- Graph migration tools (move data between graphs using internal IDs)
- Multi-graph dashboard for monitoring all registered graphs
- Automatic graph discovery and registration with ID assignment
- Graph-level access control and permissions
- Performance optimization for many graphs
- Graph templates and inheritance
- Internal ID remapping tool (emergency use only)

### Transaction Log Future Tasks (from feature_taskpad_transaction_log_wal.md)
- Distributed transaction support for multi-instance Cymbiont
- Streaming replication to secondary instances
- Point-in-time recovery ("show me the graph as of timestamp X")
- Transaction history UI in a web interface
- Prometheus metrics export for monitoring
- Compression for old log segments
- Encryption at rest for log files
- Multi-version concurrency control (MVCC) for read consistency
- Saga pattern implementation for long-running workflows

### WebSocket Bidirectional Future Tasks (from feature_taskpad_websocket_bidirectional.md)
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

## Final Implementation
{To be completed when the integration is finished - will contain authoritative summary of what was built}

## Branch Completion Process
When feature branch `aichat-integration` is merged to main:
1. Archive this taskpad to `/archive/feature_taskpad_aichat_agent_integration.md`
2. Update `CLAUDE.local.md` to remove the branch mapping