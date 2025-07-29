# Cymbiont

> **⚠️ MAJOR PIVOT IN PROGRESS**: Cymbiont is evolving to its 1.0 release as a terminal-first knowledge graph engine for AI agents. See [CYMBIONT_1.0_PLAN.md](CYMBIONT_1.0_PLAN.md) for details.

A terminal-first knowledge graph engine designed for AI agents, with import capabilities for personal knowledge management systems.

## Overview

Cymbiont transforms your PKM tool into a queryable knowledge graph, providing AI agents with rich contextual understanding of your notes, thoughts, and connections. Unlike traditional RAG (Retrieval-Augmented Generation) approaches that treat documents as isolated text chunks, Cymbiont preserves and leverages the inherent graph structure of your knowledge base.

## Features

- **Real-time Sync**: Automatically syncs with Logseq to maintain an up-to-date knowledge graph
- **Graph-Aware Context**: Provides AI agents with understanding of relationships between concepts
- **Incremental Updates**: Efficiently tracks changes without full database rescans
- **Archive Management**: Preserves deleted content for historical queries
- **Multi-Graph Support**: Manage multiple knowledge bases simultaneously

## Architecture

Cymbiont consists of three main components:

1. **Backend Server** (Rust): Manages the knowledge graph using Petgraph
2. **Logseq Plugin** (JavaScript): Provides real-time sync with Logseq
3. **AI Agent Integration**: Future integration with aichat-agent library for LLM capabilities

## Installation

### Prerequisites

- Rust 1.70+ 
- Node.js 16+
- Logseq Desktop App

### Backend Setup

```bash
cd cymbiont
cargo build --release
cargo run --release
```

### Logseq Plugin Installation

1. Open Logseq Settings > Plugins
2. Enable Developer Mode
3. Load unpacked plugin from `cymbiont/logseq_plugin/`

## Configuration

Copy `config.example.yaml` to `config.yaml` and adjust settings:

```yaml
# Data storage directory (can be absolute or relative path)
data_dir: data  # Default: "data" directory in project root

# Backend server configuration
backend:
  port: 3000  # Default port (will try alternatives if busy)
  max_port_attempts: 10

# Logseq configuration
logseq:
  auto_launch: true  # Auto-launch Logseq on server start
  # executable_path: /path/to/logseq  # Optional custom path

# Development settings
development:
  default_duration: 3  # Auto-shutdown after 3 seconds (set to null for production)

# Sync configuration
sync:
  incremental_interval_hours: 2  # Sync modified content every 2 hours
  full_interval_hours: 168      # Full re-index every 7 days
  enable_full_sync: false       # Disabled by default
```

## Usage

1. Start the backend server:
   ```bash
   cargo run
   ```

2. The Logseq plugin will automatically connect and begin syncing

3. Use CLI flags for various operations:
   ```bash
   # Force incremental sync
   cargo run -- --force-incremental-sync
   
   # Force full database sync  
   cargo run -- --force-full-sync
   
   # Run for specific duration
   cargo run -- --duration 300
   
   # Use custom data directory
   cargo run -- --data-dir /path/to/custom/data
   
   # Launch with specific graph
   cargo run -- --graph "My Graph"
   
   # Launch with graph by path
   cargo run -- --graph-path "/path/to/graph"
   
   # Shutdown running server
   cargo run -- --shutdown-server
   ```

## Configuration Details

Cymbiont automatically manages two properties in your graph's config.edn:

- **`:block-hidden-properties`** - Includes `:cymbiont-updated-ms` to hide sync timestamps from the UI
- **`:cymbiont/graph-id`** - Unique identifier for multi-graph support and data isolation

These properties are automatically maintained and will be restored if removed. The validation system ensures Cymbiont functionality remains stable even if configurations are manually edited.

## Development

See `CLAUDE.md` for detailed development guidelines and `cymbiont_architecture.md` for architectural documentation.

## License

Copyright (c) 2025 Cymbiont developers.

Cymbiont is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

This means that if you use Cymbiont in your software (including as a web service), you must:
- Make your entire application's source code available under AGPL-3.0
- Provide the source code to all users, including those accessing it over a network
- Preserve all copyright and license notices

See the [LICENSE](LICENSE) file for the complete license text.