# CYMBIONT DEVELOPMENT GUIDE

## Documentation Structure
- **cymbiont_architecture.md**: Complete codebase reference
- **src/*/CLAUDE.md**: Module-specific guides
- **tests/CLAUDE.md**: Test harness and utilities
- **README.md**: User documentation

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check
cargo check --message-format short  # Concise output (preferred over grep filtering - only use with 10+ errors)
cargo build                      # Build cymbiont
cargo test                       # Run full test suite (preferred - only filter by test during active troubleshooting)
RUST_LOG=debug cargo run         # Run cymbiont with debug logging (do not change duration or set a timeout unless user requests it)
# NEVER: cargo run 2>&1 | tail   # WRONG: '2' becomes an argument to cargo! And you shouldn't be filtering cargo commands anyway
./cyrun.sh [args] or ./cytest.sh [args]  # Capture cargo run/test output to logs/run.log or logs/test.log respectively
```

## CLI Flags

### Server & Runtime
- `--server`: Run as HTTP/WebSocket server
- `--mcp`: Run as MCP server (Model Context Protocol over stdio)
- `--duration <SECONDS>`: Run for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml)
- `--data-dir <PATH>`: Override data directory path (defaults to config value)
- `--config <PATH>`: Use specific configuration file

### Graph Management
- `--import-logseq <PATH>`: Import Logseq graph directory (then continues running)
- `--create-graph <NAME>`: Create a new graph with specified name
  - `--description <DESCRIPTION>`: Optional description for the graph
- `--delete-graph <NAME_OR_ID>`: Archive a graph by name or ID
- `--list-graphs`: List all graphs with metadata


### Core Directories
- **src/**: Cymbiont engine - graph management, API endpoints, AI interfaces
- **logseq_databases/**: Test graphs
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence (configurable via data_dir in config.yaml)
  - **IMPORTANT**: The data/ directory is git-tracked (has .gitkeep) - never rm -rf it
  - **graph_registry.json**: Graph metadata persistence
  - **agent.json**: Agent state persistence
  - **auth_token**: Auto-generated authentication token (rotates on restart)
  - **graphs/{graph-id}/**: Per-graph data
    - **knowledge_graph.json**: Graph content persistence
  - **archived_graphs/**: Deleted graphs are moved here with timestamp

### Project Structure
- **src/**
  - **main.rs**: Application entry point with 5-phase startup sequence
  - **cli.rs**: CLI argument parsing and command execution
  - **config.rs**: YAML configuration loading and validation
  - **utils.rs**: Process management, datetime parsing, async lock utilities
  - **error.rs**: Hierarchical error system with domain-specific types
  - **app_state.rs**: Pure resource container with CQRS integration
  - **cqrs/**: Command Query Responsibility Segregation for deadlock-free mutations
  - **graph/**: Graph management subsystem - see `src/graph/CLAUDE.md` for module details
  - **agent/**: AI interface layer - see `src/agent/CLAUDE.md` for module details
  - **import/**: Data import functionality - see `src/import/CLAUDE.md` for module details
  - **http_server/**: HTTP/WebSocket server - see `src/http_server/CLAUDE.md` for API reference and module details
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
- Logging: use `tracing` macros - `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()` (enforced by build.rs)
- Error handling: use the centralized `error.rs` system with domain-specific types (StorageError, ServerError, etc.) and the global `Result<T>` type alias
- Lock handling: use `AsyncRwLockExt` trait from `utils.rs` for async lock operations
- Whenever you update `config.example.yaml` ensure that you also update `config.yaml`
- Don't ever delete TODO comments unless the user gives permission first
- Don't inline imports ever (except for temp debugging like `tracing::debug!()`)
- Keep all documentation evergreen - don't reference transient details, implementation events, or deprecated modules whatsoever

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")

### Autodebugger Commands
- **Remove debug! calls**: `autodebugger remove-debug` (default: targets src/ and tests/ directories)
- **Validate documentation**: `autodebugger validate-docs` (checks module docs meet complexity thresholds)
