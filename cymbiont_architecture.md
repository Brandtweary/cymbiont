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
│   │   ├── agent.rs               # Core Agent struct with conversation management
│   │   ├── llm.rs                 # LLM backend abstraction and MockLLM implementation
│   │   ├── kg_tools.rs            # Knowledge graph tool registry
│   │   └── schemas.rs             # Ollama-compatible tool schemas
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
│   │   └── agent_validation.rs    # Agent state and conversation validation
│   └── integration/               # Integration test suite (single binary)
│       ├── main.rs                # Test entry point
│       ├── crash_recovery.rs      # Transaction recovery tests
│       ├── http_logseq_import.rs  # HTTP API tests
│       ├── websocket_commands.rs  # WebSocket tests
│       ├── agent_commands.rs      # Agent chat and admin command tests
│       ├── cli_commands.rs        # CLI command tests with contract enforcement
│       └── freeze_mechanism.rs    # Operation freeze/unfreeze tests
├── autodebugger/                  # Git submodule: LLM developer utilities toolbag
└── build.rs                       # Build script: enforces tracing macro usage
```

## Build Script

The build.rs script performs two critical functions:
1. **Enforces tracing macro usage**: Detects and fails the build when println!, eprintln!, print!, eprint!, or dbg! macros are found in src/ or tests/ directories
2. **CLI command extraction**: Parses the `cli_commands!` macro invocation to generate a list of all commands for integration test contract enforcement

## Module Requirements and Data Flow

### main.rs
**Purpose**: Application entry point with phased startup sequence and unified runtime management
**Key functionality**: 
- Execute 5-phase startup: initialization, common startup, command handling, runtime loop, cleanup
- Check for orphaned graphs during startup and warn users
- Handle duration limits and shutdown signals uniformly for both modes
**Key functions**:
- `run_startup_sequence()` - Common initialization for both server and CLI modes
- `check_orphaned_graphs()` - Warns about graphs with no authorized agents
- `run_server_loop()` / `run_cli_loop()` - Mode-specific runtime event loops
- `handle_graceful_shutdown()` - Manages transaction completion during shutdown
**Runtime behavior**: Controls duration timeout and graceful shutdown for both CLI and server modes
**Agent integration**: Ensures prime agent exists on first run for seamless experience

### cli.rs
**Purpose**: CLI argument parsing and command execution with macro-based contract enforcement
**Key functionality**:
- Parse command line arguments using clap via generated `Args` struct
- Execute all CLI-specific commands with early exit support
- Handle agent management commands (create, delete, activate, authorize, etc.)
- Process graph operations (import, delete, list)
- Display system status information
**Key macro**: `cli_commands!` - Single source of truth for all CLI commands
**Generated code**:
- `Args` struct with all command fields and clap annotations
- `from_json_with_args()` - JSON to Args conversion for WebSocket bridge
**Key functions**:
- `handle_cli_commands()` - Processes CLI-specific commands, returns true for early exit
- `show_cli_status()` - Displays graph and agent status information
- `dispatch_cli_command()` - WebSocket bridge for CLI command execution
**Contract enforcement**: Build script extracts commands for test verification

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### error.rs
**Purpose**: Hierarchical error system with domain-specific types for consistent error handling
**Key types**: 
- `CymbiontError` - Root error type wrapping all domain errors
- `StorageError` - Registry, persistence, transaction errors
- `AgentError` - LLM backend and tool errors
- `GraphError` - Graph operations and lifecycle errors
- `ServerError` - WebSocket, HTTP, authentication errors
- `ImportError` - Data import and parsing errors
- `ConfigError` - Configuration parsing errors
**Key patterns**:
- Global `Result<T>` type alias for consistency
- Automatic `From` trait implementations for error conversion
- Convenience constructors for type-safe error creation (e.g., `StorageError::graph_registry("message")`)
- Idiomatic `?` operator usage throughout codebase
**Usage example**:
```rust
use crate::error::*;
fn example() -> Result<()> {
    // Create domain-specific errors
    return Err(StorageError::not_found("graph", "id", graph_id).into());
    // Or use ? for automatic conversion
    serde_json::from_str(&data)?;  // Automatically converts to StorageError::Serialization
}

### lock.rs
**Purpose**: Lock handling utilities implementing panic-on-poison strategy with contention detection
**Key traits**:
- `RwLockExt` - Extension for `std::sync::RwLock` with `read_or_panic()` and `write_or_panic()`
- `AsyncRwLockExt` - Extension for `tokio::sync::RwLock` with consistent async API
**Key features**:
- Panic-on-poison for data integrity (sync locks only)
- Automatic lock contention warnings in debug builds
- Descriptive context messages for debugging
- `lock_registries_for_write()` - Enforces canonical lock ordering
**Design rationale**: Poisoned locks indicate data corruption; immediate panic preferred over recovery attempts

