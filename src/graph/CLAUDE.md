# Graph Module Guide 🗂️

## Module Overview
Petgraph-based knowledge graph engine with multi-graph support and agent authorization.

## Core Components

### File Structure
- **graph_manager.rs**: Core petgraph storage engine with dual ID system
- **graph_operations.rs**: GraphOps trait - standardized public API for all graph mutations
- **graph_registry.rs**: Multi-graph lifecycle and authorization tracking

### Data Types
- **NodeData**: UUID + PKM ID, type (Page/Block), content, properties, timestamps
- **EdgeData**: Type (PageRef/BlockRef/Tag/Property/ParentChild/PageToBlock), weight
- **GraphInfo**: UUID, name, path, timestamps, authorized agents list

## GraphOps API (Public Interface)

All operations automatically check agent authorization - no manual auth needed.

### Block Operations
- `add_block(agent_id, content, parent_id?, page_name?, properties?, graph_id, skip_wal)` - Create new block
- `update_block(agent_id, block_id, content, graph_id, skip_wal)` - Update block content
- `delete_block(agent_id, block_id, graph_id, skip_wal)` - Archive block and edges

### Page Operations  
- `create_page(agent_id, name, properties?, graph_id, skip_wal)` - Create or update page
- `delete_page(agent_id, name, graph_id, skip_wal)` - Archive page and child blocks

### Query Operations
- `get_node(agent_id, node_id, graph_id)` - Retrieve node data as JSON
- `query_graph_bfs(agent_id, start_id, max_depth, graph_id)` - BFS traversal (TODO)

### Graph Management
- `create_graph(name?, description?)` - Create new graph with prime agent auth
- `delete_graph(graph_id)` - Archive graph to timestamped directory
- `open_graph(graph_id)` - Load graph and replay WAL transactions
- `close_graph(graph_id)` - Save and unload from memory
- `list_graphs()` - Return all graphs with metadata
- `list_open_graphs()` - Return currently loaded graph IDs

## Key Patterns 🔑

### Lock Ordering
1. graph_registry (async)
2. agent_registry (async)  
3. graph_managers → specific manager
4. Use `lock_registries_for_write()` helper when acquiring both

### Adding New Operations
1. Define `GraphOperation::NewOp { params }` variant in storage/wal.rs
2. Add method to GraphOps trait with agent_id + graph_id params
3. Implement: check auth → create operation → begin tx → do work → commit tx
4. Add recovery match arm in storage/recovery.rs (calls method with skip_wal: true)
5. Optional: Add to cli.rs, WebSocket commands, or kg_tools.rs

### Error Handling
- Domain-specific GraphError variants
- Distinguish lifecycle (graph not open) vs data errors (node not found)
- Include relevant IDs in error context