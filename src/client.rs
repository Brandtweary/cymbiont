//! HTTP client for Graphiti `FastAPI` backend
//!
//! Provides a typed interface to the Graphiti knowledge graph server over HTTP.
//! All methods use hardcoded `group_id='default'` for simplicity, matching the
//! single-user deployment model of Cymbiont.
//!
//! # Dual Retrieval Modes
//!
//! The client exposes two complementary search strategies:
//!
//! ## 1. Semantic Search (Entities & Facts)
//!
//! Searches the knowledge graph for extracted entities (nodes) and relationships (facts/edges):
//!
//! - **`search_nodes()`**: Find entities by semantic query
//!   - Endpoint: `POST /search/nodes`
//!   - Returns: `EntityNode` summaries with embeddings
//!   - Use for: Conceptual exploration, discovering related entities
//!
//! - **`search_facts()`**: Find relationships by semantic query
//!   - Endpoint: `POST /search`
//!   - Returns: `EntityEdge` facts connecting entities
//!   - Use for: Understanding connections, temporal relationships
//!
//! Both use hybrid search (BM25 + vector similarity + graph traversal) with reranking.
//!
//! ## 2. Chunk Search (Raw Text)
//!
//! Searches raw document chunks for exact wording:
//!
//! - **`search_chunks()`**: BM25 keyword search over document chunks
//!   - Endpoint: `POST /chunks/search`
//!   - Returns: `ChunkNode` content with document URI and position
//!   - Optional: Cross-encoder semantic reranking
//!   - Use for: Exact quotes, technical precision, source verification
//!
//! # Episode Management
//!
//! - **`add_episode()`**: Ingest new memory (creates episode → extracts entities/edges)
//! - **`get_episodes()`**: Retrieve recent memory episodes chronologically
//! - **`delete_episode()`**: Remove episode and associated chunks by UUID
//!
//! # Document Synchronization
//!
//! Background sync of markdown files from corpus directory:
//!
//! - **`start_sync()`**: Start file watcher with interval
//! - **`stop_sync()`**: Stop file watcher
//! - **`trigger_sync()`**: Manually trigger immediate sync (async, returns immediately)
//!
//! # Error Handling
//!
//! All methods return `Result<T, GraphitiError>` with:
//! - `GraphitiError::Http`: Network/connection failures
//! - `GraphitiError::Request`: Non-2xx HTTP status codes with server error message
//! - `GraphitiError::InvalidResponse`: JSON deserialization failures
//!
//! # Configuration
//!
//! The client is configured via `GraphitiConfig`:
//! - `base_url`: Graphiti server URL (default: `http://localhost:8000`)
//! - `timeout_secs`: Request timeout (default: 30 seconds)

use crate::config::GraphitiConfig;
use crate::error::GraphitiError;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

/// HTTP client for Graphiti `FastAPI` backend
#[derive(Clone)]
pub struct GraphitiClient {
    client: Client,
    base_url: String,
}

impl GraphitiClient {
    /// Create new `GraphitiClient` from config
    pub fn new(config: &GraphitiConfig) -> Result<Self, GraphitiError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(GraphitiError::Http)?;

        Ok(Self {
            client,
            base_url: config.base_url.clone(),
        })
    }

    /// Add episode to knowledge graph
    /// POST /episodes
    /// Hardcodes source='text' and `group_id`='default' for simplicity
    pub async fn add_episode(
        &self,
        name: &str,
        episode_body: &str,
        source_description: Option<&str>,
    ) -> Result<String, GraphitiError> {
        let url = format!("{}/episodes", self.base_url);

        let mut body = json!({
            "name": name,
            "episode_body": episode_body,
            "group_id": "default",  // HARDCODED
            "source": "text",       // HARDCODED
        });

        if let Some(desc) = source_description {
            body["source_description"] = json!(desc);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        Ok("Episode queued for processing".to_string())
    }

    /// Search for facts (edges) in knowledge graph
    /// POST /search
    pub async fn search_facts(
        &self,
        query: &str,
        group_ids: Option<Vec<String>>,
        max_results: Option<usize>,
    ) -> Result<Value, GraphitiError> {
        let url = format!("{}/search", self.base_url);

        let mut body = json!({
            "query": query,
        });

        if let Some(gids) = group_ids {
            body["group_ids"] = json!(gids);
        }
        if let Some(limit) = max_results {
            body["limit"] = json!(limit);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))
    }

    /// Search for nodes (entities) in knowledge graph
    /// POST /search/nodes
    pub async fn search_nodes(
        &self,
        query: &str,
        group_ids: Option<Vec<String>>,
        max_results: Option<usize>,
    ) -> Result<Value, GraphitiError> {
        let url = format!("{}/search/nodes", self.base_url);

        let mut body = json!({
            "query": query,
        });

        if let Some(gids) = group_ids {
            body["group_ids"] = json!(gids);
        }
        if let Some(limit) = max_results {
            body["max_nodes"] = json!(limit);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))
    }

    /// Search for chunks (text fragments) in knowledge graph
    /// POST /chunks/search
    /// Hardcodes `group_id`='default' for simplicity
    pub async fn search_chunks(
        &self,
        keyword_query: &str,
        max_results: Option<usize>,
        rerank_query: Option<&str>,
    ) -> Result<Value, GraphitiError> {
        let url = format!("{}/chunks/search", self.base_url);

        let mut body = json!({
            "keyword_query": keyword_query,
            "group_id": "default",  // HARDCODED
        });

        if let Some(limit) = max_results {
            body["max_results"] = json!(limit);
        }
        if let Some(rerank) = rerank_query {
            body["rerank_query"] = json!(rerank);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))
    }

    /// Get recent episodes
    /// GET `/episodes/{group_id}`
    pub async fn get_episodes(
        &self,
        group_id: &str,
        last_n: Option<usize>,
    ) -> Result<Value, GraphitiError> {
        let mut url = format!("{}/episodes/{group_id}", self.base_url);

        if let Some(n) = last_n {
            url = format!("{url}?last_n={n}");
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))
    }

    /// Delete episode by UUID
    /// DELETE /episode/{uuid}
    pub async fn delete_episode(&self, uuid: &str) -> Result<String, GraphitiError> {
        let url = format!("{}/episode/{uuid}", self.base_url);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        Ok(format!("Episode {uuid} deleted"))
    }

    /// Start document sync watcher
    /// POST /sync/start
    pub async fn start_sync(
        &self,
        corpus_path: &str,
        sync_interval_hours: f64,
        group_id: &str,
    ) -> Result<String, GraphitiError> {
        let url = format!("{}/sync/start", self.base_url);

        let body = json!({
            "corpus_path": corpus_path,
            "sync_interval_hours": sync_interval_hours,
            "group_id": group_id,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let result: Value = response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))?;

        Ok(result["message"]
            .as_str()
            .unwrap_or("Document sync started")
            .to_string())
    }

    /// Stop document sync watcher
    /// POST /sync/stop
    pub async fn stop_sync(&self) -> Result<String, GraphitiError> {
        let url = format!("{}/sync/stop", self.base_url);

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let result: Value = response
            .json()
            .await
            .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))?;

        Ok(result["message"]
            .as_str()
            .unwrap_or("Document sync stopped")
            .to_string())
    }

    /// Trigger manual document sync (always async)
    /// POST /sync/trigger
    ///
    /// Triggers document sync in background and returns immediately.
    pub async fn trigger_sync(&self) -> Result<String, GraphitiError> {
        let url = format!("{}/sync/trigger", self.base_url);

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(GraphitiError::Http)?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let _ = response.text().await;
        Ok("Document sync started in background".to_string())
    }
}