### app_state.rs
**Purpose**: Centralized application state management and coordination with agent integration
**Key types**: `AppState` - coordinates graph managers, registries, agents, transactions, and WebSocket connections
**Key methods**: 
- `new_cli()`, `new_server()` - initialization with agent registry loading
- `get_or_create_graph_manager()` - lazy graph manager creation
- `get_or_load_agent()`, `activate_agent()`, `deactivate_agent()` - agent lifecycle parallel to graphs (delegates to registry complete workflows)
- `create_new_graph()`, `delete_graph_completely()` - graph lifecycle (delegates to registry complete workflows)
- `with_graph_transaction(graph_id)` - wraps operations in transactions for specific graph
- `run_graph_recovery(graph_id)` - replay pending transactions for specific graph (delegates to helper functions)
- `run_all_graphs_recovery()` - startup recovery for all graphs (both open and closed)
- `initiate_graceful_shutdown()`, `wait_for_transactions()` - shutdown coordination
- `get_transaction_coordinator()` - access to per-graph WAL
- `cleanup_and_save()` - saves all agents and graphs on shutdown
**Agent state**: Manages `agents: HashMap<Uuid, Agent>` and `agent_registry: AgentRegistry`
**Lock usage**: Uses `read_or_panic()` and `write_or_panic()` for all lock operations
**Role**: Acts as the central nervous system, connecting all components without implementing business logic

### server/http_api.rs
**Purpose**: HTTP API endpoints for health checks, imports, and WebSocket upgrades  
**Key endpoints**: Health check (`/`), Logseq import (`/import/logseq`), WebSocket upgrade (`/ws`), monitoring endpoints
**Auth**: Middleware protection for sensitive endpoints, WebSocket auth handled post-upgrade

### graph_manager.rs
**Purpose**: Generic knowledge graph storage engine using petgraph  
**Key features**: Domain-agnostic graph operations, StableGraph for index stability, automatic persistence
**Operations**: `create_node()`, `create_or_update_node()`, `find_node()`, `add_edge()`, `archive_nodes()`
**Node/Edge types**: Defined by domain layer (e.g., PKM defines Page/Block nodes, PageRef/BlockRef edges)

### graph_operations.rs
**Purpose**: Multi-agent graph operations with runtime authorization enforcement
**Key trait**: `GraphOps` - single public API for all graph operations with agent authorization
**Authorization**: Runtime checks at operation entry verify agent permissions via AgentRegistry
**Operations**: All operations require `agent_id: Uuid` and `graph_id: &Uuid` parameters:
- Block operations: `add_block()`, `update_block()`, `delete_block()`
- Page operations: `create_page()`, `delete_page()`
- Graph lifecycle: `create_graph()`, `delete_graph()`, `open_graph()`, `close_graph()`
- Query operations: `get_node()`, `query_graph_bfs()`, `list_graphs()`, `list_open_graphs()`
- Recovery: Operations replayed via `OperationExecutor::execute_operation()` for crash recovery
**Transaction integration**: Each operation stores full API parameters including agent_id in WAL for perfect recovery
**OperationExecutor**: Trait for transaction replay that bypasses authorization during recovery

**Adding New Graph Operations**:
1. Define Operation variant in `storage/transaction_log.rs` `Operation` enum
2. Add trait method to `GraphOps` trait with `agent_id: Uuid` as first parameter
3. Implement the operation in `impl GraphOps for Arc<AppState>`:
   - Add runtime authorization check at start
   - Wrap core logic in transaction if modifying data
   - Store operation with full parameters for recovery
4. Implement `OperationExecutor` for the new `Operation` variant
5. (Optional) Register in tool registry at `agent/kg_tools.rs` for LLM access
6. (Optional) Add WebSocket command in `server/websocket.rs` for real-time access

### storage/mod.rs
**Purpose**: Persistence layer module with registry, transactions, and WAL logging  
**Components**: GraphRegistry, TransactionLog, TransactionCoordinator, graph_persistence utilities
**Key features**: Multi-graph management, ACID transactions, crash recovery, graph serialization
**Module documentation**: See `src/storage/CLAUDE.md` for condensed overview 💾

### storage/graph_persistence.rs
**Purpose**: Graph serialization and persistence utilities  
**Key operations**: `load_graph()`, `save_graph()`, `archive_nodes()`, `should_save()`
**Features**: JSON serialization, auto-save thresholds, node archival

