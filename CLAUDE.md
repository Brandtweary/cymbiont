# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check - don't filter with grep/tail/head ever; cargo commands are always information-dense
cargo build                      # Build cymbiont server
cargo test                       # Run tests (quiet by default)
RUST_LOG=debug cargo run         # Run backend server with debug logging (do not alter default 3s duration or set a timeout)
env RUST_LOG=debug cargo test -- --nocapture 2>&1 | tee test_output.log  # Capture console output to file; do not filter before piping the full output
```

## CLI Flags

- `--server`: Run as HTTP/WebSocket server
- `--duration <SECONDS>`: Run for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml)
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--config <PATH>`: Use specific configuration file
- `--import-logseq <PATH>`: Import Logseq graph directory (then continues running)

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints
- **logseq_databases/**: Test graphs
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence (configurable via data_dir in config.yaml)
  - **IMPORTANT**: The data/ directory is git-tracked (has .gitkeep) - never rm -rf it
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
  - **app_state.rs**: Centralized application state management
  - **import/**: Data import functionality
    - **pkm_data.rs**: PKM data structures (PKMBlockData, PKMPageData)
    - **logseq.rs**: Logseq-specific parsing
    - **import_utils.rs**: Import coordination
    - **reference_resolver.rs**: Block reference resolution
  - **storage/**: Persistence layer
    - **mod.rs**: Storage module exports
    - **graph_registry.rs**: Multi-graph identification and management
    - **transaction_log.rs**: Write-ahead logging with sled database
    - **transaction.rs**: Transaction coordinator and state management
  - **graph_operations.rs**: Standardized public interface for all graph operations
  - **server/**: Server-specific functionality
    - **http_api.rs**: HTTP endpoints for health, import, WebSocket upgrade
    - **websocket.rs**: WebSocket server for real-time communication
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
- Whenever you update `config.example.yaml` ensure that you also update `config.yaml`, and vice versa
- Don't make live LLM calls during tests

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")
