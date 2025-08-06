//! Storage Layer
//!
//! This module contains all persistence-related functionality for Cymbiont's
//! knowledge graph system. It provides the foundation for durable storage,
//! transaction management, and multi-graph registry operations.
//!
//! ## Architecture Overview
//!
//! The storage layer is designed around three core principles:
//! - **Durability**: All graph mutations are logged before execution
//! - **Isolation**: Each graph has its own storage directory and transaction log
//! - **Consistency**: ACID guarantees through write-ahead logging with sled
//!
//! ## Components
//!
//! ### Graph Registry (`graph_registry.rs`)
//! Multi-graph management and metadata persistence:
//! - Graph UUID registration and lookup with collision handling
//! - Graph switching and lifecycle management with timestamps
//! - Registry persistence to `graph_registry.json` with atomic writes
//! - Graph archival and removal with data preservation
//! - Open/closed graph state tracking for session management
//!
//! ### Transaction Log (`transaction_log.rs`)
//! Write-ahead logging with ACID guarantees using sled embedded database:
//! - Transaction persistence with three logical trees (transactions, hash index, pending)
//! - Content hash deduplication to prevent duplicate processing
//! - Crash recovery with pending transaction enumeration
//! - State transitions: Active → Committed | Aborted
//! - Performance optimizations: 64MB cache, 100ms flush intervals
//!
//! ### Transaction Coordinator (`transaction.rs`) 
//! High-level transaction lifecycle management:
//! - Transaction begin/commit/abort operations with error handling
//! - Duplicate content detection via SHA-256 content hashing
//! - Unified transaction execution patterns with `execute_with_transaction()`
//! - Operation-level transaction wrapping for graph mutations
//! - Recovery coordination for incomplete transactions on startup
//!
//! ## Data Flow
//!
//! ```text
//! Operation Request
//!     ↓
//! Transaction Coordinator
//!     ↓
//! Content Hash Check → [Duplicate? Return existing]
//!     ↓
//! Transaction Log (sled)
//!     ↓
//! Graph Manager Operation
//!     ↓
//! Success/Error Response
//!     ↓
//! Transaction Commit/Abort
//! ```
//!
//! ## Error Handling
//!
//! The storage layer implements fail-safe patterns:
//! - Non-fatal errors (duplicate content) return existing data
//! - Fatal errors (I/O failures) bubble up with context
//! - Transaction rollback on any operation failure
//! - Registry corruption recovery through backup restoration

pub mod graph_registry;
pub mod transaction_log;
pub mod transaction;
pub mod graph_persistence;

// Re-export commonly used types
pub use graph_registry::GraphRegistry;
pub use transaction_log::{TransactionLog, Operation, Transaction, OperationExecutor};
pub use transaction::TransactionCoordinator;