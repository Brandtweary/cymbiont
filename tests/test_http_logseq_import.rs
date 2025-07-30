mod common;

use std::fs;
use std::process::{Command, Child};
use std::thread;
use std::time::Duration;
use serde_json::{json, Value};
use common::{setup_test_env, cleanup_test_env};

/// Check if --nocapture was passed to cargo test
fn is_nocapture() -> bool {
    std::env::args().any(|arg| arg == "--nocapture")
}

/// Start the Cymbiont server in the background
fn start_server(config_path: &str) -> (Child, u16) {
    // Read config to get server info filename
    let config_content = fs::read_to_string(config_path).expect("Failed to read config");
    let config: serde_yaml::Value = serde_yaml::from_str(&config_content).expect("Failed to parse config");
    let server_info_file = config["backend"]["server_info_file"].as_str()
        .expect("server_info_file not found in config");
    
    let mut cmd = Command::new("cargo");
    cmd.env("CYMBIONT_TEST_MODE", "1")
        .args(&["run", "--", "--server", "--config", config_path]);
    
    // Only inherit stdout/stderr if --nocapture was passed
    if !is_nocapture() {
        cmd.stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());
    }
    
    let mut child = cmd.spawn().expect("Failed to start server");
    
    // Wait for server to be ready by checking for server info file
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 150;  // 15 seconds with 100ms intervals (first build can be slow)
    let actual_port = loop {
        attempts += 1;
        
        // Check if server info file exists
        if let Ok(info_str) = fs::read_to_string(server_info_file) {
            if let Ok(info) = serde_json::from_str::<Value>(&info_str) {
                if let Some(port) = info["port"].as_u64() {
                    let port = port as u16;
                    
                    // Try to connect to verify it's really ready
                    let client = reqwest::blocking::Client::new();
                    let url = format!("http://localhost:{}/", port);
                    match client.get(&url).timeout(Duration::from_secs(1)).send() {
                        Ok(response) if response.status().is_success() => {
                            break port;  // Server is ready, return the port
                        }
                        _ => {
                            // Server not ready yet, keep waiting
                        }
                    }
                }
            }
        }
        
        if attempts >= MAX_ATTEMPTS {
            // Clean up the child process before panicking
            let _ = child.kill();
            panic!("Server failed to start after {} attempts ({}s)", MAX_ATTEMPTS, MAX_ATTEMPTS / 10);
        }
        
        thread::sleep(Duration::from_millis(100));
    };
    
    (child, actual_port)
}

/// Make an HTTP request to the server
fn make_import_request(port: u16, path: &str, graph_name: Option<&str>) -> Result<Value, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    
    let mut body = json!({
        "path": path
    });
    
    if let Some(name) = graph_name {
        body["graph_name"] = json!(name);
    }
    
    let url = format!("http://localhost:{}/import/logseq", port);
    let response = client
        .post(&url)
        .json(&body)
        .send()?;
    
    Ok(response.json()?)
}

#[test]
fn test_http_logseq_import() {
    // Set up test environment
    let test_env = setup_test_env();
    
    // Clone paths for use in closure
    let data_dir = test_env.data_dir.clone();
    let config_path = test_env.config_path.clone();
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Start the server
        let (mut server, port) = start_server(config_path.to_str().unwrap());
        
        // Get the absolute path to the dummy graph
        let dummy_graph_path = std::env::current_dir()
            .unwrap()
            .join("logseq_databases/dummy_graph");
        // Make the import request
        let response = make_import_request(
            port,
            dummy_graph_path.to_str().unwrap(),
            Some("test_http_import")
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
        
        // Terminate the server gracefully using --shutdown
        let mut shutdown_cmd = Command::new("cargo");
        shutdown_cmd.args(&["run", "--", "--shutdown", "--config", config_path.to_str().unwrap()]);
        
        // Suppress output unless --nocapture
        if !is_nocapture() {
            shutdown_cmd.stdout(std::process::Stdio::null())
                       .stderr(std::process::Stdio::null());
        }
        
        let shutdown_output = shutdown_cmd.output()
            .expect("Failed to run shutdown command");
        
        if !shutdown_output.status.success() {
            // Fallback to kill if shutdown fails
            let _ = server.kill();
        }
    });
    
    // Always clean up, even if test failed
    cleanup_test_env(test_env);
    
    // Re-panic if the test failed
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn test_http_import_error_cases() {
    // Set up test environment
    let test_env = setup_test_env();
    
    // Clone paths for use in closure
    let data_dir = test_env.data_dir.clone();
    let config_path = test_env.config_path.clone();
    
    let result = std::panic::catch_unwind(move || {
        // Start the server
        let (mut server, port) = start_server(config_path.to_str().unwrap());
        
        // Test 1: Non-existent path
        let response = make_import_request(port, "/path/that/does/not/exist", None)
            .expect("Failed to make import request");
        
        assert_eq!(response["success"], false);
        assert!(response["message"].as_str().unwrap().contains("does not exist"));
        
        // Test 2: Path is a file, not a directory
        let temp_file = data_dir.join("temp_file.txt");
        fs::write(&temp_file, "test").unwrap();
        
        let response = make_import_request(port, temp_file.to_str().unwrap(), None)
            .expect("Failed to make import request");
        
        assert_eq!(response["success"], false);
        assert!(response["message"].as_str().unwrap().contains("not a directory"));
        
        // Clean up temp file
        fs::remove_file(&temp_file).ok();
        
        // Terminate the server gracefully using --shutdown
        let mut shutdown_cmd = Command::new("cargo");
        shutdown_cmd.args(&["run", "--", "--shutdown", "--config", config_path.to_str().unwrap()]);
        
        // Suppress output unless --nocapture
        if !is_nocapture() {
            shutdown_cmd.stdout(std::process::Stdio::null())
                       .stderr(std::process::Stdio::null());
        }
        
        let shutdown_output = shutdown_cmd.output()
            .expect("Failed to run shutdown command");
        
        if !shutdown_output.status.success() {
            // Fallback to kill if shutdown fails
            let _ = server.kill();
        }
    });
    
    // Always clean up
    cleanup_test_env(test_env);
    
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}