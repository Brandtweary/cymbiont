pub mod test_harness;
pub mod wal_validation;

// Re-export commonly used items for convenience
pub use test_harness::{setup_test_env, cleanup_test_env};

// Export WAL validation types
pub use wal_validation::{WalValidator, MessagePattern};