# Logseq Graph Identification Research

## Overview

Logseq uses the concept of "graphs" to represent different PKM databases. Each graph is a separate repository of connected pages and blocks. This document outlines how Logseq identifies and manages multiple graphs through its plugin API.

## Graph Identification

### AppGraphInfo Interface

Logseq represents graph information through the `AppGraphInfo` interface with three key properties:

```typescript
interface AppGraphInfo {
  name: string;  // The graph's name
  path: string;  // File system path to the graph
  url: string;   // URL of the graph
}
```

### Plugin API Methods

The plugin API provides several methods to access graph information:

- `logseq.App.getCurrentGraph()` - Returns Promise<AppGraphInfo> for the current graph
- `logseq.App.getCurrentGraphConfigs()` - Returns graph configuration details
- `logseq.App.getCurrentGraphFavorites()` - Returns favorite items in current graph
- `logseq.App.getCurrentGraphRecent()` - Returns recent items in current graph

## Graph Types

Logseq supports two types of graphs:

1. **File Graphs** - Original Markdown file-based storage
2. **DB Graphs** - New SQLite database-based storage (in beta)

Plugins can declare incompatibility with specific graph types using the `unsupportedGraphType` field in their manifest:

```json
{
  "logseq": {
    "unsupportedGraphType": "file" // or "db"
  }
}
```

## Multi-Graph Challenges

### Known Issues

1. **Plugin State Persistence** - When switching graphs, plugin state from the previous graph may persist (Issue #4553)
2. **No Graph Change Events** - The plugin API currently lacks explicit events for graph switching
3. **Graph-Level Configuration** - Plugins are installed at the user level, not per-graph

### Workarounds

Some developers check the current graph periodically:
```javascript
const currentGraph = await logseq.App.getCurrentGraph();
if (currentGraph.path !== lastKnownPath) {
  // Graph has changed
}
```

## Implications for Cymbiont

For Cymbiont's multi-graph support:

1. **Graph Identification** - Use the `name` or `path` from AppGraphInfo as a unique identifier
2. **Configuration Mapping** - Map Logseq graph identifiers to Cymbiont knowledge graph instances
3. **State Management** - Handle graph switching by checking getCurrentGraph() periodically
4. **Database Compatibility** - Consider both file and DB graph types

## References

- [Logseq Plugin API Documentation](https://logseq.github.io/plugins/)
- [AppGraphInfo Interface](https://logseq.github.io/plugins/interfaces/AppGraphInfo.html)
- [IAppProxy Interface](https://logseq.github.io/plugins/interfaces/IAppProxy.html)
- [DB Version Documentation](https://github.com/logseq/docs/blob/master/db-version.md)