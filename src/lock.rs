//! Lock handling utilities for simplified RwLock error management
//! 
//! This module provides extension traits for RwLock types that implement
//! Cymbiont's panic-on-poison strategy. Since RwLock poisoning indicates
//! a thread panicked while holding the lock, data integrity cannot be
//! guaranteed. For a data-critical application like Cymbiont, we prefer
//! to panic rather than attempt recovery with potentially corrupted state.
//!
//! ## Features
//!
//! - **Panic-on-poison**: Immediate panic when encountering poisoned locks
//! - **Contention detection**: Automatic warnings for lock contention in debug builds
//! - **Descriptive contexts**: All lock operations include context for debugging
//! - **Unified API**: Same interface for both sync and async locks
//! - **Lock ordering**: Helper functions to prevent deadlocks
//!
//! ## Usage
//!
//! ```rust
//! use crate::lock::RwLockExt;
//! use std::sync::RwLock;
//! 
//! let data = RwLock::new(42);
//! 
//! // Read with panic-on-poison
//! let value = data.read_or_panic("reading configuration");
//! 
//! // Write with contention detection
//! let mut value = data.write_or_panic("updating configuration");
//! *value = 100;
//! ```
//!
//! ## Async Locks
//!
//! ```rust
//! use crate::lock::AsyncRwLockExt;
//! use tokio::sync::RwLock;
//! 
//! let data = RwLock::new(42);
//! 
//! // Async locks can't be poisoned, but API is consistent
//! let value = data.read_or_panic("async read").await;
//! let mut value = data.write_or_panic("async write").await;
//! ```
//!
//! ## Lock Ordering
//!
//! To prevent deadlocks when multiple locks are needed:
//!
//! ```rust
//! use crate::lock::lock_registries_for_write;
//! 
//! // Always acquires graph_registry before agent_registry
//! let (graph_reg, agent_reg) = lock_registries_for_write(
//!     &app_state.graph_registry,
//!     &app_state.agent_registry
//! )?;
//! ```

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tracing::warn;

/// Extension trait for std::sync::RwLock to simplify error handling
/// 
/// This trait provides panic-on-poison methods that are appropriate for
/// Cymbiont's data integrity requirements. When a lock is poisoned, it
/// means another thread panicked while holding the lock, making the
/// data potentially inconsistent.
pub trait RwLockExt<T> {
    /// Read the lock or panic on poison
    /// 
    /// This method implements Cymbiont's panic-on-poison strategy.
    /// If the lock is poisoned, the application will panic with a
    /// descriptive message rather than attempting to continue with
    /// potentially corrupted data.
    /// 
    /// # Arguments
    /// * `context` - Description of what operation was attempted for better panic messages
    /// 
    /// # Panics
    /// Panics if the lock is poisoned, indicating data integrity issues
    fn read_or_panic(&self, context: &str) -> RwLockReadGuard<T>;

    /// Write to the lock or panic on poison
    /// 
    /// This method implements Cymbiont's panic-on-poison strategy.
    /// If the lock is poisoned, the application will panic with a
    /// descriptive message rather than attempting to continue with
    /// potentially corrupted data.
    /// 
    /// # Arguments
    /// * `context` - Description of what operation was attempted for better panic messages
    /// 
    /// # Panics
    /// Panics if the lock is poisoned, indicating data integrity issues
    fn write_or_panic(&self, context: &str) -> RwLockWriteGuard<T>;


}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_or_panic(&self, context: &str) -> RwLockReadGuard<T> {
        self.read().unwrap_or_else(|poison_error| {
            panic!(
                "RwLock poisoned during read operation: {}. \
                This indicates a thread panicked while holding the lock, \
                compromising data integrity. Error: {}",
                context,
                poison_error
            );
        })
    }

    fn write_or_panic(&self, context: &str) -> RwLockWriteGuard<T> {
        // Check for lock contention in debug builds and warn (not panic)
        #[cfg(debug_assertions)]
        {
            if self.try_write().is_err() {
                warn!(
                    "⚠️ Lock contention detected during '{}': another thread is holding the lock. \
                    This may indicate a performance issue or the freeze mechanism in tests.",
                    context
                );
            }
        }
        
        self.write().unwrap_or_else(|poison_error| {
            panic!(
                "RwLock poisoned during write operation: {}. \
                This indicates a thread panicked while holding the lock, \
                compromising data integrity. Error: {}",
                context,
                poison_error
            );
        })
    }


}



