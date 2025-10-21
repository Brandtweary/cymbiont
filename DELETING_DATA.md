# Deleting Data

Cymbiont's append-only model means you typically add corrective episodes rather than deleting historical data. The temporal invalidation system handles contradictions automatically. That said, deletion is straightforward when you need it - bad ingestion runs, sensitive information accidentally added, or cleaning up during development.

## Episode Deletion

The `delete_episode` tool works well for removing conversational memories:

```bash
# Via MCP tool (your assistant can do this)
delete_episode(uuid)

# Or ask directly: "Delete episode <uuid>"
```

This removes the episode and its associated entities/facts from the graph.

## Document Deletion

Documents synced from your corpus are automatically cleaned up when you delete them from the filesystem:

```bash
# Just delete the file - automatic cleanup handled on next sync
rm /path/to/corpus/document.md

# Moving files outside corpus also triggers deletion
mv /path/to/corpus/document.md ~/somewhere/else/
```

The watcher detects deletions and moves-outside-corpus, automatically removing:
- **Chunks** (always removed - retrieval infrastructure)
- **Episodes** (removed by default - associated episodic memories)
- **DocumentNode** (document metadata)

Manual sync via MCP tool or REST endpoint triggers immediate cleanup. Otherwise, queued deletions process on the next hourly batch.

### REST API (Edge Cases)

For stale document nodes or programmatic deletion, the REST endpoint remains available:

```bash
curl -X DELETE "http://localhost:8000/document/<document-uri>?delete_episodes=true"
```

The `delete_episodes` parameter controls episode deletion (defaults to `true`).

## Fine-Grained Graph Manipulation

For surgical data operations, your assistant can execute Cypher commands directly against Neo4j:

```bash
# Example: Delete a specific entity
cypher-shell -u neo4j -p demodemo -d neo4j "MATCH (n:Entity {uuid: 'uuid-here'}) DETACH DELETE n"

# Example: Delete all facts matching a pattern
cypher-shell -u neo4j -p demodemo -d neo4j "MATCH ()-[r:RELATES_TO]->() WHERE r.fact =~ '(?i).*pattern.*' DELETE r"
```

This bypasses the graphiti API entirely. Works great when you know what you're doing, but at-your-own-peril.
