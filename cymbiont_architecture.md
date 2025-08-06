# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Core knowledge graph engine
│   ├── main.rs                    # CLI entry point with --server flag
│   ├── app_state.rs               # Centralized application state
│   ├── config.rs                  # YAML configuration management
│   ├── logging.rs                 # Custom tracing formatter
│   ├── utils.rs                   # Process management and utilities
│   ├── graph_manager.rs           # Petgraph-based knowledge graph engine
│   ├── graph_operations.rs        # Public interface for graph operations
│   ├── import/                    # Data import functionality
│   │   ├── mod.rs                 # Import module exports and errors
│   │   ├── pkm_data.rs            # PKM data structures
│   │   ├── logseq.rs              # Logseq-specific parsing
│   │   ├── import_utils.rs        # Import coordination
│   │   └── reference_resolver.rs  # Block reference resolution
│   ├── server/                    # Server-specific functionality
│   │   ├── mod.rs                 # Server module exports
│   │   ├── http_api.rs            # HTTP endpoints (health, import, WebSocket upgrade)
│   │   ├── websocket.rs           # Real-time WebSocket communication
│   │   ├── server.rs              # Server utilities and lifecycle
│   │   └── auth.rs                # Authentication system with token management
│   └── storage/                   # Persistence layer
│       ├── mod.rs                 # Storage module exports
│       ├── graph_persistence.rs   # Graph save/load utilities
│       ├── graph_registry.rs      # Multi-graph UUID management
│       ├── transaction_log.rs     # Write-ahead logging with sled
│       └── transaction.rs         # Transaction coordination
├── data/                          # Graph persistence (configurable path)
│   ├── graph_registry.json        # Graph UUID registry
│   ├── auth_token                 # Authentication token (auto-generated)
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Serialized petgraph
│   │   └── transaction_log/       # Per-graph WAL database
│   └── archived_graphs/           # Deleted graphs archive
└── tests/                         # Integration tests - see tests/CLAUDE.md
    ├── common/                    # Shared test utilities
    │   ├── mod.rs                 # Test environment setup
    │   ├── test_harness.rs        # TestServer lifecycle management
    │   └── graph_validation.rs    # Automated graph state validation
    └── integration/               # Integration test suite (single binary)
        ├── main.rs                # Test entry point
        ├── crash_recovery.rs      # Transaction recovery tests
        ├── http_logseq_import.rs  # HTTP API tests
        ├── logseq_import.rs       # CLI import tests
        └── websocket_commands.rs  # WebSocket tests
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: CLI entry point with unified runtime lifecycle management  
**Key functionality**: 
- Parse command line arguments
- Create AppState (both CLI and server modes)
- Run `run_all_graphs_recovery()` on startup for all open graphs
- Handle duration limits and shutdown signals uniformly for both modes
- Execute cleanup_and_save() on exit
**Runtime behavior**: Controls duration timeout and graceful shutdown for both CLI and server modes
**Server mode**: Starts server via `server::start_server()` but manages lifecycle in main.rs

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### app_state.rs
**Purpose**: Centralized application state management and coordination  
**Key types**: `AppState` - coordinates graph managers, registry, transactions, and WebSocket connections
**Key methods**: 
- `new_cli()`, `new_server()` - initialization for different modes
- `get_or_create_graph_manager()` - lazy graph manager creation
- `with_graph_transaction(graph_id)` - wraps operations in transactions for specific graph
- `initiate_graceful_shutdown()`, `wait_for_transactions()` - shutdown coordination
- `get_transaction_coordinator()` - access to per-graph WAL
**Role**: Acts as the central nervous system, connecting all components without implementing business logic

### server/http_api.rs
**Purpose**: HTTP API endpoints for health checks, imports, and WebSocket upgrades  
**Active endpoints**:
- `GET /` - Health check (no auth)
- `POST /import/logseq` - One-time Logseq graph import (requires auth)
- `GET /ws` - WebSocket upgrade (no auth, handled post-upgrade)
- `GET /api/websocket/status` - WebSocket connection metrics (requires auth)
- `GET /api/websocket/recent-activity` - WebSocket activity monitoring (requires auth)

