# Graph Module Guide 🗂️

## Module Overview
Petgraph-based knowledge graph engine with multi-graph support.

## Core Components

### File Structure
- **graph_manager.rs**: Core petgraph storage engine with dual ID system
- **graph_operations.rs**: GraphOps trait - standardized public API for all graph mutations
- **graph_registry.rs**: Multi-graph lifecycle and authorization tracking

### Data Types
- **NodeData**: UUID + PKM ID, type (Page/Block), content, properties, timestamps
- **EdgeData**: Type (PageRef/BlockRef/Tag/Property/ParentChild/PageToBlock), weight
- **GraphInfo**: UUID, name, path, timestamps

## GraphOps API (Public Interface)

All operations automatically validate graph access - no manual validation needed.

### Block Operations
- `add_block(agent_id, content, parent_id?, page_name?, properties?, graph_id)` - Create new block
- `update_block(agent_id, block_id, content, graph_id)` - Update block content
- `delete_block(agent_id, block_id, graph_id)` - Archive block and edges

### Page Operations  
- `create_page(agent_id, name, properties?, graph_id)` - Create or update page
- `delete_page(agent_id, name, graph_id)` - Archive page and child blocks

### Query Operations
- `get_node(agent_id, node_id, graph_id)` - Retrieve node data as JSON
- `query_graph_bfs(agent_id, start_id, max_depth, graph_id)` - BFS traversal (TODO)

### Graph Management
- `create_graph(name?, description?)` - Create new graph
- `delete_graph(graph_id)` - Archive graph to timestamped directory
- `open_graph(graph_id)` - Load graph and replay command log
- `close_graph(graph_id)` - Save and unload from memory
- `list_graphs()` - Return all graphs with metadata
- `list_open_graphs()` - Return currently loaded graph IDs

## Key Patterns 🔑

### CQRS Architecture
- All mutations route through CommandQueue
- CommandProcessor owns all mutable state
- RouterToken required for authorized operations
- No manual locking needed - sequential command processing

### Adding New Operations
1. Define new Command variant in cqrs/commands.rs
2. Add method to GraphOps trait with agent_id + graph_id params
3. Implement method using command_queue.execute()
4. Add command handling in cqrs/router.rs
5. Optional: Add to cli.rs, WebSocket commands, or kg_tools.rs

### Error Handling
- Domain-specific GraphError variants
- Distinguish lifecycle (graph not open) vs data errors (node not found)
- Include relevant IDs in error context