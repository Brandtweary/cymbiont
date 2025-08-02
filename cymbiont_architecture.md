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
│   │   └── server.rs              # Server utilities and lifecycle
│   └── storage/                   # Persistence layer
│       ├── mod.rs                 # Storage module exports
│       ├── graph_registry.rs      # Multi-graph UUID management
│       ├── transaction_log.rs     # Write-ahead logging with sled
│       └── transaction.rs         # Transaction coordination
├── data/                          # Graph persistence (configurable path)
│   ├── graph_registry.json        # Graph UUID registry
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Serialized petgraph
│   │   └── transaction_log/       # WAL database
│   └── transaction_log/           # Global transaction log
└── tests/                         # Integration tests - see tests/CLAUDE.md
    ├── common/                    # Shared test utilities
    │   ├── mod.rs                 # Test environment setup
    │   └── test_harness.rs        # TestServer lifecycle management
    └── integration/               # Integration test suite (single binary)
        ├── main.rs                # Test entry point
        ├── http_logseq_import.rs  # HTTP API tests
        ├── logseq_import.rs       # CLI import tests
        └── websocket_commands.rs  # WebSocket tests
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: CLI entry point with lifecycle management  
**Key functionality**: 
- Parse command line arguments
- Create AppState (both CLI and server modes)
- Handle shutdown signals (SIGINT/Ctrl+C)
- Execute cleanup_and_save() on exit
**CLI mode** (default): Local operations, imports, graph management
**Server mode** (--server flag): HTTP/WebSocket server via server::run_server_with_duration()

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### app_state.rs
**Purpose**: Centralized application state management  
**Key types**: `AppState` with graph managers, registry, WebSocket connections
**Methods**: `new_cli()`, `new_server()`, `get_or_create_graph_manager()`

### server/http_api.rs
**Purpose**: HTTP API endpoints for health checks, imports, and WebSocket upgrades  
**Active endpoints**:
- `GET /` - Health check
- `POST /import/logseq` - One-time Logseq graph import
- `GET /ws` - WebSocket upgrade
- `GET /api/websocket/status` - WebSocket connection metrics
- `GET /api/websocket/recent-activity` - WebSocket activity monitoring

### graph_manager.rs
**Purpose**: Core knowledge graph engine using petgraph  
**Key features**: StableGraph with nodes (Pages/Blocks), edges (relationships), JSON persistence
**Node types**: `Page { name, properties }`, `Block { uuid, content, reference_content, properties }`
**Edge types**: `PageRef`, `BlockRef`, `Tag`, `Property`, `ParentChild`, `PageToBlock`

### graph_operations.rs
**Purpose**: Standardized public interface for knowledge graph operations
**Operations**: `add_block()`, `update_block()`, `delete_block()`, `create_page()`, `delete_page()`, `create_graph()`, `delete_graph()`, `switch_graph()`, `list_graphs()`, `get_node()`

### storage/mod.rs
**Purpose**: Persistence layer module with registry, transactions, and WAL logging  
**Components**: GraphRegistry, TransactionLog, TransactionCoordinator
**Key features**: Multi-graph management, ACID transactions, crash recovery

### storage/graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management  
**Key operations**: `register_graph()`, `switch_graph()`, `remove_graph()`, registry persistence

### server/server.rs
**Purpose**: Server-specific setup and HTTP/WebSocket configuration  
**Functions**: `run_server_with_duration()` - creates and runs the axum server

### storage/transaction_log.rs
**Purpose**: Write-ahead logging with sled database  
**Features**: Content hash deduplication, ACID guarantees, crash recovery
**Trees**: Transactions, content hash index, pending index

### storage/transaction.rs
**Purpose**: Transaction lifecycle coordination  
**States**: `Active` → `Committed` | `Aborted`
**Key methods**: `execute_with_transaction()`, `begin_transaction()`, `commit_transaction()`

### server/websocket.rs
**Purpose**: Real-time WebSocket communication  
**Protocol**: Request/response with auth, heartbeat, direct command execution
**Commands**: `CreateBlock`, `UpdateBlock`, `DeleteBlock`, `CreatePage`, `DeletePage`, `SwitchGraph`, `CreateGraph`, `DeleteGraph`
**Responses**: `Success`, `Error`, `Heartbeat`

### import/logseq.rs
**Purpose**: Logseq-specific parsing and transformation  
**Key features**: Reads .md files, parses frontmatter, extracts blocks and hierarchies

### import/pkm_data.rs
**Purpose**: PKM data structures for import processing  
**Key types**: `PKMBlockData`, `PKMPageData`, `PKMReference`

### import/import_utils.rs
**Purpose**: High-level import coordination  
**Key operations**: `import_logseq_graph()` - full graph import with error collection

### import/reference_resolver.rs
**Purpose**: Block reference resolution during import  
**Key features**: Resolves `((block-id))` references, prevents circular references

### tests/common/test_harness.rs
**Purpose**: Integration test infrastructure with process lifecycle management  
**Key types**: `TestServer` - manages both server and CLI mode processes
**Key features**: Parallel test execution, isolated environments (unique ports/data dirs), phase-based testing


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
- **Client→Server**: `Auth`, `Heartbeat`, `CreateBlock`, `UpdateBlock`, `DeleteBlock`, `CreatePage`, `DeletePage`, `SwitchGraph`, `CreateGraph`, `DeleteGraph`
- **Server→Client**: `Success`, `Error`, `Heartbeat`

## Persistence Layout

Data directory configurable via `config.yaml` or `--data-dir` CLI flag:

```
{data_dir}/
├── graph_registry.json           # Graph UUID registry  
├── graphs/{graph-id}/            # Per-graph storage
│   ├── knowledge_graph.json      # Serialized petgraph
│   └── transaction_log/          # Sled WAL database
└── transaction_log/              # Global transaction log
```

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
  host: "localhost"
  port: 8888
  max_port_attempts: 10
  server_info_file: "cymbiont_server.json"  # Server discovery file (enables multi-instance)
development:
  default_duration: null          # Run duration (null = indefinite)
```

## CLI Usage

```bash
cymbiont [OPTIONS]
  --server                      # Run as HTTP/WebSocket server
  --data-dir <PATH>             # Override data directory
  --config <PATH>               # Use specific configuration file
  --import-logseq <PATH>        # Import Logseq graph directory
  --duration <SECONDS>          # Run for specific duration
```

## Graceful Shutdown

`main.rs` handles SIGINT (Ctrl+C) for graceful shutdown in both CLI and server modes, running `cleanup_and_save()` to close WebSocket connections, persist all graphs, and flush transaction logs. 

After graceful cleanup, the process uses `std::process::exit(0)` to terminate immediately due to sled database background I/O threads that cannot be cleanly shutdown (known upstream issue). This ensures reliable process termination without affecting data integrity.

## Key Flows

**Logseq Import**: HTTP POST/CLI → Path validation → .md file discovery → Frontmatter parsing → Block extraction → Reference resolution → Graph creation

**Transaction**: Operation → Content hash → WAL log → Graph update → Commit/rollback

**WebSocket**: Client auth → Direct command execution → Transaction-wrapped operation → Success/Error response

**Multi-Instance**: Configurable `server_info_file` enables concurrent server instances with isolated discovery