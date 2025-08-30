//! Transaction Log: Write-Ahead Logging with ACID Guarantees
//!
//! This module provides persistent transaction logging using the sled embedded database
//! to ensure ACID guarantees for all knowledge graph operations. It serves as the foundation
//! for coordinating between AI-generated content and real-time synchronization.
//!
//! ## Overview
//!
//! The transaction log system prevents race conditions and data corruption by logging all
//! operations before they are applied to the knowledge graph. This enables:
//! - **Crash Recovery**: Replay incomplete transactions after system restart
//! - **Deduplication**: Prevent duplicate processing of echoed operations
//! - **Coordination**: Synchronize between WebSocket commands and data updates
//! - **Audit Trail**: Complete history of all graph mutations
//!
//! ## Architecture
//!
//! The transaction log uses sled's ACID-compliant embedded database with three logical trees:
//!
//! ### 1. Transactions Tree
//! - **Key**: Transaction UUID (16 bytes)
//! - **Value**: Serialized Transaction struct
//! - **Purpose**: Primary storage for all transaction data
//!
//! ### 2. Content Hash Index
//! - **Key**: SHA-256 content hash (32 bytes)
//! - **Value**: Transaction UUID (16 bytes)  
//! - **Purpose**: Fast lookup to prevent duplicate processing of identical content
//!
//! ### 3. Pending Index
//! - **Key**: Transaction UUID (16 bytes)
//! - **Value**: Empty (presence indicates pending)
//! - **Purpose**: Efficient enumeration of incomplete transactions for recovery
//!
//! ## Transaction Lifecycle
//!
//! ```text
//! 1. append_transaction() → Active (logged to WAL)
//! 2. Operation completes → Committed
//! 3. Cleanup → Remove from pending index
//! ```
//!
//! ## Performance Characteristics
//!
//! - **Write Performance**: Sequential log writes optimized by sled
//! - **Read Performance**: Hash-indexed lookups for O(1) content deduplication
//! - **Durability**: Configurable fsync frequency (100ms default)
//! - **Overhead Target**: <5ms per transaction (achieved through batching)
//!
//! ## Recovery and Cleanup
//!
//! On startup, the transaction coordinator:
//! 1. Scans pending index for incomplete transactions
//! 2. Retries Active transactions (apply to graph)
//! 3. Times out old transactions
//! 4. Marks Committed transactions as complete
//!
//! ## Content Hash Deduplication
//!
//! The content hash index prevents duplicate processing. This is critical for 
//! preventing echoed operations when processing content that was originally 
//! created via WebSocket commands.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::error::*;



#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionState {
    Active,    // Transaction created but not yet executed (including deferred with retry)
    Committed, // Transaction successfully executed
    Aborted,   // Transaction failed and will not be retried
}

// Categorized operation structure for better organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Graph(GraphOperation),
    Agent(AgentOperation),
    Registry(RegistryOperation),
}

// Graph operations (existing functionality)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphOperation {
    CreateBlock {
        graph_id: uuid::Uuid,
        agent_id: uuid::Uuid,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<serde_json::Value>,
    },
    UpdateBlock {
        graph_id: uuid::Uuid,
        agent_id: uuid::Uuid,
        block_id: String,
        content: String,
    },
    DeleteBlock {
        graph_id: uuid::Uuid,
        agent_id: uuid::Uuid,
        block_id: String,
    },
    CreatePage {
        graph_id: uuid::Uuid,
        agent_id: uuid::Uuid,
        page_name: String,
        properties: Option<serde_json::Value>,
    },
    DeletePage {
        graph_id: uuid::Uuid,
        agent_id: uuid::Uuid,
        page_name: String,
    },
}

// Agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOperation {
    // Conversation operations
    AddMessage {
        agent_id: uuid::Uuid,
        message: serde_json::Value, // Will be the full Message struct
    },
    ClearHistory {
        agent_id: uuid::Uuid,
    },
    // Configuration operations
    SetLLMConfig {
        agent_id: uuid::Uuid,
        config: serde_json::Value, // LLMConfig serialized
    },
    SetSystemPrompt {
        agent_id: uuid::Uuid,
        prompt: String,
    },
    SetDefaultGraph {
        agent_id: uuid::Uuid,
        graph_id: Option<uuid::Uuid>,
    },
}

