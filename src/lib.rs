//! Cymbiont Library Interface
//! 
//! This library exposes the core functionality of Cymbiont for use in other projects.

// Core modules that should be preserved and exposed
pub mod graph_manager;
pub mod pkm_data;
pub mod saga;
pub mod transaction;
pub mod transaction_log;

// Additional modules that might be needed by consumers
pub mod config;
pub mod graph_registry;
pub mod session_manager;
pub mod utils;

// Re-export commonly used types and traits at the root level
pub use graph_manager::GraphManager;
pub use pkm_data::{PKMBlockData, PKMPageData};
pub use transaction_log::{Transaction, TransactionState, TransactionLog};
pub use transaction::{TransactionCoordinator, TransactionError};
pub use saga::{Saga, SagaError};

// Module for EDN manipulation (might be useful for config management)
pub mod edn;

// Note: api.rs, websocket.rs, logging.rs, and kg_api.rs are application-specific 
// and not exposed in the library interface as they depend on AppState