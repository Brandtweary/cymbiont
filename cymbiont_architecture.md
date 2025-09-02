# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Core knowledge graph engine
│   ├── main.rs                    # Entry point with 5-phase startup sequence
│   ├── cli.rs                     # CLI argument parsing and command execution
│   ├── app_state.rs               # Centralized application state with agent management
│   ├── config.rs                  # YAML configuration management
│   ├── utils.rs                   # Process management and utilities
│   ├── error.rs                   # Hierarchical error system with domain-specific types
│   ├── lock.rs                    # Lock handling utilities with panic-on-poison strategy
│   ├── graph/                     # Graph management subsystem
│   │   ├── mod.rs                 # Graph module exports
│   │   ├── graph_manager.rs       # Petgraph-based knowledge graph engine
│   │   ├── graph_operations.rs    # Multi-agent graph operations with runtime authorization
│   │   └── graph_registry.rs      # Multi-graph UUID management with agent tracking
│   ├── agent/                     # Agent abstraction layer
│   │   ├── mod.rs                 # Agent module exports
│   │   ├── agent.rs               # Core Agent struct with conversation management and tool execution
│   │   ├── agent_registry.rs      # Agent lifecycle and authorization management
│   │   ├── llm.rs                 # LLM backend abstraction with MockLLM tool support
│   │   ├── kg_tools.rs            # Static knowledge graph tool registry with 15 functional tools
│   │   └── schemas.rs             # Ollama-compatible tool schemas for function calling
│   ├── import/                    # Data import functionality
│   │   ├── mod.rs                 # Import module exports and errors
│   │   ├── pkm_data.rs            # PKM data structures and helper functions
│   │   ├── logseq.rs              # Logseq-specific parsing
│   │   └── import_utils.rs        # Import coordination with agent authorization
│   ├── server/                    # Server-specific functionality - see src/server/CLAUDE.md
│   │   ├── mod.rs                 # Server module exports
│   │   ├── server.rs              # Server lifecycle and port management
│   │   ├── http_api.rs            # HTTP endpoints (health, import, WebSocket upgrade)
│   │   ├── websocket.rs           # WebSocket protocol and connection handling
│   │   ├── websocket_utils.rs     # Shared helpers (auth, response, graph resolution)
│   │   ├── websocket_commands/    # Command handlers by domain
│   │   │   ├── mod.rs             # Command module exports
│   │   │   ├── agent_commands.rs  # Agent chat and administration
│   │   │   ├── graph_commands.rs  # Graph CRUD operations
│   │   │   └── misc_commands.rs   # Auth, test, freeze commands
│   │   └── auth.rs                # Token generation and validation
│   └── cqrs/                      # Command Query Responsibility Segregation
│       ├── mod.rs                 # CQRS module exports
│       ├── commands.rs            # All mutation commands with deterministic replay
│       ├── queue.rs               # Public API for command submission
│       ├── processor.rs           # Single-threaded state owner
│       ├── router.rs              # Command routing with RouterToken authorization
│       └── wal.rs                 # Command persistence with sled
├── data/                          # Graph and agent persistence (configurable path)
│   ├── command_log/               # CQRS command WAL database (sled)
│   ├── graph_registry.json        # JSON export for debugging
│   ├── agent_registry.json        # JSON export for debugging
│   ├── auth_token                 # Authentication token (auto-generated)
│   ├── graphs/{graph-id}/         # Per-graph exports
│   │   └── knowledge_graph.json   # JSON export for debugging
│   ├── agents/{agent-id}/         # Per-agent exports
│   │   └── agent.json             # JSON export for debugging
│   ├── archived_graphs/           # Deleted graphs archive
│   └── archived_agents/           # Deleted agents archive
├── tests/                         # Integration tests - see tests/CLAUDE.md
│   ├── common/                    # Shared test utilities
│   │   ├── mod.rs                 # Test environment setup
│   │   ├── test_harness.rs        # TestServer lifecycle management with CQRS
│   │   └── wal_validation.rs      # CQRS command WAL-based state validation
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
- `run_startup_sequence()` - Common initialization for both modes
- `check_orphaned_graphs()` - Warn about graphs with no authorized agents
- `run_server_loop()` / `run_cli_loop()` - Mode-specific event loops
- `handle_graceful_shutdown()` - Command completion during shutdown
**Startup**: CommandProcessor::start() handles WAL recovery and prime agent creation
**Features**: Duration limits, signal handling, orphan detection

