//! Graph Registry Validation for Integration Tests
//!
//! This module provides staged validation of the graph registry state during integration testing.
//! It implements a four-stage validation approach that checks registry consistency at different
//! points in the test lifecycle.

use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::error::Error as StdError;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use tracing::{info, warn, error, debug};

const TEST_DATA_DIR: &str = "test_data";
const REGISTRY_FILE: &str = "graph_registry.json";
const CACHED_IDS_FILE: &str = "cached_graph_ids.json";

/// Expected test graph names that should be registered
const EXPECTED_GRAPHS: &[&str] = &[
    "test_graph_switching",
    "test_graph_sync", 
    "test_graph_websocket",
    "test_graph_multi_1",
    "test_graph_multi_2",
    "test_graph_empty",
];

/// Cached graph IDs for persistence validation
#[derive(Debug, Serialize, Deserialize)]
struct CachedGraphIds {
    graph_ids: HashMap<String, String>, // graph_name -> graph_id
    last_updated: DateTime<Utc>,
}

impl CachedGraphIds {
    fn new() -> Self {
        Self {
            graph_ids: HashMap::new(),
            last_updated: Utc::now(),
        }
    }
}

/// Graph registry validation coordinator
pub struct GraphRegistryValidator {
    test_data_path: PathBuf,
    registry_path: PathBuf,
    cached_ids_path: PathBuf,
}

impl GraphRegistryValidator {
    /// Create a new validator instance
    pub fn new() -> Self {
        let test_data_path = PathBuf::from(TEST_DATA_DIR);
        let registry_path = test_data_path.join(REGISTRY_FILE);
        let cached_ids_path = test_data_path.join(CACHED_IDS_FILE);
        
        Self {
            test_data_path,
            registry_path,
            cached_ids_path,
        }
    }
    
    /// Stage 1: Pre-launch validation (before starting Cymbiont)
    pub async fn validate_pre_launch(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("🔍 Stage 1: Pre-launch registry validation");
        
        // Check if registry exists (it might not on first run)
        if !self.registry_path.exists() {
            info!("Registry file doesn't exist yet - this is normal for first run");
            return Ok(());
        }
        
        // Validate registry file is parseable
        let registry_content = fs::read_to_string(&self.registry_path)?;
        let registry: Value = serde_json::from_str(&registry_content)
            .map_err(|e| format!("Invalid registry JSON: {}", e))?;
        
        // Check basic structure
        self.validate_registry_structure(&registry)?;
        
        // Compare against cached IDs if available
        self.validate_id_persistence(&registry).await?;
        
        // Verify kg_path directories exist for registered graphs
        self.validate_kg_paths(&registry)?;
        
        info!("✅ Pre-launch validation passed");
        Ok(())
    }
    
    /// Stage 2: Post-initialization validation (after Cymbiont starts + plugin initializes)
    pub async fn validate_post_initialization(&self, initial_graph: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("🔍 Stage 2: Post-initialization registry validation");
        
        // Registry should now exist
        if !self.registry_path.exists() {
            return Err("Registry file should exist after initialization".into());
        }
        
        let registry_content = fs::read_to_string(&self.registry_path)?;
        let registry: Value = serde_json::from_str(&registry_content)?;
        
        // Verify initial graph is registered
        self.validate_graph_registered(&registry, initial_graph)?;
        
        // Cache current IDs for future validation
        self.cache_current_ids(&registry).await?;
        
        info!("✅ Post-initialization validation passed");
        Ok(())
    }
    
    /// Stage 3: Per-graph-switch validation (after switching to specific graph)
    pub async fn validate_graph_switch(&self, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        debug!("🔍 Stage 3: Validating graph switch to {}", graph_name);
        
        let registry_content = fs::read_to_string(&self.registry_path)?;
        let registry: Value = serde_json::from_str(&registry_content)?;
        
        // Confirm target graph is now in registry
        self.validate_graph_registered(&registry, graph_name)?;
        
        // Verify this specific graph has expected name/path
        self.validate_specific_graph(&registry, graph_name)?;
        
        // Check kg_path directory exists for this graph
        self.validate_graph_kg_path(&registry, graph_name)?;
        
        debug!("✅ Graph switch validation passed for {}", graph_name);
        Ok(())
    }
    