### storage/graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management with open/closed state and agent associations
**Key types**: Uses `Uuid` type throughout with custom JSON serialization
**Concurrency**: Uses `Arc<RwLock<GraphRegistry>>` with panic-on-poison strategy
**Key operations**: 
- `register_graph()`, `remove_graph()` - graph lifecycle
- `open_graph()`, `close_graph()` - explicit state management
- `get_open_graphs()`, `is_graph_open()` - query graph states
- `resolve_graph_target()` - centralized UUID/name resolution with smart defaults
- `ensure_graph_open()` - startup logic to guarantee at least one open graph
- `create_new_graph_complete()`, `delete_graph_complete()` - complete workflows with prime agent authorization
**Data structure**: Tracks `open_graphs: HashSet<Uuid>` and `authorized_agents` per graph
**Persistence**: Open graph state persists across restarts for automatic recovery
**Lock handling**: All lock operations use `write_or_panic()` with automatic contention detection

### storage/agent_registry.rs
**Purpose**: Agent lifecycle and authorization management parallel to GraphRegistry
**Key types**: `AgentInfo` with metadata, `AgentRegistry` for lifecycle tracking
**Key operations**:
- `register_agent()`, `remove_agent()` - agent lifecycle
- `activate_agent()`, `deactivate_agent()` - memory management
- `authorize_agent_for_graph()`, `deauthorize_agent_from_graph()` - bidirectional authorization
- `resolve_agent_target()` - UUID/name resolution with prime agent fallback
- `ensure_default_agent()` - creates prime agent on first run
- `find_orphaned_graphs()` - identifies graphs with no authorized agents
- `activate_agent_complete()`, `deactivate_agent_complete()` - complete workflows with persistence
**Prime agent**: Auto-created default agent with full graph access for seamless experience
**Persistence**: Saves to `agent_registry.json` with active/inactive state tracking

### storage/agent_persistence.rs
**Purpose**: Agent save/load functionality with auto-save thresholds
**Key operations**: `save_agent()`, `load_agent()` - full agent state serialization
**Auto-save triggers**: Time-based (5 minutes) and message-based (10 messages)
**Data format**: JSON with conversation history, LLM config, system prompt

### storage/registry_utils.rs
**Purpose**: Shared UUID serialization utilities for both registries
**Key modules**: `uuid_hashmap_serde`, `uuid_hashset_serde`, `uuid_vec_serde`
**Design**: Prevents code duplication between GraphRegistry and AgentRegistry

### storage/transaction_log.rs
**Purpose**: Write-ahead logging with sled database  
**Features**: Content hash deduplication, ACID guarantees, crash recovery
**Trees**: Transactions, content hash index, pending index

### storage/transaction.rs
**Purpose**: Transaction lifecycle coordination with graceful shutdown support and AppState verbosity reduction
**States**: `Active` → `Committed` | `Aborted`
**Key methods**: 
- `create_transaction()`, `complete_transaction()` - transaction lifecycle
- `recover_pending_transactions()` - crash recovery
- `initiate_shutdown()`, `wait_for_completion()` - graceful shutdown coordination
- `run_single_graph_recovery_helper()`, `save_graph_after_recovery_helper()` - extracted helpers to reduce AppState verbosity
**Per-graph isolation**: Each graph has its own TransactionCoordinator instance
**Shutdown behavior**: Tracks all active transactions, rejects new ones during shutdown

### agent/agent.rs
**Purpose**: Core Agent struct with conversation management and LLM interaction
**Key types**: `Agent`, `Message` enum (User/Assistant/Tool)
**Key features**:
- Conversation history with automatic context window management
- LLM backend configuration per agent
- System prompt customization
- Auto-save on configuration changes
**Methods**: `chat()`, `reset_conversation()`, `get_history()`, `save()`, `load()`

### agent/llm.rs
**Purpose**: LLM backend abstraction with MockLLM implementation
**Key types**: `LLMBackend` trait, `LLMConfig` enum, `MockLLM` struct
**MockLLM**: Test implementation with echo support for deterministic testing
**Interface**: `complete()` for message completion, `health_check()` for connectivity

### agent/kg_tools.rs
**Purpose**: Knowledge graph tool registry for agent-graph interaction
**Key types**: `KGTool` enum, `ToolRegistry` struct
**Tools**: Graph CRUD operations wrapped for agent execution

### agent/schemas.rs
**Purpose**: Ollama-compatible tool schemas for function calling
**Key types**: `ToolSchema`, `ParameterSchema`
**Design**: JSON schemas describing available graph operations

### server/server.rs
**Purpose**: Server lifecycle management and port finding
**Functions**: `start_server()` - finds available port, creates Axum server, returns handle for external control
**Features**: Automatic port selection, server info file management, graceful previous instance cleanup

