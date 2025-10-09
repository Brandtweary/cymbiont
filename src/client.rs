//! HTTP client for Graphiti backend

use crate::config::GraphitiConfig;
use crate::error::GraphitiError;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

/// HTTP client for Graphiti FastAPI backend
#[derive(Clone)]
pub struct GraphitiClient {
    client: Client,
    base_url: String,
}

impl GraphitiClient {
    /// Create new GraphitiClient from config
    pub fn new(config: &GraphitiConfig) -> Result<Self, GraphitiError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| GraphitiError::Http(e))?;

        Ok(Self {
            client,
            base_url: config.base_url.clone(),
        })
    }

    /// Add episode to knowledge graph
    /// TODO: Requires POST /episodes endpoint in Graphiti FastAPI
    /// POST /messages is ONLY for conversation arrays, not general text/json episodes
    pub async fn add_episode(
        &self,
        _name: &str,
        _episode_body: &str,
        _group_id: Option<&str>,
        _source: Option<&str>,
        _source_description: Option<&str>,
    ) -> Result<String, GraphitiError> {
        Err(GraphitiError::Request(
            "add_episode not yet implemented - requires POST /episodes endpoint in Graphiti FastAPI. \
            The existing POST /messages endpoint is only for conversation arrays.".to_string()
        ))
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
            .map_err(|e| GraphitiError::Http(e))?;

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
    /// GET /episodes/{group_id}
    pub async fn get_episodes(
        &self,
        group_id: &str,
        last_n: Option<usize>,
    ) -> Result<Value, GraphitiError> {
        let mut url = format!("{}/episodes/{}", self.base_url, group_id);

        if let Some(n) = last_n {
            url = format!("{}?last_n={}", url, n);
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| GraphitiError::Http(e))?;

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
        let url = format!("{}/episode/{}", self.base_url, uuid);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| GraphitiError::Http(e))?;

        if !response.status().is_success() {
            return Err(GraphitiError::Request(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        Ok(format!("Episode {} deleted", uuid))
    }
}