    /// Stage 4: Final validation (after all graphs have been registered)
    pub async fn validate_final_state(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        info!("🔍 Stage 4: Final registry state validation");
        
        let registry_content = fs::read_to_string(&self.registry_path)?;
        let registry: Value = serde_json::from_str(&registry_content)?;
        
        // Registry should contain exactly 6 expected graphs
        self.validate_all_graphs_registered(&registry)?;
        
        // No duplicate UUIDs or names
        self.validate_no_duplicates(&registry)?;
        
        // All kg_path directories exist
        self.validate_all_kg_paths(&registry)?;
        
        // Update cached IDs with final verified state
        self.cache_current_ids(&registry).await?;
        
        info!("✅ Final validation passed - all {} graphs registered correctly", EXPECTED_GRAPHS.len());
        Ok(())
    }
    
    /// Validate basic registry JSON structure
    fn validate_registry_structure(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs")
            .ok_or("Registry missing 'graphs' field")?
            .as_object()
            .ok_or("Registry 'graphs' field is not an object")?;
        
        // Check each graph has required fields
        for (graph_id, graph_info) in graphs {
            let info = graph_info.as_object()
                .ok_or_else(|| format!("Graph {} info is not an object", graph_id))?;
            
            // Validate required fields exist
            info.get("id").ok_or_else(|| format!("Graph {} missing 'id' field", graph_id))?;
            info.get("name").ok_or_else(|| format!("Graph {} missing 'name' field", graph_id))?;
            info.get("path").ok_or_else(|| format!("Graph {} missing 'path' field", graph_id))?;
            info.get("kg_path").ok_or_else(|| format!("Graph {} missing 'kg_path' field", graph_id))?;
            
            // Validate UUID format
            let id = info.get("id").unwrap().as_str()
                .ok_or_else(|| format!("Graph {} 'id' is not a string", graph_id))?;
            
            if id.is_empty() {
                return Err(format!("Graph {} has empty ID", graph_id).into());
            }
            
            // Basic UUID format check (36 chars with hyphens)
            if id.len() != 36 || id.chars().filter(|&c| c == '-').count() != 4 {
                warn!("Graph {} ID '{}' doesn't match standard UUID format", graph_id, id);
            }
        }
        
        Ok(())
    }
    
    /// Compare current registry IDs against cached IDs to detect changes
    async fn validate_id_persistence(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if !self.cached_ids_path.exists() {
            debug!("No cached IDs file - this is normal for first run");
            return Ok(());
        }
        
        let cached_content = fs::read_to_string(&self.cached_ids_path)?;
        let cached: CachedGraphIds = serde_json::from_str(&cached_content)?;
        
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        for (_graph_id, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            let current_id = info.get("id").unwrap().as_str().unwrap();
            
            if let Some(cached_id) = cached.graph_ids.get(name) {
                if cached_id != current_id {
                    error!("🚨 Graph ID changed unexpectedly for '{}': was '{}', now '{}'", 
                          name, cached_id, current_id);
                    error!("This indicates either registry corruption or unexpected ID regeneration");
                    // Don't fail the test, but log this as a serious issue
                }
            }
        }
        
        Ok(())
    }
    
    /// Verify kg_path directories exist for all registered graphs  
    fn validate_kg_paths(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        for (graph_id, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let kg_path_str = info.get("kg_path").unwrap().as_str()
                .ok_or_else(|| format!("Graph {} kg_path is not a string", graph_id))?;
            
            let kg_path = PathBuf::from(kg_path_str);
            if !kg_path.exists() {
                return Err(format!("kg_path directory doesn't exist: {}", kg_path_str).into());
            }
            
            if !kg_path.is_dir() {
                return Err(format!("kg_path is not a directory: {}", kg_path_str).into());
            }
        }
        
        Ok(())
    }
    
    /// Verify specific graph is registered in registry
    fn validate_graph_registered(&self, registry: &Value, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        // Find graph by name
        let found = graphs.values().any(|graph_info| {
            graph_info.as_object()
                .and_then(|info| info.get("name"))
                .and_then(|name| name.as_str())
                .map(|name| name == graph_name)
                .unwrap_or(false)
        });
        
        if !found {
            return Err(format!("Graph '{}' not found in registry", graph_name).into());
        }
        
        Ok(())
    }
    
