# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Backend server modules
│   ├── main.rs                    # Entry point, server lifecycle, graph orchestration
│   ├── config.rs                  # YAML config loading and validation
│   ├── logging.rs                 # Tracing formatter (file:line for ERROR/WARN only)
│   ├── api.rs                     # HTTP endpoints, request/response types
│   ├── utils.rs                   # Process management, datetime parsing, JSON helpers
│   ├── graph_manager.rs           # Petgraph storage, node/edge operations
│   ├── graph_registry.rs          # Multi-graph UUID tracking and switching
│   ├── pkm_data.rs                # PKMBlockData, PKMPageData structures
│   ├── websocket.rs               # WebSocket server, command protocol
│   ├── kg_api.rs                  # Transaction-safe graph operations
│   ├── transaction_log.rs         # WAL with sled, content hash indexing
│   ├── transaction.rs             # Transaction lifecycle and state machine
│   ├── saga.rs                    # Multi-step workflow coordination
│   ├── session_manager.rs         # Logseq database session management
│   └── edn.rs                     # EDN format manipulation for config.edn
├── logseq_plugin/
│   ├── index.js                   # Plugin lifecycle, graph identification, DB monitoring
│   ├── sync.js                    # Incremental/full sync orchestration
│   ├── api.js                     # HTTP/WebSocket client (window.KnowledgeGraphAPI)
│   ├── data_processor.js          # Data validation and normalization
│   └── websocket.js               # Command handlers (window.KnowledgeGraphWebSocket)
├── logseq_databases/              # Test graphs
├── data/                          # Persistence layer (configurable via data_dir)
│   ├── graph_registry.json        # Graph UUID mappings
│   ├── last_session.json          # Session persistence
│   ├── archived_nodes/            # Global deletion archives
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Graph serialization
│   │   ├── archived_nodes/        # Per-graph deletion archives
│   │   └── transaction_log/       # Per-graph WAL (sled database)
│   ├── saga_transaction_log/      # Global saga coordination (sled database)
│   └── transaction_log/           # Global transaction log (sled database)
└── tests/                         # Integration tests
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: Server entry point and lifecycle orchestration  
**Manages**: AppState, server runtime, graph coordination, session management  
**Requires**: All modules via AppState injection  
**Data flow**:
```
CLI args → Config loading → AppState init → Session manager → Logseq launch → HTTP server → Graph registry → Active graph selection
```
**Key types**: `AppState { graph_managers: HashMap<String, RwLock<GraphManager>>, websocket, active_graph_id, session_manager }`

### config.rs
**Purpose**: Configuration management  
**Manages**: Config struct hierarchy, data directory configuration  
**Requires**: serde, config crate  
**Data flow**:
```
config.yaml → Config struct → CLI overrides → Validation → AppState.config → Data directory resolution
```
**Key types**: `Config`, `BackendConfig`, `LogseqConfig`, `SyncConfig`, `DevelopmentConfig`

### api.rs
**Purpose**: HTTP endpoint handlers  
**Manages**: Router configuration, endpoint logic  
**Requires**: GraphManager, TransactionCoordinator, EDN module  
**Data flow**:
```
HTTP request → Graph validation (headers) → Handler → Graph operation → Response
```
**Endpoints**:
- `GET /` - Health check
- `POST /data` - PKMData ingestion (type-based routing)
- `POST /plugin/initialized` - Graph registration with UUID
- `GET /sync/status` - Sync timing and graph stats
- `PATCH /sync` - Update sync timestamps
- `POST /sync/verify` - Deletion detection
- `POST /config/validate` - EDN property validation
- `POST /log` - Plugin logging
- `GET /ws` - WebSocket upgrade
- `POST /api/session/switch` - Switch to different graph
- `GET /api/session/current` - Get current session info
- `GET /api/session/databases` - List all registered databases

### graph_manager.rs
**Purpose**: Per-graph petgraph storage  
**Manages**: StableGraph, node/edge operations, persistence  
**Requires**: petgraph, serde_json  
**Data flow**:
```
PKM operation → NodeIndex lookup → Graph mutation → Auto-save trigger → JSON persistence
```
**Key structures**:
- Nodes: `Page { name, properties }`, `Block { uuid, content, properties }`
- Edges: `PageRef`, `BlockRef`, `Tag`, `Property`, `ParentChild`, `PageToBlock`
- Indexes: `HashMap<String, NodeIndex>` for O(1) lookups

