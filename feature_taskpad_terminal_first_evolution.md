# Feature Taskpad: Terminal-First Knowledge Graph Evolution

## Feature Description
Transform Cymbiont from a Logseq-integrated PKM synchronization tool into a terminal-first knowledge graph engine designed for AI agents. This evolution preserves the core knowledge graph functionality while shedding the complexity of browser/plugin coordination, emerging as a clean, composable Unix-style utility that can be embedded, piped, or served.

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
- `src/session_manager.rs`: Currently manages Logseq sessions - deprecate entirely
- `src/graph_registry.rs`: Multi-graph registry - simplify to single graph focus
- `src/utils.rs`: Mixed utilities - extract useful parts, remove Logseq launches
- `src/edn.rs`: EDN manipulation - might be useful for import functionality

### New Growth Areas
- `src/lib.rs`: Public library interface (to be created)
- `src/import/`: PKM importers for Logseq/Roam/Obsidian
- `src/tui/`: Terminal UI dashboard with ratatui
- `src/cli/`: Command-line interface and pipe protocol

## Development Plan

### 1. Merge Resolution & State Stabilization
- [ ] Preserve `feature_taskpad_aichat_agent_integration.md` historical context
- [ ] Complete the merge by accepting the architectural pivot
- [ ] Update CLAUDE.local.md to reflect evolution (not crisis)
- [ ] Tag pre-evolution state for historical reference

### 2. Deprecation Wave
- [ ] Add deprecation headers to `session_manager.rs`
- [ ] Mark `logseq_plugin/` directory as deprecated
- [ ] Update integration tests with deprecation notices
- [ ] Document deprecation timeline in README

### 3. Core Extraction & Library Design
- [ ] Create `src/lib.rs` with public API surface
- [ ] Design `KnowledgeGraph` struct and core traits
- [ ] Extract core modules into `src/core/` subdirectory
- [ ] Implement feature gates in `Cargo.toml`

### 4. Interface Evolution
- [ ] Design stdin/stdout JSON command protocol
- [ ] Create CLI argument parser and command router
- [ ] Implement pipe-friendly output formats
- [ ] Add batch operation support for efficiency

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
- [ ] Remove Logseq-specific HTTP endpoints
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

### 10. Final Cleanup & Optimization
- [ ] Remove all dead code paths
- [ ] Optimize for sub-10ms query performance
- [ ] Minimize binary size with feature gates
- [ ] Profile and eliminate bottlenecks

## Development Notes

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

## Final Implementation
*To be completed when the evolution is complete - will document the new lifeform that emerged from this transformation*