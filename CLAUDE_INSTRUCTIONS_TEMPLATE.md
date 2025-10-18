# Cymbiont Instructions

## Knowledge Graph

**Configuration:**
- Neo4j graph database: `bolt://localhost:7687` (default)
- Corpus directory: `/path/to/your/corpus/` (customize in `config.yaml`)
- Document URIs are relative to corpus root
- Markdown files auto-sync hourly (configurable interval)

**Dual Retrieval Modes:**
- **`search_context`**: Semantic search for conceptual exploration, discovering relationships
  - Returns extracted entities and facts (meaning preserved, exact wording lost)
  - Use for: "What do I know about X?" "How are X and Y related?"

- **`get_chunks`**: BM25 keyword search for exact text with source verification
  - Returns raw document chunks with URI and position
  - Use for: "What exact phrase did I use?" "Show me the source"

**Automated Memory Formation** (if hooks installed):
- Background monitoring agent runs every 10 conversation turns
- Automatically identifies and adds salient information to the knowledge graph
- Manual `add_memory` tool available for explicit requests

**Context Integration:**
- Knowledge graph context refreshes each turn via dual queries (your last message + user's message)
- Integrate retrieved context naturally as recalled memory
- Don't mention the graph as a source unless discussing the system itself

---

## Neo4j Direct Queries

For advanced debugging and exploration, use `cypher-shell` to query Neo4j directly:

```bash
# Search for entity nodes by name pattern
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH (n:Entity) WHERE n.name =~ '(?i).*search_pattern.*' \
   RETURN n.name, n.uuid LIMIT 10"

# Search for relationship edges by fact content
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH ()-[r:RELATES_TO]->() WHERE r.fact =~ '(?i).*search_pattern.*' \
   RETURN r.fact, r.uuid LIMIT 10"

# Search for episodes by name (get_episodes only returns most recent)
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH (e:Episodic) WHERE e.name =~ '(?i).*search_pattern.*' \
   RETURN e.name, e.uuid LIMIT 10"

# Find entities created by a specific episode
# (entities are typically created ~30s before the episode is saved)
# Replace EPISODE_TIMESTAMP with episode's created_at value
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH (n:Entity) \
   WHERE n.created_at >= datetime('EPISODE_TIMESTAMP') - duration('PT1M') \
     AND n.created_at <= datetime('EPISODE_TIMESTAMP') + duration('PT1M') \
   RETURN n.name ORDER BY n.name"

# Count total entities in graph
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH (n:Entity) RETURN count(n) as entity_count"

# Count total relationships in graph
cypher-shell -u neo4j -p YOUR_PASSWORD -d neo4j \
  "MATCH ()-[r:RELATES_TO]->() RETURN count(r) as relationship_count"
```

*Replace `YOUR_PASSWORD` with your Neo4j password (default from installation: `demodemo` or `your-password`).*
