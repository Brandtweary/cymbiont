//! Registry reference pattern that prevents cache coherency issues.
//!
//! This module provides a newtype wrapper that forces all state queries
//! to go through the authoritative registry, making it impossible to cache
//! stale state locally.
//!
//! Also contains the authorized graph operations delegation implementation
//! to keep the boilerplate out of the main graph_operations module.

use std::sync::{Arc, RwLock};
use uuid::Uuid;
use crate::storage::agent_registry::AgentRegistry;

/// A reference to an entity in a registry that prevents local caching of registry state.
///
/// This pattern ensures that all queries about the entity's state must go through
/// the registry, preventing cache coherency bugs where local copies get out of sync.
pub struct RegistryRef<T> {
    registry: Arc<RwLock<T>>,
    entity_id: Uuid,
}

impl<T> RegistryRef<T> {
    /// Creates a new registry reference for the given entity.
    pub fn new(registry: Arc<RwLock<T>>, entity_id: Uuid) -> Self {
        Self {
            registry,
            entity_id,
        }
    }

}

impl RegistryRef<AgentRegistry> {
    /// Checks if this agent is authorized for the given graph.
    /// Always queries the registry for the current state.
    pub fn is_authorized_for(&self, graph_id: Uuid) -> bool {
        let registry = self.registry.read().unwrap();
        registry.is_agent_authorized(&self.entity_id, &graph_id)
    }

}

// Authorized graph operations delegation implementation
// This implementation provides the boilerplate delegation for AuthorizedAppState<Authorized>
// to keep graph_operations.rs focused on core operations logic.

use crate::graph_operations::{AuthorizedAppState, Authorized, GraphOperationsExt, Result};

impl GraphOperationsExt for AuthorizedAppState<Authorized> {
    async fn add_block(
        &self,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
        graph_id: &Uuid,
    ) -> Result<String> {
        // Authorization already verified by type system - delegate to underlying implementation
        GraphOperationsExt::add_block(&self.inner, content, parent_id, page_name, properties, graph_id).await
    }
    
    async fn update_block(&self, block_id: String, content: String, graph_id: &Uuid) -> Result<()> {
        GraphOperationsExt::update_block(&self.inner, block_id, content, graph_id).await
    }
    
    async fn delete_block(&self, block_id: String, graph_id: &Uuid) -> Result<()> {
        GraphOperationsExt::delete_block(&self.inner, block_id, graph_id).await
    }
    
    async fn create_page(&self, page_name: String, properties: Option<serde_json::Value>, graph_id: &Uuid) -> Result<()> {
        GraphOperationsExt::create_page(&self.inner, page_name, properties, graph_id).await
    }
    
    async fn delete_page(&self, page_name: String, graph_id: &Uuid) -> Result<()> {
        GraphOperationsExt::delete_page(&self.inner, page_name, graph_id).await
    }
    
    async fn get_node(&self, node_id: &str, graph_id: &Uuid) -> Result<serde_json::Value> {
        GraphOperationsExt::get_node(&self.inner, node_id, graph_id).await
    }
    
    fn query_graph_bfs(&self, start_id: &str, max_depth: usize, graph_id: &Uuid) -> Result<Vec<serde_json::Value>> {
        GraphOperationsExt::query_graph_bfs(&self.inner, start_id, max_depth, graph_id)
    }
    
    async fn open_graph(&self, graph_id: Uuid) -> Result<serde_json::Value> {
        GraphOperationsExt::open_graph(&self.inner, graph_id).await
    }
    
    async fn close_graph(&self, graph_id: Uuid) -> Result<()> {
        GraphOperationsExt::close_graph(&self.inner, graph_id).await
    }
    
    async fn list_open_graphs(&self) -> Result<Vec<Uuid>> {
        GraphOperationsExt::list_open_graphs(&self.inner).await
    }
    
    async fn list_graphs(&self) -> Result<Vec<serde_json::Value>> {
        GraphOperationsExt::list_graphs(&self.inner).await
    }
    
    async fn create_graph(&self, name: Option<String>, description: Option<String>) -> Result<serde_json::Value> {
        GraphOperationsExt::create_graph(&self.inner, name, description).await
    }
    
    async fn delete_graph(&self, graph_id: &Uuid) -> Result<()> {
        GraphOperationsExt::delete_graph(&self.inner, graph_id).await
    }
    
    async fn replay_transaction(&self, graph_id: &Uuid, transaction: crate::storage::Transaction, coordinator: std::sync::Arc<crate::storage::TransactionCoordinator>) -> Result<()> {
        GraphOperationsExt::replay_transaction(&self.inner, graph_id, transaction, coordinator).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_registry_ref_authorization_check() {
        // Create a registry and add an agent
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        
        let mut registry = AgentRegistry::load_or_create(&data_dir.join("agent_registry.json"), data_dir).unwrap();
        let agent_id = Uuid::new_v4();
        let graph_id = Uuid::new_v4();
        
        // Register an agent
        registry.register_agent(Some(agent_id), Some("test_agent".to_string()), Some("Test agent".to_string())).unwrap();
        
        // Create a RegistryRef for this agent
        let registry_arc = Arc::new(RwLock::new(registry));
        let registry_ref = RegistryRef::new(registry_arc.clone(), agent_id);
        
        // Initially not authorized
        assert!(!registry_ref.is_authorized_for(graph_id));
    }
}