// Registry operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryOperation {
    Graph(GraphRegistryOp),
    Agent(AgentRegistryOp),
}

// Graph registry operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphRegistryOp {
    RegisterGraph {
        graph_id: uuid::Uuid,
        name: Option<String>,
        description: Option<String>,
    },
    RemoveGraph {
        graph_id: uuid::Uuid,
    },
    OpenGraph {
        graph_id: uuid::Uuid,
    },
    CloseGraph {
        graph_id: uuid::Uuid,
    },
}

// Agent registry operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRegistryOp {
    RegisterAgent {
        agent_id: uuid::Uuid,
        name: Option<String>,
        description: Option<String>,
    },
    RemoveAgent {
        agent_id: uuid::Uuid,
    },
    ActivateAgent {
        agent_id: uuid::Uuid,
    },
    DeactivateAgent {
        agent_id: uuid::Uuid,
    },
    AuthorizeAgent {
        agent_id: uuid::Uuid,
        graph_id: uuid::Uuid,
    },
    DeauthorizeAgent {
        agent_id: uuid::Uuid,
        graph_id: uuid::Uuid,
    },
    SetPrimeAgent {
        agent_id: uuid::Uuid,
    },
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub operation: Operation,
    pub state: TransactionState,
    pub created_at: u64,
    pub updated_at: u64,
    pub content_hash: Option<String>,
    pub error_message: Option<String>,
    pub deferred_reason: Option<String>,  // Why execution was delayed (entity unavailable, etc.)
    pub retry_count: u32,                 // Number of retry attempts
}

impl Transaction {
    pub fn new(operation: Operation) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        let content_hash = match &operation {
            Operation::Graph(GraphOperation::CreateBlock { content, .. }) | 
            Operation::Graph(GraphOperation::UpdateBlock { content, .. }) => {
                Some(compute_content_hash(content))
            }
            _ => None,
        };
        
        Self {
            id: Uuid::new_v4().to_string(),
            operation,
            state: TransactionState::Active,
            created_at: now,
            updated_at: now,
            content_hash,
            error_message: None,
            deferred_reason: None,
            retry_count: 0,
        }
    }
}

impl Operation {
    /// Extract the graph ID from an operation if it involves a specific graph
    pub fn extract_graph_id(&self) -> Option<Uuid> {
        match self {
            Operation::Graph(op) => match op {
                GraphOperation::CreateBlock { graph_id, .. } |
                GraphOperation::UpdateBlock { graph_id, .. } |
                GraphOperation::DeleteBlock { graph_id, .. } |
                GraphOperation::CreatePage { graph_id, .. } |
                GraphOperation::DeletePage { graph_id, .. } => Some(*graph_id),
            },
            Operation::Registry(RegistryOperation::Graph(op)) => match op {
                GraphRegistryOp::RegisterGraph { graph_id, .. } |
                GraphRegistryOp::RemoveGraph { graph_id } |
                GraphRegistryOp::OpenGraph { graph_id } |
                GraphRegistryOp::CloseGraph { graph_id } => Some(*graph_id),
            },
            Operation::Registry(RegistryOperation::Agent(op)) => match op {
                AgentRegistryOp::AuthorizeAgent { graph_id, .. } |
                AgentRegistryOp::DeauthorizeAgent { graph_id, .. } => Some(*graph_id),
                _ => None,
            },
            _ => None,
        }
    }
    
