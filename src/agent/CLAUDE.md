# Agent Module Guide 🤖

## Module Overview
AI interface layer providing multiple integration methods for external AI agents to interact with Cymbiont's knowledge graph engine.

## Core Components

### File Structure
- **agent.rs**: Core Agent struct with conversation management and 4-phase message processing
- **llm.rs**: LLMBackend trait and MockLLM test implementation
- **tools.rs**: Canonical registry of 14 knowledge graph tools (single source of truth)
- **schemas.rs**: Ollama-compatible tool schemas for function calling
- **mcp/**: Model Context Protocol server for AI agent integration
  - **mod.rs**: Module exports
  - **protocol.rs**: JSON-RPC 2.0 message types
  - **server.rs**: MCP server implementation over stdio

### Key Types
- **Agent**: Conversation history, LLM config, message processing
- **Message**: User/Assistant/Tool with timestamps and context
- **LLMBackend**: Async trait for LLM implementations (MockLLM, future: Ollama)
- **ToolDefinition**: JSON Schema tool descriptions for LLM function calling

## Tool Registry (14 Tools)

### Block Operations
- `add_block` - Create block with content, parent, page, properties
- `update_block` - Modify block content
- `delete_block` - Archive block from graph

### Page Operations
- `create_page` - Create page with optional properties
- `delete_page` - Archive page and its blocks

### Query Operations
- `get_node` - Retrieve node by ID
- `query_graph_bfs` - BFS traversal with max depth (TODO)

### Graph Management
- `list_graphs` - Enumerate all graphs
- `list_open_graphs` - List loaded graphs
- `open_graph` - Load graph into memory
- `close_graph` - Save and unload graph
- `create_graph` - Create new knowledge graph
- `delete_graph` - Archive graph

### Import Operations
- `import_logseq` - Import Logseq graph from directory

## Key Patterns 🔑

### 4-Phase Message Processing
1. Add user message (brief lock)
2. Get LLM response (no locks)
3. Execute tools (stateless)
4. Add response (brief lock)

### MCP Server Integration
- JSON-RPC 2.0 protocol over stdio
- Tool discovery via `tools/list` method
- Tool execution via `tools/call` method  
- Critical: stdout reserved for JSON-RPC, logs to stderr


### MockLLM Testing
- `echo` - Force specific text response
- `echo_tool` - Force tool call with generated args
- Deterministic behavior for integration tests

### Authorization Flow
- Runtime checks before tool execution
- Clear unauthorized error messages

### Adding New Tools
1. Define tool function in tools.rs
2. Add to TOOLS HashMap with name and function pointer
3. Create schema in schemas.rs with parameters
4. Tool automatically available via all integrations (WebSocket, MCP)

### Error Handling
- Domain-specific AgentError variants
- Tool validation with human-readable messages
- Authorization failures with context