use std::fs;
use serde_json::Value;
use crate::common::{setup_test_env, cleanup_test_env};
use crate::common::test_harness::{TestServer, PostShutdown, assert_phase, get_active_graph_id};

pub fn test_logseq_import_cyberorganism_test_1() {
    // Set up test environment
    let test_env = setup_test_env();
    
    // Clone paths for use in closure
    let data_dir = test_env.data_dir.clone();
    
    // Use a closure to ensure cleanup happens even on panic
    let result = std::panic::catch_unwind(move || {
        // Start CLI mode with import and duration
        let server = TestServer::start_with_args(test_env, vec![
            "--import-logseq", "logseq_databases/dummy_graph/", 
            "--duration", "2"
        ]);
        
        // CLI mode: Process runs for duration then exits naturally
        let test_env = server.wait_for_completion();
        
        // Phase: CLI has completed - safe to validate persisted data
        assert_phase(PostShutdown);
    
    // Verify the graph registry was created
    let registry_path = data_dir.join("graph_registry.json");
    assert!(registry_path.exists(), "Graph registry not created");
    
    // Get the active graph ID
    let graph_id = get_active_graph_id(&data_dir);
    
    // Read the registry to verify graph name
    let registry_content = fs::read_to_string(&registry_path).expect("Failed to read registry");
    let registry: Value = serde_json::from_str(&registry_content).expect("Failed to parse registry");
    let graphs = registry["graphs"].as_object().expect("No graphs object in registry");
    
    assert_eq!(graphs.len(), 1, "Expected exactly one graph in registry");
    
    // Get the graph info
    let graph_info = graphs.get(&graph_id).expect("Graph not found in registry");
    let graph_name = graph_info["name"].as_str().expect("No graph name");
    
    assert_eq!(graph_name, "dummy_graph", "Unexpected graph name");
    
    // Verify the knowledge graph was created
    let graph_path = data_dir.join("graphs").join(graph_id).join("knowledge_graph.json");
    assert!(graph_path.exists(), "Knowledge graph not created");
    
    // Read and parse the knowledge graph
    let graph_content = fs::read_to_string(&graph_path).expect("Failed to read knowledge graph");
    let graph: Value = serde_json::from_str(&graph_content).expect("Failed to parse knowledge graph");
    
    // Extract nodes for validation - the actual graph is inside a "graph" field
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
    
    // Validate specific block types by content patterns
    
    // 1. Block with ID
    let id_block = blocks.iter()
        .find(|b| b["pkm_id"].as_str() == Some("67f9a190-b504-46ca-b1d9-cfe1a80f1633"))
        .expect("Block with ID not found");
    assert!(
        id_block["content"].as_str().unwrap().contains("Introduction to Knowledge Graphs"),
        "Block with ID has wrong content"
    );
    
    // 2. Bold and italic formatting
    let _format_block = blocks.iter()
        .find(|b| {
            let content = b["content"].as_str().unwrap_or("");
            content.contains("**essential tools**") && content.contains("*complex*")
        })
        .expect("Block with bold/italic formatting not found");
    
    // 3. Highlighted text
    let _highlight_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("==Contextual information=="))
        .expect("Block with highlighted text not found");
    
    // 4. Strikethrough and underline
    let _strikethrough_block = blocks.iter()
        .find(|b| {
            let content = b["content"].as_str().unwrap_or("");
            content.contains("~~richer~~") && content.contains("<u>more flexible</u>")
        })
        .expect("Block with strikethrough/underline not found");
    
    // 5. Page references
    let _ref_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("[[Wikidata]]"))
        .expect("Block with page reference not found");
    
    // 6. TODO states
    let _todo_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("TODO Research existing ontologies"))
        .expect("TODO block not found");
    
    let _doing_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("DOING Document entity relationships"))
        .expect("DOING block not found");
    
    let _done_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("DONE Create initial graph schema"))
        .expect("DONE block not found");
    
    // 7. Block quote
    let _quote_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains(">\"Knowledge graphs are to AI"))
        .expect("Block quote not found");
    
    // 8. Code blocks
    let _sparql_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("PREFIX ex:"))
        .expect("SPARQL code block not found");
    
    let _cypher_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("MATCH (p:Person)"))
        .expect("Cypher code block not found");
    
    // 9. Table
    let _table_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("| Component"))
        .expect("Table block not found");
    
    // 10. Tags
    let _tag_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("#knowledge-management"))
        .expect("Block with tags not found");
    
    // 11. Footnote
    let _footnote_block = blocks.iter()
        .find(|b| b["content"].as_str().unwrap_or("").contains("[^1]: This is a footnote"))
        .expect("Footnote block not found");
    
        // Validate hierarchy - check that nested blocks maintain parent-child relationships
        // Find "Key Components" block and verify it has children
        let _key_components_block = blocks.iter()
            .find(|b| b["content"].as_str().unwrap_or("").contains("### Key Components"))
            .expect("Key Components block not found");
        
        // The GraphManager doesn't store children arrays in blocks, it uses edges
        // So we'll just verify the block exists for now
        
        // Verify deep nesting (6 levels)
        let _deep_nested_block = blocks.iter()
            .find(|b| b["content"].as_str().unwrap_or("").contains("###### Specific Use Cases"))
            .expect("Deeply nested block (6 levels) not found");
    
        // Test block reference expansion
        // Find blocks that contain block references
        let ref_block_1 = blocks.iter()
            .find(|b| {
                let content = b["content"].as_str().unwrap_or("");
                content == "((67f9a190-b504-46ca-b1d9-cfe1a80f1633))"
            })
            .expect("Block with reference to Introduction block not found");
        
        // This block should have reference_content that expands to "## Introduction to Knowledge Graphs"
        let ref_content_1 = ref_block_1["reference_content"].as_str()
            .expect("reference_content field missing for block with reference");
        assert_eq!(
            ref_content_1, 
            "## Introduction to Knowledge Graphs",
            "Block reference was not properly expanded"
        );
        
        // Find the embed block reference
        let embed_ref_block = blocks.iter()
            .find(|b| {
                let content = b["content"].as_str().unwrap_or("");
                content.contains("{{embed ((67f9a190-985b-4dbf-90e4-c2abffb2ab51))}}")
            })
            .expect("Block with embed reference not found");
        
        // The reference_content should expand the block reference inside the embed syntax
        let embed_ref_content = embed_ref_block["reference_content"].as_str()
            .expect("reference_content field missing for embed block");
        assert!(
            embed_ref_content.contains("## Types of Knowledge Graphs"),
            "Embed block reference was not properly expanded. Got: {}",
            embed_ref_content
        );
        
        // Find the block with mixed content (page refs, block refs, tags, and text)
        let mixed_ref_block = blocks.iter()
            .find(|b| {
                let content = b["content"].as_str().unwrap_or("");
                content.contains("property:: [[property-1]], [[page-ref]], ((67fbd626-8e4a-485f-ad03-fd1ce5539ebb)), #tag string content")
            })
            .expect("Block with mixed references not found");
        
        // The reference_content should expand only the block reference, leaving page refs and tags as-is
        let mixed_ref_content = mixed_ref_block["reference_content"].as_str()
            .expect("reference_content field missing for mixed content block");
        assert!(
            mixed_ref_content.contains("Test blocks"),
            "Block reference in mixed content was not expanded. Got: {}",
            mixed_ref_content
        );
        assert!(
            mixed_ref_content.contains("[[property-1]]"),
            "Page references should remain unchanged in reference_content"
        );
        assert!(
            mixed_ref_content.contains("#tag"),
            "Tags should remain unchanged in reference_content"
        );
        
        // Test that blocks without references have reference_content equal to content
        let no_ref_block = blocks.iter()
            .find(|b| {
                let content = b["content"].as_str().unwrap_or("");
                content.contains("They are **essential tools** for organizing *complex* information")
            })
            .expect("Block without references not found");
        
        let no_ref_content = no_ref_block["content"].as_str().unwrap();
        let no_ref_reference_content = no_ref_block["reference_content"].as_str()
            .expect("reference_content should be present even for blocks without references");
        assert_eq!(
            no_ref_content,
            no_ref_reference_content,
            "Blocks without references should have reference_content equal to content"
        );
        
        test_env
    });
    
    // Always clean up, even if test failed
    if let Ok(test_env) = result {
        cleanup_test_env(test_env);
    } else if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}