# Feature Taskpad: Multi-Graph Support

## Feature Description

Enable Cymbiont to support multiple Logseq graphs (PKM databases) by allowing runtime selection of which graph to sync with. The backend will maintain separate knowledge graphs for each Logseq graph, with configuration-based mapping between Logseq graph identifiers and Cymbiont knowledge graph instances.

## Specifications
- Backend can load different knowledge graphs automatically when graphs connect
- Each Logseq graph (pkm-graph-db) maps to a separate Cymbiont knowledge graph (kg)
- Zero user configuration required - graphs are identified automatically
- Stamp internal UUID in each graph's config.edn as `:cymbiont/graph-id`
- Store graph registry in `data/graph_registry.json` (not user-editable)
- Support both file-based and DB-based Logseq graphs
- Maintain graph isolation - no cross-graph data leakage
- Archive nodes include graph metadata
- Enable clean integration testing with existing dummy graph
- Cymbiont owns session management via `logseq://graph/{name}` URL scheme
- Low-click workflow: switch graphs through Cymbiont, not Logseq UI
- Event-driven graph switching - no polling needed

## Relevant Components

### Configuration System
- `src/config.rs`: Current configuration loading
- `config.yaml`: Runtime configuration file
- Current usage: Single graph configuration only

### Graph Manager
- `src/graph_manager.rs`: Knowledge graph storage engine
- Key methods: `new()`, `save_graph()`, `load_graph()`
- Current usage: Manages single graph instance

### Logseq Plugin
- `logseq_plugin/index.js`: JavaScript plugin code
- `getCurrentGraph()` API available for graph identification
- Current usage: Assumes single graph

### AppState
- `src/main.rs`: Application state management
- Holds GraphManager instance
- Current usage: Single graph in AppState

### Archive System
- `data/archived_nodes/`: Deletion archives
- Current format: `archive_YYYY_MM_DD.json`
- Needs: Graph identifier in archive path

## Development Plan

### 1. Research and Design
- [x] Research Logseq plugin API graph identification
- [x] Document findings in docs/logseq_graph_identification.md
- [x] Confirm config.edn supports custom properties (`:cymbiont/graph-id`)
- [ ] Design GraphRegistry data structure
- [ ] Plan EDN parsing strategy for config.edn modification

### 2. Graph Registry Implementation
- [x] Create `src/graph_registry.rs` module
- [x] Implement GraphRegistry struct to manage graph information
- [x] Store registry data in `data/graph_registry.json`
- [ ] Add EDN parsing dependency (edn-rs crate) - DEFERRED: Using Logseq API instead
- [x] Implement config.edn modification to add `:cymbiont/graph-id` - Via Logseq API
- [x] Create methods:
  - [x] `register_graph(name, path) -> GraphInfo`
  - [ ] `find_by_name(name) -> Option<GraphInfo>` - Using find_graph_id instead
  - [ ] `find_by_path(path) -> Option<GraphInfo>` - Using find_graph_id instead  
  - [x] `get_or_create_graph(name, path, id) -> GraphInfo`
- [x] Handle backwards compatibility (single graph = default)

### 3. Parallel Multi-Graph Architecture (NEW SCOPE)
- [ ] Update AppState to support multiple GraphManagers:
  - [ ] Change `graph_manager: Mutex<GraphManager>` to `graph_managers: Arc<RwLock<HashMap<String, RwLock<GraphManager>>>>`
  - [ ] Add method `get_or_create_graph_manager(graph_id, kg_path)` to lazily create managers
  - [ ] Add method `get_active_graph_manager()` that returns the manager for current active graph
  - [ ] Save graph on switch to prevent data loss (petgraph loads into RAM)
- [ ] Update all API handlers to use active graph:
  - [ ] `receive_data` - Process data for active graph
  - [ ] `sync_status` - Return status for active graph  
  - [ ] `update_sync` - Update timestamps for active graph
  - [ ] `verify_pkm_ids` - Archive nodes for active graph
  - [ ] `plugin_initialized` - Register graph and return ID
  - [ ] WebSocket handlers - Operate on active graph
