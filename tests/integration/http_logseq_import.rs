use std::fs;
use serde_json::{json, Value};
use crate::common::{setup_test_env, cleanup_test_env};
use crate::common::test_harness::{TestServer, PreShutdown, PostShutdown, assert_phase};



/// Make an HTTP request to the server
fn make_import_request(port: u16, path: &str, graph_name: Option<&str>, auth_token: Option<&str>) -> Result<Value, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    
    let mut body = json!({
        "path": path
    });
    
    if let Some(name) = graph_name {
        body["graph_name"] = json!(name);
    }
    
    let url = format!("http://localhost:{}/import/logseq", port);
    let mut request = client.post(&url).json(&body);
    
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }
    
    let response = request.send()?;
    
    Ok(response.json()?)
}

pub fn test_http_logseq_import() {
    // Set up test environment
    let test_env = setup_test_env();
    
    
    // Clone paths for use in closure
    let data_dir = test_env.data_dir.clone();
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Start the server
        let server = TestServer::start(test_env);
        
        // Phase 1: Server is running - do HTTP operations
        assert_phase(PreShutdown);
        let port = server.port();
        let test_data_dir = server.test_env().data_dir.clone();
        
        // Read auth token
        let auth_token_path = test_data_dir.join("auth_token");
        let auth_token = fs::read_to_string(&auth_token_path)
            .expect("Failed to read auth token");
        
        // Get the absolute path to the dummy graph
        let dummy_graph_path = std::env::current_dir()
            .unwrap()
            .join("logseq_databases/dummy_graph");
        // Make the import request
        let response = make_import_request(
            port,
            dummy_graph_path.to_str().unwrap(),
            Some("test_http_import"),
            Some(&auth_token.trim())
        ).expect("Failed to make import request");
        
        // Verify the response
        assert_eq!(response["success"], true, "Import should succeed");
        assert!(response["message"].as_str().unwrap().contains("Successfully imported"));
        assert_eq!(response["graph_name"], "test_http_import");
        assert!(response["pages_imported"].as_u64().unwrap() > 0);
        assert!(response["blocks_imported"].as_u64().unwrap() > 0);
        assert_eq!(response["errors"].as_array().unwrap().len(), 0);
        
        let graph_id = response["graph_id"].as_str().expect("No graph_id in response");
        
        // Verify the graph registry was created
        let registry_path = data_dir.join("graph_registry.json");
        assert!(registry_path.exists(), "Graph registry not created");
        
        // Read the registry to verify our imported graph
        let registry_content = fs::read_to_string(registry_path).expect("Failed to read registry");
        let registry: Value = serde_json::from_str(&registry_content).expect("Failed to parse registry");
        let graphs = registry["graphs"].as_object().expect("No graphs object in registry");
        
        // Find our graph by ID
        let graph_info = graphs.get(graph_id).expect("Graph not found in registry");
        assert_eq!(graph_info["name"], "test_http_import", "Graph name mismatch");
        
        // Verify the knowledge graph was created
        let graph_path = data_dir.join("graphs").join(graph_id).join("knowledge_graph.json");
        assert!(graph_path.exists(), "Knowledge graph not created");
        
        // Read and parse the knowledge graph
        let graph_content = fs::read_to_string(&graph_path).expect("Failed to read knowledge graph");
        let graph: Value = serde_json::from_str(&graph_content).expect("Failed to parse knowledge graph");
        
        // Extract nodes for validation
        let nodes = graph["graph"]["nodes"].as_array().expect("No nodes in graph");
        
        // Count pages and blocks
        let mut pages = Vec::new();
        let mut blocks = Vec::new();
        
        for node in nodes {
            let node_type = node["node_type"].as_str().unwrap_or("");
            if node_type == "Page" {
                pages.push(node);
            } else if node_type == "Block" {
                blocks.push(node);
            }
        }
        
        // Basic validation
        assert!(pages.len() > 10, "Should have imported multiple pages");
        assert!(blocks.len() > 50, "Should have imported multiple blocks");
        
        // Find the cyberorganism-test-1 page
        let test_page = pages.iter()
            .find(|p| p["pkm_id"].as_str() == Some("cyberorganism-test-1"))
            .expect("cyberorganism-test-1 page not found");
        
        // Validate page properties
        let page_props = &test_page["properties"];
        assert_eq!(
            page_props["cymbiont-updated-ms"].as_str(),
            Some("1752719785318"),
            "Page cymbiont-updated-ms property not preserved"
        );
        
        // Test reference expansion (similar to CLI test)
        let ref_block = blocks.iter()
            .find(|b| {
                let content = b["content"].as_str().unwrap_or("");
                content == "((67f9a190-b504-46ca-b1d9-cfe1a80f1633))"
            })
            .expect("Block with reference not found");
        
        let ref_content = ref_block["reference_content"].as_str()
            .expect("reference_content field missing");
        assert_eq!(
            ref_content, 
            "## Introduction to Knowledge Graphs",
            "Block reference was not properly expanded"
        );
        
        // Phase 2: Shutdown server and wait for saves
        let test_env = server.shutdown();
        
        // Phase 3: Server has shutdown - safe to validate persisted data
        assert_phase(PostShutdown);
        
        test_env
    });
    
    // Always clean up, even if test failed
    if let Ok(test_env) = result {
        cleanup_test_env(test_env);
    } else if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

pub fn test_http_import_error_cases() {
    // Set up test environment
    let test_env = setup_test_env();
    
    // Clone paths for use in closure
    let data_dir = test_env.data_dir.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start the server
        let server = TestServer::start(test_env);
        
        // Phase 1: Server is running - do HTTP operations
        assert_phase(PreShutdown);
        let port = server.port();
        let test_data_dir = server.test_env().data_dir.clone();
        
        // Read auth token
        let auth_token_path = test_data_dir.join("auth_token");
        let auth_token = fs::read_to_string(&auth_token_path)
            .expect("Failed to read auth token");
        
        // Test 1: Non-existent path
        let response = make_import_request(port, "/path/that/does/not/exist", None, Some(&auth_token.trim()))
            .expect("Failed to make import request");
        
        assert_eq!(response["success"], false);
        assert!(response["message"].as_str().unwrap().contains("does not exist"));
        
        // Test 2: Path is a file, not a directory
        let temp_file = data_dir.join("temp_file.txt");
        fs::write(&temp_file, "test").unwrap();
        
        let response = make_import_request(port, temp_file.to_str().unwrap(), None, Some(&auth_token.trim()))
            .expect("Failed to make import request");
        
        assert_eq!(response["success"], false);
        assert!(response["message"].as_str().unwrap().contains("not a directory"));
        
        // Clean up temp file
        fs::remove_file(&temp_file).ok();
        
        // Phase 2: Shutdown server and wait for saves
        let test_env = server.shutdown();
        
        // Phase 3: Server has shutdown - safe to validate persisted data
        assert_phase(PostShutdown);
        
        test_env
    });
    
    // Always clean up
    if let Ok(test_env) = result {
        cleanup_test_env(test_env);
    } else if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}