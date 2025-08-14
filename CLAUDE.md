# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check - don't filter with grep/tail/head ever; cargo commands are always information-dense
cargo build                      # Build cymbiont
cargo test                       # Run full test suite (preferred - only filter by test during active troubleshooting)
RUST_LOG=debug cargo run         # Run cymbiont with debug logging (do not change duration or set a timeout unless user requests it)
env RUST_LOG=debug cargo test -- --nocapture 2>&1 | tee test_output.log  # Capture console output to file; do not filter before piping the full output
# NEVER: cargo run 2>&1 | tail   # WRONG: '2' becomes an argument to cargo! Plus we have verbosity checks now, just run without filtering
```

## CLI Flags

### Server & Runtime
- `--server`: Run as HTTP/WebSocket server
- `--duration <SECONDS>`: Run for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml)
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--config <PATH>`: Use specific configuration file

### Graph Management
- `--import-logseq <PATH>`: Import Logseq graph directory (then continues running)
- `--delete-graph <NAME_OR_ID>`: Archive a graph by name or ID

### Agent Management
- `--create-agent <NAME>`: Create new agent
- `--agent-description <DESC>`: Optional description for the new agent (used with --create-agent)
- `--delete-agent <NAME_OR_ID>`: Delete agent by name or ID
- `--activate-agent <NAME_OR_ID>`: Activate agent by name or ID
- `--deactivate-agent <NAME_OR_ID>`: Deactivate agent by name or ID
- `--agent-info <NAME_OR_ID>`: Show agent info by name or ID
- `--authorize-agent <NAME_OR_ID>`: Authorize agent for a graph
- `--for-graph <NAME_OR_ID>`: Graph to authorize the agent for (used with --authorize-agent)
- `--deauthorize-agent <NAME_OR_ID>`: Deauthorize agent from a graph
- `--from-graph <NAME_OR_ID>`: Graph to deauthorize the agent from (used with --deauthorize-agent)

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints
- **logseq_databases/**: Test graphs
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph and agent persistence (configurable via data_dir in config.yaml)
  - **IMPORTANT**: The data/ directory is git-tracked (has .gitkeep) - never rm -rf it
  - **graph_registry.json**: Graph UUID mappings and metadata with agent associations
  - **agent_registry.json**: Agent UUID mappings and graph authorizations
  - **auth_token**: Auto-generated authentication token (rotates on restart)
  - **graphs/{graph-id}/**: Per-graph isolated storage with knowledge_graph.json and transaction logs
  - **agents/{agent-id}/**: Per-agent storage with agent.json (conversation history, LLM config)
  - **archived_graphs/**: Deleted graphs are moved here with timestamp
  - **archived_agents/**: Deleted agents are moved here with timestamp
  - **transaction_log/**: Global transaction log (sled database)

### Project Structure
- **src/**
  - **main.rs**: CLI entry point with optional server mode (--server flag) and agent commands
  - **graph_manager.rs**: Generic knowledge graph storage engine using petgraph
  - **config.rs**: YAML configuration loading and validation
  - **utils.rs**: Process management, datetime parsing, general utilities
  - **logging.rs**: Custom formatter (file:line only for ERROR/WARN)
  - **app_state.rs**: Centralized application state management with agent integration
  - **graph_operations.rs**: PKM-oriented public API for knowledge graph operations
  - **agent/**: Agent abstraction layer
    - **mod.rs**: Agent module exports
    - **agent.rs**: Core Agent struct with conversation management
    - **llm.rs**: LLM backend abstraction and MockLLM implementation
    - **kg_tools.rs**: Knowledge graph tool registry
    - **schemas.rs**: Ollama-compatible tool schemas
  - **import/**: Data import functionality
    - **pkm_data.rs**: PKM data structures and graph transformation logic
    - **logseq.rs**: Logseq-specific parsing
    - **import_utils.rs**: Import coordination with agent authorization
    - **reference_resolver.rs**: Block reference resolution
  - **storage/**: Persistence layer
    - **mod.rs**: Storage module exports
    - **graph_persistence.rs**: Graph save/load/archive utilities
    - **graph_registry.rs**: Multi-graph identification and management with agent tracking
    - **agent_registry.rs**: Agent lifecycle and authorization management
    - **agent_persistence.rs**: Agent save/load with auto-save thresholds
    - **registry_utils.rs**: Shared UUID serialization utilities
    - **transaction_log.rs**: Write-ahead logging with sled database
    - **transaction.rs**: Transaction coordinator and state management
  - **server/**: Server-specific functionality
    - **http_api.rs**: HTTP endpoints for health, import, WebSocket upgrade
    - **websocket.rs**: WebSocket server with agent chat and admin commands
    - **auth.rs**: Authentication system with token generation and validation
    - **server.rs**: HTTP/WebSocket server setup and configuration
- **tests/**: Test binaries (e.g. integration tests) - see `tests/CLAUDE.md` for test harness details
- **autodebugger/**: Git submodule - LLM developer utilities with automated log verbosity detection
- **.gitignore**: Git ignore patterns
- **.gitmodules**: Git submodule configuration
- **Cargo.toml**: Dependencies and metadata
- **config.example.yaml**: Example configuration template
- **config.yaml**: Runtime configuration (overrides defaults, not tracked)
- **cymbiont_architecture.md**: Comprehensive codebase architecture document - keep this up-to-date as best you can
- **README.md**: User documentation and setup guide

## Codebase Guidelines
- Logging: use `tracing` macros - `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()`
- Error handling: use `thiserror` for custom error types; define module-specific `Error` enums and `type Result<T>` aliases
- Whenever you update `config.example.yaml` ensure that you also update `config.yaml`
- Don't make live LLM calls during tests
- When modifying startup logic in `main.rs`, ensure BOTH the CLI path and server path are updated equally. Extract shared logic into functions to avoid divergence.

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")

### Autodebugger Commands
- **Remove debug! calls**: `autodebugger remove-debug` (default: targets src/ and tests/ directories)