- [ ] Update cleanup/shutdown to save all loaded graphs:
  - [ ] Iterate through all GraphManagers in HashMap
  - [ ] Call `save_graph()` on each
  - [ ] Save graph registry
- [ ] Implement per-graph transaction coordinators:
  - [ ] Create transaction coordinator for each graph (alongside GraphManager)
  - [ ] Store coordinators in HashMap with graph_id as key
  - [ ] Natural isolation - no need to tag transactions with graph_id
  - [ ] Each graph has its own transaction log subdirectory
  - [ ] Recovery happens per-graph on startup
- [ ] Scope WebSocket broadcasts to graph:
  - [ ] Track which connections belong to which graph
  - [ ] Only broadcast commands to relevant connections
- [ ] Memory management considerations:
  - [ ] Document that we keep all graphs in RAM (modern machines can handle it)
  - [ ] Future optimization: LRU cache for inactive graphs

### 4. Plugin Integration
- [x] Modify plugin to send graph info with ALL requests
- [x] Add graph info to every API call:
  ```javascript
  const graphInfo = await logseq.App.getCurrentGraph();
  payload.graph_name = graphInfo.name;
  payload.graph_path = graphInfo.path;
  ```
- [x] Remove any graph polling logic (event-driven switching)
- [ ] Test with both file and DB graph types

### 5. API Updates
- [x] Add graph info to incoming request validation
- [x] Extract graph context from requests in API handlers
- [ ] Implement Axum middleware for graph validation:
  - [ ] Create `graph_validation_middleware` function
  - [ ] Extract headers and validate/register graph on EVERY request
  - [ ] If headers missing (e.g., real-time sync before plugin_initialized), skip gracefully
  - [ ] If graph changes detected, add TODO comment for SessionManager
  - [ ] Store validated graph context in request extensions for handlers
  - [ ] Apply middleware to router with `.layer()`
- [ ] Update plugin_initialized handler:
  - [ ] Extract graph context from request extensions (set by middleware)
  - [ ] Return graph_id in response for plugin to save
- [x] Detect graph changes (TODO added for session switching)
- [ ] Route operations to correct GraphManager instance
- [ ] Add graph validation to prevent cross-graph operations
- [ ] Update WebSocket messages to include graph context

### 6. Archive System Updates
- [ ] Add graph metadata to archived node batches
- [ ] Update `archive_nodes()` to include graph_id in archive records
- [ ] Ensure graph metadata is preserved in all archive operations
- [ ] Update archive recovery to filter by graph_id when needed

### 7. State Management
- [ ] Replace single GraphManager in AppState with GraphRegistry
- [ ] Add current/active graph tracking
- [ ] Implement graph context for all operations
- [ ] Handle concurrent operations on different graphs
- [ ] Add graph-specific transaction coordinators

### 8. Testing Infrastructure
- [ ] Use existing dummy graph at `logseq_databases/dummy_graph/logseq/`
- [ ] Configure dummy graph in test config with its own internal ID
- [ ] Implement full e2e integration test:
  - [ ] Load test PKM (dummy graph)
  - [ ] Send WebSocket traffic to trigger operations
  - [ ] Verify all pages/blocks reach correct KG
  - [ ] Add auto-detection for graph path with warning if wrong graph loaded
- [ ] Test 3-tiered sync with multi-graph:
  - [ ] Real-time sync per graph
  - [ ] Incremental sync per graph
  - [ ] Full sync per graph
- [ ] Test graph switching scenarios (manual for now)
- [ ] Verify graph isolation (no cross-contamination)
- [ ] Test archive metadata filtering

### 9. Session Management System
- [ ] Create `SessionManager` component to handle graph sessions
- [ ] Implement `open_graph_session(graph_id)` function:
  - [ ] Look up graph config by internal ID
  - [ ] Use `logseq://graph/{name}` URL scheme to open specific graph
  - [ ] Track active session state
  - [ ] Update AppState with current graph context
