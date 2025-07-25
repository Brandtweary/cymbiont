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
- [x] Update AppState to support multiple GraphManagers:
  - [x] Change `graph_manager: Mutex<GraphManager>` to `graph_managers: Arc<RwLock<HashMap<String, RwLock<GraphManager>>>>`
  - [x] Add method `get_or_create_graph_manager(graph_id, kg_path)` to lazily create managers
  - [x] Add method `get_active_graph_manager()` that returns the manager for current active graph
  - [x] Save graph on switch to prevent data loss (petgraph loads into RAM)
- [x] Update all API handlers to use active graph:
  - [x] `receive_data` - Process data for active graph
  - [x] `sync_status` - Return status for active graph  
  - [x] `update_sync` - Update timestamps for active graph
  - [x] `verify_pkm_ids` - Archive nodes for active graph
  - [x] `plugin_initialized` - Register graph and return ID
  - [x] WebSocket handlers - Operate on active graph
- [x] Update cleanup/shutdown to save all loaded graphs:
  - [x] Iterate through all GraphManagers in HashMap
  - [x] Call `save_graph()` on each
  - [x] Save graph registry
- [x] Implement per-graph transaction coordinators:
  - [x] Create transaction coordinator for each graph (alongside GraphManager)
  - [x] Store coordinators in HashMap with graph_id as key
  - [x] Natural isolation - no need to tag transactions with graph_id
  - [x] Each graph has its own transaction log subdirectory
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
- [x] Implement Axum middleware for graph validation:
  - [x] Create `graph_validation_middleware` function
  - [x] Extract headers and validate/register graph on EVERY request
  - [x] If headers missing (e.g., real-time sync before plugin_initialized), skip gracefully
  - [x] If graph changes detected, add TODO comment for SessionManager
  - [x] Store validated graph context in request extensions for handlers
  - [x] Apply middleware to router with `.layer()`
- [x] Update plugin_initialized handler:
  - [x] Extract graph context from request extensions (set by middleware)
  - [x] Return graph_id in response for plugin to save
- [x] Detect graph changes (TODO added for session switching)
- [x] Route operations to correct GraphManager instance
- [x] Add graph validation to prevent cross-graph operations
- [ ] Update WebSocket messages to include graph context

### 6. Archive System Updates
- [x] Add graph metadata to archived node batches
- [x] Update `archive_nodes()` to include graph_id in archive records
- [x] Ensure graph metadata is preserved in all archive operations
- [ ] Update archive recovery to filter by graph_id when needed

### 7. State Management
- [x] Replace single GraphManager in AppState with GraphRegistry
- [x] Add current/active graph tracking
- [x] Implement graph context for all operations
- [x] Handle concurrent operations on different graphs
- [x] Add graph-specific transaction coordinators

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

### 9. Session Management System (PRIORITY #1 - Foundation for All Testing)

**Status**: Not Started - Critical foundational component blocking all other priorities

**Why Priority #1**: Session management is the foundation that enables:
- **Integration Testing**: Cannot test multi-graph functionality without targeting specific graphs
- **WebSocket Multi-Graph**: WebSocket connections need graph context from session state
- **AIChat-Agent Integration**: Agents need session context to operate on correct graphs

#### Core Components
- [ ] **Create `src/session_manager.rs` module**:
  - [ ] `SessionManager` struct to handle graph sessions and state
  - [ ] `SessionState` enum: `Inactive`, `Starting(graph_id)`, `Active(graph_id)`, `Switching(from, to)`
  - [ ] `open_graph_session(graph_id)` function for programmatic graph launching
  - [ ] `switch_graph_session(target_graph_id)` for clean graph switching
  - [ ] `get_current_session()` to return active graph context
  - [ ] Session persistence across Cymbiont restarts (save/restore last active graph)

#### Platform Integration
- [ ] **Implement platform-specific URL opening**:
  - [ ] Linux: `xdg-open "logseq://graph/{name}"` with error handling
  - [ ] macOS: `open "logseq://graph/{name}"` with error handling  
  - [ ] Windows: `start "logseq://graph/{name}"` with error handling
  - [ ] Fallback mechanism if URL scheme fails (launch Logseq normally)
  - [ ] Validation that target graph exists before attempting launch

#### Logseq Launch Overhaul
- [ ] **Overhaul Logseq launch process in `main.rs`**:
  - [ ] Replace auto-launch logic with session-based launching
  - [ ] Support `--graph-id <uuid>` CLI parameter for direct graph targeting
  - [ ] Support `--graph-name <name>` CLI parameter for name-based targeting
  - [ ] If no graph specified, use last active graph from persistence
  - [ ] If no last active graph, prompt user or use default (dummy_graph for testing)


#### API Endpoints
- [ ] **Create session switching API endpoints**:
  - [ ] `POST /api/session/switch` - Switch to different graph
    - Body: `{"graph_id": "uuid"}` or `{"graph_name": "name"}`
    - Response: `{"success": bool, "active_graph": {...}}`
  - [ ] `GET /api/session/current` - Get current session info
    - Response: `{"session_state": "Active", "graph_id": "uuid", "graph_name": "name"}`
  - [ ] `GET /api/session/graphs` - List all available graphs
    - Response: `{"graphs": [{"id": "uuid", "name": "name", "path": "path"}]}`

#### CLI Commands
- [ ] **Create CLI commands for session management**:
  - [ ] `cymbiont switch-graph <name|uuid>` - Switch active graph by name or ID
  - [ ] `cymbiont list-graphs` - Show all configured graphs with status
  - [ ] `cymbiont current-graph` - Show current active graph and session state
  - [ ] `cymbiont launch-graph <name|uuid>` - Launch specific graph without switching