### graph_registry.rs
**Purpose**: Multi-graph identification  
**Manages**: Graph UUID mappings, active graph tracking, configurable storage paths  
**Requires**: File I/O for registry persistence, data directory configuration  
**Data flow**:
```
Plugin headers → Registry lookup → Graph creation/validation → Data directory path resolution → Active graph switch
```
**Key operations**: `get_or_create_graph()`, `validate_and_switch()`, `register_graph()`

### kg_api.rs
**Purpose**: Public API for graph mutations  
**Manages**: Transaction-safe operation wrappers  
**Requires**: GraphManager, TransactionCoordinator, WebSocket  
**Data flow**:
```
Operation request → Transaction begin → Content hash → Graph mutation → WAL → WebSocket broadcast → Ack wait → Commit
```
**Operations**: `add_block()`, `update_block()`, `delete_block()`, `create_page()`

### transaction_log.rs
**Purpose**: Write-ahead logging  
**Manages**: Sled database, content hash index  
**Requires**: sled, sha2  
**Data flow**:
```
Transaction → Serialize → WAL append → Content hash index → Pending queue
```
**Key types**: `TransactionLog`, `TransactionEntry`, `ContentHashIndex`

### transaction.rs
**Purpose**: Transaction state machine  
**Manages**: Transaction lifecycle, acknowledgment correlation  
**Requires**: TransactionLog integration  
**States**: `Active` → `WaitingForAck` → `Committed`

### session_manager.rs
**Purpose**: Logseq database session management  
**Manages**: Graph launch, switching, session persistence, graph registration  
**Requires**: GraphRegistry, URL scheme invocation, configurable data directory  
**Data flow**:
```
CLI/API request → Resolve name/path → Open logseq://graph/{name} → Wait for confirmation → Update state
```
**Key features**:
- Launch with `--graph` or `--graph-path` CLI args
- Platform-specific URL opening (Linux/macOS/Windows)
- WebSocket confirmation mechanism with timeout
- Session persistence in `{data_dir}/last_session.json`
- Graph registration with configurable data directory paths
**API endpoints**:
- `POST /api/session/switch` - Switch to different graph
- `GET /api/session/current` - Get current session info
- `GET /api/session/databases` - List all registered databases

### websocket.rs
**Purpose**: Bidirectional communication  
**Manages**: Connection management, command routing  
**Requires**: tokio, authentication state  
**Data flow**:
```
Client connect → Auth command → Authenticated state → Command dispatch → Broadcast to clients
```
**Commands**:
- Client→Server: `auth`, `heartbeat`, `test`, `graph_switch_confirmed`, acknowledgments
- Server→Client: `create_block`, `update_block`, `delete_block`, `create_page`, `graph_switch_requested`

### edn.rs
**Purpose**: EDN format manipulation  
**Manages**: Regex-based property updates  
**Requires**: regex crate  
**Key functions**:
- `update_block_hidden_properties()` - Add to #{} set
- `update_graph_id()` - Set UUID property
- `validate_config_properties()` - Check required properties
- `update_config_file()` - Apply changes with validation

### Plugin: index.js
**Purpose**: Logseq plugin lifecycle  
**Manages**: Plugin initialization, DB change monitoring  
**Requires**: logseq API, other plugin modules  
**Data flow**:
```
Plugin load → Graph identification → Backend registration → DB.onChanged → Batch queue → Backend sync
```
**Key operations**: Graph UUID stamping, config validation, timestamp updates

### Plugin: sync.js
**Purpose**: Sync orchestration  
**Manages**: Incremental/full sync logic  
**Requires**: api.js, data_processor.js  
**Data flow**:
```
Sync timer → Status check → Page/block query → Filter by timestamp → Batch send → Deletion verify
```
**Sync types**: Real-time (immediate), Incremental (2hr), Full (7d, disabled)

### Plugin: api.js (window.KnowledgeGraphAPI)
**Purpose**: Backend communication  
**Manages**: HTTP client, WebSocket client  
**Requires**: Port discovery, retry logic  
**Key functions**:
- `sendToBackend()` - POST to /data
- `checkBackendAvailabilityWithRetry()` - Health checks
- `websocket.connect/send/registerHandler()` - WebSocket ops

### Plugin: websocket.js (window.KnowledgeGraphWebSocket)
**Purpose**: Command handlers  
**Manages**: Logseq API command execution  
**Requires**: logseq API, correlation tracking  
**Handlers**: `create_block`, `update_block`, `delete_block`, `create_page`

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

### Graph Headers (HTTP)
- `X-Cymbiont-Graph-ID`: UUID string
- `X-Cymbiont-Graph-Name`: Graph name
- `X-Cymbiont-Graph-Path`: Filesystem path

