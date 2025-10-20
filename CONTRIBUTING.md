# Contributing to Cymbiont

Cymbiont is infrastructure, not an app. It's designed to be forked, modified, and extended. The entire stack is transparent by design - Rust MCP server, Python hooks, Neo4j graph, Graphiti backend. Pick any layer and hack on it.

## Development Setup

- **Cymbiont MCP server (Rust)**: See `CLAUDE.md`
- **Graphiti backend (Python)**: See `graphiti-cymbiont/CLAUDE.md` and `GRAPHITI_CONFIG.md`
- **Hooks and automation**: See `hooks/README.md`

## Pull Requests

We'll read anything you send. If it's broken we'll tell you why. If it's good we'll merge it.

## Extension Points

You don't need to PR to make Cymbiont yours:
- Fork `graphiti-cymbiont` for custom extraction logic
- Add your own MCP servers alongside Cymbiont
- Write new hooks for different workflows
- Query Neo4j directly for analytics
- Script corpus automation however you want

## License

**Cymbiont**: AGPL-3.0 - You can use, modify, and distribute this software freely. If you distribute modified versions or run a modified version as a network service, you must provide the source code to users. Any software incorporating Cymbiont must also be AGPL-3.0.

**graphiti-cymbiont**: Apache 2.0 - Permissive license. You can use, modify, distribute, and incorporate into proprietary software. No copyleft requirements.