#### WebSocket Coordination
- [ ] **Integrate with WebSocket system**:
  - [ ] Update WebSocket connections with new graph context on session switch
  - [ ] Broadcast session change events to authenticated connections
  - [ ] Handle connection-to-graph mapping updates during switches
  - [ ] Ensure WebSocket commands target correct graph after session changes

#### Error Handling & Edge Cases
- [ ] **Robust error handling**:
  - [ ] Handle case where target graph doesn't exist
  - [ ] Handle case where Logseq is already running with different graph
  - [ ] Handle case where URL scheme is not registered
  - [ ] Graceful degradation when session switching fails
  - [ ] Timeout handling for session state transitions

#### Testing Integration
- [ ] **Enable integration testing**:
  - [ ] Test session switching between dummy_graph and dummy_graph_2
  - [ ] Validate that each graph maintains isolation after switches
  - [ ] Test CLI commands work correctly with test graphs

**Implementation Order**:
1. Core SessionManager module with state management
2. Platform-specific URL opening and Logseq launch overhaul  
3. API endpoints for programmatic session control
4. CLI commands for user-friendly session management
5. WebSocket coordination and integration testing

**Blocking Dependencies**: None - this is the foundational component that unblocks everything else

**Success Criteria**: 
- Can programmatically launch specific graphs via CLI/API
- Integration tests can target specific graphs with clean isolation
- Session state persists across Cymbiont restarts

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

### Completed (2025-07-24)

The core parallel multi-graph architecture has been successfully implemented:

1. **AppState Refactoring**:
   - Changed from single GraphManager to HashMap of GraphManagers
   - Added active_graph_id tracking
   - Implemented per-graph transaction coordinators
   - Added helper methods for graph management

2. **Graph Validation Middleware**:
   - Created middleware that extracts graph context from headers
   - Automatically validates and switches graphs on every request
   - Handles graph registration and lazy creation
   - Saves current graph before switching

3. **API Handler Updates**:
   - All handlers now use the active graph
   - Proper async handling with RwLock
   - Error handling for missing active graph

4. **Archive System**:
   - GraphManager now includes graph_id field
   - Archives include graph metadata
   - Graph ID extracted from directory structure

5. **Graph Registry**:
   - Full persistence to data/graph_registry.json
   - Loads on startup, saves on shutdown
   - Tracks all graph metadata

### Recent Progress (2025-07-25)

**Config.edn Property Hiding Fixed**:
- ✅ Fixed multiline regex pattern with `(?m)` flag for proper deduplication
- ✅ Property hiding now works correctly: `:block-hidden-properties #{:cymbiont-updated-ms}`
- ✅ UUID stamping via plugin API working: `:cymbiont/graph-id`
- ✅ No more duplicate entries or Logseq backup file creation
- ✅ Cleaned up debug logging and removed temporary debug copies
- ✅ Fixed data directory paths to always use absolute paths (prevents data creation in wrong locations)
- ✅ Added `.gitkeep` for data directory with proper gitignore exclusions
- ✅ Removed obsolete single-graph `knowledge_graph.json` file

**Config Update System**:
- ✅ Added `config_updated: bool` field to `GraphInfo` struct with `#[serde(default)]`
- ✅ Added methods `mark_config_updated()` and `is_config_updated()` to `GraphRegistry`
- ✅ Runtime config validation via `/config/validate` endpoint handles missing properties

### Still Pending

1. **Session Management System** (PRIORITY #1 - Foundation for All Testing):
   - **Status**: Not Started - Critical foundational component
   - **Blocking**: Integration testing, WebSocket multi-graph, AIChat-Agent
   - **Why First**: Cannot do proper integration testing without targeting specific graphs
   - **Components Needed**:
     - SessionManager component in `src/session_manager.rs`
     - Platform-specific URL opening for `logseq://graph/{name}`
     - CLI commands for graph management (`cymbiont switch-graph`, `cymbiont list-graphs`)
     - Pre-launch graph selection (know graph ID before launching Logseq)
   - **Dependencies**: None (foundational component)
   - **Implementation Priority**: #1 - Start immediately

2. **Integration Testing** (PRIORITY #2 - After Session Management):
   - **Status**: Blocked by Session Management
   - **Why Second**: Need to target specific graphs for clean test isolation
   - **Components**:
     - Integration tests with dedicated test graph (not dummy_graph)
     - Multi-graph switching tests
     - E2E sync testing (real-time, incremental, full)
   - **Dependencies**: SessionManager for graph targeting

3. **Transaction Log Completion** (PRIORITY #3 - Finish & Test Exhaustively):
   - **Status**: Core complete, needs exhaustive testing
   - **Why Third**: Foundation is done, now needs rigorous validation
   - **Remaining**: Timeout handling, correlation for all ops, crash testing
   - **Dependencies**: Integration testing framework

4. **WebSocket Completion** (PRIORITY #4 - Resume Integration):
   - **Status**: Ready to resume (transaction log unblocked it)
   - **Why Fourth**: May already be done, just needs kg_api integration
   - **Components**: Multi-graph support, session awareness, timeouts
   - **Dependencies**: Session Management for graph context


The system is now capable of handling multiple Logseq graphs with proper isolation and can switch between them based on request headers. Each graph has its own GraphManager and TransactionCoordinator for complete isolation. Property hiding is working correctly with proper deduplication.