### server/websocket.rs & websocket_commands/
**Purpose**: WebSocket protocol handling and command routing to domain-specific handlers
**Architecture**: Core protocol in websocket.rs, commands split into agent/graph/misc handlers
**Command categories**: 
- **Graph operations**: Block/page CRUD, graph lifecycle (all require agent authorization via GraphOps)
- **Agent operations**: Chat, selection, history, administration, authorization management
- **System commands**: Auth, test, freeze/unfreeze for deterministic testing
**Key features**: Async task spawning per message, smart graph/agent resolution, prime agent defaults
**Error handling**: Uses hierarchical error system with domain-specific types (ServerError, etc.)
**Lock handling**: All async locks use `read_or_panic()` and `write_or_panic()` extension methods
**Full API reference**: See `src/server/CLAUDE.md` for complete command documentation

### import/mod.rs
**Purpose**: Import module exports and error definitions
**Key exports**: `import_logseq_graph()`, PKM data types, ImportError
**Module documentation**: See `src/import/CLAUDE.md` for condensed overview 🚀

### import/pkm_data.rs
**Purpose**: PKM data structures and graph application logic  
**Key types**: 
- `PKMBlockData` - Block content with hierarchy, timestamps, properties
- `PKMPageData` - Page metadata with block lists
- `PKMReference` - Cross-references between blocks
**Key methods**: 
- `apply_to_graph()` - Transforms PKM data into graph nodes/edges with reference resolution
- `new_block()`, `new_page()` - Factory methods for creating PKM data structures
- `create_or_update_page()`, `update_block_content()` - Complex operation helpers
**Node types**: Page, Block (with archived variants)
**Edge types**: PageRef, BlockRef, PageToBlock, ParentChild

### import/logseq.rs
**Purpose**: Logseq-specific parsing and transformation  
**Key functions**:
- `parse_logseq_directory()` - Entry point for directory scanning
- `parse_markdown_file()` - Extract pages and blocks from .md files
- `extract_properties()` - Parse YAML frontmatter
- `parse_blocks()` - Hierarchical block extraction with indentation tracking
**Key features**: 
- Reads .md files recursively
- Parses frontmatter properties
- Extracts nested block hierarchies
- Detects `((block-id))` references

### import/import_utils.rs
**Purpose**: High-level import coordination with agent authorization
**Key operations**: 
- `import_logseq_graph()` - Full import workflow with transaction wrapping
- Creates new graph via `create_graph()` 
- Authorizes prime agent automatically
- Returns graph UUID for further operations
**Integration**: Uses centralized `create_graph()` to ensure consistent agent authorization
**Progress tracking**: Logs import stages for monitoring

### import/reference_resolver.rs
**Purpose**: Block reference resolution during import  
**Key functions**:
- `build_block_map_from_graph()` - Creates ID → NodeIndex mapping
- `resolve_references_in_graph()` - Two-pass reference resolution
- `extract_block_references()` - Pattern matching for `((block-id))`
**Key features**: 
- Resolves `((block-id))` references post-import
- Prevents circular references
- Updates reference_content field with resolved text
**Helper functions**: Simplified resolution patterns for maintainability

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

### tests/common/agent_validation.rs
**Purpose**: Agent state and conversation validation for integration tests
**Key types**: `AgentValidationFixture`, `AgentValidator`, `MessageOrderValidator`
**Key methods**:
- `validate_agent_registry_schema()` - verify registry structure
- `expect_agent_created()`, `expect_agent_deleted()` - track lifecycle
- `expect_user_message()`, `expect_assistant_message()` - validate conversations
- `expect_authorization()`, `expect_deauthorization()` - track graph access
- `validate_all()` - comprehensive validation against disk state
**Critical feature**: Message ordering validation ensures conversation integrity


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
  - **Authentication**: `Auth { token }`
  - **Graph operations**: `OpenGraph`, `CloseGraph`, `CreateBlock`, `UpdateBlock`, `DeleteBlock`, `CreatePage`, `DeletePage`, `CreateGraph`, `DeleteGraph`, `ListGraphs` (all support optional graph_id/graph_name except ListGraphs)
  - **Agent chat**: `AgentChat`, `AgentSelect`, `AgentList`, `AgentHistory`, `AgentReset`, `AgentInfo` (all support optional agent_id/agent_name)
  - **Agent admin**: `CreateAgent`, `DeleteAgent`, `ActivateAgent`, `DeactivateAgent`, `AuthorizeAgent`, `DeauthorizeAgent`
  - **Testing**: `FreezeOperations`, `UnfreezeOperations`, `GetFreezeState`
  - **Keep-alive**: `Heartbeat`
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
  # Server mode
  --server                      # Run as HTTP/WebSocket server
  
  # Graph management
  --import-logseq <PATH>        # Import Logseq graph directory
  --delete-graph <NAME_OR_ID>   # Delete a graph by name or UUID
  --list-graphs                 # List all graphs with metadata
  
  # Agent management
  --create-agent <NAME>         # Create new agent
    --agent-description <DESC>  # Optional agent description
  --delete-agent <NAME_OR_ID>   # Delete agent by name or UUID
  --activate-agent <NAME_OR_ID> # Activate agent by name or UUID
  --deactivate-agent <NAME_OR_ID> # Deactivate agent by name or UUID
  --agent-info <NAME_OR_ID>     # Show agent info by name or UUID
  
  # Agent authorization
  --authorize-agent <NAME_OR_ID>  # Authorize agent for graph
    --for-graph <NAME_OR_ID>       # Graph to authorize for
  --deauthorize-agent <NAME_OR_ID> # Remove agent graph access
    --from-graph <NAME_OR_ID>      # Graph to deauthorize from
  
  # Runtime options
  --data-dir <PATH>             # Override data directory
  --config <PATH>               # Use specific configuration file
  --duration <SECONDS>          # Run for specific duration
