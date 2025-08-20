/**
 * Transaction Coordinator Module
 * 
 * Manages transaction lifecycle and state transitions for the WAL system.
 * Provides high-level operations for beginning, committing, and aborting transactions.
 * Includes helper functions to reduce AppState verbosity.
 * 
 * ## Transaction-Aware Persistence
 * 
 * The transaction system works alongside graph persistence:
 * - Graph state is saved to knowledge_graph.json by GraphManager
 * - Transaction state is saved to sled database by TransactionLog
 * - On startup, graph is loaded first, then pending transactions are recovered
 * - This ensures consistency: if a transaction was in-flight during shutdown,
 *   it will be retried on startup
 * 
 * The separation of concerns means:
 * - Graph can be saved frequently without worrying about transactions
 * - Transactions persist independently with ACID guarantees via sled
 * - Recovery is automatic and doesn't require graph modification
 * 
 * ## Helper Functions for AppState Verbosity Reduction
 * 
 * This module provides extracted helper functions to simplify AppState:
 * - `run_single_graph_recovery_helper()` - Core recovery logic without circular dependencies
 * - `save_graph_after_recovery_helper()` - Graph saving with error logging that never fails
 * 
 * These helpers maintain the original AppState behavior while reducing code duplication.
 */

use crate::storage::transaction_log::{Operation, Transaction, TransactionLog, TransactionLogError, TransactionState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, Notify};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::{warn, error, info};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Transaction log error: {0}")]
    LogError(#[from] TransactionLogError),
    
    #[error("Duplicate content already being processed: {0}")]
    DuplicateContent(String),
    
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    
    #[error("Shutdown in progress - no new transactions allowed")]
    ShutdownInProgress,
}

pub type Result<T> = std::result::Result<T, TransactionError>;

pub struct TransactionCoordinator {
    log: Arc<TransactionLog>,
    pending_operations: Arc<RwLock<HashMap<String, String>>>, // content_hash -> transaction_id
    active_transactions: Arc<RwLock<HashSet<String>>>, // All active transaction IDs
    shutdown_requested: Arc<AtomicBool>,
    active_count_notify: Arc<Notify>,
}

impl TransactionCoordinator {
    pub fn new(log: Arc<TransactionLog>) -> Self {
        Self {
            log,
            pending_operations: Arc::new(RwLock::new(HashMap::new())),
            active_transactions: Arc::new(RwLock::new(HashSet::new())),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            active_count_notify: Arc::new(Notify::new()),
        }
    }
    
    /// Close the underlying transaction log
    pub async fn close(&self) -> Result<()> {
        self.log.close().await.map_err(|e| e.into())
    }
    
    pub async fn begin_transaction(&self, operation: Operation) -> Result<String> {
        let transaction = Transaction::new(operation);
        let tx_id = transaction.id.clone();
        
        // Add to pending operations if it has a content hash
        if let Some(content_hash) = &transaction.content_hash {
            let mut pending = self.pending_operations.write().await;
            pending.insert(content_hash.clone(), tx_id.clone());
        }
        
        // Add to active transactions set
        {
            let mut active = self.active_transactions.write().await;
            active.insert(tx_id.clone());
        }
        
        self.log.append_transaction(transaction)?;
        Ok(tx_id)
    }
    
    pub async fn commit_transaction(&self, tx_id: &str) -> Result<()> {
        let transaction = self.log.get_transaction(tx_id)?;
        
        // Remove from pending operations
        if let Some(content_hash) = &transaction.content_hash {
            let mut pending = self.pending_operations.write().await;
            pending.remove(content_hash);
        }
        
        // Remove from active transactions and check if shutdown should complete
        {
            let mut active = self.active_transactions.write().await;
            active.remove(tx_id);
            
            // If shutdown requested and no more active transactions, notify
            if self.shutdown_requested.load(Ordering::Acquire) && active.is_empty() {
                self.active_count_notify.notify_waiters();
            }
        }
        
        self.log.update_transaction_state(tx_id, TransactionState::Committed)?;
        Ok(())
    }
    
