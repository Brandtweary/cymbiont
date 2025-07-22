# LOGSEQ PLUGIN DEVELOPMENT GUIDE

## File Structure
- **index.js**: Main plugin entry point, orchestrates lifecycle and real-time sync
- **api.js**: HTTP communication layer, exposes `window.KnowledgeGraphAPI`
- **sync.js**: Database synchronization orchestration, exposes `window.KnowledgeGraphSync`
- **data_processor.js**: Data validation and processing, exposes `window.KnowledgeGraphDataProcessor`
- **websocket.js**: WebSocket command handlers, exposes `window.KnowledgeGraphWebSocket`
- **index.html**: Plugin loader (loads all JS modules as globals)
- **package.json**: Plugin metadata and dependencies
- **icon.png**: Plugin icon for Logseq UI

### Test Files
- **data_processor.test.js**: Tests for reference extraction and data validation
- **sync.test.js**: Tests for sync status logic and tree traversal utilities
- **jest.config.js**: Jest configuration (jsdom environment)
- **eslint.config.js**: ESLint rules for browser/Node.js/test environments

### Development Tools
- **stress_test_generator.js**: Creates 2000 test pages with 10 blocks each for performance testing

## Build/Test Commands
```bash
npm test                         # Run JavaScript plugin tests
npx eslint *.js                  # Run linter to find unused code and errors
```

## Critical Browser-Based Architecture
- NO Node.js features: No `require()`, `import`, `module.exports`, `fs`, `path`, etc.
- Global window objects: All modules expose via `window.KnowledgeGraph*`
- Script tag loading: Dependencies loaded via `<script>` tags in index.html

## Plugin-Specific Guidelines
- Use `KnowledgeGraphAPI.log.error/warn/info/debug/trace()` to send logs to Rust server
- Don't use `console.log()` - use the HTTP logging API instead
- `console.error()` and `console.warn()` are acceptable for critical issues
- The api.js module can't use HTTP logging for its own errors (chicken-egg problem)

## Module Communication Pattern
```
index.html loads all modules → 
  window.KnowledgeGraphAPI (api.js) →
  window.KnowledgeGraphDataProcessor (data_processor.js) →
  window.KnowledgeGraphSync (sync.js) →
  index.js orchestrates everything
```