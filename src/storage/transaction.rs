/**
 * Transaction Coordinator Module
 * 
 * Manages transaction lifecycle and state transitions for the WAL system.
 * Provides high-level operations for beginning, committing, and aborting transactions.
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
 */

use crate::storage::transaction_log::{Operation, Transaction, TransactionLog, TransactionLogError, TransactionState};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tracing::{warn, error};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Transaction log error: {0}")]
    LogError(#[from] TransactionLogError),
    
    #[error("Duplicate content already being processed: {0}")]
    DuplicateContent(String),
    
    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub type Result<T> = std::result::Result<T, TransactionError>;

pub struct TransactionCoordinator {
    log: Arc<TransactionLog>,
    pending_operations: Arc<RwLock<HashMap<String, String>>>, // content_hash -> transaction_id
}

impl TransactionCoordinator {
    pub fn new(log: Arc<TransactionLog>) -> Self {
        Self {
            log,
            pending_operations: Arc::new(RwLock::new(HashMap::new())),
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