### WebSocket Protocol
```json
// Client → Server
{"type": "auth", "token": "dummy"}
{"type": "heartbeat"}
{"type": "BlockCreated", "correlation_id": "...", "uuid": "..."}
{"type": "graph_switch_confirmed", "graph_id": "...", "graph_name": "...", "graph_path": "..."}

// Server → Client
{"type": "create_block", "correlation_id": "...", "temp_id": "...", "content": "..."}
{"type": "update_block", "correlation_id": "...", "uuid": "...", "content": "..."}
{"type": "graph_switch_requested", "target_graph_id": "...", "target_graph_name": "...", "target_graph_path": "..."}
```

## Persistence Layout

### Configurable Data Directory
The data directory is configurable via `config.yaml` (`data_dir` field) or CLI `--data-dir` override.
Default structure under data directory:

### Complete Data Directory Structure
```
{data_dir}/
├── graph_registry.json           # Registry of all graphs with UUIDs
├── last_session.json             # Session persistence and active graph tracking
├── archived_nodes/               # Global deletion archives
│   └── archive_YYYYMMDD_HHMMSS.json
├── graphs/{graph-id}/            # Per-graph isolated storage
│   ├── knowledge_graph.json      # Full graph serialization (petgraph)
│   ├── archived_nodes/           # Per-graph deletion archives
│   │   └── archive_YYYYMMDD_HHMMSS.json
│   └── transaction_log/          # Per-graph WAL (sled database)
│       ├── blobs/               # Sled blob storage
│       ├── conf                 # Sled configuration
│       ├── db                   # Sled main database file
│       └── snap.*               # Sled snapshots
├── saga_transaction_log/         # Global saga coordination (sled database)
│   ├── blobs/
│   ├── conf
│   ├── db
│   └── snap.*
└── transaction_log/              # Global transaction log (sled database)
    ├── blobs/
    ├── conf
    ├── db
    └── snap.*
```

### Graph Registry
```json
{
  "graphs": [
    {
      "id": "uuid",
      "name": "graph-name",
      "path": "/path/to/graph",
      "created_at": "2025-01-20T10:00:00Z",
      "last_accessed": "2025-01-20T10:00:00Z",
      "config_updated": true
    }
  ]
}
```

## Configuration Schema

### config.yaml
```yaml
# Data storage directory (configurable)
data_dir: data

backend:
  host: "localhost"
  port: 3000
  max_port_attempts: 10

sync:
  incremental_interval_hours: 2
  full_interval_hours: 168
  enable_full_sync: false

logseq:
  auto_launch: false
  executable_path: null

development:
  default_duration: null
```

## CLI Interface
```bash
cargo run [OPTIONS]
  --duration <SECONDS>           Run for specific duration
  --force-incremental-sync       Force incremental sync
  --force-full-sync             Force full sync
  --test-websocket <COMMAND>    Test WebSocket commands
  --graph <NAME>                Launch with specific graph by name
  --graph-path <PATH>           Launch with specific graph by path
  --data-dir <PATH>             Override data directory (defaults to config value)
  --shutdown-server             Shutdown running Cymbiont server gracefully
```

## Testing Entry Points
- `cargo test` - Rust unit tests
- `npm test` (in logseq_plugin/) - JavaScript tests
- `RUST_LOG=debug cargo run` - Debug logging
- Test graphs in `logseq_databases/`

## Transaction Flow
```
1. Operation requested (HTTP/WebSocket)
2. Transaction created with content hash
3. Check pending operations by hash
4. Apply to graph (mutation)
5. Log to WAL
6. Broadcast via WebSocket
7. Wait for acknowledgment (timeout: 30s)
8. Commit or rollback
```

## Graph Update Flow
```
1. PKMData received with graph headers
2. Validate graph ID/name/path
3. Switch active graph if needed
4. Parse payload by type
5. Update graph nodes/edges
6. Trigger auto-save if threshold met
```

## Sync Status Tracking
- Incremental: `last_incremental_sync` timestamp
- Full: `last_full_sync` timestamp
- Force flags override time-based checks
- Per-graph isolation of sync state

## Error Handling Patterns
- Module-specific `Error` enums with thiserror
- `type Result<T> = std::result::Result<T, Error>` aliases
- HTTP errors return `ApiResponse { success: false, message }`
- WebSocket errors trigger reconnection with backoff

## Process Management
- Server info written to `cymbiont_server.json`
- Previous instance terminated via PID
- Port discovery on conflict (3000-3010)
- Graceful shutdown with 10s timeout

## Development Constraints
- No debug logs in production code
- Dead code must be removed (no `_` prefixes)
- Server runs with default 3s duration
- Plugin requires backend running first