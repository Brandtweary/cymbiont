# Agent Module Guide 🤖

## Module Overview
Multi-agent system with LLM backends, tool execution, and graph authorization.

## Core Components

### File Structure
- **agent.rs**: Core Agent struct with conversation management and 4-phase message processing
- **agent_registry.rs**: Agent lifecycle, authorization tracking, and prime agent system
- **llm.rs**: LLMBackend trait and MockLLM test implementation
- **kg_tools.rs**: Static registry of 15 knowledge graph tools
- **schemas.rs**: Ollama-compatible tool schemas for function calling

### Key Types
- **Agent**: Conversation history, LLM config, default graph, message processing
- **Message**: User/Assistant/Tool with timestamps and context
- **AgentRegistry**: Centralized metadata and authorization management
- **LLMBackend**: Async trait for LLM implementations (MockLLM, future: Ollama)
- **ToolDefinition**: JSON Schema tool descriptions for LLM function calling

## Tool Registry (15 Tools)

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

### Agent Graph Settings
- `set_default_graph` - Set agent's default graph
- `get_default_graph` - Get current default
- `list_my_graphs` - List authorized graphs

## Key Patterns 🔑

### 4-Phase Message Processing
1. Add user message (brief lock)
2. Get LLM response (no locks)
3. Execute tools (stateless)
4. Add response (brief lock)

### Prime Agent
- Auto-created on first run
- Cannot be deleted
- Authorized for all new graphs
- Default for WebSocket connections

### MockLLM Testing
- `echo` - Force specific text response
- `echo_tool` - Force tool call with generated args
- Deterministic behavior for integration tests

### Authorization Flow
- AgentRegistry is single source of truth
- Runtime checks before tool execution
- Bidirectional agent-graph tracking
- Clear unauthorized error messages

### Adding New Tools
1. Define tool function in kg_tools.rs
2. Add to TOOLS HashMap with name and function pointer
3. Create schema in schemas.rs with parameters
4. Tool automatically available to all agents

### Error Handling
- Domain-specific AgentError variants
- Tool validation with human-readable messages
- Authorization failures with context