# Cymbiont 1.0: Terminal-First Knowledge Graph Agent

## Executive Summary

Cymbiont pivots from a Logseq-integrated PKM to a terminal-first knowledge graph engine designed for AI agents. The core knowledge graph functionality remains, but we remove the complexity of browser/plugin coordination in favor of a clean Unix-style pipe utility.

## Core Vision

- **Primary Interface**: stdin/stdout for agent consumption
- **Data Format**: JSON/EDN structured data
- **TUI Dashboard**: Read-only monitoring via ratatui (no text editing)
- **Library First**: Expose as a Rust library for embedding
- **API Preserved**: Keep HTTP/WebSocket APIs for programmatic access

## Architecture Changes

### 1. Module Classification by Blast Radius

**Core (Keep As-Is)**
- `graph_manager.rs` - Petgraph-based knowledge graph engine
- `kg_api.rs` - Knowledge graph operations API
- `transaction_log.rs` - Write-ahead logging
- `transaction.rs` - Transaction coordination
- `saga.rs` - Saga pattern for workflows
- `pkm_data.rs` - Core data structures

**Adapt (Minor Changes)**
- `api.rs` - Keep HTTP endpoints, remove Logseq-specific routes
- `websocket.rs` - Keep for programmatic access, remove plugin commands
- `config.rs` - Simplify, remove Logseq configuration

**Deprecate (Remove)**
- `session_manager.rs` - No more Logseq session management
- `graph_registry.rs` - Simplified to single graph focus
- `utils.rs` - Remove Logseq launch/URL handling
- `logseq_plugin/` - Entire directory deprecated
- Integration tests - Start fresh with agent-focused tests

### 2. New Architecture

```
cymbiont/
├── src/
│   ├── lib.rs          # New library interface
│   ├── main.rs         # CLI/pipe interface
│   ├── core/           # Core KG functionality
│   │   ├── graph.rs    # (renamed from graph_manager.rs)
│   │   ├── transaction.rs
│   │   └── saga.rs
│   ├── api/            # Optional HTTP/WS APIs
│   │   ├── http.rs     # (from api.rs)
│   │   └── websocket.rs
│   ├── import/         # PKM importers
│   │   ├── logseq.rs
│   │   ├── roam.rs
│   │   └── obsidian.rs
│   └── tui/            # Ratatui dashboard
│       ├── app.rs
│       └── views/
├── tests/
│   └── unit/           # Unit tests only
└── examples/           # Agent usage examples
```

### 3. Feature Gates

```toml
[features]
default = ["cli", "tui"]
cli = []
tui = ["ratatui"]
api = ["axum", "tokio-tungstenite"]
import-logseq = []
import-roam = []
import-obsidian = []
```

### 4. Unix Pipe Interface

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

### 5. Library Interface

```rust
use cymbiont::{KnowledgeGraph, Query};

let kg = KnowledgeGraph::new();
kg.import_logseq("path/to/graph")?;

let results = kg.query(Query::Related { 
    node: "Rust".into(),
    depth: 2 
})?;
```

## Implementation Plan

### Phase 1: Deprecation & Cleanup
1. Commit current state with deprecation notices
2. Remove `logseq_plugin/` directory
3. Archive integration tests
4. Clean up dead code paths

### Phase 2: Core Refactoring
1. Create `lib.rs` with public API
2. Move modules to new structure
3. Add feature gates
4. Simplify configuration

### Phase 3: New Interfaces
1. Implement stdin/stdout pipe interface
2. Design JSON command protocol
3. Create import system framework
4. Build TUI dashboard skeleton

### Phase 4: Agent Features
1. Streaming API for real-time updates
2. Batch operations for efficiency
3. Query optimization for agents
4. Export formats (JSON, EDN, GraphML)

## Benefits

1. **Simplicity**: No browser coordination, no plugin state
2. **Testability**: Pure functions, no UI interaction
3. **Composability**: Unix philosophy, works with any tool
4. **Performance**: No WebSocket overhead for local agents
5. **Flexibility**: Can be library, CLI, or service

## Migration Path

1. Existing Logseq users can one-time import their graphs
2. API remains compatible for existing integrations
3. WebSocket available but optional
4. Focus shifts from sync to import/export

## Success Metrics

- Agent query latency < 10ms
- Import 100k nodes in < 1 minute
- Zero browser dependencies
- 90% code coverage with unit tests
- Clean pipe interface adhering to Unix philosophy