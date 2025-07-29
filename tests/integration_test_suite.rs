//! Main integration test suite that runs all tests on a single Cymbiont+Logseq instance
//!
//! This test suite launches Cymbiont with Logseq once and runs all integration tests
//! sequentially with proper graph isolation between tests.

#[path = "common/mod.rs"]
mod common;
use common::test_harness::IntegrationTestHarness;

// Import test modules
#[path = "integration_tests/sync_test.rs"]
mod sync_test;

#[path = "integration_tests/websocket_test.rs"]
mod websocket_test;

#[path = "integration_tests/multi_graph_test.rs"]
mod multi_graph_test;

use tracing::{info, error};

use serial_test::serial;

#[serial]
#[tokio::test]
async fn run_all_integration_tests() {
    // Initialize logging for tests
    tracing_subscriber::fmt()
        .with_env_filter("info,cymbiont=debug")
        .init();
    
    info!("Starting Cymbiont integration test suite");
    
    // Setup the test harness once
    let harness = match IntegrationTestHarness::setup().await {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to setup test harness: {}", e);
            panic!("Test harness setup failed");
        }
    };
    
    info!("Test harness ready, running test suite");
    
    // Track test results
    let mut failed_tests = Vec::new();
    
    // Run sync tests
    info!("=== Running sync tests ===");
    if let Err(e) = harness.run_test("test_graph_sync", |h| Box::pin(async {
        sync_test::test_real_time_sync(h).await?;
        sync_test::test_incremental_sync(h).await?;
        sync_test::test_deletion_detection(h).await?;
        sync_test::test_force_sync_flags(h).await?;
        Ok(())
    })).await {
        error!("Sync tests failed: {}", e);
        failed_tests.push("sync_tests");
    }
    
    // Run WebSocket tests
    info!("=== Running WebSocket tests ===");
    if let Err(e) = harness.run_test("test_graph_websocket", |h| Box::pin(async {
        websocket_test::test_websocket_connection(h).await?;
        websocket_test::test_sync_operations_no_websocket_commands(h).await?;
        // Note: kg_api WebSocket command tests will be added once HTTP endpoints exist
        Ok(())
    })).await {
        error!("WebSocket tests failed: {}", e);
        failed_tests.push("websocket_tests");
    }
    
    // Run multi-graph tests
    info!("=== Running multi-graph tests ===");
    if let Err(e) = harness.run_test("test_graph_switching", |h| Box::pin(async {
        multi_graph_test::test_basic_graph_switch(h).await?;
        multi_graph_test::test_graph_switch_persistence(h).await?;
        Ok(())
    })).await {
        error!("Multi-graph switching tests failed: {}", e);
        failed_tests.push("multi_graph_switching_tests");
    }
    
    // Run multi-graph isolation tests (uses two graphs)
    info!("=== Running multi-graph isolation tests ===");
    if let Err(e) = multi_graph_test::test_multi_graph_isolation(&harness).await {
        error!("Multi-graph isolation tests failed: {}", e);
        failed_tests.push("multi_graph_isolation_tests");
    }
    
    // Teardown
    info!("Test suite complete, tearing down");
    if let Err(e) = harness.teardown().await {
        error!("Failed to teardown test harness: {}", e);
    }
    
    // Report results
    if failed_tests.is_empty() {
        info!("✅ All integration tests passed!");
    } else {
        error!("❌ {} test groups failed: {:?}", failed_tests.len(), failed_tests);
        panic!("Integration tests failed");
    }
}