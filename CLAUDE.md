# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check - don't filter with grep ever
cargo build                      # Build cymbiont server
cargo test                       # Run tests (quiet by default)
RUST_LOG=debug cargo run         # Run backend server with debug logging (do not alter default 3s duration or set a timeout)
```

## CLI Flags (Cymbiont Server)

- `--duration <SECONDS>`: Run server for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml )
- `--force-incremental-sync`: Force an incremental sync (updates modified blocks/pages since last sync and catches deletion events)
- `--force-full-sync`: Force a full database sync (needed if block content is being modified by external tools)
- `--graph <NAME>`: Launch with specific graph by name
- `--graph-path <PATH>`: Launch with specific graph by path
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--shutdown-server`: Shutdown running Cymbiont server gracefully

## Architecture
- See `cymbiont_architecture.md` for comprehensive codebase architecture

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints, sync logic
- **logseq_plugin/**: JavaScript plugin for Logseq real-time sync (see logseq_plugin/CLAUDE.md)
- **logseq_databases/**: Test graphs and multi-graph support
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence (configurable via data_dir in config.yaml)
  - **graph_registry.json**: Graph UUID mappings and metadata
  - **last_session.json**: Session persistence and active graph tracking
  - **archived_nodes/**: Global deletion archives
  - **graphs/{graph-id}/**: Per-graph isolated storage with knowledge_graph.json and transaction logs
  - **saga_transaction_log/**: Global saga coordination (sled database)
  - **transaction_log/**: Global transaction log (sled database)

### Project Structure
- **src/**
  - **main.rs**: Server orchestration, lifecycle management, Logseq launching
  - **api.rs**: HTTP endpoints and request handlers for data sync
  - **graph_manager.rs**: Petgraph-based knowledge graph storage engine
  - **config.rs**: YAML configuration loading and validation
  - **utils.rs**: Process management, datetime parsing, general utilities
  - **logging.rs**: Custom formatter (file:line only for ERROR/WARN)
  - **pkm_data.rs**: Shared data structures (PKMBlockData, PKMPageData)
  - **websocket.rs**: WebSocket server for bidirectional communication
  - **transaction_log.rs**: Write-ahead logging with sled database
  - **transaction.rs**: Transaction coordinator and state management
  - **saga.rs**: Saga pattern for multi-step workflows
  - **kg_api.rs**: Public API for knowledge graph operations
  - **graph_registry.rs**: Multi-graph identification and management
  - **session_manager.rs**: Logseq database session management and switching
  - **edn.rs**: EDN format manipulation for config.edn
- **tests/**: Integration tests
- **Cargo.toml**: Dependencies and metadata

## Codebase Guidelines
- Rust backend: use `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()` macros for logging (tracing crate)
- Error handling: use `thiserror` for custom error types, define module-specific `Error` enums and `type Result<T>` aliases
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
