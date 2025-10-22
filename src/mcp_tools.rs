//! MCP tool implementations using official rmcp SDK
//!
//! Exposes knowledge graph operations as MCP tools for Claude Code integration.
//! All tools use hardcoded `group_id='default'` for simplicity.
//!
//! # Available Tools
//!
//! ## Memory Management
//!
//! - **`add_memory`**: Add new memory episode to knowledge graph
//!   - Creates `EpisodicNode` and associated `ChunkNode`
//!   - Triggers LLM extraction of entities and relationships
//!   - Use for: Capturing conversations, insights, events
//!
//! - **`get_episodes`**: Retrieve recent memory episodes chronologically
//!   - Returns last N episodes (default 10)
//!   - Use for: Reviewing recent memories, debugging
//!
//! - **`delete_episode`**: Remove episode by UUID
//!   - Deletes episode and associated chunks
//!   - Use for: Cleanup, removing incorrect data
//!
//! ## Retrieval
//!
//! - **`search_context`**: Semantic search for entities and relationships
//!   - Hybrid search: BM25 + vector similarity + graph traversal
//!   - Returns 5 nodes + 10 facts (default)
//!   - Use for: Conceptual exploration, relationship discovery
//!   - Note: Returns compressed summaries, not exact text
//!
//! - **`get_chunks`**: BM25 keyword search over raw document chunks
//!   - Optional cross-encoder semantic reranking
//!   - Returns chunks with document URI and position
//!   - Use for: Exact wording, technical precision, source verification
//!
//! ## Document Sync
//!
//! - **`sync_documents`**: Trigger manual document synchronization
//!   - Syncs all markdown files in corpus directory
//!   - Runs in background, returns immediately
//!   - Use for: Forcing immediate sync after adding documents
//!
//! # Dual Retrieval Strategy
//!
//! The two search tools serve complementary purposes:
//!
//! - **`search_context`**: Discovers entities and their relationships
//!   - Returns extracted knowledge (`EntityNode`, `EntityEdge`)
//!   - Semantic meaning preserved, exact wording lost
//!   - Best for: "What do I know about X?" "How are X and Y related?"
//!
//! - **`get_chunks`**: Retrieves exact document text
//!   - Returns raw chunk content with provenance
//!   - Preserves exact wording and technical details
//!   - Best for: "What exact phrase did I use?" "Show me the source"
//!
//! # Implementation Notes
//!
//! - All tools return JSON-formatted strings for Claude Code
//! - Errors are formatted as user-readable error messages
//! - No authentication (single-user deployment model)
//! - Graphiti backend must be running (auto-launched by main.rs)

use crate::client::GraphitiClient;
use crate::config::Config;
use crate::types::{
    AddMemoryRequest, DeleteEpisodeRequest, GetChunksRequest, GetEpisodesRequest,
    SearchContextRequest, SyncDocumentsRequest,
};
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::InitializeResult,
    tool, tool_handler, tool_router, ServerHandler,
};

/// Cymbiont MCP service
#[derive(Clone)]
pub struct CymbiontService {
    client: GraphitiClient,
    tool_router: ToolRouter<Self>,
}

impl CymbiontService {
    /// Create new service
    pub fn new(client: GraphitiClient, _config: Config) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl CymbiontService {
    /// Add memory episode to knowledge graph
    #[tool(
        name = "add_memory",
        description = "Add new memory episode to knowledge graph"
    )]
    async fn add_memory(&self, params: Parameters<AddMemoryRequest>) -> Result<String, String> {
        let req = &params.0;

        self.client
            .add_episode(
                &req.name,
                &req.episode_body,
                req.source_description.as_deref(),
            )
            .await
            .map_err(|e| format!("Graphiti request failed: {e}"))
    }

    /// Get recent episodes from knowledge graph
    #[tool(
        name = "get_episodes",
        description = "Get recent episodes from knowledge graph"
    )]
    async fn get_episodes(&self, params: Parameters<GetEpisodesRequest>) -> Result<String, String> {
        let req = &params.0;
        let last_n = req.last_n.unwrap_or(10).min(100);

        let episodes = self
            .client
            .get_episodes("default", Some(last_n))
            .await
            .map_err(|e| format!("Graphiti request failed: {e}"))?;

        Ok(serde_json::to_string_pretty(&episodes).unwrap_or_default())
    }

    /// Delete episode by UUID
    #[tool(
        name = "delete_episode",
        description = "Delete episode by UUID"
    )]
    async fn delete_episode(
        &self,
        params: Parameters<DeleteEpisodeRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        self.client
            .delete_episode(&req.uuid)
            .await
            .map_err(|e| format!("Graphiti request failed: {e}"))
    }

    // TODO: Consider adding delete_document MCP tool if document deletion becomes common in real workflows
    // (currently only needed for test cleanup, can use curl directly to DELETE /document/{uri} endpoint)
    // See future_tasks.md for full context. Automated deletion detection would be cleaner long-term.

    /// Trigger manual document synchronization
    #[tool(
        name = "sync_documents",
        description = "Trigger manual document sync for corpus files"
    )]
    async fn sync_documents(
        &self,
        _params: Parameters<SyncDocumentsRequest>,
    ) -> Result<String, String> {
        self.client
            .trigger_sync()
            .await
            .map_err(|e| format!("Graphiti request failed: {e}"))
    }

    /// Search for both nodes and facts in parallel
    #[tool(
        name = "search_context",
        description = "Search entities and relationships (compressed summaries - use get_chunks for exact text)"
    )]
    async fn search_context(
        &self,
        params: Parameters<SearchContextRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        let max_results = req.max_results.unwrap_or(5).min(100);
        let max_facts = (max_results * 2).min(200); // 1:2 ratio - facts are more information-dense

        // Run both searches in parallel (group_ids=None searches all groups, but "default" is the only group used)
        let (nodes_result, facts_result) = tokio::join!(
            self.client
                .search_nodes(&req.query, None, Some(max_results)),
            self.client.search_facts(&req.query, None, Some(max_facts))
        );

        // Handle errors
        let nodes = nodes_result.map_err(|e| format!("Node search failed: {e}"))?;
        let facts = facts_result.map_err(|e| format!("Fact search failed: {e}"))?;

        // Merge results into combined JSON
        let combined = serde_json::json!({
            "nodes": nodes["nodes"],
            "facts": facts["facts"]
        });

        Ok(serde_json::to_string_pretty(&combined).unwrap_or_default())
    }

    /// Search document chunks by keyword (BM25)
    #[tool(
        name = "get_chunks",
        description = "BM25 keyword search over raw document chunks"
    )]
    async fn get_chunks(&self, params: Parameters<GetChunksRequest>) -> Result<String, String> {
        let req = &params.0;
        let max_results = req.max_results.unwrap_or(10).min(100);

        let response = self
            .client
            .search_chunks(
                &req.keyword_query,
                Some(max_results),
                req.rerank_query.as_deref(),
            )
            .await
            .map_err(|e| format!("Chunk search failed: {e}"))?;

        Ok(serde_json::to_string_pretty(&response["chunks"]).unwrap_or_default())
    }
}

#[tool_handler]
impl ServerHandler for CymbiontService {
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            server_info: rmcp::model::Implementation {
                name: "cymbiont".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            capabilities: rmcp::model::ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability { list_changed: None }),
                ..Default::default()
            },
            instructions: None,
        }
    }
}
