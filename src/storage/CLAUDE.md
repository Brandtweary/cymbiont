# Storage Module Guide 📦

## Module Overview
Persistence layer for graphs, agents, and transactions with ACID guarantees.

## Core Components

### Registries
- **graph_registry.rs**: Multi-graph UUID tracking with open/closed states
- **agent_registry.rs**: Agent lifecycle and bidirectional authorization
- **registry_utils.rs**: Shared UUID serialization helpers

### Persistence
- **graph_persistence.rs**: Graph save/load with auto-save triggers (5min/10ops)
- **agent_persistence.rs**: Agent state serialization with conversation history

### Transaction System
- **transaction_log.rs**: Sled-based WAL with SHA-256 deduplication
- **transaction.rs**: Coordinator for ACID operations and graceful shutdown

## Key Patterns

### Lock Ordering 🔒
Always acquire in this order to prevent deadlocks:
1. graph_registry (sync)
2. agent_registry (sync)
3. Use `lock_registries_for_write()` helper

### Transaction Flow
```rust
// All mutations follow this pattern
with_graph_transaction(graph_id, || {
    // 1. Log operation to WAL
    // 2. Execute business logic
    // 3. Commit or rollback
})
```

### Registry Operations
Complete workflows handle full lifecycle:
- `create_new_graph_complete()` - Create + register + authorize prime agent
- `delete_graph_complete()` - Archive + deregister + update agents
- `activate_agent_complete()` - Load + mark active + save

## Data Locations
```
data/
├── graph_registry.json       # Graph metadata 🗂️
├── agent_registry.json       # Agent metadata
├── graphs/{id}/
│   ├── knowledge_graph.json  # Serialized petgraph
│   └── transaction_log/      # Per-graph WAL
└── agents/{id}/
    └── agent.json            # Conversation + config
```

## Error Handling
All storage operations return `Result<T>` with domain-specific errors:
- `StorageError::not_found("graph", "id", id)`
- `StorageError::serialization(e)`
- `StorageError::io(e)`

## Testing Helpers
- `create_test_registry()` - In-memory registry for tests
- `temp_data_dir()` - Isolated test directories
- Transaction rollback on test failure

## Recovery Mechanism 🔄
1. Startup: `run_all_graphs_recovery()` for ALL graphs
2. Graph open: Replay pending transactions
3. Operations store full API params for exact replay
4. States: Active → Committed | Aborted