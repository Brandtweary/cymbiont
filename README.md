# Cymbiont

> **A knowledge graph engine for self-organizing AI agents**

Most AI works with pure vectors and tokens—statistically probable responses grounded in nothing but training data and recent conversation. There's no structure, no explicit relationships, no way to reason about how concepts actually connect. You can't build shared understanding, can't collaboratively construct knowledge, can't create genuine coordination between human and machine intelligence.

Cymbiont provides the missing substrate: a **knowledge graph as interface**. Not just storage for facts, but structured representation of relationships, context, and patterns that both human and AI can read, write, and reason about together. The graph becomes shared cognitive infrastructure—a medium for genuine collaboration that's impossible with vectors alone.

## Why Knowledge Graphs

Traditional AI assistance is single-turn pattern matching. You prompt, it predicts. Even with "memory," you're still fundamentally isolated—the AI generates responses based on your input, but you can't see or influence its internal representations. There's no shared workspace, no collaborative knowledge construction.

**Knowledge graphs change the game.** They provide:

- **Explicit structure**: Entities and relationships you can inspect, query, and traverse—not opaque embeddings
- **Collaborative construction**: Both human and AI build the graph together, contributing different perspectives on the same knowledge
- **Collective intelligence substrate**: Graph becomes the interface through which different forms of intelligence coordinate
- **Beyond RAG**: Not just retrieving context, but reasoning over structure—discovering paths, detecting patterns, understanding how concepts relate

## The Vision

Cymbiont is infrastructure for **collective intelligence through shared representation**. The knowledge graph isn't a database for your AI—it's collaborative workspace where human insight and machine computation meet.

Your notes, conversations, and documents become a living network. AI agents don't just retrieve from it; they traverse it, extend it, discover connections within it. You're not prompting an oracle—you're thinking alongside an agent that can explore knowledge space in ways you can't, while you contribute structure and meaning it can't generate alone.

## Quick Start

### What You'll Need

- **Rust**: https://rustup.rs/
- **Python 3.10+**: For the knowledge graph backend
- **Neo4j**: Graph database
- **OpenAI API Key**: For entity extraction and semantic search

### Installation

**1. Install Neo4j**

```bash
# Add repository
wget -O - https://debian.neo4j.com/neotechnology.gpg.key | gpg --dearmor -o /tmp/neotechnology.gpg.key
sudo mv /tmp/neotechnology.gpg.key /usr/share/keyrings/
echo 'deb [signed-by=/usr/share/keyrings/neotechnology.gpg.key] https://debian.neo4j.com stable latest' | sudo tee /etc/apt/sources.list.d/neo4j.list

# Install
sudo apt update && sudo apt install neo4j
sudo systemctl enable neo4j

# Set password
sudo systemctl stop neo4j
sudo neo4j-admin dbms set-initial-password YOUR_PASSWORD
sudo systemctl start neo4j
```

**2. Install uv (Python package manager)**

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

**3. Set up the knowledge graph backend**

```bash
# Clone Graphiti fork
git clone https://github.com/Brandtweary/graphiti-cymbiont.git
cd graphiti-cymbiont/server

# Install dependencies
uv sync

# Configure
cat > .env << 'EOF'
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=YOUR_PASSWORD
OPENAI_API_KEY=your-api-key-here
MODEL_NAME=gpt-4o-mini
SMALL_MODEL_NAME=gpt-4o-mini
SEMAPHORE_LIMIT=10
LLM_TEMPERATURE=0.0
EOF
```

**4. Build Cymbiont**

```bash
git clone https://github.com/Brandtweary/cymbiont.git
cd cymbiont
cargo build --release
```

**5. Connect to your AI assistant**

For Claude Code:
```bash
claude mcp add cymbiont --transport stdio -- /path/to/cymbiont/target/release/cymbiont
```

For other MCP-compatible AI assistants: Configure stdio transport to launch the `cymbiont` binary.

That's it! The knowledge graph backend starts automatically when your AI assistant connects. You can now use memory tools in your conversations.

---

## Technical Overview

### Cymbiont Stack

