//! Lock handling utilities for simplified RwLock error management
//! 
//! This module provides extension traits for RwLock types that implement
//! Cymbiont's panic-on-poison strategy. Since RwLock poisoning indicates
//! a thread panicked while holding the lock, data integrity cannot be
//! guaranteed. For a data-critical application like Cymbiont, we prefer
//! to panic rather than attempt recovery with potentially corrupted state.

// TODO: This module was created but not integrated - needs proper integration

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::error::{CymbiontError, LockError};

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

    /// Read the lock or return an error
    /// 
    /// This method provides error handling for cases where panic-on-poison
    /// is not appropriate (e.g., optional features, graceful degradation).
    /// Most Cymbiont code should use `read_or_panic` instead.
    fn read_or_error(&self, context: &str) -> Result<RwLockReadGuard<T>, CymbiontError>;

    /// Write to the lock or return an error
    /// 
    /// This method provides error handling for cases where panic-on-poison
    /// is not appropriate (e.g., optional features, graceful degradation).
    /// Most Cymbiont code should use `write_or_panic` instead.
    fn write_or_error(&self, context: &str) -> Result<RwLockWriteGuard<T>, CymbiontError>;

    /// Check if lock can be read without blocking (for contention detection)
    /// 
    /// This is primarily used during development to detect lock contention
    /// issues. In production, contention detection may be disabled.
    fn can_read_without_blocking(&self) -> bool;

    /// Check if lock can be written without blocking (for contention detection)
    /// 
    /// This is primarily used during development to detect lock contention
    /// issues. In production, contention detection may be disabled.
    fn can_write_without_blocking(&self) -> bool;
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

    fn read_or_error(&self, context: &str) -> Result<RwLockReadGuard<T>, CymbiontError> {
        self.read().map_err(|poison_error| {
            CymbiontError::Lock(LockError::poisoned(format!(
                "Lock poisoned during read operation: {}. Error: {}",
                context, poison_error
            )))
        })
    }

    fn write_or_error(&self, context: &str) -> Result<RwLockWriteGuard<T>, CymbiontError> {
        self.write().map_err(|poison_error| {
            CymbiontError::Lock(LockError::poisoned(format!(
                "Lock poisoned during write operation: {}. Error: {}",
                context, poison_error
            )))
        })
    }

    fn can_read_without_blocking(&self) -> bool {
        self.try_read().is_ok()
    }

    fn can_write_without_blocking(&self) -> bool {
        self.try_write().is_ok()
    }
}

/// Extension trait for Arc<RwLock<T>> which is commonly used in Cymbiont
/// 
/// This provides the same panic-on-poison behavior for Arc-wrapped locks.
pub trait ArcRwLockExt<T> {
    /// Read the lock or panic on poison
    fn read_or_panic(&self, context: &str) -> RwLockReadGuard<T>;

    /// Write to the lock or panic on poison
    fn write_or_panic(&self, context: &str) -> RwLockWriteGuard<T>;

    /// Read the lock or return an error
    fn read_or_error(&self, context: &str) -> Result<RwLockReadGuard<T>, CymbiontError>;

    /// Write to the lock or return an error
    fn write_or_error(&self, context: &str) -> Result<RwLockWriteGuard<T>, CymbiontError>;
}

impl<T> ArcRwLockExt<T> for std::sync::Arc<RwLock<T>> {
    fn read_or_panic(&self, context: &str) -> RwLockReadGuard<T> {
        self.as_ref().read_or_panic(context)
    }

    fn write_or_panic(&self, context: &str) -> RwLockWriteGuard<T> {
        self.as_ref().write_or_panic(context)
    }

    fn read_or_error(&self, context: &str) -> Result<RwLockReadGuard<T>, CymbiontError> {
        self.as_ref().read_or_error(context)
    }

    fn write_or_error(&self, context: &str) -> Result<RwLockWriteGuard<T>, CymbiontError> {
        self.as_ref().write_or_error(context)
    }
}

/// Utility function to check for lock contention (development mode)
/// 
/// This function can be used to detect potential deadlock scenarios
/// during development. It should not be used in production code paths.
pub fn check_lock_contention<T>(
    locks: &[(&str, &RwLock<T>)],
    operation_name: &str,
) -> Result<(), CymbiontError> {
    for (lock_name, lock) in locks {
        if !lock.can_read_without_blocking() && !lock.can_write_without_blocking() {
            return Err(CymbiontError::Lock(LockError::contention(format!(
                "Lock contention detected for '{}' during '{}' operation",
                lock_name, operation_name
            ))));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    
    

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

    #[test]
    fn test_read_or_error_success() {
        let lock = RwLock::new(42);
        let guard = lock.read_or_error("test read").unwrap();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_write_or_error_success() {
        let lock = RwLock::new(42);
        {
            let mut guard = lock.write_or_error("test write").unwrap();
            *guard = 100;
        }
        let guard = lock.read_or_error("test read after write").unwrap();
        assert_eq!(*guard, 100);
    }

    #[test]
    fn test_arc_rwlock_ext() {
        let lock = Arc::new(RwLock::new(42));
        let guard = lock.read_or_panic("test arc read");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_can_read_without_blocking() {
        let lock = RwLock::new(42);
        assert!(lock.can_read_without_blocking());
        
        let _guard = lock.read().unwrap();
        assert!(lock.can_read_without_blocking()); // Multiple readers allowed
    }

    #[test]
    fn test_can_write_without_blocking() {
        let lock = RwLock::new(42);
        assert!(lock.can_write_without_blocking());
        
        let _guard = lock.write().unwrap();
        assert!(!lock.can_write_without_blocking()); // Write lock is exclusive
    }

    #[test]
    fn test_check_lock_contention_no_contention() {
        let lock1 = RwLock::new(1);
        let lock2 = RwLock::new(2);
        
        let locks = vec![("lock1", &lock1), ("lock2", &lock2)];
        assert!(check_lock_contention(&locks, "test operation").is_ok());
    }
}