//! Integration tests for graph registry functionality
//!
//! These tests verify graph registration and switching logic
//! without requiring Logseq to be installed.

use cymbiont::graph_registry::GraphRegistry;
use cymbiont::session_manager::{SessionManager, DbIdentifier};
use cymbiont::config::LogseqConfig;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[test]
fn test_graph_registry_multiple_graphs() {
    let temp_dir = tempdir().unwrap();
    let registry_path = temp_dir.path().join("test_registry.json");
    
    let mut registry = GraphRegistry::new();
    
    // Register first graph
    let graph1 = registry.register_graph(
        "dummy_graph".to_string(),
        "logseq_databases/dummy_graph".to_string(),
        None
    ).unwrap();
    
    assert_eq!(graph1.name, "dummy_graph");
    assert_eq!(graph1.path, "logseq_databases/dummy_graph");
    assert!(!graph1.id.is_empty());
    
    // Register second graph
    let graph2 = registry.register_graph(
        "dummy_graph_2".to_string(),
        "logseq_databases/dummy_graph_2".to_string(),
        None
    ).unwrap();
    
    assert_eq!(graph2.name, "dummy_graph_2");
    assert_ne!(graph1.id, graph2.id, "Graphs should have different IDs");
    
    // Set active graph
    registry.set_active_graph(&graph2.id).unwrap();
    let active = registry.get_active_graph().unwrap();
    assert_eq!(active.id, graph2.id);
    
    // Save and reload
    registry.save(&registry_path).unwrap();
    let loaded_registry = GraphRegistry::load_or_create(&registry_path).unwrap();
    
    // Verify persistence
    assert_eq!(loaded_registry.get_all_graphs().len(), 2);
    assert_eq!(loaded_registry.get_active_graph_id(), Some(&graph2.id as &str));
}

#[test]
fn test_graph_switching_by_name_and_path() {
    let mut registry = GraphRegistry::new();
    
    // Register graphs with both name and path
    let graph1 = registry.register_graph(
        "personal".to_string(),
        "/home/user/logseq/personal".to_string(),
        None
    ).unwrap();
    
    let graph2 = registry.register_graph(
        "work".to_string(),
        "/home/user/logseq/work".to_string(),
        None
    ).unwrap();
    
    // Test finding by name
    let found_graphs = registry.get_all_graphs();
    let found_by_name = found_graphs.iter()
        .find(|g| g.name == "personal")
        .unwrap();
    assert_eq!(found_by_name.id, graph1.id);
    
    // Test finding by path
    let found_by_path = found_graphs.iter()
        .find(|g| g.path == "/home/user/logseq/work")
        .unwrap();
    assert_eq!(found_by_path.id, graph2.id);
}

#[tokio::test]
async fn test_session_manager_launch_targets() {
    let registry = Arc::new(Mutex::new(GraphRegistry::new()));
    
    // Pre-register some graphs
    {
        let mut reg = registry.lock().unwrap();
        reg.register_graph(
            "test_graph_1".to_string(),
            "path/to/graph1".to_string(),
            None
        ).unwrap();
        
        reg.register_graph(
            "test_graph_2".to_string(),
            "path/to/graph2".to_string(),
            None
        ).unwrap();
    }
    
    // Create config with auto_launch disabled to avoid actually launching Logseq
    let config = LogseqConfig {
        auto_launch: false,
        executable_path: None,
        launch_specific_database: true,
        default_database: Some("test_graph_1".to_string()),
        databases: vec![],
    };
    
    let session_manager = SessionManager::new(registry.clone(), config);
    
    // Test launching with CLI override by name
    let result = session_manager.launch_logseq(Some(DbIdentifier::Name("test_graph_2".to_string()))).await;
    assert!(result.is_ok());
    
    // Since auto_launch is false, it should return None
    assert!(result.unwrap().is_none());
    
    // But the registry should have the graphs registered
    let reg = registry.lock().unwrap();
    assert_eq!(reg.get_all_graphs().len(), 2);
}

#[test]
fn test_duplicate_graph_handling() {
    let mut registry = GraphRegistry::new();
    
    // Register a graph
    let graph1 = registry.register_graph(
        "my_notes".to_string(),
        "/home/user/my_notes".to_string(),
        Some("uuid-123".to_string())
    ).unwrap();
    
    // Try to register the same graph again (same name AND path)
    let graph2 = registry.register_graph(
        "my_notes".to_string(),
        "/home/user/my_notes".to_string(),
        None
    ).unwrap();
    
    // Should get the same ID (recovery mode)
    assert_eq!(graph1.id, graph2.id);
    
    // Register with same name but different path
    let graph3 = registry.register_graph(
        "my_notes".to_string(),
        "/home/user/other_location/my_notes".to_string(),
        None
    ).unwrap();
    
    // Should get a different ID (different graph)
    assert_ne!(graph1.id, graph3.id);
    
    // Verify we have 2 graphs total
    assert_eq!(registry.get_all_graphs().len(), 2);
}