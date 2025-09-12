# Agent Module

## Files
- **tools.rs** - Tool registry (static HashMap of 14 tools)
- **schemas.rs** - Tool schemas for LLM function calling
- **mcp/** - Model Context Protocol server
  - **protocol.rs** - JSON-RPC 2.0 types
  - **server.rs** - MCP server over stdio

## Tools (14)

**Block Operations**
- `add_block` - Create block with content, parent, page, properties
- `update_block` - Modify block content
- `delete_block` - Archive block from graph

**Page Operations**
- `create_page` - Create page with optional properties
- `delete_page` - Archive page and its blocks

**Query Operations**
- `get_node` - Retrieve node by ID
- `query_graph_bfs` - BFS traversal with max depth (TODO)

**Graph Management**
- `list_graphs` - Enumerate all graphs
- `list_open_graphs` - List loaded graphs
- `open_graph` - Load graph into memory
- `close_graph` - Save and unload graph
- `create_graph` - Create new knowledge graph
- `delete_graph` - Archive graph

**Import Operations**
- `import_logseq` - Import Logseq graph from directory

## Adding a Tool

1. Define function in `tools.rs` with signature: `fn(app_state: &Arc<AppState>, args: Value) -> Pin<Box<dyn Future<Output = Result<Value>> + Send>>`
2. Add to `TOOLS` HashMap: `tools.insert("tool_name", tool_function as ToolFn)`
3. Create schema in `schemas.rs` with parameters
4. Tool available via MCP (`cymbiont_tool_name`) and TestToolCall (debug builds)

## MCP Protocol
- **Discovery**: `tools/list` returns all tool schemas
- **Execution**: `tools/call` with `{name: "cymbiont_tool_name", arguments: {...}}`
- **Critical**: stdout reserved for JSON-RPC, all logs to stderr