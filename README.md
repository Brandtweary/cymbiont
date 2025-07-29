# Cymbiont

> **A knowledge graph engine designed for self-organizing AI agents**

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
- **🌐 Real-time Updates**: WebSocket protocol for live synchronization
- **🔌 HTTP API**: RESTful interface for data ingestion and querying

## Future Capabilities

The roadmap includes:
- **Terminal-first interface** for Unix-style composition and piping
- **Import adapters** for Logseq, Obsidian, Roam Research, and more
- **Natural language queries** powered by integrated LLM agents
- **Library interface** for embedding in other Rust applications
- **Export formats** for interoperability with existing tools

## Getting Started

### Building

```bash
cargo build --release
```

### Running

Start the knowledge graph server:

```bash
cargo run
```

The server will start on `localhost:3000` by default.

### Configuration

Create a `config.yaml` file to customize settings:

```yaml
data_dir: data                    # Where graphs are stored

backend:
  host: "localhost"
  port: 3000
  max_port_attempts: 10

development:
  default_duration: null          # Run indefinitely
```

### CLI Options

```bash
cargo run -- --help              # View all options
cargo run -- --data-dir ./custom # Use custom data directory
cargo run -- --duration 60       # Run for 60 seconds
```


## License

Cymbiont is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

This means:
- You can use, modify, and distribute this software
- If you modify and distribute it, you must share your changes
- If you run a modified version as a network service, you must provide the source code to users
- Any software that incorporates Cymbiont must also be released under AGPL-3.0

For the full license text, see the [LICENSE](LICENSE) file.