### graph_manager.rs
**Purpose**: Generic knowledge graph storage engine using petgraph  
**Key features**: Domain-agnostic graph operations, StableGraph for index stability, automatic persistence
**Operations**: `create_node()`, `create_or_update_node()`, `find_node()`, `add_edge()`, `archive_nodes()`
**Node/Edge types**: Defined by domain layer (e.g., PKM defines Page/Block nodes, PageRef/BlockRef edges)

### graph_operations.rs
**Purpose**: PKM-specific operations as an extension trait on Arc<AppState>  
**Design**: Extension trait pattern (`GraphOperationsExt`) provides PKM operations directly on AppState
**Key role**: Adds domain-specific graph operations with full transaction support and crash recovery
**Operations**: All operations now require explicit `graph_id: &Uuid` parameter:
- `add_block()`, `update_block()`, `delete_block()` - block CRUD operations
- `create_page()`, `delete_page()` - page management
- `create_graph()`, `delete_graph()` - graph lifecycle
- `open_graph()`, `close_graph()` - replaces switch_graph() with explicit resource management
- `list_graphs()`, `list_open_graphs()` - graph enumeration
- `get_node()`, `replay_transaction()` - query and recovery
**Transaction integration**: Each operation stores full API parameters in WAL for perfect recovery
**Note**: Not a separate service - these are extension methods on Arc<AppState>

### storage/mod.rs
**Purpose**: Persistence layer module with registry, transactions, and WAL logging  
**Components**: GraphRegistry, TransactionLog, TransactionCoordinator, graph_persistence utilities
**Key features**: Multi-graph management, ACID transactions, crash recovery, graph serialization

### storage/graph_persistence.rs
**Purpose**: Graph serialization and persistence utilities  
**Key operations**: `load_graph()`, `save_graph()`, `archive_nodes()`, `should_save()`
**Features**: JSON serialization, auto-save thresholds, node archival

### storage/graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management with open/closed state  
**Key types**: Uses `Uuid` type throughout with custom JSON serialization
**Concurrency**: Uses `Arc<RwLock<GraphRegistry>>` with development-time contention detection
**Key operations**: 
- `register_graph()`, `remove_graph()` - graph lifecycle
- `open_graph()`, `close_graph()` - explicit state management (replaces switch_graph)
- `get_open_graphs()`, `is_graph_open()` - query graph states
- `resolve_graph_target()` - centralized UUID/name resolution with smart defaults
- `ensure_graph_open()` - startup logic to guarantee at least one open graph
**Data structure**: Tracks `open_graphs: HashSet<Uuid>` instead of single active_graph_id
**Persistence**: Open graph state persists across restarts for automatic recovery
**Safety pattern**: Write operations use `debug_assert!(registry.try_write().is_ok())` as tripwires to detect lock contention during development. These can be removed after profiling if some contention is acceptable, but never preemptively.

### server/server.rs
**Purpose**: Server initialization and HTTP/WebSocket setup  
**Functions**: `start_server()` - creates axum server and returns handle for external lifecycle management

### storage/transaction_log.rs
**Purpose**: Write-ahead logging with sled database  
**Features**: Content hash deduplication, ACID guarantees, crash recovery
**Trees**: Transactions, content hash index, pending index

### storage/transaction.rs
**Purpose**: Transaction lifecycle coordination with graceful shutdown support  
**States**: `Active` → `Committed` | `Aborted`
**Key methods**: 
- `create_transaction()`, `complete_transaction()` - transaction lifecycle
- `recover_pending_transactions()` - crash recovery
- `initiate_shutdown()`, `wait_for_completion()` - graceful shutdown coordination
**Per-graph isolation**: Each graph has its own TransactionCoordinator instance
**Shutdown behavior**: Tracks all active transactions, rejects new ones during shutdown

