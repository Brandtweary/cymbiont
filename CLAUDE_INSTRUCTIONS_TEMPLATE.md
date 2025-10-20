# Cymbiont Instructions

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
