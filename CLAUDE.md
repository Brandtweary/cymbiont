# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check - don't filter with grep ever
cargo build                      # Build cymbiont server
cargo test                       # Run tests (quiet by default)
RUST_LOG=debug cargo run         # Run backend server with debug logging (do not alter default 3s duration or set a timeout)
```

## CLI Flags

- `--server`: Run as HTTP/WebSocket server
- `--duration <SECONDS>`: Run for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml)
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--shutdown`: Shutdown running Cymbiont instance gracefully

## Architecture
- See `cymbiont_architecture.md` for comprehensive codebase architecture

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints
- **logseq_databases/**: Test graphs and multi-graph support
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence (configurable via data_dir in config.yaml)
  - **graph_registry.json**: Graph UUID mappings and metadata
  - **graphs/{graph-id}/**: Per-graph isolated storage with knowledge_graph.json and transaction logs
  - **transaction_log/**: Global transaction log (sled database)

### Project Structure
- **src/**
  - **main.rs**: CLI entry point with optional server mode (--server flag)
  - **graph_manager.rs**: Petgraph-based knowledge graph storage engine
  - **config.rs**: YAML configuration loading and validation
  - **utils.rs**: Process management, datetime parsing, general utilities
  - **logging.rs**: Custom formatter (file:line only for ERROR/WARN)
  - **pkm_data.rs**: Shared data structures (PKMBlockData, PKMPageData)
  - **transaction_log.rs**: Write-ahead logging with sled database
  - **transaction.rs**: Transaction coordinator and state management
  - **saga.rs**: Saga pattern for multi-step workflows
  - **graph_registry.rs**: Multi-graph identification and management
  - **app_state.rs**: Centralized application state management
  - **server/**: Server-specific functionality
    - **api.rs**: HTTP endpoints and request handlers
    - **websocket.rs**: WebSocket server for real-time communication
    - **kg_api.rs**: Public API for knowledge graph operations (currently unused)
    - **server.rs**: Server utility functions
- **tests/**: Integration tests
- **Cargo.toml**: Dependencies and metadata

## Codebase Guidelines
- Logging: use `tracing` macros - `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()`
- Error handling: use `thiserror` for custom error types; define module-specific `Error` enums and `type Result<T>` aliases
- Don't make live LLM calls during tests

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")

## Continuous Documentation

- Keep the architecture document (`cymbiont_architecture.md`) up to date
- When updating documentation, read it entirely first to avoid redundancy
