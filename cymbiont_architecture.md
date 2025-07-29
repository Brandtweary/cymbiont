# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Core knowledge graph engine
│   ├── main.rs                    # Server entry point and lifecycle
│   ├── config.rs                  # YAML configuration management
│   ├── logging.rs                 # Custom tracing formatter
│   ├── api.rs                     # HTTP endpoints for data ingestion
│   ├── utils.rs                   # Process management and utilities
│   ├── graph_manager.rs           # Petgraph-based knowledge graph storage
│   ├── graph_registry.rs          # Multi-graph UUID management
│   ├── pkm_data.rs                # PKM data structures
│   ├── websocket.rs               # Real-time WebSocket communication
│   ├── kg_api.rs                  # High-level graph operations API (unused)
│   ├── transaction_log.rs         # Write-ahead logging with sled
│   ├── transaction.rs             # Transaction coordination
│   ├── saga.rs                    # Multi-step workflow patterns
│   └── lib.rs                     # Library interface
├── data/                          # Graph persistence (configurable path)
│   ├── graph_registry.json        # Graph UUID registry
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Serialized petgraph
│   │   └── transaction_log/       # WAL database
│   └── transaction_log/           # Global transaction log
└── tests/                         # Test suite (TODO: need to add integration tests)
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: Server entry point and lifecycle management  
**Key functionality**: AppState initialization, HTTP server startup
**AppState**: `{ graph_managers: HashMap<String, RwLock<GraphManager>>, active_graph_id }`

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### api.rs
**Purpose**: HTTP API endpoints  
**Active endpoints**:
- `GET /` - Health check
- `POST /data` - PKM data ingestion (blocks, pages, batches)
- `GET /ws` - WebSocket upgrade
- `GET /api/websocket/status` - Connection metrics

### graph_manager.rs
**Purpose**: Core knowledge graph storage using petgraph  
**Key features**: StableGraph with nodes (Pages/Blocks), edges (relationships), JSON persistence
**Node types**: `Page { name, properties }`, `Block { uuid, content, properties }`
**Edge types**: `PageRef`, `BlockRef`, `Tag`, `Property`, `ParentChild`, `PageToBlock`

### graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management  
**Key operations**: `get_or_create_graph()`, `validate_and_switch()`, `register_graph()`

### kg_api.rs  
**Purpose**: High-level graph operations API (currently unused - marked as dead code)
**Operations**: `add_block()`, `update_block()`, `delete_block()`, `create_page()`

### transaction_log.rs
**Purpose**: Write-ahead logging with sled database  
**Features**: Content hash deduplication, ACID guarantees

### transaction.rs
**Purpose**: Transaction lifecycle coordination  
**States**: `Active` → `WaitingForAck` → `Committed`

### websocket.rs
**Purpose**: Real-time WebSocket communication  
**Protocol**: Auth, heartbeat, command acknowledgments
**Commands**: `create_block`, `update_block`, `delete_block`, `create_page`

### lib.rs
**Purpose**: Library interface exposing core modules
**Exports**: `GraphManager`, `PKMBlockData`, `PKMPageData`, transaction modules

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
development:
  default_duration: null          # Run duration (null = indefinite)
```

## CLI Usage

```bash
cargo run [OPTIONS]
  --duration <SECONDS>           # Run for specific duration  
  --data-dir <PATH>             # Override data directory
  --shutdown-server             # Graceful shutdown
```

## Key Flows

**Data Ingestion**: HTTP POST → Graph headers validation → Multi-graph switching → PKM parsing → Graph mutation → Auto-save

**Transaction**: Operation → Content hash → WAL log → Graph update → Commit/rollback

**WebSocket**: Client auth → Command dispatch → Graph operation → Broadcast to clients