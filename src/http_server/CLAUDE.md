# CYMBIONT SERVER GUIDE

## Server Structure
```
server/
├── mod.rs                    # Module exports and organization
├── server.rs                 # Server lifecycle (start_server, port finding, cleanup)
├── http_api.rs              # HTTP endpoints and routing with auth middleware
├── websocket.rs             # WebSocket protocol, types, and connection handling
├── websocket_utils.rs       # Shared helpers (auth checks, response sending, graph resolution)
├── websocket_commands/      # Command handlers organized by domain
│   ├── mod.rs              # Command module exports
│   ├── graph_commands.rs   # Graph CRUD and block/page operations  
│   └── misc_commands.rs    # Auth, test, and debug commands
└── auth.rs                  # Token generation, validation, middleware
```

## HTTP API Endpoints

### Public Endpoints (No Auth)
- `GET /` - Health check returning "PKM Knowledge Graph Backend Server"
- `GET /ws` - WebSocket upgrade endpoint (auth handled post-upgrade)

### Protected Endpoints (Bearer Token Required)
- `POST /import/logseq` - Import Logseq graph from `{path}` with optional `{graph_name}`
- `GET /api/websocket/status` - Connection count and open graph IDs for monitoring
- `GET /api/websocket/recent-activity` - Active connections and activity metadata (stub)

## WebSocket Protocol

### Connection Lifecycle
1. HTTP upgrade at `/ws` endpoint (no auth required)
2. Send `Auth { token }` command to authenticate
4. Commands execute as spawned async tasks for concurrency
5. Heartbeat every 30s, automatic cleanup on disconnect

### Response Types
- `Success { data?: Value }` - Command succeeded with optional data
- `Error { message: string }` - Command failed with reason
- `Heartbeat` - Keep-alive pulse from server

### Graph Commands
- `CreateBlock { content, parent_id?, page_name?, temp_id?, graph_id?, graph_name? }` - Create block via GraphOps trait
- `UpdateBlock { block_id, content, graph_id?, graph_name? }` - Update block content preserving edges
- `DeleteBlock { block_id, graph_id?, graph_name? }` - Archive block node
- `CreatePage { name, properties?, graph_id?, graph_name? }` - Create or update page
- `DeletePage { page_name, graph_id?, graph_name? }` - Archive page and blocks
- `OpenGraph { graph_id?, graph_name? }` - Load graph and trigger recovery
- `CloseGraph { graph_id?, graph_name? }` - Save and unload graph
- `CreateGraph { name?, description? }` - Create new graph
- `DeleteGraph { graph_id?, graph_name? }` - Archive graph to archived_graphs/
- `ListGraphs` - Return all graphs with metadata

### System Commands
- `Auth { token }` - Authenticate connection
- `Test { message }` - Echo test with connection stats
- `Heartbeat` - Client keep-alive (no response to prevent loops)
- `TestCliCommand { command, params }` - CLI command bridge (debug builds only)
- `TestToolCall { tool_name, tool_args }` - Direct tool execution (debug builds only)

## Authentication
- **Token**: Auto-generated UUID v4 on startup, saved to `{data_dir}/auth_token` (0600), rotates per restart
- **HTTP**: `Authorization: Bearer TOKEN` header required for protected endpoints
- **WebSocket**: Send `Auth { token }` after connection to authenticate

## Key Patterns
- **Connection State**: Each WsConnection has ID, sender channel, and auth flag
- **Command Routing**: Routes by type to domain handlers (agent/graph/misc) with connection context
- **Async Processing**: Each message spawns separate task to prevent blocking
- **Lock-Free Sending**: Clone sender before use to avoid deadlocks
- **Smart Resolution**: Commands use UUID/name with intelligent defaults for graph/agent targeting

## Testing Support
- **Direct Tool Testing**: TestToolCall for tool execution without agent layer
- **CLI Bridge**: TestCliCommand for integration test coverage

## Gotchas
- WebSocket upgrade is public; auth happens post-connection
- All graph ops validate access
- Client heartbeats acknowledged but don't respond (prevents loops)