    pub async fn abort_transaction(&self, tx_id: &str, reason: &str) -> Result<()> {
        let mut transaction = self.log.get_transaction(tx_id)?;
        
        // Remove from pending operations
        if let Some(content_hash) = &transaction.content_hash {
            let mut pending = self.pending_operations.write().await;
            pending.remove(content_hash);
        }
        
        // Remove from active transactions and check if shutdown should complete
        {
            let mut active = self.active_transactions.write().await;
            active.remove(tx_id);
            
            // If shutdown requested and no more active transactions, notify
            if self.shutdown_requested.load(Ordering::Acquire) && active.is_empty() {
                self.active_count_notify.notify_waiters();
            }
        }
        
        // Update transaction with error message
        transaction.error_message = Some(reason.to_string());
        
        // TODO: We should add a method to update the full transaction with error message
        // For now, just update the state
        self.log.update_transaction_state(tx_id, TransactionState::Aborted)?;
        warn!("Aborted transaction {}: {}", tx_id, reason);
        Ok(())
    }
    
    
    pub async fn is_content_pending(&self, content_hash: &str) -> bool {
        let pending = self.pending_operations.read().await;
        pending.contains_key(content_hash)
    }
    
    /// Create a transaction with deduplication check
    pub async fn create_transaction(&self, operation: Operation) -> Result<String> {
        // Check if shutdown is in progress
        if self.shutdown_requested.load(Ordering::Acquire) {
            warn!("Transaction rejected - graceful shutdown in progress");
            return Err(TransactionError::ShutdownInProgress);
        }
        
        // Check for duplicate content if applicable
        if let Operation::CreateBlock { content, .. } | Operation::UpdateBlock { content, .. } = &operation {
            let hash = compute_content_hash(content);
            if self.is_content_pending(&hash).await {
                warn!("Duplicate content detected in pending transactions - potential race condition. Content hash: {}", hash);
                return Err(TransactionError::DuplicateContent(hash));
            }
        }
        
        // Begin transaction
        self.begin_transaction(operation).await
    }
    
    /// Complete a transaction based on execution result
    pub async fn complete_transaction<T>(
        &self,
        tx_id: &str,
        result: std::result::Result<T, String>,
    ) -> Result<T> {
        match result {
            Ok(value) => {
                self.commit_transaction(tx_id).await?;
                Ok(value)
            }
            Err(error_msg) => {
                self.abort_transaction(tx_id, &error_msg).await?;
                Err(TransactionError::OperationFailed(error_msg))
            }
        }
    }
    
    pub async fn recover_pending_transactions(&self) -> Result<Vec<Transaction>> {
        let pending = self.log.list_pending_transactions()?;
        let mut recoverable = Vec::new();
        
        if !pending.is_empty() {
            warn!("Found {} pending transactions from previous session - initiating recovery", 
                  pending.len());
        }
        
        for transaction in pending {
            // Don't re-add to pending operations map - these transactions are already recorded
            // and will be replayed. Adding them would cause duplicate content detection
            // when replay_transaction tries to create a new transaction.
            
            match transaction.state {
                TransactionState::Active => {
                    // These can be retried
                    recoverable.push(transaction);
                }
                _ => {
                    // Committed or Aborted - shouldn't be in pending, but log it
                    error!("Found {:?} transaction in pending list: {} - this indicates a bug", 
                           transaction.state, transaction.id);
                }
            }
        }
        
        Ok(recoverable)
    }
    
    /// Initiate graceful shutdown - no new transactions will be accepted
    /// Returns the count of currently active transactions
    pub async fn initiate_shutdown(&self) -> usize {
        self.shutdown_requested.store(true, Ordering::Release);
        let active = self.active_transactions.read().await;
        let count = active.len();
        if count > 0 {
            info!("Shutdown initiated for TransactionCoordinator with {} active transactions", count);
        }
        count
    }
    
    /// Wait for all active transactions to complete or timeout
    /// Returns true if all completed, false if timeout
    pub async fn wait_for_completion(&self, timeout: Duration) -> bool {
        let active_count = {
            let active = self.active_transactions.read().await;
            active.len()
        };
        
        if active_count == 0 {
            return true;
        }
        
        // Wait for notification or timeout
        match tokio::time::timeout(timeout, self.active_count_notify.notified()).await {
            Ok(_) => {
                info!("All transactions completed");
                true
            }
            Err(_) => {
                let remaining = self.active_transactions.read().await.len();
                warn!("Timeout waiting for transactions - {} still active", remaining);
                false
            }
        }
    }
    
    /// Force shutdown - immediately flush transaction log
    pub async fn force_shutdown(&self) -> Result<()> {
        error!("Force shutdown requested - flushing transaction log");
        self.log.close().await.map_err(|e| e.into())
    }
}

