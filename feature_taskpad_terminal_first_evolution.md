# Feature Taskpad: Terminal-First Knowledge Graph Evolution

## Feature Description
Transform Cymbiont from a Logseq-integrated PKM synchronization tool into a terminal-first knowledge graph engine designed for AI agents. This evolution preserves the core knowledge graph functionality while shedding the complexity of browser/plugin coordination, emerging as a clean, composable Unix-style utility that can be embedded, piped, or served.

## Core Vision
- **Primary Interface**: stdin/stdout for agent consumption
- **Data Format**: JSON/EDN structured data
- **TUI Dashboard**: Persistent pinned notes for agent context (not graph browsing)
- **Library First**: Expose as a Rust library for embedding
- **API Preserved**: Keep HTTP/WebSocket APIs for programmatic access

## Specifications
- **Primary Interface**: stdin/stdout JSON/EDN protocol for agent consumption
- **Library First**: Expose as embeddable Rust library with clean public API
- **TUI Dashboard**: Read-only monitoring interface using ratatui (no text editing)
- **Import-Only PKM**: One-way import from Logseq/Roam/Obsidian (no bidirectional sync)
- **Preserved APIs**: Keep HTTP/WebSocket endpoints for backwards compatibility
- **Feature Gates**: Modular compilation with opt-in components
- **Unix Philosophy**: Composable, pipeable, does one thing well
- **Performance Target**: <10ms query latency for agent operations

## Relevant Components

### Core Knowledge Graph (Preserve)
- `src/graph_manager.rs`: Petgraph-based storage engine - the beating heart
- `src/kg_api.rs`: Transaction-safe graph operations API
- `src/pkm_data.rs`: Core data structures (PKMBlockData, PKMPageData)
- `src/transaction_log.rs`: Write-ahead logging with sled
- `src/transaction.rs`: Transaction coordination and state machine
- `src/saga.rs`: Multi-step workflow patterns

### Adaptable Infrastructure
- `src/api.rs`: HTTP endpoints - remove Logseq routes, keep graph operations
- `src/websocket.rs`: Real-time updates - remove plugin commands, keep streaming
- `src/config.rs`: Configuration - simplify, remove Logseq-specific fields

### Evolution Candidates (Transform or Deprecate)
- `src/session_manager.rs`: Currently manages Logseq sessions - deprecate entirely (REMOVED ✓)
- `src/graph_registry.rs`: Multi-graph registry - keep for multi-graph support
- `src/utils.rs`: Mixed utilities - extract useful parts, remove Logseq launches (CLEANED ✓)
- `src/edn.rs`: EDN manipulation - might be useful for import functionality

### New Growth Areas
- `src/lib.rs`: Public library interface (to be created)
- `src/import/`: PKM importers for Logseq/Roam/Obsidian
- `src/tui/`: Terminal UI dashboard with ratatui
- `src/cli/`: Command-line interface and pipe protocol

## Development Plan

### 1. Merge Resolution & State Stabilization
- [x] Preserve `feature_taskpad_aichat_agent_integration.md` historical context (PRESERVED ✓)
- [x] Complete the merge by accepting the architectural pivot (MERGED TO MAIN ✓)
- [x] Update CLAUDE.local.md to reflect evolution (not crisis) (UPDATED ✓)
- [ ] Tag pre-evolution state for historical reference

### 2. Deprecation Wave
- [x] Add deprecation headers to `session_manager.rs` (REMOVED ENTIRELY ✓)
- [x] Mark `logseq_plugin/` directory as deprecated (REMOVED ENTIRELY ✓)
- [x] Update integration tests with deprecation notices (REMOVED ✓)
- [x] Document deprecation timeline in README (UPDATED ✓)

### 3. Core Extraction & Library Design
- [x] Create `src/lib.rs` with public API surface (COMPLETED ✓)
- [x] Implement clean separation of server and core functionality (COMPLETED ✓)
- [x] Create `src/app_state.rs` for centralized state management (COMPLETED ✓)
- [x] Establish dual binary architecture (cymbiont + cymbiont-server) (COMPLETED ✓)
- [x] **Library Architecture Complete**: Clean separation achieved with dual binaries
  - Core functionality: Terminal-first CLI with AppState coordination
  - Network layer: Optional HTTP/WebSocket server in separate binary
  - Library interface: All core modules exposed via lib.rs
  - No feature flag complexity: Physical separation instead of conditional compilation

### 4. Interface Evolution
- [ ] Design stdin/stdout JSON command protocol
- [ ] Create CLI argument parser and command router
- [ ] Implement pipe-friendly output formats
- [ ] Add batch operation support for efficiency
- [ ] **Unix Pipe Examples**:
  ```bash
  # Query knowledge graph
  echo '{"query": "find_related", "node": "Rust"}' | cymbiont
  
  # Import from Logseq
  cymbiont import --format logseq --path ~/Documents/notes
  
  # Stream updates to agent
  cymbiont stream | my-agent --consume-kg
  
  # TUI monitoring
  cymbiont tui
  ```

### 5. Import System Architecture
- [ ] Create `src/import/mod.rs` framework
- [ ] Design common `Importer` trait
- [ ] Implement Logseq importer using existing PKM structures
- [ ] Add progress reporting for large imports