/// Macro to acquire multiple locks in a consistent order to prevent deadlocks
/// 
/// This macro ensures that locks are always acquired in the same order,
/// preventing deadlock scenarios. It's particularly useful for operations
/// that need both graph_registry and agent_registry locks.
/// 
/// Example:
/// ```rust
/// let (graph_guard, agent_guard) = acquire_locks_ordered!(
///     app_state.graph_registry.write_or_panic("operation name"),
///     app_state.agent_registry.write_or_panic("operation name")
/// );
/// ```
#[macro_export]
macro_rules! acquire_locks_ordered {
    ($first:expr, $second:expr) => {{
        let first_guard = $first;
        let second_guard = $second;
        (first_guard, second_guard)
    }};
}

// ============== ASYNC LOCK SUPPORT ==============

use tokio::sync::{RwLock as AsyncRwLock, RwLockReadGuard as AsyncRwLockReadGuard, RwLockWriteGuard as AsyncRwLockWriteGuard};

/// Extension trait for tokio::sync::RwLock
/// 
/// Provides consistent API with sync locks, including debug assertions
/// for write operations. Note that async locks cannot be poisoned.
pub trait AsyncRwLockExt<T: 'static> {
    /// Read the lock asynchronously
    async fn read_or_panic(&self, context: &str) -> AsyncRwLockReadGuard<'_, T>;
    
    /// Write to the lock asynchronously with contention detection
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T>;
}

impl<T: 'static> AsyncRwLockExt<T> for AsyncRwLock<T> {
    async fn read_or_panic(&self, _context: &str) -> AsyncRwLockReadGuard<'_, T> {
        // Async locks can't be poisoned, just await
        self.read().await
    }
    
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T> {
        // Check for lock contention in debug builds and warn (not panic)
        #[cfg(debug_assertions)]
        {
            if self.try_write().is_err() {
                warn!(
                    "⚠️ Lock contention detected during '{}': another task is holding the lock. \
                    This may indicate a performance issue or the freeze mechanism in tests.",
                    context
                );
            }
        }
        
        self.write().await
    }
}

impl<T: 'static> AsyncRwLockExt<T> for Arc<AsyncRwLock<T>> {
    async fn read_or_panic(&self, context: &str) -> AsyncRwLockReadGuard<'_, T> {
        self.as_ref().read_or_panic(context).await
    }
    
    async fn write_or_panic(&self, context: &str) -> AsyncRwLockWriteGuard<'_, T> {
        self.as_ref().write_or_panic(context).await
    }
}

// ============== LOCK ORDERING ==============

use crate::storage::{GraphRegistry, AgentRegistry};
use std::sync::RwLock as SyncRwLock;
use std::sync::RwLockWriteGuard as SyncRwLockWriteGuard;

/// Acquire both registries for write access in the correct order to prevent deadlocks
/// 
/// This enforces the canonical lock ordering: graph_registry before agent_registry.
/// Always use this function when you need write access to both registries.
/// 
/// # Example
/// ```
/// let (mut graph_registry, mut agent_registry) = lock_registries_for_write(
///     &app_state.graph_registry,
///     &app_state.agent_registry
/// )?;
/// // Perform operations requiring both registries
/// agent_registry.authorize_agent_for_graph(&agent_id, &graph_id, &mut graph_registry)?;
/// ```
pub fn lock_registries_for_write<'a>(
    graph_registry: &'a Arc<SyncRwLock<GraphRegistry>>,
    agent_registry: &'a Arc<SyncRwLock<AgentRegistry>>,
) -> crate::error::Result<(
    SyncRwLockWriteGuard<'a, GraphRegistry>,
    SyncRwLockWriteGuard<'a, AgentRegistry>
)> {
    // Always acquire graph_registry first (canonical ordering)
    let graph_guard = graph_registry.write_or_panic("lock registries for write - graph registry");
    let agent_guard = agent_registry.write_or_panic("lock registries for write - agent registry");
    Ok((graph_guard, agent_guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    

    #[test]
    fn test_read_or_panic_success() {
        let lock = RwLock::new(42);
        let guard = lock.read_or_panic("test read");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_write_or_panic_success() {
        let lock = RwLock::new(42);
        {
            let mut guard = lock.write_or_panic("test write");
            *guard = 100;
        }
        let guard = lock.read_or_panic("test read after write");
        assert_eq!(*guard, 100);
    }


}