```

## Graceful Shutdown

`main.rs` handles SIGINT (Ctrl+C) for graceful shutdown in both CLI and server modes:
- First Ctrl+C: Initiates graceful shutdown, waits up to 30 seconds for active transactions to complete
- Second Ctrl+C: Forces immediate termination with transaction log flush

The shutdown sequence runs `cleanup_and_save()` to close WebSocket connections, persist all graphs, and flush transaction logs. After graceful cleanup, the process uses `std::process::exit(0)` to terminate immediately due to sled database background I/O threads that cannot be cleanly shutdown (known upstream issue).

## Key Flows

**Logseq Import**: HTTP POST/CLI → Path validation → .md file discovery → Frontmatter parsing → Block extraction → Reference resolution → Graph creation → Prime agent authorization

**Transaction**: Operation → Content hash → WAL log → Graph update → Commit/rollback

**WebSocket**: Client auth → Prime agent selection → Async command execution (spawned tasks) → Runtime authorization check → Transaction-wrapped operation → Success/Error response

**Agent Chat**: WebSocket command → Resolve agent target → Load agent if needed → LLM completion → Save conversation → Return response

**Agent Lifecycle**: Create in registry → Save to disk → Activate (load to memory) → Chat interactions → Auto-save triggers → Deactivate (save and unload) → Archive on deletion

**Multi-Instance**: Configurable `server_info_file` enables concurrent server instances with isolated discovery

**Authentication**: Server generates auth token on startup, saves to `{data_dir}/auth_token`. HTTP endpoints check Authorization header, WebSocket requires Auth command. Token rotates on restart for security. Auth command sets prime agent as current.

**Crash Recovery**: 
1. On startup (main.rs): Runs `run_all_graphs_recovery()` for ALL graphs (both open and closed)
   - Iterates through every registered graph
   - Temporarily opens closed graphs for recovery
   - Closes them again after recovery completes
2. On graph open: `open_graph()` triggers recovery for that specific graph
3. Recovery mechanism: Operations store full API parameters, replay calls exact same methods
4. Transaction states: Active → Committed (success) or Aborted (failure)
5. Open graphs persist across restarts: Registry tracks which graphs were open for automatic recovery

**Prime Agent**: Auto-created on first run → Authorized for all new graphs → Cannot be deleted → Default for all operations

**Lock Ordering**: To prevent deadlocks, all code that needs both `graph_registry` and `agent_registry` locks must acquire them in a consistent order:
1. `graph_registry` (SyncRwLock) - Always acquired first
2. `agent_registry` (SyncRwLock) - Acquired after graph_registry
3. Use `lock_registries_for_write()` function from `lock.rs` to ensure correct ordering

### autodebugger/
**Purpose**: Git submodule providing LLM-oriented developer utilities  
**Features**: Automated log verbosity detection via tracing Layer, command execution wrappers for structured results, and utilities that address common pain points in LLM-assisted development. The VerbosityCheckLayer automatically monitors log output and warns when applications exceed reasonable thresholds (50/100/200 logs for INFO/DEBUG/TRACE levels). Also provides a complete tracing subscriber with clean console output optimized for terminal development.

**Usage Examples**:
```bash
autodebugger remove-debug              # Remove all debug! calls from current directory
autodebugger remove-debug src/         # Target specific directory
```