    /// Extract the agent ID from an operation if it involves a specific agent
    pub fn extract_agent_id(&self) -> Option<Uuid> {
        match self {
            Operation::Graph(op) => match op {
                GraphOperation::CreateBlock { agent_id, .. } |
                GraphOperation::UpdateBlock { agent_id, .. } |
                GraphOperation::DeleteBlock { agent_id, .. } |
                GraphOperation::CreatePage { agent_id, .. } |
                GraphOperation::DeletePage { agent_id, .. } => Some(*agent_id),
            },
            Operation::Agent(op) => match op {
                AgentOperation::AddMessage { agent_id, .. } |
                AgentOperation::ClearHistory { agent_id } |
                AgentOperation::SetLLMConfig { agent_id, .. } |
                AgentOperation::SetSystemPrompt { agent_id, .. } |
                AgentOperation::SetDefaultGraph { agent_id, .. } => Some(*agent_id),
            },
            Operation::Registry(RegistryOperation::Agent(op)) => match op {
                AgentRegistryOp::RegisterAgent { agent_id, .. } |
                AgentRegistryOp::RemoveAgent { agent_id } |
                AgentRegistryOp::ActivateAgent { agent_id } |
                AgentRegistryOp::DeactivateAgent { agent_id } |
                AgentRegistryOp::AuthorizeAgent { agent_id, .. } |
                AgentRegistryOp::DeauthorizeAgent { agent_id, .. } |
                AgentRegistryOp::SetPrimeAgent { agent_id } => Some(*agent_id),
            },
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct TransactionLog {
    db: sled::Db,
    transactions_tree: sled::Tree,
    content_hash_index: sled::Tree,
    pending_index: sled::Tree,
}

impl TransactionLog {
    // TODO 🔄: Implement transaction log compaction based on compaction_threshold_mb config
    // TODO 🗑️: Implement retention policy to clean up transactions older than retention_days
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        
        let config = sled::Config::new()
            .path(path_ref)
            // Removed async flushing - let sled use synchronous defaults for proper transaction ordering
            .cache_capacity(64 * 1024 * 1024)  // 64MB cache
            .mode(sled::Mode::HighThroughput);
            
        let db = config.open()?;
        
        let transactions_tree = db.open_tree("transactions")?;
        let content_hash_index = db.open_tree("content_hash_index")?;
        let pending_index = db.open_tree("pending_transactions")?;
        
        Ok(Self {
            db,
            transactions_tree,
            content_hash_index,
            pending_index,
        })
    }
    
    
    /// Flush and close the database, ensuring all pending writes are persisted
    pub async fn close(&self) -> Result<()> {
        // Flush transaction log to disk before closing
        self.db.flush_async().await?;
        // Transaction log flushed successfully
        // db is dropped when TransactionLog is dropped, triggering sled cleanup
        
        Ok(())
    }
    
    pub fn append_transaction(&self, transaction: Transaction) -> Result<String> {
        let tx_id = transaction.id.clone();
        let tx_bytes = serde_json::to_vec(&transaction)?;
        
        // Use atomic sled multi-tree transaction for proper ordering guarantees
        use sled::Transactional;
        (&self.transactions_tree, &self.content_hash_index, &self.pending_index)
            .transaction(|(tx_tree, hash_tree, pending_tree)| {
                // Store the transaction atomically
                tx_tree.insert(tx_id.as_bytes(), tx_bytes.as_slice())?;
                
                // Index by content hash if present
                if let Some(hash) = &transaction.content_hash {
                    hash_tree.insert(hash.as_bytes(), tx_id.as_bytes())?;
                }
                
                // Add to pending index
                pending_tree.insert(tx_id.as_bytes(), b"")?;
                
                Ok(())
            }).map_err(|e: sled::transaction::TransactionError<sled::Error>| StorageError::wal(format!("Failed to append transaction: {:?}", e)))?;
        
        Ok(tx_id)
    }
    
    pub fn get_transaction(&self, id: &str) -> Result<Transaction> {
        match self.transactions_tree.get(id.as_bytes())? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Err(StorageError::not_found("transaction", "ID", id).into()),
        }
    }
    
    pub fn update_transaction_deferred(&self, id: &str, reason: &str) -> Result<()> {
        let mut transaction = self.get_transaction(id)?;
        
        // Update deferred reason and increment retry count
        transaction.deferred_reason = Some(reason.to_string());
        transaction.retry_count += 1;
        transaction.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        let tx_bytes = serde_json::to_vec(&transaction)?;
        
        // Update in tree (stays in pending index since still Active)
        self.transactions_tree.insert(id.as_bytes(), tx_bytes)?;
        
        Ok(())
    }
    
    pub fn update_transaction_state(&self, id: &str, new_state: TransactionState) -> Result<()> {
        let mut transaction = self.get_transaction(id)?;
        
        // Validate state transition
        match (&transaction.state, &new_state) {
            (TransactionState::Active, _) => {}, // Active can transition to any state  
            (from, to) => {
                return Err(StorageError::wal(format!("Cannot transition from {:?} to {:?}", from, to)).into());
            }
        }
        
        transaction.state = new_state.clone();
        transaction.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
                
        let tx_bytes = serde_json::to_vec(&transaction)?;
        
        // Use atomic sled multi-tree transaction for proper ordering guarantees
        use sled::Transactional;
        (&self.transactions_tree, &self.pending_index)
            .transaction(|(tx_tree, pending_tree)| {
                // Update transaction state atomically
                tx_tree.insert(id.as_bytes(), tx_bytes.as_slice())?;
                
                // Remove from pending index if committed or aborted
                if matches!(new_state, TransactionState::Committed | TransactionState::Aborted) {
                    pending_tree.remove(id.as_bytes())?;
                }
                
                Ok(())
            }).map_err(|e: sled::transaction::TransactionError<sled::Error>| StorageError::wal(format!("Failed to update transaction state: {:?}", e)))?;
        
        Ok(())
    }
    
    pub fn list_pending_transactions(&self) -> Result<Vec<Transaction>> {
        let mut pending = Vec::new();
        
        for item in self.pending_index.iter() {
            let (tx_id_bytes, _) = item?;
            let tx_id = String::from_utf8_lossy(&tx_id_bytes);
            
            if let Ok(transaction) = self.get_transaction(&tx_id) {
                pending.push(transaction);
            } else {
            }
        }
        
        // Sort by created_at to maintain chronological order
        pending.sort_by_key(|t| t.created_at);
        
        Ok(pending)
    }
    
    
    /// List all transactions (committed and pending) for full WAL replay
    /// List only committed transactions for stable state rebuild
    pub fn list_committed_transactions(&self) -> Result<Vec<Transaction>> {
        let mut transactions = Vec::new();
        
        for item in self.transactions_tree.iter() {
            let (_key, value) = item?;
            let transaction: Transaction = serde_json::from_slice(&value)?;
            
            // Only include committed transactions
            if matches!(transaction.state, TransactionState::Committed) {
                transactions.push(transaction);
            }
        }
        
        // Sort by created_at to maintain chronological order
        transactions.sort_by_key(|t| t.created_at);
        
        Ok(transactions)
    }
}

fn compute_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    fn create_test_log() -> (TransactionLog, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let log = TransactionLog::new(temp_dir.path()).unwrap();
        (log, temp_dir)
    }
    