### 6. TUI Dashboard Birth
- [ ] Set up ratatui dependencies and basic app structure
- [ ] Design read-only graph visualization
- [ ] Implement real-time update streaming
- [ ] Create keyboard navigation system

### 7. API Adaptation
- [x] Remove Logseq-specific HTTP endpoints (COMPLETED ✓)
- [ ] Simplify WebSocket protocol for agent streaming
- [ ] Add new agent-focused query endpoints
- [ ] Implement efficient batch query API

### 8. Testing Evolution
- [ ] Archive integration tests as historical artifacts
- [ ] Create new unit test suite for library interface
- [ ] Design agent-focused test scenarios
- [ ] Add pipe interface testing

### 9. Documentation Metamorphosis
- [ ] Update README with new vision and examples
- [ ] Integrate 1.0 plan into architecture document
- [ ] Create migration guide for existing users
- [ ] Write agent integration cookbook

### 10. Re-evaluate Transaction Log and Sagas
- [ ] Review transaction recovery methods (marked with `#[allow(dead_code)]`)
- [ ] Assess if content deduplication is still needed without Logseq
- [ ] Determine if saga pattern is overkill for terminal-first architecture
- [ ] Consider simpler append-only log alternative
- [ ] Remove or refactor marked functions in:
  - `transaction.rs`: `find_pending_transaction_by_content`, `handle_acknowledgment`, `recover_pending_transactions`
  - `transaction_log.rs`: `list_pending_transactions`, `find_transaction_by_content_hash`, `flush`
  - `saga.rs`: `get_saga` method

### 11. Final Cleanup & Optimization
- [ ] Remove all dead code paths
- [ ] Optimize for sub-10ms query performance
- [ ] Minimize binary size with feature gates
- [ ] Profile and eliminate bottlenecks

## Development Notes

### 2025-01-29: Library Extraction Strategy Resolution
After the logseq-removal agent successfully removed 5,526 lines of browser-specific code, we initially planned to expose Cymbiont as a library. However, we discovered that:

1. **Everything is tightly coupled** - GraphManager needs TransactionLog, which needs sled, which needs file paths, etc.
2. **Cymbiont is a stateful service** - Not a stateless library that can be easily embedded
3. **HTTP/WebSocket APIs are sufficient** - External applications can integrate via these standard protocols
4. **Single binary with --server flag** - Much simpler than dual binaries or complex feature flags

We opted NOT to provide a library interface because:
- The coupling is intentional and necessary for data integrity
- Trying to extract a "clean" API would require major architectural changes
- HTTP/WebSocket access is more universal than Rust library embedding
- This matches the pattern of other complex systems (databases, search engines, etc.)

The final architecture is a single binary that defaults to CLI mode and can run as a server with `--server`.

### 2025-01-29: Birth of a New Paradigm
The decision to pivot from Logseq integration to terminal-first architecture represents a fundamental evolution in Cymbiont's design philosophy. Rather than fighting the complexity of browser automation and plugin state management, we're embracing the Unix philosophy of composable tools. This isn't a failure of the original vision, but a recognition that AI agents need different interfaces than human users.

### CRITICAL: AIChat-Agent Integration Still Active
**The AIChat-Agent integration is NOT being abandoned!** The `feature_taskpad_aichat_agent_integration.md` file contains the complete plan for integrating aichat-agent as a git submodule to provide LLM-powered chat agents with native knowledge graph capabilities. This integration remains a core part of the vision - we're simply approaching it differently:

- **Original Plan**: Browser-based integration through Logseq plugin
- **New Approach**: Terminal-first integration with aichat-agent consuming the Rust library directly
- **Preserved Goals**: Knowledge graph queries through natural language, native Rust functions for KG operations, specialized "knowledge-graph-agent" with direct access to graph traversal

The file must be preserved in its entirety as it contains:
1. Complete specifications for aichat-agent integration
2. Development plan with 14 detailed phases
3. Function signatures for KG operations
4. WebSocket bidirectional completion tasks
5. Multi-graph support requirements
6. Transaction log completion tasks

This is not historical context - it's the active integration plan adapted for terminal-first architecture!

### Architectural Principles
1. **Library First**: Every feature should be accessible programmatically
2. **Zero Browser Dependencies**: No WebExtensions, no Electron, pure Rust
3. **Agent Ergonomics**: Optimize for machine consumption, not human clicking
4. **Backwards Compatibility**: Existing HTTP/WS APIs remain functional

## Future Tasks
- Performance benchmarking suite for agent workloads
- GraphML/Cypher export formats for interoperability
- Distributed graph sharding for massive knowledge bases
- WASM compilation target for browser embedding
- Plugin system for custom graph algorithms
- Semantic search using vector embeddings
- Multi-agent coordination protocols
- Real-time collaborative graph editing


## Migration Path
1. Existing Logseq users can one-time import their graphs
2. API remains compatible for existing integrations
3. WebSocket available but optional
4. Focus shifts from sync to import/export

## Success Metrics
- Agent query latency < 10ms
- Import 100k nodes in < 1 minute
- Zero browser dependencies
- 80% code coverage with unit tests
- Clean pipe interface adhering to Unix philosophy

## Final Implementation
*To be completed when the evolution is complete - will document the new lifeform that emerged from this transformation*