# Graphiti Configuration Reference

A compiled list of tunable parameters in the Graphiti backend. These are **not exposed** in Cymbiont's `config.yaml` - to modify them, patch your local copy of `graphiti-cymbiont/` or fork the repository.

**Disclaimer**: Graphiti itself doesn't expose most of these parameters via configuration files. Modifying them requires editing source code. This is modify-at-your-own-risk territory.

## How to Modify

You have two approaches:

1. **Local patching**: Edit files directly in `cymbiont/graphiti-cymbiont/` submodule
2. **Fork graphiti-cymbiont**: Fork the repository and update your submodule URL to point to your fork

After making changes, kill the Graphiti server (`pkill -f "uvicorn graph_service.main:app"`) and reload your session to pick up the new code.

---

## Environment Variables

Graphiti reads 57+ environment variables. Most are for alternative providers/testing. Core variables for standard setup:

### Required
- `OPENAI_API_KEY`: OpenAI API key
- `NEO4J_URI`: Neo4j connection string (e.g., `bolt://localhost:7687`)
- `NEO4J_USER`: Neo4j username (e.g., `neo4j`)
- `NEO4J_PASSWORD`: Neo4j password

### LLM Configuration
- `MODEL_NAME`: Main LLM for entity/relationship extraction (default: `gpt-4o`)
- `SMALL_MODEL_NAME`: Small LLM for deduplication/attributes (default: `gpt-4o-mini`)
- `EMBEDDING_MODEL_NAME`: Embedding model (default: `text-embedding-3-small`)
- `EMBEDDING_DIM`: Embedding dimension (default: `1024`)

### Performance
- `SEMAPHORE_LIMIT`: Concurrent LLM operations gate (default: `20`)
- `MAX_REFLEXION_ITERATIONS`: Self-correction loop iterations (default: `0`, disabled)
- `USE_PARALLEL_RUNTIME`: Neo4j enterprise parallel runtime (default: `false`)

### Logging
- `LOG_FILE`: Log output path (optional)
- `LOG_LEVEL`: Logging level (default: `INFO`)

### Azure OpenAI (Alternative Provider)
- `AZURE_OPENAI_ENDPOINT`: Azure endpoint URL
- `AZURE_OPENAI_API_KEY`: Azure API key
- `AZURE_OPENAI_API_VERSION`: API version (e.g., `2024-02-15-preview`)
- `AZURE_OPENAI_MODEL`: Main model deployment name
- `AZURE_OPENAI_SMALL_MODEL`: Small model deployment name
- `AZURE_OPENAI_EMBEDDING_MODEL`: Embedding deployment name
- `AZURE_OPENAI_EMBEDDING_DIM`: Embedding dimension
- `AZURE_OPENAI_RERANKER_MODEL`: Reranker deployment name

### Other LLM Providers
- `ANTHROPIC_API_KEY`: Anthropic API key (experimental)

### Alternative Graph Databases
**FalkorDB**:
- `FALKORDB_HOST`, `FALKORDB_PORT`, `FALKORDB_USER`, `FALKORDB_PASSWORD`, `FALKORDB_GRAPH`

**AWS Neptune**:
- `NEPTUNE_HOST`, `NEPTUNE_PORT`, `NEPTUNE_IAM_ENABLED`

**Kuzū**:
- `KUZU_DB_PATH`: Local database path

### Testing/Development
- `TEST_NEO4J_URI`, `TEST_NEO4J_USER`, `TEST_NEO4J_PASSWORD`: Test database
- `OPENAI_ORG_ID`: OpenAI organization ID
- `OTEL_ENABLED`: OpenTelemetry tracing (default: `false`)
- Various Graphiti MCP server settings

**Location**: See `graphiti-cymbiont/.env.example` for full list with descriptions

---

## Ingestion Parameters

### Entity Extraction
**File**: `graphiti_core/nodes/node_operations.py`
- Model: Medium (`MODEL_NAME`)
- Prompt: `graphiti_core/prompts/extract_nodes.py`
- Max reflexion: Controlled by `MAX_REFLEXION_ITERATIONS` env var (default: 0)