```
AI Assistant (Claude Code, etc.)
    ↓ MCP Protocol (stdio JSON-RPC)
Cymbiont MCP Server (Rust)
    ↓ HTTP REST API
Graphiti FastAPI Backend (Python)
    ↓ Bolt Protocol
Neo4j Knowledge Graph
```

**Cymbiont MCP Server**: Rust-based protocol adapter translating MCP's stdio JSON-RPC to Graphiti's HTTP API. Handles backend lifecycle, rotating file logging via autodebugger.

**Graphiti Backend**: Python FastAPI server with LLM-powered entity/relationship extraction, hybrid search (BM25 + vector + graph traversal + reranking), and temporal reasoning.

**Neo4j Database**: Graph storage with vector indices for embeddings, full-text indices for keyword search, and Cypher query engine.

## Features

- **Persistent Memory**: Temporal knowledge graph maintains context across AI assistant sessions
- **Hybrid Search**: Combines semantic similarity (embeddings), keyword matching (BM25), graph traversal (BFS), and reranking (RRF/cross-encoder)
- **Entity Extraction**: LLM automatically identifies entities and relationships from text, JSON, or conversations
- **Temporal Reasoning**: Bi-temporal model tracks when facts were learned (`created_at`) vs. when events occurred (`valid_at`)
- **Multi-Format Ingestion**: Accepts plain text, structured JSON, and message formats
- **Group Isolation**: Separate knowledge domains via `group_id` within single database
- **Incremental Construction**: Process episodes independently without recomputing entire graph

### Configuration

Create `config.yaml` to customize (all settings optional):

```yaml
graphiti:
  base_url: "http://localhost:8000"
  timeout_secs: 30
  default_group_id: "default"
  server_path: "/path/to/graphiti-cymbiont/server"  # Required for auto-launch

similarity:
  min_score: 0.7

corpus:
  path: "/path/to/markdown/documents"
  sync_interval_hours: 1.0

logging:
  level: "info"  # trace, debug, info, warn, error
  log_directory: "/absolute/path/to/logs"  # MUST be absolute
  max_files: 10
  max_size_mb: 5
  console_output: false  # MUST be false for MCP mode

verbosity:
  info_threshold: 50
  debug_threshold: 100
  trace_threshold: 200
```

**Note**: All paths must be absolute. Relative paths will cause validation errors.

### Data Model

### Three-Layer Structure

**Episodes**: Raw content units (input)
- Text snippets, conversations, JSON documents
- Timestamps: `created_at` (ingestion), `valid_at` (event occurrence)
- Metadata: URIs, content hashes, sync timestamps

**Entities**: Extracted concepts (automatically identified)
- People, organizations, ideas, technical concepts
- Summaries and embeddings for semantic search
- LLM-extracted from episode content

**Facts**: Relationships between entities (enables graph traversal)
- Example: "Rust PREVENTS data races"
- Embeddings for semantic search
- Temporal tracking and invalidation

### Bi-Temporal Model

- `created_at`: When information entered the system
- `valid_at`: When the event actually occurred
- Enables point-in-time queries and historical reasoning

## How It Works

### Ingestion Pipeline

1. **Submit episode** via `add_memory` (text/JSON/messages)
2. **LLM extraction**: Identify entities and relationships
3. **Deduplication**: Semantic similarity matching against existing graph
4. **Embedding generation**: Create vectors for nodes and edges
5. **Graph update**: Save nodes/edges, create episodic links
6. **Index update**: Refresh full-text and vector indices
7. **Community detection** (optional): Cluster related entities

### Retrieval Pipeline

**Hybrid Search Process**:
1. **BM25 keyword search**: Full-text indices for exact matches
2. **Vector similarity**: Cosine similarity on embeddings
3. **Graph traversal**: BFS from relevant nodes
4. **Reranking**: Reciprocal rank fusion (RRF), cross-encoder scoring, or node distance

