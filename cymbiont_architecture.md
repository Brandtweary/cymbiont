# Cymbiont Architecture

## Repository Structure
```
cymbiont/
├── src/                           # Core knowledge graph engine
│   ├── main.rs                    # CLI entry point with --server flag + agent commands
│   ├── app_state.rs               # Centralized application state with agent management
│   ├── config.rs                  # YAML configuration management
│   ├── logging.rs                 # Custom tracing formatter
│   ├── utils.rs                   # Process management and utilities
│   ├── graph_manager.rs           # Petgraph-based knowledge graph engine
│   ├── graph_operations.rs        # Multi-agent graph operations with phantom type authorization
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
│   ├── server/                    # Server-specific functionality
│   │   ├── mod.rs                 # Server module exports
│   │   ├── http_api.rs            # HTTP endpoints (health, import, WebSocket upgrade)
│   │   ├── websocket.rs           # Real-time WebSocket with agent commands
│   │   ├── server.rs              # Server utilities and lifecycle
│   │   └── auth.rs                # Authentication system with token management
│   └── storage/                   # Persistence layer
│       ├── mod.rs                 # Storage module exports
│       ├── graph_persistence.rs   # Graph save/load utilities
│       ├── graph_registry.rs      # Multi-graph UUID management with agent tracking
│       ├── agent_registry.rs      # Agent lifecycle and authorization management
│       ├── agent_persistence.rs   # Agent save/load with auto-save thresholds
│       ├── registry_utils.rs      # Shared UUID serialization utilities
│       ├── registry_ref.rs        # Registry reference pattern for authorization checks
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
│       ├── logseq_import.rs       # CLI import tests
│       ├── websocket_commands.rs  # WebSocket tests
│       ├── agent_commands.rs      # Agent chat and admin command tests
│       └── freeze_mechanism.rs    # Operation freeze/unfreeze tests
└── autodebugger/                  # Git submodule: LLM developer utilities toolbag
```

## Module Requirements and Data Flow

### main.rs
**Purpose**: CLI entry point with unified runtime lifecycle management and agent commands
**Key functionality**: 
- Parse command line arguments including agent management commands
- Create AppState (both CLI and server modes) with agent registry initialization
- Run `run_all_graphs_recovery()` on startup for all open graphs
- Execute agent CLI commands (create, delete, activate, authorize, etc.)
- Handle duration limits and shutdown signals uniformly for both modes
- Execute cleanup_and_save() on exit (saves agents and graphs)
**Runtime behavior**: Controls duration timeout and graceful shutdown for both CLI and server modes
**Server mode**: Starts server via `server::start_server()` but manages lifecycle in main.rs
**Agent integration**: Ensures prime agent exists on first run for seamless experience

### config.rs
**Purpose**: YAML configuration loading with CLI overrides  
**Key types**: `Config`, `BackendConfig`, `DevelopmentConfig`

### app_state.rs
**Purpose**: Centralized application state management and coordination with agent integration
**Key types**: `AppState` - coordinates graph managers, registries, agents, transactions, and WebSocket connections
**Key methods**: 
- `new_cli()`, `new_server()` - initialization with agent registry loading
- `get_or_create_graph_manager()` - lazy graph manager creation
- `get_or_load_agent()`, `activate_agent()`, `deactivate_agent()` - agent lifecycle parallel to graphs
- `with_graph_transaction(graph_id)` - wraps operations in transactions for specific graph
- `initiate_graceful_shutdown()`, `wait_for_transactions()` - shutdown coordination
- `get_transaction_coordinator()` - access to per-graph WAL
- `cleanup_and_save()` - saves all agents and graphs on shutdown
**Agent state**: Manages `agents: HashMap<Uuid, Agent>` and `agent_registry: AgentRegistry`
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
**Purpose**: Multi-agent graph operations with compile-time authorization enforcement
**Architecture**: Four trait layers for security and clean API design:
- `GraphOps` - agent-aware operations with automatic authorization (public chokepoint API)
- `GraphOperationsExt` - raw operations without authorization (internal implementation)
- `WithAgent` / phantom types - compile-time authorization flow using `Authorized`/`Unauthorized` markers
- `OperationExecutor` - transaction replay interface (bypasses auth during recovery)
**Authorization pattern**: Phantom types make unauthorized operations impossible to compile:
```rust
state.with_agent(agent_id).authorize_for_graph(graph_id)?.add_block(...)  // Authorized
state.add_block_as(agent_id, ...)  // GraphOps chokepoint (recommended)
```
**Operations**: All operations require explicit `graph_id: &Uuid` parameter:
- Block operations: `add_block()`, `update_block()`, `delete_block()`
- Page operations: `create_page()`, `delete_page()`
- Graph lifecycle: `create_graph()`, `delete_graph()`, `open_graph()`, `close_graph()`
- Query operations: `get_node()`, `list_graphs()`, `list_open_graphs()`
- Recovery: `replay_transaction()` for crash recovery
**Transaction integration**: Each operation stores full API parameters in WAL for perfect recovery

### storage/mod.rs
**Purpose**: Persistence layer module with registry, transactions, and WAL logging  
**Components**: GraphRegistry, TransactionLog, TransactionCoordinator, graph_persistence utilities
**Key features**: Multi-graph management, ACID transactions, crash recovery, graph serialization

### storage/graph_persistence.rs
**Purpose**: Graph serialization and persistence utilities  
**Key operations**: `load_graph()`, `save_graph()`, `archive_nodes()`, `should_save()`
**Features**: JSON serialization, auto-save thresholds, node archival

