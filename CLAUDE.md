# CYMBIONT DEVELOPMENT GUIDE

## Documentation Structure
- **This file**: Development commands and guidelines
- **config.example.yaml**: Configuration template
- **README.md**: User documentation and technical overview
- **GRAPHITI_CONFIG.md**: Graphiti backend configuration reference (env vars, search recipes, tunable parameters)

## Build/Test Commands
```bash
# In cymbiont root
cargo check                      # Quick syntax check
cargo build                      # Build cymbiont
cargo test                       # Run test suite
RUST_LOG=info cargo run          # Run with logging
```

## Development Workflow

**Editing Cymbiont**: After changes, run `cargo build` (debug build), then wait for session reload to pick up new binary.

**Editing Graphiti**: Kill the Graphiti server process first (`pkill -f "uvicorn graph_service.main:app"`). Cymbiont will auto-launch fresh instance on next connection.

## Core Directories
- **src/**: Cymbiont MCP server implementation
- **graphiti-cymbiont/**: Git submodule - Graphiti backend (FastAPI server + knowledge graph engine)
- **hooks/**: Claude Code hooks and git hook templates (portable, for users)
  - **inject_kg_context.py**: UserPromptSubmit hook - dual-context KG injection
  - **monitoring_agent.py**: Monitoring trigger (UserPromptSubmit/PreCompact/SessionEnd)
  - **monitoring_worker.py**: Background worker spawned by monitoring_agent
  - **monitoring_protocol.txt**: Monitoring agent instructions
  - **post-commit.template.sh**: Git post-commit hook template
  - **generate_codebase_maps.template.py**: Codebase map generator template
  - **README.md**: Hook documentation and installation guide
- **logs/**: Log directory
  - **timestamped/**: Timestamped log files
  - **cymbiont_mcp_latest.log**: Symlink to latest log
- **autodebugger/**: Git submodule - logging utilities with verbosity monitoring

## Project Structure
- **src/**
  - **main.rs**: MCP server bootstrap and lifecycle
  - **config.rs**: YAML configuration loading
  - **client.rs**: HTTP client for Graphiti FastAPI
  - **mcp_tools.rs**: MCP tool implementations
  - **graphiti_launcher.rs**: Graphiti backend lifecycle management
  - **types.rs**: Request/response JSON schemas
  - **error.rs**: Error types

## Codebase Guidelines
- Logging: use `tracing` macros - `error!()`, `warn!()`, `info!()`, `debug!()`, `trace!()`
- Error handling: use `anyhow::Result` for application errors, `thiserror` for library errors
- Config file is optional - all settings have sensible defaults
- Don't inline imports (except for temp debugging)
- Comments are welcome for complex logic
- Hooks: Read log path from config.yaml (`logging.directory`), default to `logs/` if not configured


### Log Level Guidelines
- **INFO**: Use sparingly, only for messages you would want to see on every single run
- **DEBUG**: Temporary troubleshooting only - all debug logs must be removed before committing
- **WARN**: Use for problematic behavior that can be fully recovered from
- **ERROR**: Use for any true software bugs - when in doubt whether something should be warn vs error, choose error
- **TRACE**: Low-level implementation details worth preserving

### Autodebugger Commands
- **Remove debug! calls**: `autodebugger remove-debug`
- **Validate documentation**: `autodebugger validate-docs`