### Entity Deduplication
**File**: `graphiti_core/helpers/dedup_helpers.py`
- `_FUZZY_JACCARD_THRESHOLD = 0.9`: Jaccard similarity for fuzzy match
- `_NAME_ENTROPY_THRESHOLD = 1.5`: Entropy filter for low-info names
- `_MIN_NAME_LENGTH = 6`: Minimum chars for fuzzy matching
- `_MIN_TOKEN_COUNT = 2`: Minimum tokens for fuzzy matching
- `_MINHASH_PERMUTATIONS = 32`: LSH parameter
- `_MINHASH_BAND_SIZE = 4`: LSH parameter
- LLM fallback: Medium model (`MODEL_NAME`)

### Relationship Extraction
**File**: `graphiti_core/edges/edge_operations.py`
- Model: Medium (`MODEL_NAME`)
- Max tokens: `16384` (line 100)
- Prompt: `graphiti_core/prompts/extract_edges.py`
- Max reflexion: Controlled by `MAX_REFLEXION_ITERATIONS`

### Relationship Deduplication
**File**: `graphiti_core/edges/edge_operations.py`
- Model: Small (`SMALL_MODEL_NAME`)
- Temporal invalidation: Enabled (contradicted edges expire based on `valid_at`)

### Attribute Extraction
**File**: `graphiti_core/nodes/node_operations.py` (line 453)
- Model: Small (`SMALL_MODEL_NAME`)
- `MAX_SUMMARY_CHARS = 500`: Summary truncation (file: `graphiti_core/utils/text_utils.py`)

### Context Windows
**File**: `graphiti_core/search/search_utils.py`
- `EPISODE_WINDOW_LEN = 3`: Episodes for general context
- `RELEVANT_SCHEMA_LIMIT = 10`: Episodes for extraction context

---

## Retrieval Parameters

### Search Configuration
**File**: `graphiti_core/search/search_config.py`

Default values:
- `limit = 10`: Max results
- `sim_min_score = 0.6`: Vector similarity threshold
- `bfs_max_depth = 3`: Graph traversal depth (if BFS enabled)
- `mmr_lambda = 0.5`: MMR relevance/diversity balance
- `reranker_min_score = 0.0`: Post-reranking filter

### Search Utilities
**File**: `graphiti_core/search/search_utils.py`
- `DEFAULT_MIN_SCORE = 0.6`: Vector search cutoff
- `MAX_SEARCH_DEPTH = 3`: BFS max depth
- `MAX_QUERY_LENGTH = 128`: BM25 token limit

### Search Recipes
**File**: `graphiti_core/search/search_config_recipes.py` (lines 156-198)

Node-only recipes:
- `NODE_HYBRID_SEARCH_RRF`: BM25 + Vector + RRF (default)
- `NODE_HYBRID_SEARCH_NODE_DISTANCE`: BM25 + Vector + 1-hop adjacency
- `NODE_HYBRID_SEARCH_MMR`: BM25 + Vector + diversity
- `NODE_HYBRID_SEARCH_EPISODE_MENTIONS`: BM25 + Vector + mention frequency
- `NODE_HYBRID_SEARCH_CROSS_ENCODER`: BM25 + Vector + BFS + LLM reranking

Combined recipes (nodes + edges + communities):
- `COMBINED_HYBRID_SEARCH_RRF`
- `COMBINED_HYBRID_SEARCH_CROSS_ENCODER`

Edge-only recipes:
- `EDGE_HYBRID_SEARCH_RRF`
- `EDGE_HYBRID_SEARCH_MMR`
- `EDGE_HYBRID_SEARCH_EPISODE_MENTIONS`

Community-only recipes:
- `COMMUNITY_HYBRID_SEARCH_RRF`
- `COMMUNITY_HYBRID_SEARCH_MMR`

**Note**: Default hybrid search (RRF) does **not** include BFS graph traversal - only CROSS_ENCODER recipes use BFS.

### Reranking Algorithms
**File**: `graphiti_core/search/search_utils.py`
- RRF (Reciprocal Rank Fusion): Line 1733, no model required
- Node Distance (1-hop adjacency): Line 1751, not shortest path
- MMR (Maximal Marginal Relevance): Line 1838
- Episode Mentions: Line 1805

---

## Cross-Encoder Configuration

### OpenAI Reranker (Default)
**File**: `graphiti_core/cross_encoder/openai_reranker_client.py`
- `DEFAULT_MODEL = 'gpt-4.1-nano'` (line 31) - **not configurable via .env**
- Temperature: 0 (deterministic)
- Max tokens: 1 (single boolean output)
- Method: Boolean relevance prompt + logprob scoring