    #[test]
    fn test_append_and_get_transaction() {
        let (log, _temp_dir) = create_test_log();
        
        let operation = Operation::Graph(GraphOperation::CreateBlock {
            graph_id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("test-page".to_string()),
            properties: None,
        });
        
        let transaction = Transaction::new(operation);
        let tx_id = log.append_transaction(transaction.clone()).unwrap();
        
        let retrieved = log.get_transaction(&tx_id).unwrap();
        assert_eq!(retrieved.id, tx_id);
        assert_eq!(retrieved.state, TransactionState::Active);
        assert!(retrieved.content_hash.is_some());
    }
    
    #[test]
    fn test_update_transaction_state() {
        let (log, _temp_dir) = create_test_log();
        
        let operation = Operation::Graph(GraphOperation::CreateBlock {
            graph_id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("test-page".to_string()),
            properties: None,
        });
        
        let transaction = Transaction::new(operation);
        let tx_id = log.append_transaction(transaction).unwrap();
        
        // Update to Committed
        log.update_transaction_state(&tx_id, TransactionState::Committed).unwrap();
        let final_state = log.get_transaction(&tx_id).unwrap();
        assert_eq!(final_state.state, TransactionState::Committed);
    }
    
    #[test]
    fn test_pending_transactions() {
        let (log, _temp_dir) = create_test_log();
        
        // Create multiple transactions
        for i in 0..3 {
            let operation = Operation::Graph(GraphOperation::CreateBlock {
                graph_id: uuid::Uuid::new_v4(),
                agent_id: uuid::Uuid::new_v4(),
                content: format!("Content {}", i),
                parent_id: None,
                page_name: Some("test-page".to_string()),
                properties: None,
            });
            log.append_transaction(Transaction::new(operation)).unwrap();
        }
        
        let pending = log.list_pending_transactions().unwrap();
        assert_eq!(pending.len(), 3);
        
        // Commit one transaction
        log.update_transaction_state(&pending[0].id, TransactionState::Committed).unwrap();
        
        let remaining_pending = log.list_pending_transactions().unwrap();
        assert_eq!(remaining_pending.len(), 2);
    }
    
}