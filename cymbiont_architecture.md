# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Core knowledge graph engine
│   ├── main.rs                    # Entry point with 5-phase startup sequence
│   ├── cli.rs                     # CLI argument parsing and command execution
│   ├── app_state.rs               # Pure resource container with CQRS integration
│   ├── config.rs                  # YAML configuration management
│   ├── utils.rs                   # Process management, utilities, and async lock handling
│   ├── error.rs                   # Hierarchical error system with domain-specific types
│   ├── graph/                     # Graph management subsystem
│   │   ├── mod.rs                 # Graph module exports
│   │   ├── graph_manager.rs       # Petgraph-based knowledge graph engine
│   │   ├── graph_operations.rs    # Graph operations via CQRS commands
│   │   └── graph_registry.rs      # Multi-graph UUID management with open/closed state
│   ├── agent/                     # AI tool interface layer - see src/agent/CLAUDE.md
│   │   ├── mod.rs                 # Agent module exports
│   │   ├── tools.rs               # Canonical tool registry with 14 knowledge graph tools
│   │   ├── schemas.rs             # Tool schemas for LLM function calling
│   │   └── mcp/                   # Model Context Protocol server
│   │       ├── mod.rs             # MCP module exports
│   │       ├── protocol.rs        # JSON-RPC 2.0 message types
│   │       └── server.rs          # MCP server implementation over stdio
│   ├── import/                    # Data import functionality
│   │   ├── mod.rs                 # Import module exports and errors
│   │   ├── pkm_data.rs            # PKM data structures and helper functions
│   │   ├── logseq.rs              # Logseq-specific parsing
│   │   └── import_utils.rs        # Import coordination and graph creation
│   ├── http_server/               # HTTP/WebSocket server - see src/http_server/CLAUDE.md
│   │   ├── mod.rs                 # Server module exports
│   │   ├── server.rs              # Server lifecycle and port management
│   │   ├── http_api.rs            # HTTP endpoints (health, import, WebSocket upgrade)
│   │   ├── websocket.rs           # WebSocket protocol and connection handling
│   │   ├── websocket_utils.rs     # Shared helpers (auth, response, graph resolution)
│   │   ├── websocket_commands/    # Command handlers by domain
│   │   │   ├── mod.rs             # Command module exports
│   │   │   ├── graph_commands.rs  # Graph CRUD operations
│   │   │   └── misc_commands.rs   # Auth, system, and test commands
│   │   └── auth.rs                # Token generation and validation
│   └── cqrs/                      # Command Query Responsibility Segregation
│       ├── mod.rs                 # CQRS module exports
│       ├── commands.rs            # All mutation commands
│       ├── queue.rs               # Public API for command submission
│       ├── processor.rs           # Single-threaded state owner
│       └── router.rs              # Command routing with RouterToken authorization
├── data/                          # Graph persistence (configurable path)
│   ├── graph_registry.json        # Graph metadata persistence
│   ├── auth_token                 # Authentication token (auto-generated)
│   ├── graphs/{graph-id}/         # Per-graph data
│   │   └── knowledge_graph.json   # Graph content persistence
│   └── archived_graphs/           # Deleted graphs archive
├── tests/                         # Integration tests - see tests/CLAUDE.md
│   ├── common/                    # Shared test utilities
│   │   ├── mod.rs                 # Test environment setup
│   │   ├── test_harness.rs        # TestServer lifecycle management with CQRS
│   │   └── test_validator.rs      # JSON-based state validation
│   └── integration/               # Integration test suite (single binary - multiple test modules)
├── autodebugger/                  # Git submodule: LLM developer utilities toolbag
└── build.rs                       # Build script: enforces tracing macro usage
```

## Build Script (build.rs)
- **Enforce tracing**: Scans src/ and tests/ directories, fails build if finds println!/eprintln!/print!/eprint!/dbg!
- **Extract CLI commands**: Parses `cli_commands!` macro to generate test verification contract
- **Rationale**: Forces use of tracing macros for proper structured logging
- **Exception**: Allows these macros in build.rs itself and main.rs (for early bootstrap)

## Module Requirements and Data Flow

### main.rs
**Purpose**: Application entry point with 5-phase startup sequence
**Phases**: Initialization → Common startup → Command handling → Runtime loop → Cleanup
**Key functions**:
- `run_startup_sequence()` - Common initialization for all modes
- `run_server_loop()` / `run_cli_loop()` / `run_mcp_server()` - Mode-specific execution
- `handle_graceful_shutdown()` - Command completion during shutdown
**Startup**: CommandProcessor::start() initializes CQRS system
**Features**: Duration limits (0=infinite), signal handling, graceful shutdown, MCP server mode

### cli.rs
**Purpose**: CLI argument parsing and CQRS command execution
**Key macro**: `cli_commands!` - Single source of truth for all CLI commands
**Generated**: `Args` struct with clap annotations, `from_json_with_args()` for WebSocket bridge
**Functions**:
- `handle_cli_commands()` - Process CLI commands via CommandQueue, returns true for early exit
- `handle_agent_mode()` - Spawn Claude with MCP integration (two-process architecture)
- `show_cli_status()` - Display graph status (read-only)
- `dispatch_cli_command()` - WebSocket bridge for CLI execution
**Agent Mode**: Parent process spawns two children: (1) `cymbiont --mcp --duration 0` as MCP server providing knowledge graph tools via JSON-RPC over stdio, (2) `claude` CLI with generated MCP config pointing to child #1. Interactive mode requires TTY (uses `IsTerminal` trait to detect), non-interactive mode uses `-p` flag for single-shot prompts. Parent waits for Claude to exit then cleans up temp config file. This architecture leverages Claude Code's native MCP support, providing Cymbiont's 14 tools through the standard protocol without custom SDK integration.
**Architecture**: All mutations use `app_state.command_queue.execute()`
**Commands**: Graph operations, import, system status, agent mode
**Contract**: Build script extracts commands for test verification

### config.rs
**Purpose**: YAML configuration loading with smart fallback strategy
**Search order**: --config flag → executable dir → parent dirs (up to 3) → defaults
**Test mode**: Uses config.test.yaml when CYMBIONT_TEST_MODE is set
**Configuration (config.yaml)**:
```yaml
data_dir: data                    # Storage directory
backend:
  port: 8888                      # Base HTTP port
  max_port_attempts: 10           # Port search range
  server_info_file: "cymbiont_server.json"
