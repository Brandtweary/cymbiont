use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

// Global counter for unique test directories
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Test environment with paths
pub struct TestEnv {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

/// Set up test environment with unique config and data directory
pub fn setup_test_env() -> TestEnv {
    // Create unique test ID
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let test_dir = format!("test_data_{}", test_id);
    let test_data_dir = Path::new(&test_dir);
    
    // Clean up if it exists (shouldn't happen but be safe)
    if test_data_dir.exists() {
        fs::remove_dir_all(test_data_dir).expect("Failed to remove existing test directory");
    }
    
    // Create the data directory
    fs::create_dir_all(test_data_dir).expect("Failed to create test data directory");
    
    // Create unique config file with unique port
    let test_port = 3000 + test_id as u16;
    let config_path = PathBuf::from(format!("config.test.{}.yaml", test_id));
    let config_content = format!(
        r#"# Cymbiont Test Configuration for test {}

# Backend server configuration
backend:
  host: 127.0.0.1
  port: {}
  max_port_attempts: 10
  server_info_file: "cymbiont_server_test_{}.json"

# Development-only settings
development:
  # 3 second duration for tests
  default_duration: 3

# Data storage directory - unique per test
data_dir: {}
"#,
        test_id, test_port, test_id, test_dir
    );
    
    fs::write(&config_path, config_content).expect("Failed to write test config");
    
    // Set environment variable to use test config
    env::set_var("CYMBIONT_TEST_MODE", "1");
    
    TestEnv {
        data_dir: test_data_dir.to_path_buf(),
        config_path,
    }
}

/// Clean up test environment after tests
pub fn cleanup_test_env(test_env: TestEnv) {
    // Remove test data directory
    if test_env.data_dir.exists() {
        fs::remove_dir_all(&test_env.data_dir).expect("Failed to remove test data directory");
    }
    
    // Remove test config file
    if test_env.config_path.exists() {
        fs::remove_file(&test_env.config_path).expect("Failed to remove test config file");
    }
    
    // Clean up server info file (extract test_id from config path)
    if let Some(config_name) = test_env.config_path.file_stem() {
        if let Some(config_str) = config_name.to_str() {
            if let Some(test_id_str) = config_str.strip_prefix("config.test.") {
                let server_info_file = format!("cymbiont_server_test_{}.json", test_id_str);
                let _ = fs::remove_file(&server_info_file);
            }
        }
    }
    
    // Unset test environment variable
    env::remove_var("CYMBIONT_TEST_MODE");
}