//! Request/response types for MCP tools
//!
//! ## rmcp SDK Behavior: Struct Doc Comments Override Tool Descriptions
//!
//! **CRITICAL**: The rmcp SDK prioritizes struct doc comments (`///`) over `#[tool(description)]`
//! attributes when generating MCP tool schemas. This is a footgun.
//!
//! - **With struct doc comment**: rmcp uses the struct comment, ignoring `#[tool(description)]`
//! - **Without struct doc comment**: rmcp correctly uses `#[tool(description)]` from mcp_tools.rs
//!
//! **Our convention**: DO NOT add doc comments to these request structs. Tool descriptions belong
//! in `mcp_tools.rs` under the `#[tool]` attribute, not here. Keep these structs documentation-free
//! to avoid shadowing the canonical descriptions.
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddMemoryRequest {
    #[schemars(description = "Episode name")]
    pub name: String,

    #[schemars(description = "Episode content")]
    pub episode_body: String,

    #[schemars(description = "Source description")]
    pub source_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetEpisodesRequest {
    #[schemars(description = "Recent episodes to retrieve (default: 10)")]
    pub last_n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteEpisodeRequest {
    #[schemars(description = "Episode UUID")]
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchContextRequest {
    #[schemars(description = "Search query")]
    pub query: String,

    #[schemars(description = "Max nodes (default: 5, facts: 2x)")]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyncDocumentsRequest {
    // Empty - no parameters needed
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetChunksRequest {
    #[schemars(description = "Keyword query")]
    pub keyword_query: String,

    #[schemars(description = "Max chunks (default: 10)")]
    pub max_results: Option<usize>,

    #[schemars(description = "Reranking query (cross-encoder)")]
    pub rerank_query: Option<String>,
}
