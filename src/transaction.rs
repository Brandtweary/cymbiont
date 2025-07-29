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

use crate::transaction_log::{Operation, Transaction, TransactionLog, TransactionLogError, TransactionState};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tracing::{error, info, warn};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Transaction log error: {0}")]
    LogError(#[from] TransactionLogError),
    
    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),
    
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
    
    pub async fn begin_transaction(&self, operation: Operation) -> Result<String> {
        let transaction = Transaction::new(operation);
        let tx_id = transaction.id.clone();
        
        // Add to pending operations if it has a content hash
        if let Some(content_hash) = &transaction.content_hash {
            let mut pending = self.pending_operations.write().await;
            pending.insert(content_hash.clone(), tx_id.clone());
        }
        
        self.log.append_transaction(transaction)?;
        info!("Started transaction: {}", tx_id);
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
        info!("Committed transaction: {}", tx_id);
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
    
    pub async fn wait_for_acknowledgment(&self, tx_id: &str) -> Result<()> {
        self.log.update_transaction_state(tx_id, TransactionState::WaitingForAck)?;
        Ok(())
    }
    
    pub async fn is_content_pending(&self, content_hash: &str) -> bool {
        let pending = self.pending_operations.read().await;
        pending.contains_key(content_hash)
    }
    
    pub async fn find_pending_transaction_by_content(&self, content_hash: &str) -> Option<String> {
        let pending = self.pending_operations.read().await;
        pending.get(content_hash).cloned()
    }
    
    pub async fn handle_acknowledgment(
        &self,
        correlation_id: &str,
        success: bool,
        external_uuid: Option<String>,
    ) -> Result<()> {
        // Find transaction by correlation ID
        // For now, we'll use the transaction ID as correlation ID
        let tx_id = correlation_id;
        
        let operation = Operation::ReceivedAck {
            correlation_id: correlation_id.to_string(),
            success,
            external_uuid: external_uuid.clone(),
        };
        
        // Create a new transaction for the acknowledgment
        let ack_tx_id = self.begin_transaction(operation).await?;
        self.commit_transaction(&ack_tx_id).await?;
        
        // Update the original transaction state
        if success {
            self.commit_transaction(tx_id).await?;
            info!("Transaction {} acknowledged successfully with UUID: {:?}", tx_id, external_uuid);
        } else {
            self.abort_transaction(tx_id, "Acknowledgment failed").await?;
        }
        
        Ok(())
    }
    
    pub async fn recover_pending_transactions(&self) -> Result<Vec<String>> {
        let pending = self.log.list_pending_transactions()?;
        let mut recovered = Vec::new();
        
        for transaction in pending {
            // Re-add to pending operations map
            if let Some(content_hash) = &transaction.content_hash {
                let mut pending_ops = self.pending_operations.write().await;
                pending_ops.insert(content_hash.clone(), transaction.id.clone());
            }
            
            match transaction.state {
                TransactionState::Active => {
                    // These can be retried
                    info!("Recovered active transaction: {}", transaction.id);
                    recovered.push(transaction.id);
                }
                TransactionState::WaitingForAck => {
                    // Check age and potentially timeout
                    let age_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64 - transaction.created_at;
                    
                    if age_ms > 30000 { // 30 second timeout
                        warn!("Timing out old transaction waiting for ack: {}", transaction.id);
                        self.abort_transaction(&transaction.id, "Timeout waiting for acknowledgment").await?;
                    } else {
                        info!("Recovered transaction waiting for ack: {}", transaction.id);
                        recovered.push(transaction.id);
                    }
                }
                _ => {
                    // Committed or Aborted - shouldn't be in pending, but log it
                    warn!("Found {:?} transaction in pending list: {}", transaction.state, transaction.id);
                }
            }
        }
        
        info!("Recovered {} pending transactions", recovered.len());
        Ok(recovered)
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
        
        let operation = Operation::CreateNode {
            node_type: "block".to_string(),
            content: "Test content".to_string(),
            temp_id: Some("temp-123".to_string()),
        };
        
        // Begin transaction
        let tx_id = coordinator.begin_transaction(operation).await.unwrap();
        
        // Check if content is pending
        let content_hash = compute_content_hash("Test content");
        assert!(coordinator.is_content_pending(&content_hash).await);
        
        // Wait for ack
        coordinator.wait_for_acknowledgment(&tx_id).await.unwrap();
        
        // Handle acknowledgment
        coordinator.handle_acknowledgment(&tx_id, true, Some("uuid-456".to_string())).await.unwrap();
        
        // Content should no longer be pending
        assert!(!coordinator.is_content_pending(&content_hash).await);
    }
    
    #[tokio::test]
    async fn test_abort_transaction() {
        let (coordinator, _temp_dir) = create_test_coordinator().await;
        
        let operation = Operation::UpdateNode {
            node_id: "node-123".to_string(),
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
                let operation = Operation::CreateNode {
                    node_type: "block".to_string(),
                    content: format!("Content {}", i),
                    temp_id: None,
                };
                coordinator.begin_transaction(operation).await.unwrap();
            }
        }
        
        // Create new coordinator and recover
        let coordinator = TransactionCoordinator::new(log);
        let recovered = coordinator.recover_pending_transactions().await.unwrap();
        assert_eq!(recovered.len(), 3);
    }
    
    fn compute_content_hash(content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}