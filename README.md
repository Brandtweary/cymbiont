# Cymbiont

> **A knowledge graph engine for self-organizing AI agents**

Most conversations with AI vanish the moment they end. Every insight, every connection you draw together, every pattern that emerges from working through a problem gets lost when you close the window. You start over each time, rebuilding context, re-explaining what you meant, rehashing old ground. There's no shared workspace, no persistent understanding, no way to build knowledge together over time.

Cymbiont creates that workspace. A knowledge graph that both you and your AI can read, write, and traverse. Your conversations leave traces that connect to everything else you've explored. Ideas branch into relationships. Patterns emerge across months of work. The graph remembers so you don't have to, and your AI draws from this accumulated understanding automatically, like you would from your own memory.

## Why Knowledge Graphs

Knowledge graphs give you explicit structure. Entities and relationships you can inspect, query, and traverse. Both human and AI build the graph together, contributing different perspectives on the same knowledge. The graph becomes the interface through which different forms of intelligence coordinate.

This enables reasoning over structure: discovering paths, detecting patterns, understanding how concepts relate. Your AI can traverse the network of what you've built together, finding connections you didn't know existed, surfacing context that's actually relevant rather than statistically probable.

## The Vision

Cymbiont is infrastructure for collective intelligence through shared representation. Your notes, conversations, and documents become a living network. The AI traverses it, extends it, discovers connections within it. You contribute structure and meaning. The AI explores knowledge space in ways you can't. Together you build understanding neither could create alone.

This is compound computing: different cognitive substrates coordinating through a shared interface. The knowledge graph becomes the medium where human insight and machine computation meet, each preserving what makes it unique while amplifying what the other contributes.

## Using Cymbiont

### With Claude Code

Once installed, Cymbiont integrates seamlessly with your AI assistant. Your agent will automatically build and query the knowledge graph as you work, capturing insights from conversations, documents, and structured data. No manual intervention required.

The graph grows organically as you use your assistant, forming connections between concepts, tracking how information evolves over time, and surfacing relevant context when needed.

Cymbiont is memory augmentation for general-purpose AI. Use it for:

- **Software development**: Code alongside your assistant with full memory of your codebase, past debugging sessions, and architectural decisions
- **Research**: Literature notes, experimental results, evolving hypotheses
- **Writing**: Draft iterations, research sources, thematic connections
- **Custom agents**: Build AI agents with persistent identity and long-term memory

For developers, tools like [code2prompt](https://github.com/raphaelmansuy/code2prompt) generate markdown codebase maps that work seamlessly with Cymbiont. Just dump the output in your synced corpus folder and the knowledge graph ingests your entire codebase structure.

### Document Sync

Cymbiont can automatically sync markdown files from a corpus directory into the knowledge graph. Place your notes, documentation, or research in a designated folder and the system will ingest them hourly, tracking changes and creating diff summaries.

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

### Recommended Editors

Cymbiont works with any editor that uses local markdown files. Choose based on your workflow:

**IDEs** (recommended for most users):
- **Zed**: Fast, modern, written in Rust
- **VS Code**: Extensive ecosystem, mature tooling
- **Cursor/Windsurf**: Agentic IDEs with AI-native features

IDEs with integrated terminals let you chat with your assistant and edit documents in the same application, the most convenient setup for most users. Some prefer separate windows for chat and editing, which works just as well.

**PKM Apps** (specialized note-taking):
- **Logseq**: Open-source PKM with graph view and local markdown
- **Obsidian**: PKM with local vault and extensive plugins

**Text Editors** (lightweight):
- **Sublime Text**: Fast with minimap navigation
- **Neovim**: For terminal enthusiasts

**Writing-Focused** (non-developers):
- **Typora**: Clean markdown with live preview
- **iA Writer**: Distraction-free writing
- **Zettlr**: Academic writing with Zettelkasten features

## Quick Start

### What You'll Need

- **MCP-Compatible AI Assistant**: [Claude Code](https://claude.ai/download) recommended (only officially supported agent at present)
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
cd graphiti-cymbiont

# Create .env in root directory (required for editable install)
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

# Install server dependencies with editable graphiti-core
cd server
uv sync
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

**6. Install Claude Code Hooks (Required)**

The hooks enable automatic context injection and memory formation - without them, you'd need to manually query and save to the graph every time.

**Option 1: Point to cymbiont installation (Faster)**

```bash
# Find your cymbiont installation (if you don't remember where you installed it)
find ~ -type d -name "cymbiont" -path "*/cymbiont" 2>/dev/null | grep -v node_modules

# Set CYMBIONT_PATH to the path shown above
CYMBIONT_PATH="/full/path/to/cymbiont"

# Create settings backup
cp ~/.claude/settings.json ~/.claude/settings.json.backup

# Add hooks to your Claude Code settings
cat > /tmp/hook_config.json << EOF
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 ${CYMBIONT_PATH}/hooks/inject_kg_context.py"
          },
          {
            "type": "command",
            "command": "python3 ${CYMBIONT_PATH}/hooks/monitoring_agent.py"
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 ${CYMBIONT_PATH}/hooks/monitoring_agent.py --force"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 ${CYMBIONT_PATH}/hooks/monitoring_agent.py --force"
          }
        ]
      }
    ]
  }
}
EOF

# Merge with existing settings (requires jq)
jq -s '.[0] * .[1]' ~/.claude/settings.json /tmp/hook_config.json > ~/.claude/settings.json.new
mv ~/.claude/settings.json.new ~/.claude/settings.json
rm /tmp/hook_config.json

echo "Hooks installed! Backup saved to ~/.claude/settings.json.backup"
```

**Option 2: Copy hooks to ~/.claude/ (Recommended)**

Copy hooks to your own directory for customization:

```bash
# Find your cymbiont installation
find ~ -type d -name "cymbiont" -path "*/cymbiont" 2>/dev/null | grep -v node_modules

# Copy hooks (replace /full/path/to/cymbiont with path from above)
mkdir -p ~/.claude/hooks
cp /full/path/to/cymbiont/hooks/*.py ~/.claude/hooks/
cp /full/path/to/cymbiont/hooks/monitoring_protocol.txt ~/.claude/hooks/

# Install hooks using ~/.claude/hooks/ path
CYMBIONT_PATH="$HOME/.claude"

# Run the same installation commands as Option 1, using $CYMBIONT_PATH
```

Then edit the files in `~/.claude/hooks/` as needed.

**If you don't have jq installed**, manually edit `~/.claude/settings.json` and add the hooks block shown above.

That's it! Restart Claude Code and the automated memory system is active.

---

## How It Works

**Automatic Context**: Every message triggers parallel knowledge graph queries (your message + agent's previous response). Relevant entities and facts (~3 nodes + 6 facts) inject silently into the agent's context.

**Automatic Memory**: Every 10 messages, a background agent analyzes the conversation and adds salient information to the graph. Monitoring logs go to `monitoring_logs/timestamped/YYYYMMDD_HHMMSS/` with a `latest/` symlink.

**Customization**: Copy hooks to `~/.claude/hooks/` if you want to modify behavior, then update your settings to point there.

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

The Graphiti FastAPI backend starts automatically when the first Cymbiont instance connects. It continues running even after your AI assistant exits to prevent data loss during asynchronous episode ingestion. You can safely close your AI client without interrupting memory formation.

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

## Upcoming Features

- **Enhanced search**: Personalized PageRank and learned edge weights via GNN
- **Graph maintenance**: Automated orphan cleanup and semantic drift detection
- **Chunk retrieval**: Access original document text for precise citations

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
