//! Storage Layer - WAL-Only Architecture
//!
//! This module implements a pure Write-Ahead Logging (WAL) system for all
//! Cymbiont persistence. The WAL is the single source of truth, with all
//! state reconstructed from transaction replay.
//!
//! ## Architecture Overview
//!
//! The storage layer follows these core principles:
//! - **Single Source of Truth**: WAL contains all state changes
//! - **Transaction-First**: All mutations go through the transaction system
//! - **In-Memory State**: Registries and graphs live in memory, rebuilt from WAL
//! - **On-Demand Snapshots**: JSONs generated only for debugging/testing
//!
//! ## Components
//!
//! ### Write-Ahead Log (`wal.rs`)
//! Write-ahead logging with ACID guarantees using sled embedded database:
//! - Comprehensive operation types: Graph, Agent, Registry operations
//! - Content hash deduplication to prevent duplicate processing
//! - Three logical trees: transactions, content_hash_index, pending_index
//! - State transitions: Active → Committed | Aborted
//!
//! ### Transaction Coordinator (`transaction_coordinator.rs`) 
//! High-level transaction lifecycle management:
//! - Global coordinator for all operation types
//! - Duplicate detection via SHA-256 content hashing
//! - Graceful shutdown with transaction completion
//! - Recovery coordination for pending transactions
//!
//! ### Recovery System (`recovery.rs`)
//! Unified recovery for all operation types:
//! - RecoveryContext bundles all necessary resources
//! - Replays entire WAL on startup
//! - Rebuilds complete state from operations
//! - Handles graph, agent, and registry operations
//!
//! ## Data Flow
//!
//! ```text
//! Operation Request
//!     ↓
//! Transaction Coordinator
//!     ↓
//! Log to WAL (sled)
//!     ↓
//! Update In-Memory State
//!     ↓
//! Success Response
//! ```
//!
//! ## Recovery Flow
//!
//! ```text
//! Startup
//!     ↓
//! Replay WAL
//!     ↓
//! Rebuild All State
//!     ↓
//! Ready to Serve
//! ```

pub mod transaction_coordinator;
pub mod wal;
pub mod recovery;

// Re-export commonly used types
pub use wal::{TransactionLog, Operation};
pub use transaction_coordinator::TransactionCoordinator;