### storage/graph_registry.rs
**Purpose**: Multi-graph UUID tracking and management with open/closed state and agent associations
**Key types**: Uses `Uuid` type throughout with custom JSON serialization
**Concurrency**: Uses `Arc<RwLock<GraphRegistry>>` with development-time contention detection
**Key operations**: 
- `register_graph()`, `remove_graph()` - graph lifecycle
- `open_graph()`, `close_graph()` - explicit state management
- `get_open_graphs()`, `is_graph_open()` - query graph states
- `resolve_graph_target()` - centralized UUID/name resolution with smart defaults
- `ensure_graph_open()` - startup logic to guarantee at least one open graph
**Data structure**: Tracks `open_graphs: HashSet<Uuid>` and `authorized_agents` per graph
**Persistence**: Open graph state persists across restarts for automatic recovery
**Safety pattern**: Write operations use `debug_assert!(registry.try_write().is_ok())` as tripwires to detect lock contention during development

### storage/agent_registry.rs
**Purpose**: Agent lifecycle and authorization management parallel to GraphRegistry
**Key types**: `AgentInfo` with metadata, `AgentRegistry` for lifecycle tracking
**Key operations**:
- `register_agent()`, `remove_agent()` - agent lifecycle
- `activate_agent()`, `deactivate_agent()` - memory management
- `authorize_agent_for_graph()`, `deauthorize_agent_from_graph()` - bidirectional authorization
- `resolve_agent_target()` - UUID/name resolution with prime agent fallback
- `ensure_default_agent()` - creates prime agent on first run
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

### storage/registry_ref.rs
**Purpose**: Registry reference pattern that prevents cache coherency issues
**Key type**: `RegistryRef<T>` - newtype wrapper that forces all state queries through authoritative registry
**Authorization**: Contains `is_authorized_for()` method for agent-graph authorization checks
**Phantom type delegation**: Provides `GraphOperationsExt` implementation for `AuthorizedAppState<Authorized>`
**Pattern benefit**: Makes it impossible to cache stale registry state locally

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
**Purpose**: Real-time WebSocket communication with agent integration and high-throughput async processing
**Architecture**: Each command spawns as independent async task for concurrent execution
**Protocol**: Request/response with token auth, heartbeat, async command execution
**Authorization**: All graph operations use `GraphOps` trait for automatic agent authorization
**Graph commands**: 
- `Auth { token }` - authentication (sets prime agent as current)
- `OpenGraph`, `CloseGraph` - explicit graph lifecycle
- `CreateBlock`, `UpdateBlock`, `DeleteBlock` - block operations (require current agent, accept optional graph_id/graph_name)
- `CreatePage`, `DeletePage` - page operations (require current agent, accept optional graph_id/graph_name)
- `CreateGraph`, `DeleteGraph` - graph management
**Agent chat commands**:
- `AgentChat { message, echo?, agent_id?, agent_name? }` - chat with agent (echo for testing)
- `AgentSelect { agent_id?, agent_name? }` - switch current agent for connection
- `AgentList` - list all agents with status
- `AgentHistory { limit?, agent_id?, agent_name? }` - get conversation history
- `AgentReset { agent_id?, agent_name? }` - clear conversation
- `AgentInfo { agent_id?, agent_name? }` - detailed agent information
**Agent admin commands**:
- `CreateAgent { name, description? }` - create new agent
- `DeleteAgent { agent_id?, agent_name? }` - remove agent (prime protected)
- `ActivateAgent`, `DeactivateAgent` - memory management
- `AuthorizeAgent`, `DeauthorizeAgent` - graph access control
**Test commands**: `FreezeOperations`, `UnfreezeOperations`, `GetFreezeState`
**Graph targeting**: All CRUD commands accept optional `graph_id` (UUID string) or `graph_name` fields
**Agent targeting**: All agent commands accept optional agent_id/agent_name, default to current/prime
**Responses**: `Success`, `Error`, `Heartbeat`
**Authentication**: Requires `Auth { token }` command before other operations
**Connection state**: Each WebSocket maintains current_agent_id (defaults to prime)

### import/logseq.rs
**Purpose**: Logseq-specific parsing and transformation  
**Key features**: Reads .md files, parses frontmatter, extracts blocks and hierarchies

### import/pkm_data.rs
**Purpose**: PKM data structures and graph application logic  
**Key types**: `PKMBlockData`, `PKMPageData`, `PKMReference`
**Key methods**: `apply_to_graph()` - Transforms PKM data into graph nodes/edges with reference resolution

### import/import_utils.rs
**Purpose**: High-level import coordination with agent authorization
**Key operations**: `import_logseq_graph()` - full graph import with prime agent authorization
**Integration**: Uses centralized `create_graph()` to ensure consistent agent authorization

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
  - **Graph operations**: `OpenGraph`, `CloseGraph`, `CreateBlock`, `UpdateBlock`, `DeleteBlock`, `CreatePage`, `DeletePage`, `CreateGraph`, `DeleteGraph` (all support optional graph_id/graph_name)
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

**WebSocket**: Client auth → Prime agent selection → Async command execution (spawned tasks) → Agent authorization via `GraphOps` → Transaction-wrapped operation → Success/Error response

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

### autodebugger/
**Purpose**: Git submodule providing LLM-oriented developer utilities  
**Features**: Automated log verbosity detection via tracing Layer, command execution wrappers for structured results, and utilities that address common pain points in LLM-assisted development. The VerbosityCheckLayer automatically monitors log output and warns when applications exceed reasonable thresholds (50/100/200 logs for INFO/DEBUG/TRACE levels).

**Usage Examples**:
```bash
autodebugger remove-debug              # Remove all debug! calls from current directory
autodebugger remove-debug src/         # Target specific directory
```