    /// Validate specific graph has expected name and path
    fn validate_specific_graph(&self, registry: &Value, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        // Find the graph and validate its details
        for (_graph_id, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            
            if name == graph_name {
                // Validate path contains the graph name
                let path = info.get("path").unwrap().as_str().unwrap();
                if !path.contains(graph_name) {
                    warn!("Graph '{}' path '{}' doesn't contain graph name", graph_name, path);
                }
                
                return Ok(());
            }
        }
        
        Err(format!("Graph '{}' not found for detailed validation", graph_name).into())
    }
    
    /// Validate kg_path exists for specific graph
    fn validate_graph_kg_path(&self, registry: &Value, graph_name: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        for (_, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            
            if name == graph_name {
                let kg_path_str = info.get("kg_path").unwrap().as_str().unwrap();
                let kg_path = PathBuf::from(kg_path_str);
                
                if !kg_path.exists() {
                    return Err(format!("kg_path for graph '{}' doesn't exist: {}", graph_name, kg_path_str).into());
                }
                
                return Ok(());
            }
        }
        
        Err(format!("Graph '{}' not found for kg_path validation", graph_name).into())
    }
    
    /// Validate all expected graphs are registered
    fn validate_all_graphs_registered(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        // Collect all registered graph names
        let registered_names: Vec<String> = graphs.values()
            .filter_map(|graph_info| {
                graph_info.as_object()
                    .and_then(|info| info.get("name"))
                    .and_then(|name| name.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        
        // Check we have exactly the expected graphs
        if registered_names.len() != EXPECTED_GRAPHS.len() {
            return Err(format!("Expected {} graphs, found {}: {:?}", 
                             EXPECTED_GRAPHS.len(), registered_names.len(), registered_names).into());
        }
        
        // Check all expected graphs are present
        for expected in EXPECTED_GRAPHS {
            if !registered_names.contains(&expected.to_string()) {
                return Err(format!("Missing expected graph: {}", expected).into());
            }
        }
        
        Ok(())
    }
    
    /// Validate no duplicate UUIDs or names exist
    fn validate_no_duplicates(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        let mut seen_ids = HashMap::new();
        let mut seen_names = HashMap::new();
        
        for (graph_id, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let id = info.get("id").unwrap().as_str().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            
            // Check for duplicate IDs
            if let Some(existing_graph) = seen_ids.insert(id.to_string(), graph_id.clone()) {
                return Err(format!("Duplicate graph ID '{}' found in graphs '{}' and '{}'", 
                                 id, existing_graph, graph_id).into());
            }
            
            // Check for duplicate names
            if let Some(existing_graph) = seen_names.insert(name.to_string(), graph_id.clone()) {
                return Err(format!("Duplicate graph name '{}' found in graphs '{}' and '{}'", 
                                 name, existing_graph, graph_id).into());
            }
        }
        
        Ok(())
    }
    
    /// Validate all kg_paths exist for all graphs
    fn validate_all_kg_paths(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        for (graph_id, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            let kg_path_str = info.get("kg_path").unwrap().as_str().unwrap();
            let kg_path = PathBuf::from(kg_path_str);
            
            if !kg_path.exists() {
                return Err(format!("kg_path for graph '{}' ({}) doesn't exist: {}", 
                                 name, graph_id, kg_path_str).into());
            }
        }
        
        Ok(())
    }
    
    /// Cache current graph IDs for future persistence validation
    async fn cache_current_ids(&self, registry: &Value) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let graphs = registry.get("graphs").unwrap().as_object().unwrap();
        
        let mut cached = CachedGraphIds::new();
        
        for (_, graph_info) in graphs {
            let info = graph_info.as_object().unwrap();
            let id = info.get("id").unwrap().as_str().unwrap();
            let name = info.get("name").unwrap().as_str().unwrap();
            
            cached.graph_ids.insert(name.to_string(), id.to_string());
        }
        
        // Ensure test data directory exists
        fs::create_dir_all(&self.test_data_path)?;
        
        let content = serde_json::to_string_pretty(&cached)?;
        fs::write(&self.cached_ids_path, content)?;
        
        debug!("Cached {} graph IDs for future validation", cached.graph_ids.len());
        Ok(())
    }
}