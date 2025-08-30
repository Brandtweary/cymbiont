# Storage Module Guide 📦

## Module Overview
WAL-based persistence layer with global transaction coordination and lazy entity loading.

## Core Components
- **wal.rs**: Sled-based WAL with operation categories (Graph/Agent/Registry)
- **transaction_coordinator.rs**: Global coordinator at `data/transaction_log/`
- **recovery.rs**: WAL rebuild and lazy entity reconstruction

## Key Patterns

### Lock Ordering 🔒
Always acquire in this order to prevent deadlocks:
1. graph_registry (async)
2. agent_registry (async)
3. Use `lock_registries_for_write()` helper

### Transaction Flow
```rust
// Normal operations - MUST explicitly commit!
let tx = coordinator.begin(Some(operation)).await?;
// Do work (any error here leaves transaction pending)
let result = do_something().await?;
tx.commit().await?;  // Critical: without this, tx stays pending

// Recovery/skip_wal - uses None for no-op handle
let tx = coordinator.begin(None).await?;
// Do recovery work
tx.commit().await?;  // No-op since ID is empty
```

**Important**: Drop trait can't async rollback, so uncommitted transactions remain pending until recovery.

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