### cli.rs
**Purpose**: CLI argument parsing and CQRS command execution
**Key macro**: `cli_commands!` - Single source of truth for all CLI commands
**Generated**: `Args` struct with clap annotations, `from_json_with_args()` for WebSocket bridge
**Functions**:
- `handle_cli_commands()` - Process CLI commands via CommandQueue, returns true for early exit
- `show_cli_status()` - Display graph and agent status (read-only)
- `dispatch_cli_command()` - WebSocket bridge for CLI execution
**Architecture**: All create/delete/activate operations use `app_state.command_queue.execute()`
**Commands**: Agent management, graph operations, system status
**Contract**: Build script extracts commands for test verification

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

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
**Architecture**: Command-Query Responsibility Segregation with unified WAL
**Pattern**: Single-threaded CommandProcessor owns all mutable state, eliminating deadlocks
**Components**: Commands, CommandQueue, CommandProcessor, RouterToken, CommandLog

### cqrs/commands.rs
**Purpose**: All mutation commands with deterministic replay support
**Command types**:
- `RegistryCommand`: Graph/agent registry operations (CreateGraph, CreateAgent, etc.)
- `GraphCommand`: Graph content operations (CreateBlock, UpdateBlock, DeleteBlock, etc.)
- `AgentCommand`: Agent operations (AuthorizeAgent, DeauthorizeAgent, etc.)
- `SystemCommand`: System operations (FreezeOperations, UnfreezeOperations)
**Features**: Deterministic replay via `resolve()` method, resolved_id fields for UUID generation
**WAL**: All commands logged before execution for crash recovery

### cqrs/queue.rs
**Purpose**: Public API for command submission with async futures
**Type**: `CommandQueue` - thread-safe command submission interface
**Methods**: `execute(command)` returns Future<Result<Response>>
**Features**: Internal mpsc channel to CommandProcessor, future-based responses
**Usage**: Primary mutation interface for all external callers

### cqrs/processor.rs
**Purpose**: Single-threaded owner of all mutable state
**Type**: `CommandProcessor` - executes commands sequentially in background task
**Responsibilities**: Command execution, WAL logging, state mutation, entity lazy loading
**Features**: Startup recovery, entity rebuild from WAL, RouterToken authorization
**Architecture**: Owns AppState resources, provides RouterTokens for authorized operations

### cqrs/router.rs
**Purpose**: Authorization system for command execution
**Type**: `RouterToken` - proof of authorization for domain operations
**Features**: Scoped access to specific registries and managers
**Usage**: CommandProcessor creates tokens, domain modules require them for mutations
**Security**: Prevents direct state mutation, ensures all changes go through CQRS

### cqrs/wal.rs
**Purpose**: Command Write-Ahead Log for crash recovery
**Type**: `CommandLog` - sled-based persistent command storage
**Features**: Command serialization, startup recovery, entity-specific filtering
**Storage**: Commands stored with timestamps, entity IDs, and serialized data
**Recovery**: Replay commands in chronological order for state reconstruction

### utils.rs
**Purpose**: Process management, utilities, and lock handling (moved from lock.rs)
**Features**: Process utilities, datetime parsing, general helpers
**Lock handling**: `read_or_panic()`, `write_or_panic()` for RwLock operations
**Pattern**: Panic-on-poison strategy for lock errors

### app_state.rs
**Purpose**: Pure resource container with CQRS command queue integration
**Architecture**: All fields public, mutation via CommandQueue, direct read access
**Resources**:
- `command_queue`: Primary interface for all mutations
- `graph_managers`: HashMap of graph managers (read-only from external access)
- `agents`: HashMap of active agents (read-only from external access)
- `graph_registry`, `agent_registry`: Metadata registries
- `ws_connections`: WebSocket connection tracking (server mode)
- `auth_token`: Authentication token
- `operation_freeze`: Test infrastructure
**Key methods** (minimal - just lifecycle):
- `new_with_config()` - Initialize with configuration and CommandQueue
- `cleanup_and_save()` - Shutdown persistence
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
**Purpose**: Generic petgraph-based storage engine
**Features**: Domain-agnostic, StableGraph for index stability
**Operations**: `create_node()`, `create_or_update_node()`, `find_node()`, `add_edge()`, `delete_nodes()`
**Types**: Defined by domain (PKM: Page/Block nodes, PageRef/BlockRef edges)

### graph/graph_operations.rs
**Purpose**: Multi-agent graph operations via CQRS commands
**Trait**: `GraphOps` - all operations require `agent_id: Uuid` and `graph_id: &Uuid`
**Operations**:
- Blocks: `add_block()`, `update_block()`, `delete_block()`
- Pages: `create_page()`, `delete_page()`
- Lifecycle: `create_graph()`, `delete_graph()`, `open_graph()`, `close_graph()`
- Queries: `get_node()`, `query_graph_bfs()`, `list_graphs()`, `list_open_graphs()`
**Architecture**: Mutations route through CommandQueue, reads access state directly
**Adding operations**: 1) Add to Command enum 2) Add to GraphOps trait 3) Implement via command_queue.execute() 4) Optional: Add to kg_tools 5) Optional: Add WebSocket command

