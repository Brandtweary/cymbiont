// tests/integration/main.rs
//
// Integration test suite entry point
// All integration tests are compiled as a single binary to avoid dead code warnings

// Import common utilities once for all integration tests
#[path = "../common/mod.rs"]
mod common;

// Import all integration test modules
mod http_logseq_import;
mod logseq_import;
mod websocket_commands;

// Re-export test functions with #[test] attribute
#[test]
fn test_http_logseq_import() {
    http_logseq_import::test_http_logseq_import();
}

#[test]
fn test_http_import_error_cases() {
    http_logseq_import::test_http_import_error_cases();
}

#[test]
fn test_logseq_import_cyberorganism_test_1() {
    logseq_import::test_logseq_import_cyberorganism_test_1();
}

#[test]
fn test_websocket_commands() {
    websocket_commands::test_websocket_commands();
}