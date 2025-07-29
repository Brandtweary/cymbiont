# Cymbiont Architecture

> **🚀 ARCHITECTURAL EVOLUTION IN PROGRESS**: Cymbiont is transforming into a terminal-first knowledge graph engine for AI agents. This represents a natural evolution from browser-coupled to composable Unix utility.
> 
> See [CYMBIONT_1.0_PLAN.md](CYMBIONT_1.0_PLAN.md) for the vision and [feature_taskpad_terminal_first_evolution.md](feature_taskpad_terminal_first_evolution.md) for the implementation roadmap.

## Active Evolution Status

### Current Metamorphosis
- **Branch State**: Merging `aichat-integration` → `main` with architectural evolution
- **Transformation Progress**: Integration test framework added, preparing for terminal-first rebirth
- **Pending Resolution**: Historical context preservation for `feature_taskpad_aichat_agent_integration.md`

### Evolution Coordination
Active development is being tracked in the [Terminal-First Evolution Taskpad](feature_taskpad_terminal_first_evolution.md). Multiple aspects of the transformation are proceeding in parallel:

1. **Core Preservation**: Knowledge graph engine remains the beating heart
2. **Interface Evolution**: From browser plugin to Unix pipe utility  
3. **API Adaptation**: HTTP/WebSocket endpoints evolving for agent consumption
4. **Import Capability**: One-way PKM import replacing bidirectional sync
5. **Library Emergence**: Public Rust API for embedding in agent systems

---

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
- `GET /api/session/current` - Get current session info (includes WebSocket status)
- `GET /api/session/databases` - List all registered databases
- `GET /api/websocket/status` - WebSocket connection status and metrics
- `GET /api/websocket/recent-activity` - Recent WebSocket commands and confirmations

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
- `GET /api/session/current` - Get current session info (includes WebSocket status)
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
**Verification endpoints**:
- `GET /api/websocket/status` - Connection health and metrics
- `GET /api/websocket/recent-activity` - Command/confirmation history

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
- `getWebSocketStatus()` - GET /api/websocket/status
- `getWebSocketActivity()` - GET /api/websocket/recent-activity
- `getCurrentSession()` - GET /api/session/current (enhanced)

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

## API Endpoint Documentation

When adding new endpoints, update documentation in: `cymbiont_architecture.md`, `src/api.rs`, and `logseq_plugin/api.js`.

## Development Constraints
- No debug logs in production code
- Dead code must be removed (no `_` prefixes)
- Server runs with default 3s duration
- Plugin requires backend running first
## Component Salvageability Analysis for Terminal-First Evolution

### Core Components to Preserve As-Is

#### 1. **graph_manager.rs** - The Heart of Cymbiont ✅
- **Status**: Fully salvageable, minimal coupling to Logseq
- **Key Value**: Complete petgraph-based knowledge graph implementation
- **Dependencies**: Only standard Rust crates (petgraph, serde, chrono)
- **What to Keep**:
  - StableGraph structure with NodeData/EdgeData
  - PKM ID → NodeIndex mapping system
  - Auto-save mechanism (time and operation-based)
  - Node/edge creation and reference resolution
  - Graph serialization/persistence
- **Minor Adaptations Needed**:
  - Remove PKM-specific naming (rename to generic node/edge types)
  - Make node types configurable (not just Page/Block)
  - Extract interface traits for different node sources

#### 2. **pkm_data.rs** - Reusable Data Structures ✅
- **Status**: Fully salvageable as import format specification
- **Key Value**: Well-defined data structures for knowledge import
- **What to Keep**:
  - PKMBlockData and PKMPageData as import formats
  - Validation methods
  - Flexible timestamp parsing
  - Reference extraction structures
- **Evolution Path**: 
  - Keep as-is for Logseq import functionality
  - Create parallel structures for other PKM systems
  - Define common traits for importable content

#### 3. **transaction_log.rs** - Robust WAL Implementation ✅
- **Status**: Fully salvageable, no Logseq coupling
- **Key Value**: ACID-compliant write-ahead logging with sled
- **Dependencies**: sled database, standard Rust
- **What to Keep**:
  - Complete WAL implementation
  - Content hash indexing for deduplication
  - Transaction state machine
  - Recovery mechanisms
- **Perfect for**: Ensuring data consistency in terminal operations

#### 4. **utils.rs** - Cross-cutting Utilities ✅
- **Status**: Partially salvageable
- **What to Keep**:
  - DateTime parsing functions
  - JSON parsing utilities
  - Port availability checking
  - Server info management
- **What to Remove**:
  - Logseq executable finding/launching
  - URL scheme registration
  - Platform-specific Logseq handling

### Components Needing Minor Adaptation