### graph/graph_registry.rs
**Purpose**: Multi-graph UUID tracking with open/closed state
**Operations**:
- `register_graph()`, `remove_graph()` - lifecycle
- `open_graph()`, `close_graph()` - state management
- `resolve_graph_target()` - UUID/name resolution
- `create_new_graph_complete()`, `delete_graph_complete()` - full workflows
**Data**: `open_graphs: HashSet<Uuid>`, `authorized_agents` per graph
**Persistence**: Open state survives restarts
**Concurrency**: `Arc<RwLock<GraphRegistry>>` with panic-on-poison

### agent/mod.rs
**Purpose**: Agent module exports
**Details**: See `src/agent/CLAUDE.md` for module guide

### agent/agent_registry.rs
**Purpose**: Agent lifecycle via CQRS commands
**Operations**: All mutations route through CommandQueue
- `register_agent()`, `remove_agent()` - lifecycle via CQRS
- `activate_agent()`, `deactivate_agent()` - memory management via CQRS
- `authorize_agent_for_graph()`, `deauthorize_agent_from_graph()` - bidirectional auth via CQRS
- `resolve_agent_target()` - UUID/name resolution (read-only)
- `ensure_default_agent()` - create prime agent on first run via CQRS
- `find_orphaned_graphs()` - detect graphs without agents (read-only)
**Prime agent**: Auto-created, full graph access, cannot be deleted
**Architecture**: CommandProcessor owns registry, RouterToken required for mutations
**Persistence**: `agent_registry.json` with active/inactive states

### agent/agent.rs
**Purpose**: Core Agent with CQRS-based conversation management
**Types**: `Agent`, `Message` (User/Assistant/Tool with AgentContext)
**Features**: Conversation history, LLM backend, default graph, CQRS tool execution
**Key function**: `process_agent_message()` - 4-phase LLM pipeline via CQRS commands
**Methods**:
- `chat()`, `process_message()` - LLM interaction
- `execute_tool()` - Tool execution via CommandQueue
- `get/set_default_graph_id()` - Default graph (read-only access)
- `reset_conversation()`, `get_history()`, `save()`, `load()` - persistence
**Architecture**: Tool calls route through CommandQueue for mutations

### agent/llm.rs
**Purpose**: LLM backend abstraction with MockLLM
**Types**: `LLMBackend` trait, `LLMConfig` enum, `MockLLM`, `ToolCall`, `LLMResponse`
**MockLLM**: Test implementation with `echo_tool`, `generate_mock_args()`, valid UUIDs
**Interface**: `complete()` with tool schemas, `health_check()`

### agent/kg_tools.rs
**Purpose**: Static tool registry (15 knowledge graph tools)
**Architecture**: Static `TOOLS` HashMap with function pointers
**Tools**:
- Blocks: `add_block`, `update_block`, `delete_block`
- Pages: `create_page`, `delete_page`
- Queries: `get_node`, `query_graph_bfs` (stub)
- Lifecycle: `open_graph`, `close_graph`, `create_graph`, `delete_graph`
- Lists: `list_graphs`, `list_open_graphs`, `list_my_graphs`
- Agent: `set_default_graph`, `get_default_graph`
**Features**: Graph resolution, authorization, JSON responses

### agent/schemas.rs
**Purpose**: Ollama-compatible tool schemas (15 operations)
**Types**: `ToolDefinition`, `ParameterSchema`, `PropertySchema`
**Features**: Graph targeting (UUID/name), required/optional params, type info
**Function**: `all_tool_definitions()` - generate all schemas

### server/server.rs
**Purpose**: Server lifecycle and port finding
**Function**: `start_server()` - find port, create Axum server, return handle
**Features**: Auto port selection, server info file, previous instance cleanup

### server/websocket.rs & websocket_commands/
**Purpose**: WebSocket protocol and CQRS command routing
**Architecture**: Core in websocket.rs, handlers in websocket_commands/
**Features**: Async task spawning, auth verification, command dispatch via CommandQueue
**CQRS Integration**: All mutations route through CommandQueue for execution
**Commands**: Agent chat/admin, graph CRUD, auth/testing utilities
**Details**: See `src/server/CLAUDE.md` for full API

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
**Purpose**: Import coordination with agent authorization
**Function**: `import_logseq_graph()` - Full workflow
**Steps**: Create graph → Import data → Authorize prime agent → Return UUID
**Integration**: Uses `create_graph()` for consistent authorization