**Search Recipes** (configurable):
- `EDGE_HYBRID_SEARCH_RRF`: Relationship search with rank fusion
- `NODE_HYBRID_SEARCH_NODE_DISTANCE`: Entity search reranked by graph proximity
- `COMBINED_HYBRID_SEARCH_CROSS_ENCODER`: Full hybrid with deep reranking

### Backend Management

The Graphiti FastAPI backend starts automatically when the first Cymbiont instance connects. It continues running even after your AI assistant exits to prevent data loss during asynchronous episode ingestion—you can safely close your AI client without interrupting memory formation.

The backend terminates naturally on system restart. To manually restart for troubleshooting:

```bash
# Find process
ps aux | grep uvicorn | grep graphiti

# Kill
kill <PID>
```

Next Cymbiont connection will start a fresh instance.

## Development

### Building

```bash
cargo build          # Debug
cargo build --release # Optimized
cargo test           # Run tests
```

### Logging

Logs written to `logs/timestamped/cymbiont_mcp_YYYYMMDD_HHMMSS.log` with `cymbiont_mcp_latest.log` symlink to most recent session.

## Graphiti-Cymbiont Fork

Cymbiont uses a fork of Graphiti with enhancements:

- **Document sync system**: Auto-ingest markdown files with change tracking
- **FastAPI server**: HTTP/REST interface for remote access
- **Enhanced retrieval**: Additional search recipes and ranking strategies

**Fork**: [github.com/Brandtweary/graphiti-cymbiont](https://github.com/Brandtweary/graphiti-cymbiont)
**Upstream**: [github.com/getzep/graphiti](https://github.com/getzep/graphiti)

## Document Sync

Cymbiont can automatically sync markdown files from a corpus directory into the knowledge graph. Place your notes, documentation, or research in a designated folder and Graphiti will ingest them hourly, tracking changes and creating diff summaries.

**Setup:**
1. Create a corpus directory for your markdown files
2. Configure the path in `config.yaml`:
```yaml
corpus:
  path: "/absolute/path/to/your/corpus"
  sync_interval_hours: 1.0
```
3. Files sync automatically on the hourly interval
4. Your AI assistant can manually trigger sync using `sync_all_documents()`

**How it works:**
- New files: Full content ingested as episodes
- Modified files: Semantic diff summaries added to graph
- Renamed files: Episode metadata updated automatically
- Deleted files: History preserved (append-only)

## Recommended Editors

Cymbiont works with any editor that uses local markdown files. Choose based on your workflow:

**PKM Apps** (best for note-taking):
- **Logseq**: Local markdown with graph view, perfect fit for Cymbiont
- **Obsidian**: Local vault with extensive plugins

**IDEs** (best for developers):
- **Zed**: Fast, modern, great git integration
- **VS Code**: Extensive ecosystem, mature tooling
- **Cursor**: AI-native features complement knowledge graph

**Text Editors** (lightweight):
- **Sublime Text**: Fast with minimap navigation
- **Neovim**: For terminal enthusiasts

**Writing-Focused** (non-developers):
- **Typora**: Clean markdown with live preview
- **iA Writer**: Distraction-free writing
- **Zettlr**: Academic writing with Zettelkasten features

## Upcoming Features

- **Hook-based automation**: Automatic conversation monitoring and episode creation
- **Enhanced search**: Personalized PageRank and learned edge weights
- **Graph maintenance**: Orphan cleanup and semantic drift detection

## Resources

- **Cymbiont Repository**: [github.com/Brandtweary/cymbiont](https://github.com/Brandtweary/cymbiont)
- **Graphiti Fork**: [github.com/Brandtweary/graphiti-cymbiont](https://github.com/Brandtweary/graphiti-cymbiont)
- **Neo4j Documentation**: [neo4j.com/docs](https://neo4j.com/docs/)
- **MCP Specification**: [modelcontextprotocol.io](https://modelcontextprotocol.io/)

## License

Cymbiont is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

- You can use, modify, and distribute this software
- If you modify and distribute it, you must share your changes
- If you run a modified version as a network service, you must provide source code to users
- Any software incorporating Cymbiont must also be AGPL-3.0

See [LICENSE](LICENSE) for full text.
