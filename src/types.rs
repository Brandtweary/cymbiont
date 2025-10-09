//! Request/response types for MCP tools

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// === Functional Tool Requests (v1) ===

/// Add memory episode to knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddMemoryRequest {
    #[schemars(description = "Name for the episode")]
    pub name: String,

    #[schemars(description = "Episode content/body")]
    pub episode_body: String,

    #[schemars(description = "Group ID (defaults to config value if not specified)")]
    pub group_id: Option<String>,

    #[schemars(description = "Source type (e.g., 'text', 'message', 'json')")]
    pub source: Option<String>,

    #[schemars(description = "Source description")]
    pub source_description: Option<String>,
}

/// Search for facts (edges) in knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchFactsRequest {
    #[schemars(description = "Search query string")]
    pub query: String,

    #[schemars(description = "Optional list of group IDs to filter results")]
    pub group_ids: Option<Vec<String>>,

    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub max_results: Option<usize>,
}

/// Get recent episodes from knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetEpisodesRequest {
    #[schemars(description = "Group ID to retrieve episodes from (defaults to config value)")]
    pub group_id: Option<String>,

    #[schemars(description = "Number of most recent episodes to retrieve (default: 10)")]
    pub last_n: Option<usize>,
}

/// Delete episode by UUID
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteEpisodeRequest {
    #[schemars(description = "UUID of the episode to delete")]
    pub uuid: String,
}

// === Stubbed Tool Requests (Future) ===

/// Search for both nodes and facts (STUBBED - requires POST /search/nodes endpoint)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchContextRequest {
    #[schemars(description = "Search query string")]
    pub query: String,

    #[schemars(description = "Maximum results per type (fetches N nodes + N facts)")]
    pub max_results: Option<usize>,
}

/// Trigger manual document sync (STUBBED - requires sync endpoints)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyncDocumentsRequest {
    // No parameters - triggers full corpus sync
}