Used by:
- Chunk search (`POST /chunks/search` with `rerank_query`)
- `NODE_HYBRID_SEARCH_CROSS_ENCODER` recipe

### BGE Reranker (Optional)
**File**: `graphiti_core/cross_encoder/bge_reranker_client.py`
- `DEFAULT_MODEL = 'BAAI/bge-reranker-v2-m3'` (line 36)
- Requires: `pip install graphiti-core[sentence-transformers]`
- Local inference (no API calls)

---

## Model Assignments by Task

- **Entity extraction**: Medium (`MODEL_NAME`)
- **Relationship extraction**: Medium (`MODEL_NAME`)
- **Entity deduplication**: Medium (`MODEL_NAME`)
- **Edge deduplication**: Small (`SMALL_MODEL_NAME`)
- **Attribute extraction**: Small (`SMALL_MODEL_NAME`)
- **Summary extraction**: Small (`SMALL_MODEL_NAME`)
- **Cross-encoder reranking**: `gpt-4.1-nano` (hardcoded)
- **Embeddings**: `text-embedding-3-small` (configurable via .env)

---

## Chunking & Document Sync

### Chunking Strategy
**Location**: `graphiti_core/document_sync/` (Cymbiont modifications)
- Episode chunk size: 1800-2200 tokens (for LLM extraction)
- Retrieval chunk size: ~256 tokens (for BM25 search)
- Chunker: Docling HybridChunker (two-stage semantic)
- Merge behavior: `merge_peers` only merges chunks with identical heading/caption metadata

### Document Sync
**Location**: Cymbiont `config.yaml` (not Graphiti)
- Corpus path: Configurable in Cymbiont config
- Sync interval: Configurable in Cymbiont config (hours)
- Trigger: Manual via `POST /sync/trigger` or automatic on interval

---

## Community Detection

**Status**: Supported by Graphiti, disabled by default

**How to enable**: Call `add_episode()` with `update_communities=True`

**Implementation**:
- Algorithm: Label propagation clustering
- Hierarchical summarization via LLM
- Each community has: name, summary, name_embedding

**Usage during retrieval**:
- Communities are **automatically included** in `COMBINED_HYBRID_SEARCH_*` recipes
- Not included in `NODE_HYBRID_SEARCH_*` or `EDGE_HYBRID_SEARCH_*` recipes
- Can search communities exclusively with `COMMUNITY_HYBRID_SEARCH_*` recipes
- Returned alongside nodes/edges in SearchResults object
- Provides high-level thematic clusters vs individual entity details

**Search methods**:
- BM25 (keyword search on name/summary)
- Cosine similarity (semantic search on name_embedding)
- Same reranking options as nodes (RRF, MMR, cross-encoder)

**Location**: `graphiti_core/graph/graph_clustering.py`

---

## Vector Embeddings

**What gets embedded**:
- Node names: `name_embedding` field (1024-dim)
- Edge facts: `embedding` field (1024-dim)
- Not embedded: Summaries, attributes (BM25 fulltext only)

**Model**: Controlled by `EMBEDDING_MODEL_NAME` (default: `text-embedding-3-small`)
**Dimension**: Controlled by `EMBEDDING_DIM` (default: `1024`)

---

## Prompt Templates

**Location**: `graphiti_core/prompts/`

All prompts are Python files returning template strings:
- `extract_nodes.py`: Entity extraction (supports text/message/JSON)
- `extract_edges.py`: Relationship extraction
- `dedupe_nodes.py`: Entity deduplication
- `dedupe_edges.py`: Relationship deduplication + temporal invalidation
- Various entity-specific attribute templates

---

## Quick Modification Examples

**Increase entity dedup threshold**:
```python
# File: graphiti_core/helpers/dedup_helpers.py
_FUZZY_JACCARD_THRESHOLD = 0.95  # Stricter merging
```

**Enable reflexion**:
```python
# File: graphiti_core/helpers/helpers.py
MAX_REFLEXION_ITERATIONS = 2  # Or set via .env
```

**Lower vector search threshold**:
```python
# File: graphiti_core/search/search_utils.py
DEFAULT_MIN_SCORE = 0.5  # More permissive
```

**Expand extraction context**:
```python
# File: graphiti_core/search/search_utils.py
RELEVANT_SCHEMA_LIMIT = 15  # More episodes
```

---

**Last Updated**: 2025-10-19
