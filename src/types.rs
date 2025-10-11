//! Request/response types for MCP tools
//!
//! ## Claude Code MCP Display Bug
//!
//! There is a known flakey bug in Claude Code where MCP tools with empty parameter schemas
//! sometimes display without the tool name/args header (showing only output). The behavior:
//!
//! - **Empty schemas**: Consistently truncated display (no header)
//! - **With parameters**: Moderately flakey but overall better behavior
//!
//! **Recommendation**: Always include at least one optional parameter in tool schemas, even if
//! not strictly necessary for functionality. This significantly improves display reliability.
//!
//! The bug is not related to:
//! - Parameter naming (e.g., `_params` vs `params`)
//! - Tool naming or position in the file
//! - Implementation complexity or timing
//! - Hook configuration or context size
//!
//! Multiple systematic debugging attempts failed to identify the root cause. The parameter
//! workaround is pragmatic and has the benefit of providing actual functionality when the
//! parameter is meaningful (e.g., `async_mode` for `sync_documents`).

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

    #[schemars(description = "Source description")]
    pub source_description: Option<String>,
}

/// Get recent episodes from knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetEpisodesRequest {
    #[schemars(description = "Number of most recent episodes to retrieve (default: 10)")]
    pub last_n: Option<usize>,
}

/// Delete episode by UUID
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteEpisodeRequest {
    #[schemars(description = "UUID of the episode to delete")]
    pub uuid: String,
}

/// Search for both nodes and facts in parallel
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchContextRequest {
    #[schemars(description = "Search query string")]
    pub query: String,

    #[schemars(description = "Maximum number of nodes to return (default: 5). Facts are returned at 2x this value (N nodes + 2N facts)")]
    pub max_results: Option<usize>,
}

/// Trigger manual document sync
///
/// CAUTION: Do not remove the async_mode parameter. There is a flakey Claude Code MCP display bug
/// where tools with empty parameter schemas sometimes show no tool name/args header (only output).
/// Having at least one parameter in the schema mitigates this issue, though the root cause is unknown.
/// The bug is not related to parameter naming, tool naming, tool position, complexity, or timing.
/// Multiple systematic debugging attempts failed to identify the cause. Keeping this parameter
/// ensures more reliable tool display AND provides useful functionality (toggle sync mode).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyncDocumentsRequest {
    #[schemars(
        description = "Run async (default: true). Set to false only if you need detailed sync results - sync will block until complete."
    )]
    pub async_mode: Option<bool>,
}
