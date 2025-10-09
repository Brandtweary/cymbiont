//! MCP tool implementations using official rmcp SDK

use crate::client::GraphitiClient;
use crate::config::Config;
use crate::types::*;
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::InitializeResult,
    tool, tool_handler, tool_router, ServerHandler,
};

/// Cymbiont MCP service
#[derive(Clone)]
pub struct CymbiontService {
    client: GraphitiClient,
    config: Config,
    tool_router: ToolRouter<Self>,
}

impl CymbiontService {
    /// Create new service
    pub fn new(client: GraphitiClient, config: Config) -> Self {
        Self {
            client,
            config,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl CymbiontService {
    /// Add memory episode to knowledge graph (STUBBED)
    /// TODO: Requires POST /episodes endpoint in Graphiti FastAPI
    #[tool(name = "add_memory", description = "Add a new memory episode to the knowledge graph (NOT YET IMPLEMENTED)")]
    async fn add_memory(&self, params: Parameters<AddMemoryRequest>) -> Result<String, String> {
        let req = &params.0;
        let group_id = req
            .group_id
            .as_deref()
            .or(Some(self.config.graphiti.default_group_id.as_str()));

        self.client
            .add_episode(
                &req.name,
                &req.episode_body,
                group_id,
                req.source.as_deref(),
                req.source_description.as_deref(),
            )
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))
    }

    /// Search for facts (relationships) in knowledge graph
    #[tool(name = "search_facts", description = "Search for facts (relationships between entities) in the knowledge graph")]
    async fn search_facts(
        &self,
        params: Parameters<SearchFactsRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        let max_results = req.max_results.or(Some(10));

        let results = self
            .client
            .search_facts(&req.query, req.group_ids.clone(), max_results)
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))?;

        Ok(serde_json::to_string_pretty(&results).unwrap_or_default())
    }

    /// Get recent episodes from knowledge graph
    #[tool(name = "get_episodes", description = "Get recent episodes from the knowledge graph")]
    async fn get_episodes(
        &self,
        params: Parameters<GetEpisodesRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        let group_id = req
            .group_id
            .as_deref()
            .unwrap_or(&self.config.graphiti.default_group_id);

        let last_n = req.last_n.or(Some(10));

        let episodes = self
            .client
            .get_episodes(group_id, last_n)
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))?;

        Ok(serde_json::to_string_pretty(&episodes).unwrap_or_default())
    }

    /// Delete episode by UUID
    #[tool(name = "delete_episode", description = "Delete an episode from the knowledge graph by UUID")]
    async fn delete_episode(
        &self,
        params: Parameters<DeleteEpisodeRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        self.client
            .delete_episode(&req.uuid)
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))
    }

    // === STUBBED TOOLS ===

    /// Search for both nodes and facts (STUBBED)
    /// TODO: Requires POST /search/nodes endpoint in Graphiti FastAPI
    /// TODO: Should call both node and fact search in parallel, merge results
    #[tool(name = "search_context", description = "Search for both nodes and facts in the knowledge graph (NOT YET IMPLEMENTED)")]
    async fn search_context(
        &self,
        _params: Parameters<SearchContextRequest>,
    ) -> Result<String, String> {
        Err("search_context not yet implemented - requires POST /search/nodes endpoint in Graphiti FastAPI".to_string())
    }

    /// Trigger manual document sync (STUBBED)
    /// TODO: Requires POST /sync/start, /sync/stop, /sync/trigger endpoints in Graphiti FastAPI
    /// TODO: Cymbiont should call /sync/start on startup, /sync/trigger here, /sync/stop on shutdown
    #[tool(name = "sync_documents", description = "Trigger manual document synchronization (NOT YET IMPLEMENTED)")]
    async fn sync_documents(
        &self,
        _params: Parameters<SyncDocumentsRequest>,
    ) -> Result<String, String> {
        Err("sync_documents not yet implemented - requires document sync lifecycle endpoints in Graphiti FastAPI".to_string())
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
                tools: Some(rmcp::model::ToolsCapability {
                    list_changed: None,
                }),
                ..Default::default()
            },
            instructions: None,
        }
    }
}
