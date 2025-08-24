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
│   ├── graph_manager.rs           # Petgraph-based knowledge graph engine
│   ├── graph_operations.rs        # Multi-agent graph operations with runtime authorization
│   ├── agent/                     # Agent abstraction layer
│   │   ├── mod.rs                 # Agent module exports
│   │   ├── agent.rs               # Core Agent struct with conversation management and tool execution
│   │   ├── llm.rs                 # LLM backend abstraction with MockLLM tool support
│   │   ├── kg_tools.rs            # Static knowledge graph tool registry with 15 functional tools
│   │   └── schemas.rs             # Ollama-compatible tool schemas for function calling
│   ├── import/                    # Data import functionality
│   │   ├── mod.rs                 # Import module exports and errors
│   │   ├── pkm_data.rs            # PKM data structures
│   │   ├── logseq.rs              # Logseq-specific parsing
│   │   ├── import_utils.rs        # Import coordination with agent authorization
│   │   └── reference_resolver.rs  # Block reference resolution
│   ├── server/                    # Server-specific functionality - see src/server/CLAUDE.md
│   │   ├── mod.rs                 # Server module exports
│   │   ├── server.rs              # Server lifecycle and port management
│   │   ├── http_api.rs            # HTTP endpoints (health, import, WebSocket upgrade)
│   │   ├── websocket.rs           # WebSocket protocol and connection handling
│   │   ├── websocket_utils.rs     # Shared helpers (auth, response, graph resolution)
│   │   ├── websocket_commands/    # Command handlers by domain
│   │   │   ├── agent_commands.rs  # Agent chat and administration
│   │   │   ├── graph_commands.rs  # Graph CRUD operations
│   │   │   └── misc_commands.rs   # Auth, test, freeze commands
│   │   └── auth.rs                # Token generation and validation
│   └── storage/                   # Persistence layer
│       ├── mod.rs                 # Storage module exports
│       ├── graph_persistence.rs   # Graph save/load utilities
│       ├── graph_registry.rs      # Multi-graph UUID management with agent tracking
│       ├── agent_registry.rs      # Agent lifecycle and authorization management
│       ├── agent_persistence.rs   # Agent save/load with auto-save thresholds
│       ├── registry_utils.rs      # Shared UUID serialization utilities
│       ├── transaction_log.rs     # Write-ahead logging with sled
│       └── transaction.rs         # Transaction coordination
├── data/                          # Graph and agent persistence (configurable path)
│   ├── graph_registry.json        # Graph UUID registry with agent associations
│   ├── agent_registry.json        # Agent UUID registry with graph authorizations
│   ├── auth_token                 # Authentication token (auto-generated)
│   ├── graphs/{graph-id}/         # Per-graph storage
│   │   ├── knowledge_graph.json   # Serialized petgraph
│   │   └── transaction_log/       # Per-graph WAL database
│   ├── agents/{agent-id}/         # Per-agent storage
│   │   └── agent.json             # Agent state, conversation history, LLM config
│   ├── archived_graphs/           # Deleted graphs archive
│   └── archived_agents/           # Deleted agents archive
├── tests/                         # Integration tests - see tests/CLAUDE.md
│   ├── common/                    # Shared test utilities
│   │   ├── mod.rs                 # Test environment setup
│   │   ├── test_harness.rs        # TestServer lifecycle management
│   │   ├── graph_validation.rs    # Automated graph state validation
│   │   └── agent_validation.rs    # Agent state, conversation, and tool message validation
│   └── integration/               # Integration test suite (single binary)
│       ├── main.rs                # Test entry point
│       ├── crash_recovery.rs      # Transaction recovery tests
│       ├── http_logseq_import.rs  # HTTP API tests
│       ├── websocket_commands.rs  # WebSocket tests
│       ├── agent_commands.rs      # Agent chat and admin command tests
│       ├── agent_tools.rs         # Complete agent tool execution test suite
│       ├── cli_commands.rs        # CLI command tests with contract enforcement
│       └── freeze_mechanism.rs    # Operation freeze/unfreeze tests
├── autodebugger/                  # Git submodule: LLM developer utilities toolbag
└── build.rs                       # Build script: enforces tracing macro usage
```

## Build Script (build.rs)
- **Enforce tracing**: Fail on println!/eprintln!/print!/eprint!/dbg! in src/ or tests/
- **Extract CLI commands**: Parse `cli_commands!` macro for test contract enforcement

## Module Requirements and Data Flow

### main.rs
**Purpose**: Application entry point with 5-phase startup sequence
**Phases**: Initialization → Common startup → Command handling → Runtime loop → Cleanup
**Key functions**:
- `run_startup_sequence()` - Common initialization for both modes
- `check_orphaned_graphs()` - Warn about graphs with no authorized agents
- `run_server_loop()` / `run_cli_loop()` - Mode-specific event loops
- `handle_graceful_shutdown()` - Transaction completion during shutdown
**Features**: Duration limits, signal handling, orphan detection, prime agent creation

### cli.rs
**Purpose**: CLI argument parsing and command execution
**Key macro**: `cli_commands!` - Single source of truth for all CLI commands
**Generated**: `Args` struct with clap annotations, `from_json_with_args()` for WebSocket bridge
**Functions**:
- `handle_cli_commands()` - Process CLI commands, returns true for early exit
- `show_cli_status()` - Display graph and agent status
- `dispatch_cli_command()` - WebSocket bridge for CLI execution
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

### lock.rs
**Purpose**: Lock handling with panic-on-poison strategy
**Traits**: `RwLockExt` (sync), `AsyncRwLockExt` (async)
**Methods**: `read_or_panic()`, `write_or_panic()` with context messages
**Features**: Contention warnings (debug), canonical ordering helper
**Lock ordering**: Use `lock_registries_for_write()` for graph→agent order

### app_state.rs
**Purpose**: Central state coordination
**Type**: `AppState` - coordinates graphs, registries, agents, transactions, WebSocket
**Key methods**:
- `new_cli()`, `new_server()` - mode-specific initialization
- `get_or_create_graph_manager()` - lazy graph loading
- `get_or_load_agent()`, `activate_agent()`, `deactivate_agent()` - agent lifecycle
- `create_new_graph()`, `delete_graph_completely()` - graph lifecycle
- `with_graph_transaction()` - transaction wrapper
- `run_graph_recovery()`, `run_all_graphs_recovery()` - crash recovery
- `cleanup_and_save()` - shutdown persistence
**Concurrency**: Per-agent/graph RwLocks, brief HashMap access
**Lock ordering**: Always graph_registry → agent_registry (use helper)

### server/http_api.rs
**Purpose**: HTTP API endpoints
**Endpoints**: `/` (health), `/import/logseq` (POST), `/ws` (upgrade), monitoring paths
**Auth**: Bearer token for protected endpoints, WebSocket auth post-upgrade

### graph_manager.rs
**Purpose**: Generic petgraph-based storage engine
**Features**: Domain-agnostic, StableGraph for index stability
**Operations**: `create_node()`, `create_or_update_node()`, `find_node()`, `add_edge()`, `archive_nodes()`
**Types**: Defined by domain (PKM: Page/Block nodes, PageRef/BlockRef edges)

### graph_operations.rs
**Purpose**: Multi-agent graph operations with runtime authorization
**Trait**: `GraphOps` - all operations require `agent_id: Uuid` and `graph_id: &Uuid`
**Operations**:
- Blocks: `add_block()`, `update_block()`, `delete_block()`
- Pages: `create_page()`, `delete_page()`
- Lifecycle: `create_graph()`, `delete_graph()`, `open_graph()`, `close_graph()`
- Queries: `get_node()`, `query_graph_bfs()`, `list_graphs()`, `list_open_graphs()`
**Transaction**: Operations store full parameters in WAL for recovery
**Recovery**: `OperationExecutor` replays without authorization checks
**Adding operations**: 1) Add to Operation enum 2) Add to GraphOps trait 3) Implement with auth check 4) Add OperationExecutor 5) Optional: Add to kg_tools 6) Optional: Add WebSocket command

### storage/mod.rs
**Purpose**: Persistence layer module
**Components**: Registries, TransactionLog, TransactionCoordinator, persistence utils
**Features**: Multi-graph management, ACID transactions, crash recovery
**Details**: See `src/storage/CLAUDE.md`

### storage/graph_persistence.rs
**Purpose**: Graph serialization utilities
**Operations**: `load_graph()`, `save_graph()`, `archive_nodes()`, `should_save()`
**Features**: JSON format, auto-save thresholds (5min/10ops)

### storage/graph_registry.rs
**Purpose**: Multi-graph UUID tracking with open/closed state
**Operations**:
- `register_graph()`, `remove_graph()` - lifecycle
- `open_graph()`, `close_graph()` - state management
- `resolve_graph_target()` - UUID/name resolution
- `create_new_graph_complete()`, `delete_graph_complete()` - full workflows
**Data**: `open_graphs: HashSet<Uuid>`, `authorized_agents` per graph
**Persistence**: Open state survives restarts
**Concurrency**: `Arc<RwLock<GraphRegistry>>` with panic-on-poison

### storage/agent_registry.rs
**Purpose**: Agent lifecycle and authorization management
**Operations**:
- `register_agent()`, `remove_agent()` - lifecycle
- `activate_agent()`, `deactivate_agent()` - memory management
- `authorize_agent_for_graph()`, `deauthorize_agent_from_graph()` - bidirectional auth
- `resolve_agent_target()` - UUID/name resolution
- `ensure_default_agent()` - create prime agent on first run
- `find_orphaned_graphs()` - detect graphs without agents
**Prime agent**: Auto-created, full graph access, cannot be deleted
**Persistence**: `agent_registry.json` with active/inactive states

### storage/agent_persistence.rs
**Purpose**: Agent persistence
**Operations**: `save_agent()`, `load_agent()`
**Auto-save**: 5 minutes or 10 messages
**Format**: JSON (conversation, LLM config, prompt, default_graph_id)

### storage/registry_utils.rs
**Purpose**: Shared UUID serialization
**Modules**: `uuid_hashmap_serde`, `uuid_hashset_serde`, `uuid_vec_serde`

### storage/transaction_log.rs
**Purpose**: Sled-based WAL
**Features**: SHA-256 deduplication, ACID guarantees
**Trees**: Transactions, content hash index, pending index

### storage/transaction.rs
**Purpose**: Transaction coordination with graceful shutdown
**States**: Active → Committed | Aborted
**Methods**:
- `create_transaction()`, `complete_transaction()` - lifecycle
- `recover_pending_transactions()` - crash recovery
- `initiate_shutdown()`, `wait_for_completion()` - graceful shutdown
**Per-graph**: Each graph has own TransactionCoordinator
**Shutdown**: Tracks active transactions, rejects new during shutdown

### agent/agent.rs
**Purpose**: Core Agent with conversation management and tool execution
**Types**: `Agent`, `Message` (User/Assistant/Tool with AgentContext)
**Features**: Conversation history, LLM backend, default graph, argument validation, auto-save
**Methods**:
- `chat()`, `process_message()` - LLM interaction
- `execute_tool()` - Tool execution with auth
- `get/set_default_graph_id()` - Default graph
- `reset_conversation()`, `get_history()`, `save()`, `load()`

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
**Purpose**: WebSocket protocol and command routing
**Architecture**: Core in websocket.rs, handlers in websocket_commands/
**Features**: Async task spawning, per-agent locking, smart resolution
**Performance**: Parallel execution for different agents
**Details**: See `src/server/CLAUDE.md` for full API

### import/mod.rs
**Purpose**: Import module exports
**Exports**: `import_logseq_graph()`, PKM types, ImportError
**Details**: See `src/import/CLAUDE.md`

### import/pkm_data.rs
**Purpose**: PKM data structures and graph application
**Types**: `PKMBlockData` (hierarchy, properties), `PKMPageData` (metadata), `PKMReference`
**Methods**:
- `apply_to_graph()` - Transform to nodes/edges
- `new_block()`, `new_page()` - Factory methods
**Node types**: Page, Block, ArchivedPage, ArchivedBlock
**Edge types**: PageRef, BlockRef, PageToBlock, ParentChild

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

### import/reference_resolver.rs
**Purpose**: Block reference resolution
**Functions**:
- `build_block_map_from_graph()` - ID → NodeIndex mapping
- `resolve_references_in_graph()` - Two-pass resolution
- `extract_block_references()` - `((block-id))` patterns
**Features**: Post-import resolution, circular prevention, reference_content updates

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

### tests/common/graph_validation.rs
**Purpose**: Automated graph state validation
**Type**: `GraphValidationFixture` - track and validate transformations
**Methods**:
- `expect_dummy_graph()` - test data expectations
- `expect_create_block()`, `expect_update_block()`, `expect_delete()` - node ops
- `expect_edge()` - relationship validation
- `validate_graph()` - check against persisted state

### tests/common/agent_validation.rs
**Purpose**: Agent state and conversation validation
**Types**: `AgentValidationFixture`, `AgentValidator`, `MessageOrderValidator`
**Methods**:
- `validate_agent_registry_schema()` - registry structure
- `expect_agent_created/deleted()` - lifecycle tracking
- `expect_user/assistant/tool_message()` - message validation
- `expect_authorization/deauthorization()` - graph access
**Patterns**: `Exact()`, `Contains()` for message matching
**Validation**: Sequence ordering (user→tool→assistant)


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
| **Import** | CLI/HTTP → Parse .md → Extract blocks → Create graph → Authorize prime agent |
| **Transaction** | Operation → Content hash → WAL → Graph update → Commit/Rollback |
| **WebSocket** | Auth → Agent select → Spawn task → Check auth → Transaction → Response |
| **Agent Chat** | Resolve agent → LLM complete → Execute tools → Save conversation → Return |
| **Agent Lifecycle** | Register → Save → Activate → Chat → Auto-save → Deactivate → Archive |
| **Recovery** | Startup: ALL graphs → Open: specific graph → Replay operations from WAL |
| **Auth** | Generate token → Save to `auth_token` → HTTP: Bearer header, WS: Auth command |
| **Prime Agent** | Auto-create → Authorize for all graphs → Cannot delete → Default agent |

### autodebugger/
**Purpose**: Git submodule - LLM developer utilities
**Features**: Log verbosity detection, command wrappers, doc validation
**Commands**: `remove-debug` (strip debug calls), `validate-docs` (check module docs)