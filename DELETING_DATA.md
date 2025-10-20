# Deleting Data

Cymbiont's append-only model means you typically add corrective episodes rather than deleting historical data. The temporal invalidation system handles contradictions automatically. That said, sometimes you do need to remove data - bad ingestion runs, sensitive information accidentally added, or cleaning up during development.

## Episode Deletion

The `delete_episode` tool works well for removing conversational memories:

```bash
# Via MCP tool (your assistant can do this)
delete_episode(uuid)

# Or ask directly: "Delete episode <uuid>"
```

This removes the episode and its associated entities/facts from the graph.

## Document Deletion

Documents synced from your corpus require a REST API call:

```bash
curl -X DELETE "http://localhost:8000/document/<document-uri>?delete_episodes=true"
```

The `delete_episodes=true` parameter removes associated episodes from the graph. Without it, only the document metadata is deleted.

**Note**: The document sync pipeline doesn't currently detect file deletions automatically. If there's user demand, deletion detection could be added near-term so that removing a file from your corpus triggers cleanup in the graph.

## Fine-Grained Graph Manipulation

For surgical data operations, your assistant can execute Cypher commands directly against Neo4j:

```bash
# Example: Delete a specific entity
cypher-shell -u neo4j -p demodemo -d neo4j "MATCH (n:Entity {uuid: 'uuid-here'}) DETACH DELETE n"

# Example: Delete all facts matching a pattern
cypher-shell -u neo4j -p demodemo -d neo4j "MATCH ()-[r:RELATES_TO]->() WHERE r.fact =~ '(?i).*pattern.*' DELETE r"
```

This bypasses the graphiti API entirely. Works great when you know what you're doing, but at-your-own-peril.
