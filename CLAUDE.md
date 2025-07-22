# CYMBIONT DEVELOPMENT GUIDE

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check
cargo build                      # Build cymbiont server
cargo test                       # Run tests (quiet by default)
RUST_LOG=debug cargo run         # Run backend server with debug logging (do not alter default 3s duration or set a timeout)
```

## CLI Flags (Cymbiont Server)

- `--duration <SECONDS>`: Run server for a specific duration in seconds (to run indefinitely, set default_duration to null in config.yaml )
- `--force-incremental-sync`: Force an incremental sync (updates modified blocks/pages since last sync and catches deletion events)
- `--force-full-sync`: Force a full database sync (needed if block content is being modified by external tools)

## Architecture
- See `cymbiont_architecture.md` for comprehensive codebase architecture

### Core Directories
- **src/**: Cymbiont server - graph management, API endpoints, sync logic
- **logseq_plugin/**: JavaScript plugin for Logseq real-time sync (see logseq_plugin/CLAUDE.md)
- **logseq_databases/**: Test graphs and multi-graph support
  - **dummy_graph/**: Test data for development
- **data/**: Knowledge graph persistence
  - **archived_nodes/**: Deleted node archives

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
- **tests/**: Integration tests
- **Cargo.toml**: Dependencies and metadata

## Codebase Guidelines
- Rust backend: use `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()` macros for logging (tracing crate)
- Don't make live LLM calls during tests

### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from, e.g. an invalid parameter which gracefully falls back to a default value
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving (e.g. "Entering function X", "Cache hit for key Y", "Parsed N bytes")

## Development Best Practices

### Read Files Completely

- When working with a file for the first time in a conversation, read it in its entirety before making changes
- Avoid hunting through large files with grep when a full read would provide helpful context

### Clean Console Output

- **Before committing**: Remove all debug logs (grep for `debug!` to find them)
- Do not add logging inside hot paths which will flood the console
- Optimize logging levels if output becomes overwhelming
- When reviewing logs, make sure to point out ANY warnings or errors. The user is NOT reading these logs, it is your responsibility to report issues

### Fail-Fast During Feature Development

- **Prototype without fallbacks**: When developing new features, avoid default values or fallback mechanisms that mask underlying issues.
- **Explicit error handling**: Let failures be loud and visible during initial implementation - don't silently continue on errors.
- **No backwards compatibility**: Keeping deprecated code creates confusion and adds developer burden. Remove old code paths decisively.

### Eliminate Dead Code

- **Case-by-case evaluation**: Never blindly remove dead code without understanding its context in the larger codebase.
- **Consider multiple scenarios**: For each dead code instance, evaluate possible underlying causes (e.g., planned features that were forgotten, logic that got inlined elsewhere, or remains of deleted features requiring git history investigation).
- **YAGNI Principle**: "You Ain't Gonna Need It" - Only keep what you actually need right now. Avoid building for imagined future requirements.
- **Use `#[allow(dead_code)]` sparingly**: Only when the user explicitly confirms code is kept for forward-compatibility.
- **Use `#[cfg(test)]` for test code**: If appropriate, silence warnings for production code that is authentically only currently used in tests. But generally, test code need not be caught by the dead code checker. Consider if there is a cleaner solution, such as using a test fixture.
- **NEVER prefix unused variables with underscores**: This makes it impossible to locate dead code later. Always use compiler flags instead. 

### End of the Dance: Identify and Fix Root Causes

- **Root cause analysis**: Thoroughly investigate and identify exactly what's causing a bug before implementing solutions.
- **Demand concrete proof**: Always insist on measurement and verification - avoid endlessly theorizing about abstract causes.
- **No compensatory features**: Do NOT add new features as band-aids to work around bugs without proving they're necessary first. For example, don't add a checksum without first showing that the underlying data corruption isn't fixable at the source.

## Continuous Documentation

- Keep the architecture document (`cymbiont_architecture.md`) up to date
- When updating documentation, read it entirely first to avoid redundancy