### server/auth.rs
**Purpose**: Token-based authentication
**Features**: Auto-generate on startup, save to `auth_token` (0600), rotate on restart
**Usage**: HTTP Bearer header, WebSocket Auth command
**Config**: Optional fixed token or disable auth

### tests/common/test_harness.rs
**Purpose**: Integration test infrastructure
**Type**: `TestServer` - process lifecycle management
**Features**: Parallel execution, isolated environments, phase-based testing
**Details**: See `tests/CLAUDE.md`

### tests/common/wal_validation.rs
**Purpose**: Unified WAL-based test validation
**Type**: `WALValidationFixture` - validate all state through transaction log
**Methods**:
- `expect_operation()` - Track expected WAL operations
- `validate_wal()` - Verify operations exist in transaction log
- `validate_graph_state()` - Check graph state from WAL
- `validate_agent_state()` - Check agent state from WAL
**Features**: Direct sled database access, operation categorization, comprehensive state validation


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
| **Agent Chat** | `AgentChat`, `AgentSelect`, `AgentList` | `agent_id?/agent_name?` + op-specific |
| | `AgentHistory`, `AgentReset`, `AgentInfo` | |
| **Agent Admin** | `CreateAgent`, `DeleteAgent`, `ActivateAgent` | Agent identification params |
| | `DeactivateAgent`, `AuthorizeAgent`, `DeauthorizeAgent` | |
| **Testing** | `FreezeOperations`, `UnfreezeOperations`, `GetFreezeState` | None |
| **System** | `Heartbeat` | None |

**Responses**: `Success {data?}`, `Error {message}`, `Heartbeat`
**Processing**: Async task spawning per command

### Registry Formats
- **Graph**: `{graphs: [{id, name, path, created_at, last_accessed}]}`
- **Agent**: `{agents: [{id, name, active, authorized_graphs[]}]}`

## Configuration (config.yaml)
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
transaction_log:
  fsync_interval_ms: 100
  compaction_threshold_mb: 100
  retention_days: 7
  redundant_copies: 10
  integrity_check_on_startup: true
```

## CLI Commands
| Category | Command | Description |
|----------|---------|-------------|
| **Server** | `--server` | Run as HTTP/WebSocket server |
| **Graph** | `--import-logseq <PATH>` | Import Logseq directory |
| | `--delete-graph <NAME/ID>` | Archive graph |
| | `--list-graphs` | List all graphs |
| **Agent** | `--create-agent <NAME>` | Create agent (with `--agent-description`) |
| | `--delete-agent <NAME/ID>` | Delete agent |
| | `--activate-agent <NAME/ID>` | Load agent to memory |
| | `--deactivate-agent <NAME/ID>` | Unload from memory |
| | `--agent-info <NAME/ID>` | Show agent details |
| **Auth** | `--authorize-agent <NAME/ID>` | Grant graph access (with `--for-graph`) |
| | `--deauthorize-agent <NAME/ID>` | Revoke access (with `--from-graph`) |
| **Runtime** | `--data-dir <PATH>` | Override data directory |
| | `--config <PATH>` | Use specific config |
| | `--duration <SECONDS>` | Run duration limit |

## Graceful Shutdown
- **First Ctrl+C**: Graceful shutdown, waits 30s for transactions
- **Second Ctrl+C**: Force termination with WAL flush
- **Cleanup**: `cleanup_and_save()` persists graphs, closes connections
- **Exit**: `std::process::exit(0)` due to sled background threads

## Key Flows

| Flow | Steps |
|------|-------|
| **Import** | CLI/HTTP → Parse .md → Extract blocks → CQRS CreateGraph → Authorize prime agent |
| **CQRS Command** | Submit to CommandQueue → WAL write → Execute via RouterToken → Return response |
| **WebSocket** | Auth → Agent select → Spawn task → Check auth → CQRS command → Response |
| **Agent Chat** | Resolve agent → LLM complete → Execute tools via CommandQueue → Save conversation → Return |
| **Agent Lifecycle** | CQRS RegisterAgent → CQRS ActivateAgent → Chat → CQRS DeactivateAgent → Archive |
| **Recovery** | CommandProcessor startup → CommandLog replay → Entity lazy loading → Bootstrap |
| **Auth** | Generate token → Save to `auth_token` → HTTP: Bearer header, WS: Auth command |
| **Prime Agent** | Auto-create → Authorize for all graphs → Cannot delete → Default agent |

### autodebugger/
**Purpose**: Git submodule - LLM developer utilities
**Features**: Log verbosity detection, command wrappers, doc validation
**Commands**: `remove-debug` (strip debug calls), `validate-docs` (check module docs)