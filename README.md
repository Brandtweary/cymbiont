# Cymbiont

> **A knowledge graph engine for self-organizing AI agents**

Transform your notes into an intelligent, queryable network. Cymbiont is building the infrastructure for a new kind of knowledge management system—one where local AI agents work directly with your personal knowledge graphs, learning patterns in your notes and connecting ideas across domains. Instead of static notes or rigid databases, Cymbiont creates living knowledge structures that evolve and adapt.

## Vision

Imagine local AI agents that can:
- **Discover hidden connections** by traversing paths between seemingly unrelated notes
- **Recognize recurring themes** across different projects and automatically tag related content
- **Answer complex queries** by synthesizing information from multiple sources in your graph
- **Suggest new connections** based on content similarity and conceptual relationships
- **Learn your thinking patterns** and proactively surface relevant information as you work
- **Maintain graph health** by identifying orphaned nodes, broken links, and redundant information

## Current Foundation

Cymbiont currently provides the core engine that makes this vision possible:

- **🏗️ Robust Graph Storage**: Petgraph-based engine with typed nodes and edges
- **🔄 ACID Transactions**: Write-ahead logging ensures data integrity  
- **🗂️ Multi-Graph Support**: Isolated storage for different knowledge domains
- **🤝 Multi-Agent Architecture**: Flexible graph-to-agent authorization system
- **📥 Logseq Import**: Complete import system with reference resolution
- **🌐 Real-time Updates**: WebSocket protocol for live synchronization
- **🔌 HTTP API**: RESTful interface for data ingestion and querying
- **⚙️ Multi-Instance**: Concurrent instances with isolated discovery

## Future Capabilities

The roadmap includes:
- **Terminal-first interface** for Unix-style composition and piping
- **Additional import adapters** for Obsidian, Roam Research, and more
- **Natural language queries** powered by integrated local LLM agents
- **Export formats** for interoperability with existing tools

## Getting Started

### Installation

**Prerequisites:** You'll need [Rust](https://rustup.rs/) installed on your system.

**Download from GitHub:**
```bash
git clone https://github.com/Brandtweary/cymbiont.git
cd cymbiont
```

### Quick Start

1. **Build the project**:
   ```bash
   cargo build --release
   ```

2. **Run Cymbiont**:
   ```bash
   cargo run
   ```

3. **Import your notes** (if you have them):
   ```bash
   # Logseq import (available now)
   cargo run -- --import-logseq ~/Documents/logseq-notes
   
   # Roam and Obsidian support planned
   ```

4. **Run as a server** (for programmatic access):
   ```bash
   cargo run -- --server
   ```

That's it! Cymbiont will handle graph storage, transactions, and data persistence automatically.

### Import Your Knowledge

If you have an existing Logseq graph, you can import it in seconds:

```bash
# Import your Logseq directory
cargo run -- --import-logseq ~/Documents/my-notes

# The import will:
# - Parse all .md files 
# - Extract blocks and pages
# - Resolve ((block-id)) references
# - Create a complete knowledge graph
# - Show you detailed statistics
```

### Basic Commands

```bash
# View current graphs and status
cargo run

# Import knowledge from Logseq
cargo run -- --import-logseq /path/to/notes

# Delete a graph by name or UUID
cargo run -- --delete-graph my-old-notes

# Use custom data directory
cargo run -- --data-dir ./my-graphs

# Use specific config file
cargo run -- --config custom.yaml

# Run for development/testing (auto-exit after 60s)
cargo run -- --duration 60

# Stop a running instance (use Ctrl+C)
# The process will gracefully save all data before exiting
```

### Configuration

Create a configuration file:

```bash
cp config.example.yaml config.yaml
```

Or create `config.yaml` manually with these settings:

```yaml
data_dir: data                    # Where graphs are stored

backend:
  host: "localhost"
  port: 8888
  max_port_attempts: 10
  server_info_file: "cymbiont_server.json"  # Server discovery file

development:
  default_duration: null          # Run indefinitely

# Optional authentication settings
# auth:
#   token: "your-secret-token"   # Fixed token (auto-generated if not set)
#   disabled: false              # Set to true to disable auth
```

## Advanced Usage

### Programmatic Access

For developers building applications on top of Cymbiont:

```bash
# Start the server
cargo run -- --server
```

When running as a server, Cymbiont generates an authentication token on startup:

```
🔐 Authentication token: 7f3a8b2c-d9e5-4a6f-b1c3-9e8d7f6a5b4c
📁 Token saved to: data/auth_token
```

Use this token in the Authorization header for HTTP requests or via the WebSocket Auth command.

Cymbiont provides HTTP and WebSocket APIs:

- **HTTP API**: RESTful endpoints at `http://localhost:8888`
- **WebSocket**: Real-time communication at `ws://localhost:8888/ws`

**HTTP Import Example:**
```bash
curl -X POST http://localhost:8888/import/logseq \
  -H "Content-Type: application/json" \
  -d '{"path": "/path/to/logseq/graph", "graph_name": "my-graph"}'
```

### Multiple Instances

Run multiple Cymbiont instances simultaneously:

```bash
# Instance 1 (default config)
cargo run -- --server

# Instance 2 (custom config)  
cargo run -- --server --config instance2.yaml

# Use Ctrl+C to gracefully stop any instance
# Each instance saves data independently on shutdown
```

### All CLI Options

```bash
# Basic options
cargo run -- --help                        # View all options
cargo run -- --data-dir ./custom           # Use custom data directory
cargo run -- --config custom.yaml          # Use specific configuration file

# Server mode
cargo run -- --server                      # Start HTTP/WebSocket server
cargo run -- --server --duration 60        # Run server for 60 seconds

# Graph management
cargo run -- --import-logseq ~/Documents/notes  # Import Logseq graph
cargo run -- --delete-graph my-notes       # Delete a graph by name

# Agent management
cargo run -- --create-agent "Research Assistant" --agent-description "Handles research queries"
cargo run -- --agent-info "Research Assistant"  # View specific agent
cargo run -- --agent-info                  # Defaults to prime agent if not specified
cargo run -- --delete-agent "Old Assistant"  # Delete by name
cargo run -- --activate-agent 550e8400-e29b-41d4-a716-446655440000  # Activate by UUID
cargo run -- --deactivate-agent "Research Assistant"  # Deactivate by name
cargo run -- --authorize-agent "Research Assistant" --for-graph "my-notes"
cargo run -- --deauthorize-agent "Research Assistant" --from-graph "old-notes"

# Use Ctrl+C to gracefully stop any instance
```


## License

Cymbiont is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

This means:
- You can use, modify, and distribute this software
- If you modify and distribute it, you must share your changes
- If you run a modified version as a network service, you must provide the source code to users
- Any software that incorporates Cymbiont must also be released under AGPL-3.0

For the full license text, see the [LICENSE](LICENSE) file.