#### 1. **transaction.rs** - Transaction Coordinator 🔧
- **Status**: Mostly salvageable
- **Coupling**: Light coupling to WebSocket acknowledgments
- **Adaptations**:
  - Remove WebSocket-specific acknowledgment logic
  - Make acknowledgment mechanism pluggable
  - Keep core transaction lifecycle management
  - Add support for CLI operation confirmations

#### 2. **saga.rs** - Workflow Coordination 🔧
- **Status**: Mostly salvageable
- **Coupling**: Examples use WebSocket workflows
- **Adaptations**:
  - Remove WebSocket-specific saga examples
  - Keep core saga pattern implementation
  - Add CLI-oriented workflow examples
  - Make compensation strategies configurable

#### 3. **kg_api.rs** - Public API Layer 🔧
- **Status**: Salvageable with interface changes
- **Coupling**: Heavy WebSocket integration
- **Adaptations**:
  - Extract core graph operations into traits
  - Remove WebSocket broadcast requirements
  - Make sync mechanism pluggable (CLI output, file, etc.)
  - Keep transaction-safe wrappers

#### 4. **config.rs** - Configuration Management 🔧
- **Status**: Mostly salvageable
- **Adaptations**:
  - Remove LogseqConfig section
  - Keep backend, sync, and development configs
  - Add terminal-specific configurations
  - Add import source configurations

### Components to Deprecate

#### 1. **session_manager.rs** ❌
- **Reason**: Entirely focused on Logseq session management
- **Replacement**: New import manager for various PKM sources

#### 2. **websocket.rs** ❌
- **Reason**: Browser-specific bidirectional communication
- **Salvageable Parts**: Command structure could inspire CLI commands

#### 3. **edn.rs** ❌
- **Reason**: Logseq-specific configuration format
- **No replacement needed**: Terminal app won't modify user configs

#### 4. **api.rs** (Partial) ⚠️
- **Keep**: Basic HTTP structure for agent API
- **Remove**: Logseq-specific endpoints
- **Transform**: Into REST API for graph queries

#### 5. **graph_registry.rs** (Partial) ⚠️
- **Keep**: Multi-graph concept
- **Remove**: Logseq UUID tracking
- **Transform**: Into workspace/project management

### Hidden Gems and Utilities

#### 1. **Archive System** 💎
- Found in graph_manager.rs
- Sophisticated node archival with full relationship preservation
- Perfect for terminal "undo" operations

#### 2. **Content Hash Deduplication** 💎
- In transaction_log.rs
- Prevents duplicate content processing
- Essential for idempotent CLI operations

#### 3. **Auto-save Mechanism** 💎
- Time and operation-based triggers
- Batch operation support with disable/enable
- Great for long-running terminal sessions

#### 4. **Graph Traversal Infrastructure** 💎
- While not fully implemented, the groundwork exists
- NodeIndex mappings enable efficient algorithms
- Ready for graph query language implementation

### Dependency Analysis

#### Clean Dependencies (No Changes Needed)
- petgraph
- serde/serde_json  
- chrono
- thiserror
- tracing
- uuid
- tokio (for async runtime)
- sled (for WAL)

#### Dependencies to Add for Terminal-First
- clap or similar for CLI parsing
- rustyline for REPL interface
- indicatif for progress bars
- crossterm for terminal UI

#### Dependencies to Remove
- axum (web framework)
- tower-http
- tungstenite (WebSocket)

### Recommended Architecture for Terminal-First

```
cymbiont-cli/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── commands/            # CLI command implementations
│   │   ├── mod.rs
│   │   ├── import.rs        # Import from various PKMs
│   │   ├── query.rs         # Graph queries
│   │   ├── mutate.rs        # Graph modifications
│   │   └── export.rs        # Export functionality
│   ├── core/                # Preserved core modules
│   │   ├── graph_manager.rs
│   │   ├── transaction_log.rs
│   │   ├── transaction.rs
│   │   ├── saga.rs
│   │   └── config.rs
│   ├── import/              # Import adapters
│   │   ├── mod.rs
│   │   ├── logseq.rs        # Uses pkm_data.rs
│   │   ├── obsidian.rs
│   │   └── markdown.rs
│   └── lib.rs               # Public API for embedding
```

### Migration Priority

1. **Phase 1**: Extract and test core modules in isolation
   - graph_manager.rs (with renamed types)
   - transaction_log.rs
   - Essential utilities

2. **Phase 2**: Build CLI interface
   - Command structure
   - REPL for interactive mode
   - Pipe support for Unix philosophy

3. **Phase 3**: Implement importers
   - Start with Logseq (reuse pkm_data.rs)
   - Add other PKM systems
   - Design common import traits

4. **Phase 4**: Query and mutation interface
   - Graph query language
   - Transaction-safe mutations
   - Export capabilities

EOF < /dev/null
