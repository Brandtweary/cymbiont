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
    #[tool(name = "add_memory", description = "Add a new memory episode to the knowledge graph")]
    async fn add_memory(&self, params: Parameters<AddMemoryRequest>) -> Result<String, String> {
        let req = &params.0;

        self.client
            .add_episode(
                &req.name,
                &req.episode_body,
                req.source_description.as_deref(),
            )
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))
    }

    /// Get recent episodes from knowledge graph
    #[tool(name = "get_episodes", description = "Get recent episodes from the knowledge graph")]
    async fn get_episodes(
        &self,
        params: Parameters<GetEpisodesRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        let last_n = req.last_n.unwrap_or(10);

        let episodes = self
            .client
            .get_episodes("default", Some(last_n))
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

    /// Trigger manual document synchronization
    #[tool(name = "sync_documents", description = "Trigger manual document synchronization for all corpus files")]
    async fn sync_documents(
        &self,
        _params: Parameters<SyncDocumentsRequest>,
    ) -> Result<String, String> {
        self.client
            .trigger_sync()
            .await
            .map_err(|e| format!("Graphiti request failed: {}", e))
    }

    /// Search for both nodes and facts in parallel
    #[tool(name = "search_context", description = "Search for both nodes and facts in the knowledge graph")]
    async fn search_context(
        &self,
        params: Parameters<SearchContextRequest>,
    ) -> Result<String, String> {
        let req = &params.0;
        let max_results = req.max_results.unwrap_or(5);
        let max_facts = max_results * 2; // 1:2 ratio - facts are more information-dense

        // Run both searches in parallel (group_ids=None searches all groups, but "default" is the only group used)
        let (nodes_result, facts_result) = tokio::join!(
            self.client
                .search_nodes(&req.query, None, Some(max_results)),
            self.client
                .search_facts(&req.query, None, Some(max_facts))
        );

        // Handle errors
        let nodes = nodes_result.map_err(|e| format!("Node search failed: {}", e))?;
        let facts = facts_result.map_err(|e| format!("Fact search failed: {}", e))?;

        // Merge results into combined JSON
        let combined = serde_json::json!({
            "nodes": nodes["nodes"],
            "facts": facts["facts"]
        });

        Ok(serde_json::to_string_pretty(&combined).unwrap_or_default())
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
