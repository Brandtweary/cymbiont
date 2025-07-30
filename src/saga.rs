use crate::transaction::{TransactionCoordinator, TransactionError};
use crate::transaction_log::Operation;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum SagaError {
    #[error("Transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    
    #[error("Saga not found: {0}")]
    SagaNotFound(String),
    
    #[error("Invalid saga state: {0}")]
    InvalidState(String),
    
    #[error("Compensation failed: {0}")]
    CompensationFailed(String),
}

pub type Result<T> = std::result::Result<T, SagaError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SagaState {
    InProgress,
    Completed,
    Failed,
    Compensating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Saga {
    pub id: String,
    pub transactions: Vec<String>,
    pub state: SagaState,
    pub created_at: u64,
    pub updated_at: u64,
    pub description: Option<String>,
}

impl Saga {
    pub fn new(description: Option<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        Self {
            id: Uuid::new_v4().to_string(),
            transactions: Vec::new(),
            state: SagaState::InProgress,
            created_at: now,
            updated_at: now,
            description,
        }
    }
}

pub struct SagaCoordinator {
    coordinator: Arc<TransactionCoordinator>,
    sagas: Arc<RwLock<HashMap<String, Saga>>>,
}

impl SagaCoordinator {
    pub fn new(coordinator: Arc<TransactionCoordinator>) -> Self {
        Self {
            coordinator,
            sagas: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn begin_saga(&self, description: Option<String>) -> Result<String> {
        let saga = Saga::new(description.clone());
        let saga_id = saga.id.clone();
        
        let mut sagas = self.sagas.write().await;
        sagas.insert(saga_id.clone(), saga);
        
        info!("Started saga: {} - {:?}", saga_id, description);
        Ok(saga_id)
    }
    
    pub async fn add_transaction_to_saga(&self, saga_id: &str, operation: Operation) -> Result<String> {
        let mut sagas = self.sagas.write().await;
        let saga = sagas.get_mut(saga_id)
            .ok_or_else(|| SagaError::SagaNotFound(saga_id.to_string()))?;
            
        if saga.state != SagaState::InProgress {
            return Err(SagaError::InvalidState(
                format!("Cannot add transaction to saga in state: {:?}", saga.state)
            ));
        }
        
        // Create transaction with saga association
        let tx_id = self.coordinator.begin_transaction(operation).await?;
        saga.transactions.push(tx_id.clone());
        saga.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        debug!("Added transaction {} to saga {}", tx_id, saga_id);
        Ok(tx_id)
    }
    
    pub async fn complete_saga(&self, saga_id: &str) -> Result<()> {
        let mut sagas = self.sagas.write().await;
        let saga = sagas.get_mut(saga_id)
            .ok_or_else(|| SagaError::SagaNotFound(saga_id.to_string()))?;
            
        if saga.state != SagaState::InProgress {
            return Err(SagaError::InvalidState(
                format!("Cannot complete saga in state: {:?}", saga.state)
            ));
        }
        
        saga.state = SagaState::Completed;
        saga.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        info!("Completed saga: {}", saga_id);
        Ok(())
    }
    
    pub async fn fail_saga(&self, saga_id: &str, compensate: bool) -> Result<()> {
        let mut sagas = self.sagas.write().await;
        let saga = sagas.get_mut(saga_id)
            .ok_or_else(|| SagaError::SagaNotFound(saga_id.to_string()))?;
            
        if compensate && saga.state == SagaState::InProgress {
            saga.state = SagaState::Compensating;
            let transactions_to_compensate = saga.transactions.clone();
            
            // Release lock before compensation
            drop(sagas);
            
            // Compensate transactions in reverse order
            for tx_id in transactions_to_compensate.iter().rev() {
                match self.compensate_transaction(tx_id).await {
                    Ok(_) => debug!("Compensated transaction: {}", tx_id),
                    Err(e) => {
                        error!("Failed to compensate transaction {}: {}", tx_id, e);
                        return Err(SagaError::CompensationFailed(e.to_string()));
                    }
                }
            }
            
            // Re-acquire lock to update state
            let mut sagas = self.sagas.write().await;
            if let Some(saga) = sagas.get_mut(saga_id) {
                saga.state = SagaState::Failed;
            }
        } else {
            saga.state = SagaState::Failed;
        }
        
        warn!("Failed saga: {} (compensated: {})", saga_id, compensate);
        Ok(())
    }
    
    async fn compensate_transaction(&self, tx_id: &str) -> Result<()> {
        // For now, just abort the transaction
        // In a real system, this would execute compensating actions
        self.coordinator.abort_transaction(tx_id, "Saga compensation").await?;
        Ok(())
    }
    
    // TODO: Remove allow(dead_code) once saga recovery is implemented
    #[allow(dead_code)]
    pub async fn get_saga(&self, saga_id: &str) -> Result<Saga> {
        let sagas = self.sagas.read().await;
        sagas.get(saga_id)
            .cloned()
            .ok_or_else(|| SagaError::SagaNotFound(saga_id.to_string()))
    }
}

// Example saga implementations for common workflows

pub struct WorkflowSagas {
    saga_coordinator: Arc<SagaCoordinator>,
    transaction_coordinator: Arc<TransactionCoordinator>,
}

impl WorkflowSagas {
    pub fn new(
        saga_coordinator: Arc<SagaCoordinator>,
        transaction_coordinator: Arc<TransactionCoordinator>,
    ) -> Self {
        Self {
            saga_coordinator,
            transaction_coordinator,
        }
    }
    
    pub async fn create_block_workflow(
        &self,
        content: String,
        node_type: String,
    ) -> Result<(String, String)> { // Returns (saga_id, temp_id)
        let saga_id = self.saga_coordinator.begin_saga(
            Some(format!("Create {} block", node_type))
        ).await?;
        
        let temp_id = format!("temp-{}", Uuid::new_v4());
        
        // Step 1: Create node in graph
        let create_op = Operation::CreateNode {
            node_type: node_type.clone(),
            content: content.clone(),
            temp_id: Some(temp_id.clone()),
        };
        let tx1_id = self.saga_coordinator.add_transaction_to_saga(&saga_id, create_op).await?;
        
        // Step 2: Send via WebSocket
        let send_op = Operation::SendWebSocket {
            command: format!("create_{}", node_type),
            correlation_id: tx1_id.clone(),
        };
        let tx2_id = self.saga_coordinator.add_transaction_to_saga(&saga_id, send_op).await?;
        
        // Mark transaction as waiting for acknowledgment
        self.transaction_coordinator.wait_for_acknowledgment(&tx2_id).await?;
        
        info!("Created block workflow saga: {} with temp_id: {}", saga_id, temp_id);
        Ok((saga_id, temp_id))
    }
    
    pub async fn handle_block_acknowledgment(
        &self,
        saga_id: &str,
        temp_id: &str,
        success: bool,
        external_uuid: Option<String>,
    ) -> Result<()> {
        if success && external_uuid.is_some() {
            // Step 3: Update mapping with real UUID
            let update_op = Operation::UpdateNode {
                node_id: temp_id.to_string(),
                content: format!("{{uuid: {}}}", external_uuid.as_ref().unwrap()),
            };
            self.saga_coordinator.add_transaction_to_saga(saga_id, update_op).await?;
            
            // Complete the saga
            self.saga_coordinator.complete_saga(saga_id).await?;
        } else {
            // Fail and compensate
            self.saga_coordinator.fail_saga(saga_id, true).await?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction_log::TransactionLog;
    use tempfile::TempDir;
    
    async fn create_test_coordinators() -> (Arc<SagaCoordinator>, Arc<TransactionCoordinator>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let log = Arc::new(TransactionLog::new(temp_dir.path()).unwrap());
        let tx_coordinator = Arc::new(TransactionCoordinator::new(log));
        let saga_coordinator = Arc::new(SagaCoordinator::new(tx_coordinator.clone()));
        (saga_coordinator, tx_coordinator, temp_dir)
    }
    
    #[tokio::test]
    async fn test_saga_lifecycle() {
        let (saga_coordinator, _, _temp_dir) = create_test_coordinators().await;
        
        // Begin saga
        let saga_id = saga_coordinator.begin_saga(Some("Test saga".to_string())).await.unwrap();
        
        // Add transactions
        let op1 = Operation::CreateNode {
            node_type: "block".to_string(),
            content: "Content 1".to_string(),
            temp_id: None,
        };
        let _tx1 = saga_coordinator.add_transaction_to_saga(&saga_id, op1).await.unwrap();
        
        let op2 = Operation::UpdateNode {
            node_id: "node-123".to_string(),
            content: "Updated".to_string(),
        };
        let _tx2 = saga_coordinator.add_transaction_to_saga(&saga_id, op2).await.unwrap();
        
        // Complete saga
        saga_coordinator.complete_saga(&saga_id).await.unwrap();
        
        let saga = saga_coordinator.get_saga(&saga_id).await.unwrap();
        assert_eq!(saga.state, SagaState::Completed);
        assert_eq!(saga.transactions.len(), 2);
    }
    
    #[tokio::test]
    async fn test_saga_compensation() {
        let (saga_coordinator, _, _temp_dir) = create_test_coordinators().await;
        
        let saga_id = saga_coordinator.begin_saga(Some("Test compensation".to_string())).await.unwrap();
        
        // Add some transactions
        for i in 0..3 {
            let op = Operation::CreateNode {
                node_type: "block".to_string(),
                content: format!("Content {}", i),
                temp_id: None,
            };
            saga_coordinator.add_transaction_to_saga(&saga_id, op).await.unwrap();
        }
        
        // Fail with compensation
        saga_coordinator.fail_saga(&saga_id, true).await.unwrap();
        
        let saga = saga_coordinator.get_saga(&saga_id).await.unwrap();
        assert_eq!(saga.state, SagaState::Failed);
    }
    
    #[tokio::test]
    async fn test_block_workflow() {
        let (saga_coordinator, tx_coordinator, _temp_dir) = create_test_coordinators().await;
        let workflows = WorkflowSagas::new(saga_coordinator.clone(), tx_coordinator);
        
        let (saga_id, temp_id) = workflows.create_block_workflow(
            "Test block content".to_string(),
            "block".to_string(),
        ).await.unwrap();
        
        // Simulate successful acknowledgment
        workflows.handle_block_acknowledgment(
            &saga_id,
            &temp_id,
            true,
            Some("uuid-12345".to_string()),
        ).await.unwrap();
        
        let saga = saga_coordinator.get_saga(&saga_id).await.unwrap();
        assert_eq!(saga.state, SagaState::Completed);
    }
}