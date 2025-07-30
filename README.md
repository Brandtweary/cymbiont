# Cymbiont

> **A knowledge graph engine for self-organizing AI agents**

Cymbiont is building the infrastructure for a new kind of knowledge management system—one where AI agents work directly with your personal knowledge graphs, learning patterns in how you think and connecting ideas across domains. Instead of static notes or rigid databases, Cymbiont creates living knowledge structures that grow more useful over time.

## Vision

Imagine AI agents that can:
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
- **📥 Logseq Import**: Complete import system with reference resolution
- **🌐 Real-time Updates**: WebSocket protocol for live synchronization
- **🔌 HTTP API**: RESTful interface for data ingestion and querying
- **⚙️ Multi-Instance**: Concurrent instances with isolated discovery

## Future Capabilities

The roadmap includes:
- **Terminal-first interface** for Unix-style composition and piping
- **Additional import adapters** for Obsidian, Roam Research, and more
- **Natural language queries** powered by integrated LLM agents
- **Export formats** for interoperability with existing tools

## Getting Started

### Integration

Cymbiont provides HTTP and WebSocket APIs for external applications:

- **HTTP API**: RESTful endpoints at `http://localhost:3000` for CRUD operations
- **WebSocket**: Real-time bidirectional communication at `ws://localhost:3000/ws`

Use any HTTP client or WebSocket library to interact with your knowledge graphs programmatically.

### Building

```bash
cargo build --release
```

### Running

View current knowledge graph status:

```bash
cargo run
```

Start the HTTP/WebSocket server:

```bash
cargo run -- --server
```

The server will start on `localhost:3000` by default.

### Import Knowledge Graphs

Import your existing Logseq graph:

```bash
# Import entire Logseq directory
cargo run -- --import-logseq ~/Documents/logseq-notes

# Or via HTTP API (with server running)
curl -X POST http://localhost:3000/import/logseq \
  -H "Content-Type: application/json" \
  -d '{"path": "/path/to/logseq/graph", "graph_name": "my-graph"}'
```

The import process:
- Parses all `.md` files in the directory
- Extracts blocks and page hierarchies
- Resolves `((block-id))` references
- Creates a complete knowledge graph
- Reports comprehensive statistics and any errors

### Configuration

Create a `config.yaml` file to customize settings:

```yaml
data_dir: data                    # Where graphs are stored

backend:
  host: "localhost"
  port: 3000
  max_port_attempts: 10
  server_info_file: "cymbiont_server.json"  # Server discovery file

development:
  default_duration: null          # Run indefinitely
```

### CLI Options

```bash
cargo run -- --help                        # View all options
cargo run -- --data-dir ./custom           # Use custom data directory
cargo run -- --config custom.yaml          # Use specific configuration file
cargo run -- --import-logseq ~/Documents/notes  # Import Logseq graph
cargo run -- --server                      # Start HTTP/WebSocket server
cargo run -- --server --duration 60        # Run server for 60 seconds
cargo run -- --shutdown                    # Gracefully stop running instance
cargo run -- --shutdown --config custom.yaml  # Target specific instance
```


## License

Cymbiont is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

This means:
- You can use, modify, and distribute this software
- If you modify and distribute it, you must share your changes
- If you run a modified version as a network service, you must provide the source code to users
- Any software that incorporates Cymbiont must also be released under AGPL-3.0

For the full license text, see the [LICENSE](LICENSE) file.