- [ ] Overhaul Logseq launch process in `main.rs`:
  - [ ] Remove auto-launch of default Logseq
  - [ ] Launch specific graph based on config or CLI args
  - [ ] Support `--graph-id` CLI parameter
- [ ] Create session switching API endpoints:
  - [ ] `POST /api/session/switch` - Switch to different graph
  - [ ] `GET /api/session/current` - Get current session info
- [ ] Implement platform-specific URL opening:
  - [ ] Linux: `xdg-open "logseq://graph/{name}"`
  - [ ] macOS: `open "logseq://graph/{name}"`
  - [ ] Windows: `start "logseq://graph/{name}"`
- [ ] Handle session state persistence:
  - [ ] Remember last active graph
  - [ ] Restore session on Cymbiont restart
- [ ] Create CLI commands for session management:
  - [ ] `cymbiont switch-graph <name>` - Switch active graph
  - [ ] `cymbiont list-graphs` - Show all configured graphs
  - [ ] `cymbiont current-graph` - Show current active graph

### 10. Migration and Documentation
- [ ] Create migration guide for existing users
- [ ] Update CLAUDE.md with multi-graph instructions
- [ ] Document session management and low-click workflow
- [ ] Document graph identifier best practices
- [ ] Add examples for common multi-graph setups
- [ ] Update architecture documentation

## Development Notes

**Graph Identification Strategy**: Use internal Cymbiont UUIDs for all mappings to ensure stability. Primary identification by graph name (user-visible), but store both name and path. This makes the system immune to external changes - users can rename graphs or move them without breaking Cymbiont's internal mappings.

**Configuration Approach**: No user configuration needed! Graphs are auto-detected when they connect. UUIDs are stamped in config.edn and tracked in graph_registry.json. Optional config.yaml entries only needed for session management features (launching specific graphs).

**Archive System**: The existing archive system works at a granular batch level. Just need to add graph metadata (graph_id) to archive records for proper filtering and isolation.

**Testing Infrastructure**: The dummy graph already exists at `logseq_databases/dummy_graph/logseq/`. This will be our primary test data - no need to create separate test data. Manual graph loading for now with auto-detection warnings.

**Graph Switching**: Event-driven switching based on incoming requests. When a request arrives with different graph info, Cymbiont automatically switches sessions. No polling needed! The `logseq://graph/{name}` URL scheme enables programmatic graph launching for the low-click workflow.

**Session Management**: Complete overhaul of Logseq launching. Instead of auto-launching default Logseq, Cymbiont will launch specific graphs on demand. This enables the LLM agent to manage multiple PKM databases programmatically.

**Parallel Multi-Graph Architecture**: After careful consideration, we're implementing full parallel multi-graph support, not just sequential switching. This is necessary because:
1. Petgraph loads graphs entirely into RAM - switching without saving would lose data
2. We're already doing most of the work (headers, registry, refactoring AppState)  
3. It prevents complex save/unload logic on every switch
4. Modern machines can handle multiple knowledge graphs in memory

Key decisions:
- Multiple GraphManagers stored in HashMap, created lazily as graphs connect
- Middleware validates/switches active graph before handlers run
- All handlers updated to use `get_active_graph_manager()` method
- Save graph on switch to prevent data loss
- Cleanup saves all loaded graphs on shutdown
- Per-graph transaction coordinators for natural isolation and parallel processing
- WebSocket broadcasts scoped to specific graph's connections

## Future Tasks

- Enhanced session management (multiple simultaneous Logseq instances)
- Graph preloading and caching for instant switching
- Graph-specific plugin configurations
- Cross-graph query capabilities (with explicit permission)
- Graph migration tools (move data between graphs using internal IDs)
- Multi-graph dashboard for monitoring all registered graphs
- Automatic graph discovery and registration with ID assignment
- Graph-level access control and permissions
- Performance optimization for many graphs
- Graph templates and inheritance
- Internal ID remapping tool (emergency use only)

## Final Implementation

(To be completed when feature is finished)