### server/websocket.rs
**Purpose**: Real-time WebSocket communication with high-throughput async processing  
**Architecture**: Each command spawns as independent async task for concurrent execution
**Protocol**: Request/response with token auth, heartbeat, async command execution
**Commands**: 
- `Auth { token }` - authentication
- `OpenGraph`, `CloseGraph` - explicit graph lifecycle (replaces SwitchGraph)
- `CreateBlock`, `UpdateBlock`, `DeleteBlock` - block operations (now accept optional graph_id/graph_name)
- `CreatePage`, `DeletePage` - page operations (now accept optional graph_id/graph_name)
- `CreateGraph`, `DeleteGraph` - graph management
- `FreezeOperations`, `UnfreezeOperations` - test infrastructure
**Graph targeting**: All CRUD commands accept optional `graph_id` (UUID string) or `graph_name` fields
**Responses**: `Success`, `Error`, `Heartbeat`
**Authentication**: Requires `Auth { token }` command before other operations
**Performance**: Supports high-throughput scenarios with multiple concurrent operations

### import/logseq.rs
**Purpose**: Logseq-specific parsing and transformation  
**Key features**: Reads .md files, parses frontmatter, extracts blocks and hierarchies

### import/pkm_data.rs
**Purpose**: PKM data structures and graph application logic  
**Key types**: `PKMBlockData`, `PKMPageData`, `PKMReference`
**Key methods**: `apply_to_graph()` - Transforms PKM data into graph nodes/edges with reference resolution

### import/import_utils.rs
**Purpose**: High-level import coordination  
**Key operations**: `import_logseq_graph()` - full graph import with error collection

### import/reference_resolver.rs
**Purpose**: Block reference resolution during import  
**Key features**: Resolves `((block-id))` references, prevents circular references

### server/auth.rs
**Purpose**: Token-based authentication system with auto-generation and rotation  
**Key features**: 
- Auto-generates cryptographically secure tokens on startup
- Saves token to `{data_dir}/auth_token` with restricted permissions (0600)
- Token rotation on each server restart for enhanced security
- HTTP middleware for protecting sensitive endpoints
- WebSocket authentication via `Auth { token }` command
- Optional config overrides (fixed token or disabled auth)

### tests/common/test_harness.rs
**Purpose**: Integration test infrastructure with process lifecycle management  
**Key types**: `TestServer` - manages both server and CLI mode processes
**Key features**: Parallel test execution, isolated environments (unique ports/data dirs), phase-based testing

### tests/common/graph_validation.rs
**Purpose**: Automated graph state validation for integration tests  
**Key types**: `GraphValidationFixture` - tracks expected graph transformations and validates final state
**Key methods**: 
- `expect_dummy_graph()` - sets up expectations for imported test data
- `expect_create_block()`, `expect_update_block()`, `expect_delete()` - track node operations
- `expect_edge()` - validate custom relationships (ParentChild, PageToBlock, etc.)
- `validate_graph()` - checks all expectations against actual persisted graph
**Benefits**: Eliminates manual assertions, reduces test brittleness, comprehensive edge validation


## Data Structures

### PKMBlockData
```rust
{
    id: String,
    content: String,
    created: String,
    updated: String,
    parent: Option<String>,
    children: Vec<String>,
    page: Option<String>,
    properties: serde_json::Value,
    references: Vec<PKMReference>,
    reference_content: Option<String>
}
```

### PKMPageData
```rust
{
    name: String,
    normalized_name: Option<String>,
    created: String,
    updated: String,
    properties: serde_json::Value,
    blocks: Vec<String>
}
```

