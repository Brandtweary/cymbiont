# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check - don't filter with grep/tail/head ever; cargo commands are always information-dense
cargo build                      # Build cymbiont
cargo test                       # Run full test suite (preferred - only filter by test during active troubleshooting)
RUST_LOG=debug cargo run         # Run cymbiont with debug logging (do not change duration or set a timeout unless user requests it)
env RUST_LOG=debug cargo test -- --nocapture 2>&1 | tee test_output.log  # Capture console output to file; do not filter before piping the full output
```

## CLI Flags

- `--server`: Run as HTTP/WebSocket server
- `--duration <SECONDS>`: Run for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml)
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--config <PATH>`: Use specific configuration file
- `--import-logseq <PATH>`: Import Logseq graph directory (then continues running)
- `--delete-graph <NAME_OR_ID>`: Archive a graph by name or ID (use `--force` if active graph)

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints
- **logseq_databases/**: Test graphs
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence (configurable via data_dir in config.yaml)
  - **IMPORTANT**: The data/ directory is git-tracked (has .gitkeep) - never rm -rf it
  - **graph_registry.json**: Graph UUID mappings and metadata
  - **auth_token**: Auto-generated authentication token (rotates on restart)
  - **graphs/{graph-id}/**: Per-graph isolated storage with knowledge_graph.json and transaction logs
  - **archived_graphs/**: Deleted graphs are moved here with timestamp
  - **transaction_log/**: Global transaction log (sled database)

### Project Structure
- **src/**
  - **main.rs**: CLI entry point with optional server mode (--server flag)
  - **graph_manager.rs**: Generic knowledge graph storage engine using petgraph
  - **config.rs**: YAML configuration loading and validation
  - **utils.rs**: Process management, datetime parsing, general utilities
  - **logging.rs**: Custom formatter (file:line only for ERROR/WARN)
  - **app_state.rs**: Centralized application state management
  - **import/**: Data import functionality
    - **pkm_data.rs**: PKM data structures and graph transformation logic
    - **logseq.rs**: Logseq-specific parsing
    - **import_utils.rs**: Import coordination
    - **reference_resolver.rs**: Block reference resolution
  - **storage/**: Persistence layer
    - **mod.rs**: Storage module exports
    - **graph_persistence.rs**: Graph save/load/archive utilities
    - **graph_registry.rs**: Multi-graph identification and management
    - **transaction_log.rs**: Write-ahead logging with sled database
    - **transaction.rs**: Transaction coordinator and state management
  - **graph_operations.rs**: PKM-oriented public API for knowledge graph operations
  - **server/**: Server-specific functionality
    - **http_api.rs**: HTTP endpoints for health, import, WebSocket upgrade
    - **websocket.rs**: WebSocket server for real-time communication
    - **auth.rs**: Authentication system with token generation and validation
    - **server.rs**: HTTP/WebSocket server setup and configuration
- **tests/**: Test binaries (e.g. integration tests) - see `tests/CLAUDE.md` for test harness details
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

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")
