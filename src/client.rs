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
    /// POST /episodes
    /// Hardcodes source='text' and group_id='default' for simplicity
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
            .map_err(|e| GraphitiError::Http(e))?;

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
            .map_err(|e| GraphitiError::Http(e))?;

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
            .map_err(|e| GraphitiError::Http(e))?;

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

    /// Trigger manual document sync
    /// POST /sync/trigger?async_mode={async_mode}
    ///
    /// If async_mode=true (default), returns immediately.
    /// If async_mode=false, waits for sync and returns detailed stats.
    pub async fn trigger_sync(&self, async_mode: bool) -> Result<String, GraphitiError> {
        let url = format!("{}/sync/trigger?async_mode={}", self.base_url, async_mode);

        let response = self
            .client
            .post(&url)
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

        if async_mode {
            // Async mode - just return simple message
            let _ = response.text().await;
            Ok("Document sync started in background".to_string())
        } else {
            // Sync mode - parse and format stats
            let result: Value = response
                .json()
                .await
                .map_err(|e| GraphitiError::InvalidResponse(e.to_string()))?;

            let synced = result["synced"].as_u64().unwrap_or(0);
            let skipped = result["skipped"].as_u64().unwrap_or(0);
            let errors = result["errors"].as_array().map(|a| a.len()).unwrap_or(0);

            Ok(format!(
                "Sync complete: {} synced, {} skipped, {} errors",
                synced, skipped, errors
            ))
        }
    }
}