### WebSocket Message Types
- **Client→Server**: 
  - `Auth { token }` - authentication
  - `OpenGraph { graph_id?, graph_name? }`, `CloseGraph { graph_id?, graph_name? }` - graph lifecycle
  - `CreateBlock { ..., graph_id?, graph_name? }`, `UpdateBlock { ..., graph_id?, graph_name? }`, `DeleteBlock { ..., graph_id?, graph_name? }` - block operations
  - `CreatePage { ..., graph_id?, graph_name? }`, `DeletePage { ..., graph_id?, graph_name? }` - page operations
  - `CreateGraph`, `DeleteGraph` - graph management
  - `FreezeOperations`, `UnfreezeOperations`, `GetFreezeState` - test infrastructure
  - `Heartbeat` - connection keep-alive
- **Server→Client**: `Success { data? }`, `Error { message }`, `Heartbeat`
- **Processing**: Commands execute asynchronously as independent tasks for high-throughput performance

### Graph Registry Format
```json
{
  "graphs": [
    {"id": "uuid", "name": "graph-name", "path": "/path", "created_at": "...", "last_accessed": "..."}
  ]
}
```

## Configuration

```yaml
data_dir: data                    # Storage directory
backend:
  port: 8888                      # Base HTTP server port
  max_port_attempts: 10           # Port search range if base port is busy
  server_info_file: "cymbiont_server.json"  # Server discovery file (enables multi-instance)
development:
  default_duration: 3             # Auto-exit after 3 seconds (set to null for production)
auth:                             # Authentication configuration
  token: null                     # Fixed token (auto-generated if null)
  disabled: false                 # Disable auth entirely (not recommended)
transaction_log:                  # WAL configuration
  fsync_interval_ms: 100          # Durability flush interval
  compaction_threshold_mb: 100    # Size trigger for log compaction
  retention_days: 7               # Keep completed transactions for N days
  redundant_copies: 10            # Byzantine fault tolerance copies
  integrity_check_on_startup: true # Auto-repair via consensus
```

## CLI Usage

```bash
cymbiont [OPTIONS]
  --server                      # Run as HTTP/WebSocket server
  --data-dir <PATH>             # Override data directory
  --config <PATH>               # Use specific configuration file
  --import-logseq <PATH>        # Import Logseq graph directory
  --delete-graph <NAME_OR_ID>   # Delete a graph by name or UUID
  --duration <SECONDS>          # Run for specific duration
```

## Graceful Shutdown

`main.rs` handles SIGINT (Ctrl+C) for graceful shutdown in both CLI and server modes:
- First Ctrl+C: Initiates graceful shutdown, waits up to 30 seconds for active transactions to complete
- Second Ctrl+C: Forces immediate termination with transaction log flush

The shutdown sequence runs `cleanup_and_save()` to close WebSocket connections, persist all graphs, and flush transaction logs. After graceful cleanup, the process uses `std::process::exit(0)` to terminate immediately due to sled database background I/O threads that cannot be cleanly shutdown (known upstream issue).

## Key Flows

**Logseq Import**: HTTP POST/CLI → Path validation → .md file discovery → Frontmatter parsing → Block extraction → Reference resolution → Graph creation

**Transaction**: Operation → Content hash → WAL log → Graph update → Commit/rollback

**WebSocket**: Client auth → Async command execution (spawned tasks) → Transaction-wrapped operation → Success/Error response

**Multi-Instance**: Configurable `server_info_file` enables concurrent server instances with isolated discovery

**Authentication**: Server generates auth token on startup, saves to `{data_dir}/auth_token`. HTTP endpoints check Authorization header, WebSocket requires Auth command. Token rotates on restart for security.

**Crash Recovery**: 
1. On startup (main.rs): Runs `run_all_graphs_recovery()` for ALL graphs (both open and closed)
   - Iterates through every registered graph
   - Temporarily opens closed graphs for recovery
   - Closes them again after recovery completes
2. On graph open: `open_graph()` triggers recovery for that specific graph
3. Recovery mechanism: Operations store full API parameters, replay calls exact same methods
4. Transaction states: Active → Committed (success) or Aborted (failure)
5. Open graphs persist across restarts: Registry tracks which graphs were open for automatic recovery