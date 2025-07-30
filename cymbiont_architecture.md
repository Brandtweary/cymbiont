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
│   ├── graph_manager.rs           # Petgraph-based knowledge graph storage
│   ├── graph_registry.rs          # Multi-graph UUID management
│   ├── transaction_log.rs         # Write-ahead logging with sled
│   ├── transaction.rs             # Transaction coordination
│   ├── saga.rs                    # Multi-step workflow patterns
│   ├── import/                    # Data import functionality
│   │   ├── mod.rs                 # Import module exports and errors
│   │   ├── pkm_data.rs            # PKM data structures
│   │   ├── logseq.rs              # Logseq-specific parsing
│   │   ├── import_utils.rs        # Import coordination
│   │   └── reference_resolver.rs  # Block reference resolution
│   └── server/                    # Server-specific functionality
│       ├── mod.rs                 # Server module exports
│       ├── api.rs                 # HTTP endpoints for data ingestion
│       ├── websocket.rs           # Real-time WebSocket communication
│       ├── kg_api.rs              # High-level graph operations API (unused)
│       └── server.rs              # Server utilities and lifecycle
├── data/                          # Graph persistence (configurable path)
│   ├── graph_registry.json        # Graph UUID registry
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Serialized petgraph
│   │   └── transaction_log/       # WAL database
│   └── transaction_log/           # Global transaction log
└── tests/                         # Integration tests with isolation
    ├── common/                    # Shared test utilities
    └── test_*.rs                  # Integration test suites
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: CLI entry point with optional server functionality  
**Key functionality**: Parse args, branch on --server flag, display graph info
**Server mode**: Delegates to server::run_server_with_duration()

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### app_state.rs
**Purpose**: Centralized application state management  
**Key types**: `AppState` with graph managers, registry, WebSocket connections
**Methods**: `new_cli()`, `new_server()`, `get_or_create_graph_manager()`

### server/api.rs
**Purpose**: HTTP API endpoints  
**Active endpoints**:
- `GET /` - Health check
- `POST /data` - PKM data ingestion (blocks, pages, batches)
- `POST /import/logseq` - Logseq graph import
- `GET /ws` - WebSocket upgrade
- `GET /api/websocket/status` - Connection metrics

### graph_manager.rs
**Purpose**: Core knowledge graph storage using petgraph  
**Key features**: StableGraph with nodes (Pages/Blocks), edges (relationships), JSON persistence
**Node types**: `Page { name, properties }`, `Block { uuid, content, reference_content, properties }`
**Edge types**: `PageRef`, `BlockRef`, `Tag`, `Property`, `ParentChild`, `PageToBlock`

### graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management  
**Key operations**: `get_or_create_graph()`, `validate_and_switch()`, `register_graph()`

### server/kg_api.rs  
**Purpose**: High-level graph operations API (currently unused - marked as dead code)
**Operations**: `add_block()`, `update_block()`, `delete_block()`, `create_page()`

### server/server.rs
**Purpose**: Server utility functions for clean main.rs separation  
**Functions**: `handle_shutdown_command()`, `run_server_with_duration()`

### transaction_log.rs
**Purpose**: Write-ahead logging with sled database  
**Features**: Content hash deduplication, ACID guarantees

### transaction.rs
**Purpose**: Transaction lifecycle coordination  
**States**: `Active` → `WaitingForAck` → `Committed`

### server/websocket.rs
**Purpose**: Real-time WebSocket communication  
**Protocol**: Auth, heartbeat, command acknowledgments
**Commands**: `create_block`, `update_block`, `delete_block`, `create_page`

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


## Data Structures

### PKMBlockData
```rust
{
    uuid: String,
    content: String,
    properties: HashMap<String, Value>,
    parent: Option<String>,
    page: String,
    left: Option<String>,
    format: String,
    created_at: i64,
    updated_at: i64
}
```

### PKMPageData
```rust
{
    name: String,
    original_name: String,
    properties: HashMap<String, Value>,
    created_at: i64,
    updated_at: i64,
    journal_day: Option<i64>
}
```

### HTTP Headers for Multi-Graph Support
- `X-Cymbiont-Graph-ID`: UUID string
- `X-Cymbiont-Graph-Name`: Graph name  
- `X-Cymbiont-Graph-Path`: Filesystem path

### WebSocket Message Types
- **Client→Server**: `auth`, `heartbeat`, acknowledgments
- **Server→Client**: `create_block`, `update_block`, `delete_block`, `create_page`

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
  port: 3000
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
  --shutdown                    # Graceful shutdown of running instance
```

## Key Flows

**Data Ingestion**: HTTP POST → Graph headers validation → Multi-graph switching → PKM parsing → Graph mutation → Auto-save

**Logseq Import**: CLI/HTTP → Path validation → .md file discovery → Frontmatter parsing → Block extraction → Reference resolution → Graph creation

**Transaction**: Operation → Content hash → WAL log → Graph update → Commit/rollback

**WebSocket**: Client auth → Command dispatch → Graph operation → Broadcast to clients

**Multi-Instance**: Configurable `server_info_file` enables concurrent server instances with isolated discovery