development:
  default_duration: 3             # Auto-exit seconds (null=forever)
auth:
  token: null                     # Fixed token (null=auto-generate)
  disabled: false                 # Disable auth
tracing:
  output: "stderr"                # Log output: "stdout" or "stderr" (must be "stderr" for MCP)
verbosity:                        # Log verbosity thresholds (for autodebugger)
  info_threshold: 50
  debug_threshold: 100
  trace_threshold: 200
transaction_log:                  # Future WAL configuration (not yet implemented)
  fsync_interval_ms: 100
  compaction_threshold_mb: 100
  retention_days: 7
```

### error.rs
**Purpose**: Hierarchical error system with domain-specific types
**Types**: CymbiontError (root), StorageError, AgentError, GraphError, ServerError, ImportError, ConfigError
**Features**:
- Global `Result<T>` type alias
- Automatic `From` trait implementations
- Convenience constructors: `StorageError::not_found("graph", "id", id)`
- Idiomatic `?` operator support
**Usage**: `return Err(StorageError::not_found("graph", "id", id).into())` or `serde_json::from_str(&data)?`

### cqrs/mod.rs
**Purpose**: CQRS architecture implementation for deadlock-free mutations
**Architecture**: Command-Query Responsibility Segregation
**Pattern**: Single-threaded CommandProcessor owns all mutable state, eliminating deadlocks
**Components**: Commands, CommandQueue, CommandProcessor, RouterToken

### cqrs/commands.rs
**Purpose**: All mutation commands
**Command types**:
- `RegistryCommand`: Graph registry operations (CreateGraph, RemoveGraph, etc.)
- `GraphCommand`: Graph content operations (CreateBlock, UpdateBlock, DeleteBlock, etc.)
- `SystemCommand`: System operations (Shutdown)
**Features**: Type-safe command definitions, response types

### cqrs/queue.rs
**Purpose**: Public API for command submission with async futures
**Type**: `CommandQueue` - thread-safe command submission interface
**Methods**: `execute(command)` returns Future<Result<Response>>
**Features**: Internal mpsc channel to CommandProcessor, future-based responses
**Usage**: Primary mutation interface for all external callers

### cqrs/processor.rs
**Purpose**: Single-threaded owner of all mutable state
**Type**: `CommandProcessor` - executes commands sequentially in background task
**Responsibilities**: Command execution, state mutation, entity lazy loading, JSON persistence
**Features**: Lazy graph loading, RouterToken authorization, autosave management
**Architecture**: Owns AppState resources, provides RouterTokens for operations

### cqrs/router.rs
**Purpose**: Authorization system for command execution
**Type**: `RouterToken` - proof of authorization for domain operations
**Features**: Scoped access to specific registries and managers
**Usage**: CommandProcessor creates tokens, domain modules require them for mutations
**Security**: Prevents direct state mutation, ensures all changes go through CQRS


### utils.rs
**Purpose**: Process management, utilities, and async lock handling
**Features**: Process utilities, datetime parsing, UUID serialization helpers
**Lock handling**: `AsyncRwLockExt` trait for async locks (cannot be poisoned)
**Functions**: `cleanup_and_save_all()` for graceful shutdown persistence

### app_state.rs
**Purpose**: Pure resource container with CQRS command queue integration
**Architecture**: All fields public, mutation via CommandQueue, direct read access
**Resources**:
- `command_queue`: Primary interface for all mutations
- `graph_managers`: HashMap of graph managers (read-only from external access)
- `graph_registry`: Graph metadata registry
- `ws_connections`: WebSocket connection tracking (server mode)
- `auth_token`: Authentication token
**Key methods** (minimal - just lifecycle):
- `new_with_config()` - Initialize with configuration and CommandQueue
- `initiate_graceful_shutdown()` - Start shutdown sequence
**Pattern**: CommandQueue for mutations, direct access for reads
**Architecture**: CQRS separates command/query responsibilities

### server/http_api.rs
**Purpose**: HTTP API endpoints
**Endpoints**: `/` (health), `/import/logseq` (POST), `/ws` (upgrade), monitoring paths
**Auth**: Bearer token for protected endpoints, WebSocket auth post-upgrade

### graph/mod.rs
**Purpose**: Graph module exports
**Exports**: `graph_manager`, `graph_operations`, `graph_registry`
**Details**: See `src/graph/CLAUDE.md` for module guide

### graph/graph_manager.rs
**Purpose**: Generic petgraph-based storage engine with JSON persistence
**Features**: Domain-agnostic, StableGraph for index stability, autosave mechanism
**Operations**: `create_node()`, `create_or_update_node()`, `find_node()`, `add_edge()`, `delete_nodes()`
**Autosave**: Triggers on 5-minute timer or 10 operations threshold
**Persistence**: `save()` exports to `graphs/{id}/knowledge_graph.json`
**Types**: Defined by domain (PKM: Page/Block nodes, PageRef/BlockRef edges)

### graph/graph_operations.rs
**Purpose**: Graph operations via CQRS commands
**Trait**: `GraphOps` - all operations require `graph_id: &Uuid`
**Operations**:
- Blocks: `add_block()`, `update_block()`, `delete_block()`
- Pages: `create_page()`, `delete_page()`
- Lifecycle: `create_graph()`, `delete_graph()`, `open_graph()`, `close_graph()`
- Queries: `get_node()`, `query_graph_bfs()`, `list_graphs()`, `list_open_graphs()`
**Architecture**: Mutations route through CommandQueue, reads access state directly
**Adding operations**: 1) Add to Command enum 2) Add to GraphOps trait 3) Implement via command_queue.execute() 4) Optional: Add to tools.rs 5) Optional: Add WebSocket command

### graph/graph_registry.rs
**Purpose**: Multi-graph UUID tracking with open/closed state
**Operations**:
- `register_graph()`, `remove_graph()` - lifecycle
- `open_graph()`, `close_graph()` - state management
- `resolve_graph_target()` - UUID/name resolution with smart defaults
- `save()`, `load()` - JSON persistence
**Data**: `graphs: HashMap<Uuid, GraphInfo>`, `open_graphs: HashSet<Uuid>`
**Smart defaults**: When only one graph is open, operations default to it
**Persistence**: Metadata saved to `graph_registry.json`

### agent/mod.rs
**Purpose**: AI tool interface layer exports
**Exports**: `tools`, `schemas`, `mcp`
**Details**: See `src/agent/CLAUDE.md` for module guide

### agent/tools.rs
**Purpose**: Canonical tool registry (14 knowledge graph tools)
**Architecture**: Static `TOOLS` HashMap with function pointers, single source of truth
**Tools**:
- Blocks: `add_block`, `update_block`, `delete_block`
- Pages: `create_page`, `delete_page`
- Queries: `get_node`, `query_graph_bfs`
- Lifecycle: `open_graph`, `close_graph`, `create_graph`, `delete_graph`
- Lists: `list_graphs`, `list_open_graphs`
- Import: `import_logseq`
**Functions**: `execute_tool()`, `get_tool_schemas()`
**Features**: Graph resolution with smart defaults, JSON responses

### agent/schemas.rs
**Purpose**: Ollama-compatible tool schemas (14 operations)
**Types**: `ToolDefinition`, `ParameterSchema`, `PropertySchema`
**Features**: Graph targeting (UUID/name), required/optional params, type info
**Function**: `all_tool_definitions()` - generate all schemas

### agent/mcp/mod.rs
**Purpose**: MCP module exports
**Exports**: `protocol`, `server`, `run_mcp_server()`

### agent/mcp/protocol.rs
**Purpose**: JSON-RPC 2.0 protocol types for MCP
**Types**: `Request`, `Response`, `Error`, `Notification`
**Constants**: Standard error codes, MCP method names (including prompts/resources lists)
**Features**: Type-safe message handling, error formatting

### agent/mcp/server.rs
**Purpose**: MCP server implementation over stdio
**Type**: `MCPServer` - JSON-RPC server for AI agent integration
**Features**: Tool discovery, execution via `tools::execute_tool()`, stdio communication
**Protocol**: JSON-RPC 2.0 with MCP extensions
**Capabilities**: Tools, resources, prompts, logging (empty arrays for resources/prompts)
**Critical**: stdout reserved for JSON-RPC, all logs to stderr

### http_server/server.rs
**Purpose**: Server lifecycle and port finding
**Function**: `start_server()` - find port, create Axum server, return handle
**Features**: Auto port selection, server info file, previous instance cleanup

### http_server/websocket.rs & websocket_commands/
**Purpose**: WebSocket protocol and CQRS command routing
**Architecture**: Core in websocket.rs, handlers in websocket_commands/
**Features**: Async task spawning, auth verification, command dispatch via CommandQueue
**CQRS Integration**: All mutations route through CommandQueue for execution
**Commands**: Graph CRUD, auth, test utilities
**Details**: See `src/http_server/CLAUDE.md` for full API

### import/mod.rs
**Purpose**: Import module exports
**Exports**: `import_logseq_graph()`, PKM types, ImportError
**Details**: See `src/import/CLAUDE.md`

### import/pkm_data.rs
**Purpose**: PKM data structures and helper functions
**Types**: `PKMBlockData` (hierarchy, properties), `PKMPageData` (metadata), `PKMReference`
**Helper functions**:
- `create_block_with_resolution()` - Create block with reference expansion
- `update_block_with_resolution()` - Update block with reference resolution
- `setup_block_relationships()` - Create parent-child and page edges
- `create_or_update_page()` - Smart page creation/update
- `resolve_block_references()` - Expand `((block-id))` patterns
**Features**: Reference resolution with circular protection, page normalization

### import/logseq.rs
**Purpose**: Logseq-specific parsing
**Functions**:
- `parse_logseq_directory()` - Directory scanning
- `parse_markdown_file()` - Extract pages/blocks
- `extract_properties()` - YAML frontmatter
- `parse_blocks()` - Hierarchical extraction
**Features**: Recursive .md reading, frontmatter, nested blocks, `((block-id))` detection

### import/import_utils.rs
**Purpose**: Import coordination and graph creation
**Function**: `import_logseq_graph()` - Full workflow
**Steps**: Create graph → Import data → Return UUID
**Integration**: Uses `create_graph()` for consistent graph creation


### http_server/auth.rs
**Purpose**: Token-based authentication
**Features**: Auto-generate on startup, save to `auth_token` (0600), rotate on restart
**Usage**: HTTP Bearer header, WebSocket Auth command
**Config**: Optional fixed token or disable auth

### tests/common/test_harness.rs
**Purpose**: Integration test infrastructure
**Type**: `TestServer` - process lifecycle management
**Features**: Parallel execution, isolated environments, phase-based testing
**Key functions**: `execute_tool_sync()` - Direct tool execution for testing (debug builds)
**Details**: See `tests/CLAUDE.md`

### tests/common/test_validator.rs
**Purpose**: JSON-based test validation
**Type**: `TestValidator` - validate state through JSON files
**Methods**:
- `expect_*()` - Track expected operations
- `validate_all()` - Verify state in JSON files
- `validate_graph_state()` - Check graph state from JSON
**Features**: Operation consolidation, timestamp validation, case-insensitive page lookups


## Data Structures

### PKM Types
- **PKMBlockData**: `{id, content, created, updated, parent?, children[], page?, properties, references[], reference_content?}`
- **PKMPageData**: `{name, normalized_name?, created, updated, properties, blocks[]}`
- **PKMReference**: Cross-reference between blocks

### WebSocket Commands
| Category | Commands | Parameters |
|----------|----------|------------|
| **Auth** | `Auth` | `token` |
| **Graph Ops** | `OpenGraph`, `CloseGraph`, `CreateBlock`, `UpdateBlock`, `DeleteBlock` | `graph_id?/graph_name?` + op-specific |
| | `CreatePage`, `DeletePage`, `CreateGraph`, `DeleteGraph`, `ListGraphs` | |
| **System** | `Heartbeat` | None |
| **Test** | `TestToolCall` (debug builds) | `tool_name`, `tool_args` |

**Responses**: `Success {data?}`, `Error {message}`, `Heartbeat`
**Processing**: Async task spawning per command

### Registry Format
- **Graph**: `{graphs: [{id, name, path, created_at, last_accessed}]}`

## CLI Commands
| Category | Command | Description |
|----------|---------|-------------|
| **Server** | `--server` | Run as HTTP/WebSocket server |
| | `--mcp` | Run as MCP server (Model Context Protocol) |
| | `--agent` | Spawn Claude with MCP integration |
| **Graph** | `--import-logseq <PATH>` | Import Logseq directory |
| | `--create-graph <NAME>` | Create new graph |
| | `--delete-graph <NAME/ID>` | Archive graph |
| | `--list-graphs` | List all graphs |
| **Runtime** | `--data-dir <PATH>` | Override data directory |
| | `--config <PATH>` | Use specific config |
| | `--duration <SECONDS>` | Run duration limit (0=infinite) |
| | `--prompt <TEXT>` | Non-interactive agent prompt |
| **Supporting** | `--description <DESC>` | Graph description (with --create-graph) |

## Graceful Shutdown
- **First Ctrl+C**: Graceful shutdown, saves all state to JSON
- **Second Ctrl+C**: Force termination
- **Cleanup**: `cleanup_and_save_all()` persists graphs and agent state
- **Exit**: `std::process::exit(0)` for clean termination

## Key Flows

| Flow | Steps |
|------|-------|
| **Import** | CLI/HTTP → Parse .md → Extract blocks → CQRS CreateGraph → Return UUID |
| **CQRS Command** | Submit to CommandQueue → Execute via RouterToken → Return response |
| **WebSocket** | Auth → Spawn task → Check auth → CQRS command → Response |
| **Tool Execution** | MCP request → execute_tool() → CQRS command → Return result |
| **Graph Lifecycle** | Create → Open → Operations → Close → Archive |
| **Startup** | Load registries → Start CommandProcessor |
| **Auth** | Generate token → Save to `auth_token` → HTTP: Bearer header, WS: Auth command |
| **Persistence** | JSON autosave (5 min/10 ops) → Shutdown save → Load on startup |

### autodebugger/
**Purpose**: Git submodule - LLM developer utilities
**Features**: Log verbosity detection, command wrappers, doc validation
**Commands**: `remove-debug` (strip debug calls), `validate-docs` (check module docs)