/// Helper function for single graph recovery to reduce AppState verbosity
/// 
/// This extracts the core recovery logic without creating circular dependencies.
/// Takes the coordinator, operation executor, and graph ID as parameters.
pub async fn run_single_graph_recovery_helper<E>(
    coordinator: &TransactionCoordinator,
    operation_executor: &E,
    graph_id: &uuid::Uuid,
) -> std::result::Result<usize, Box<dyn std::error::Error + Send + Sync>>
where
    E: crate::storage::OperationExecutor,
{
    use crate::storage::OperationExecutor;
    
    let pending_transactions = coordinator.recover_pending_transactions().await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(format!("Failed to recover transactions: {}", e)))?;
    
    let count = pending_transactions.len();
    if count > 0 {
        info!("🔄 Replaying {} pending transactions for graph {}", count, graph_id);
        
        // Replay each transaction with proper state updates
        for transaction in pending_transactions {
            let tx_id = transaction.id.clone();
            let operation = transaction.operation.clone();
            
            // Execute the operation using the OperationExecutor trait
            let result = OperationExecutor::execute_operation(operation_executor, graph_id, operation).await;
            
            // Update transaction state based on result
            match result {
                Ok(()) => {
                    coordinator.commit_transaction(&tx_id).await
                        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))?;
                }
                Err(e) => {
                    coordinator.abort_transaction(&tx_id, &e).await
                        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))?;
                    error!("❌ Failed to replay transaction {}: {}", tx_id, e);
                }
            }
        }
    }
    
    Ok(count)
}

/// Helper function to save graph after recovery
/// 
/// Extracted from AppState to reduce verbosity. Logs errors but never fails
/// to match original AppState behavior.
pub async fn save_graph_after_recovery_helper(
    graph_manager: &mut crate::graph_manager::GraphManager,
    graph_id: &uuid::Uuid,
) {
    match graph_manager.save_graph() {
        Ok(_) => info!("💾 Saved graph {} after recovery", graph_id),
        Err(e) => error!("Failed to save graph {} after recovery: {}", graph_id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    async fn create_test_coordinator() -> (TransactionCoordinator, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let log = Arc::new(TransactionLog::new(temp_dir.path()).unwrap());
        let coordinator = TransactionCoordinator::new(log);
        (coordinator, temp_dir)
    }
    
    #[tokio::test]
    async fn test_transaction_lifecycle() {
        let (coordinator, _temp_dir) = create_test_coordinator().await;
        
        let operation = Operation::CreateBlock {
            agent_id: uuid::Uuid::new_v4(),
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("test-page".to_string()),
            properties: None,
        };
        
        // Begin transaction
        let tx_id = coordinator.begin_transaction(operation).await.unwrap();
        
        // Check if content is pending
        let content_hash = compute_content_hash("Test content");
        assert!(coordinator.is_content_pending(&content_hash).await);
        
        // Commit the transaction
        coordinator.commit_transaction(&tx_id).await.unwrap();
        
        // Content should no longer be pending
        assert!(!coordinator.is_content_pending(&content_hash).await);
    }
    
    #[tokio::test]
    async fn test_abort_transaction() {
        let (coordinator, _temp_dir) = create_test_coordinator().await;
        
        let operation = Operation::UpdateBlock {
            agent_id: uuid::Uuid::new_v4(),
            block_id: "block-123".to_string(),
            content: "Updated content".to_string(),
        };
        
        let tx_id = coordinator.begin_transaction(operation).await.unwrap();
        coordinator.abort_transaction(&tx_id, "Test abort").await.unwrap();
        
        let content_hash = compute_content_hash("Updated content");
        assert!(!coordinator.is_content_pending(&content_hash).await);
    }
    
    #[tokio::test]
    async fn test_recovery() {
        let temp_dir = TempDir::new().unwrap();
        let log = Arc::new(TransactionLog::new(temp_dir.path()).unwrap());
        
        // Create some transactions
        {
            let coordinator = TransactionCoordinator::new(log.clone());
            
            for i in 0..3 {
                let operation = Operation::CreateBlock {
                    agent_id: uuid::Uuid::new_v4(),
                    content: format!("Content {}", i),
                    parent_id: None,
                    page_name: Some("test-page".to_string()),
                    properties: None,
                };
                coordinator.begin_transaction(operation).await.unwrap();
            }
        }
        
        // Create new coordinator and recover
        let coordinator = TransactionCoordinator::new(log);
        let recovered = coordinator.recover_pending_transactions().await.unwrap();
        assert_eq!(recovered.len(), 3);
        
        // Verify they are all Active transactions
        for tx in &recovered {
            assert_eq!(tx.state, TransactionState::Active);
        }
    }
}

fn compute_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

