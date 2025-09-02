//! Command Log - Persistence Layer for CQRS (Transitional)
//!
//! This module provides command persistence using sled embedded database for
//! durability and crash recovery. Commands are logged before execution to ensure
//! no state changes are lost. This implementation will be replaced with simpler
//! JSON snapshots in the next iteration, but the CQRS architecture will remain.
//!
//! ## Current Role
//!
//! The CommandLog serves three purposes in the current architecture:
//! 1. **Crash Recovery**: Replay commands after unexpected shutdown
//! 2. **Lazy Loading**: Rebuild entities from filtered command history
//! 3. **Deduplication**: Prevent duplicate command execution
//!
//! ## Why This Will Be Simplified
//!
//! The current WAL approach was designed for multi-agent consensus, but we're
//! pivoting to a single-agent model. The complexity of:
//! - Three-tree sled database structure
//! - SHA-256 content hashing
//! - Pending transaction tracking
//! - Lazy entity reconstruction
//!
//! ...is overkill for a single-user knowledge graph application.
//!
//! ## Future Direction
//!
//! The next iteration will use:
//! - JSON snapshots for persistence (simple, debuggable)
//! - In-memory command queue (fast, simple)
//! - Periodic saves (good enough for single-user)
//! - No complex recovery (just load JSON on startup)
//!
//! The CQRS pattern itself is valuable and will remain - it's the distributed
//! systems baggage (WAL, consensus, etc.) that will be removed.
//!
//! ## Current Implementation
//!
//! ### Storage Structure
//! Uses sled embedded database with three trees:
//! - **Commands**: UUID → serialized command
//! - **Content Hash**: SHA-256 → UUID (deduplication)
//! - **Pending**: UUID → empty (tracking incomplete)
//!
//! ### Transaction Lifecycle
//! 1. Begin: Create transaction, mark as pending
//! 2. Execute: Process command
//! 3. Commit: Remove from pending, mark complete
//! 4. Recovery: Replay pending on startup
//!
//! ### Performance Notes
//! - Sled uses fsync for durability (configurable interval)
//! - Content hashing adds CPU overhead
//! - Recovery time scales with command count
//!
//! This is functional but overengineered for our needs. The simpler approach
//! will be faster, use less memory, and be easier to debug.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::error::*;

use super::commands::{Command, GraphCommand};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandState {
    Active,    // Command created but not yet executed
    Committed, // Command successfully executed
    Aborted,   // Command failed and will not be retried
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandTransaction {
    pub id: String,
    pub command: Command,
    pub state: CommandState,
    pub created_at: u64,
    pub updated_at: u64,
    pub content_hash: Option<String>,
    pub error_message: Option<String>,
    pub deferred_reason: Option<String>,  // Why execution was delayed (freeze mechanism)
}

impl CommandTransaction {
    pub fn new(command: Command) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        let content_hash = match &command {
            Command::Graph(GraphCommand::CreateBlock { content, .. }) | 
            Command::Graph(GraphCommand::UpdateBlock { content, .. }) => {
                Some(compute_content_hash(content))
            }
            _ => None,
        };
        
        Self {
            id: Uuid::new_v4().to_string(),
            command,
            state: CommandState::Active,
            created_at: now,
            updated_at: now,
            content_hash,
            error_message: None,
            deferred_reason: None,
        }
    }
}

#[derive(Debug)]
pub struct CommandLog {
    db: sled::Db,
    commands_tree: sled::Tree,
    content_hash_index: sled::Tree,
    pending_index: sled::Tree,
}

impl CommandLog {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        
        let config = sled::Config::new()
            .path(path_ref)
            .cache_capacity(64 * 1024 * 1024)  // 64MB cache
            .mode(sled::Mode::HighThroughput);
            
        let db = config.open()?;
        
        let commands_tree = db.open_tree("commands")?;
        let content_hash_index = db.open_tree("content_hash_index")?;
        let pending_index = db.open_tree("pending_commands")?;
        
        Ok(Self {
            db,
            commands_tree,
            content_hash_index,
            pending_index,
        })
    }
    
    /// Flush and close the database
    pub async fn close(&self) -> Result<()> {
        self.db.flush_async().await?;
        Ok(())
    }
    
    pub fn append_command(&self, transaction: CommandTransaction) -> Result<String> {
        let tx_id = transaction.id.clone();
        let tx_bytes = serde_json::to_vec(&transaction)?;
        
        // Use atomic sled multi-tree transaction
        use sled::Transactional;
        (&self.commands_tree, &self.content_hash_index, &self.pending_index)
            .transaction(|(tx_tree, hash_tree, pending_tree)| {
                // Store the command atomically
                tx_tree.insert(tx_id.as_bytes(), tx_bytes.as_slice())?;
                
                // Index by content hash if present
                if let Some(hash) = &transaction.content_hash {
                    hash_tree.insert(hash.as_bytes(), tx_id.as_bytes())?;
                }
                
                // Add to pending index
                pending_tree.insert(tx_id.as_bytes(), b"")?;
                
                Ok(())
            }).map_err(|e: sled::transaction::TransactionError<sled::Error>| 
                StorageError::wal(format!("Failed to append command: {:?}", e)))?;
        
        Ok(tx_id)
    }
    
    pub fn get_command(&self, id: &str) -> Result<CommandTransaction> {
        match self.commands_tree.get(id.as_bytes())? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Err(StorageError::not_found("command", "ID", id).into()),
        }
    }
    
    pub fn update_command_state(&self, id: &str, new_state: CommandState) -> Result<()> {
        let mut transaction = self.get_command(id)?;
        
        // Validate state transition
        match (&transaction.state, &new_state) {
            (CommandState::Active, _) => {}, // Active can transition to any state  
            (from, to) => {
                return Err(StorageError::wal(
                    format!("Cannot transition from {:?} to {:?}", from, to)
                ).into());
            }
        }
        
        transaction.state = new_state.clone();
        transaction.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
                
        let tx_bytes = serde_json::to_vec(&transaction)?;
        
        // Use atomic sled multi-tree transaction
        use sled::Transactional;
        (&self.commands_tree, &self.pending_index)
            .transaction(|(tx_tree, pending_tree)| {
                // Update command state atomically
                tx_tree.insert(id.as_bytes(), tx_bytes.as_slice())?;
                
                // Remove from pending index if committed or aborted
                if matches!(new_state, CommandState::Committed | CommandState::Aborted) {
                    pending_tree.remove(id.as_bytes())?;
                }
                
                Ok(())
            }).map_err(|e: sled::transaction::TransactionError<sled::Error>| 
                StorageError::wal(format!("Failed to update command state: {:?}", e)))?;
        
        Ok(())
    }
    
    pub fn list_pending_commands(&self) -> Result<Vec<CommandTransaction>> {
        let mut pending = Vec::new();
        
        for item in self.pending_index.iter() {
            let (tx_id_bytes, _) = item?;
            let tx_id = String::from_utf8_lossy(&tx_id_bytes);
            
            if let Ok(transaction) = self.get_command(&tx_id) {
                pending.push(transaction);
            }
        }
        
        // Sort by created_at to maintain chronological order
        pending.sort_by_key(|t| t.created_at);
        
        Ok(pending)
    }
    
    /// List only committed commands for stable state rebuild
    pub fn list_committed_commands(&self) -> Result<Vec<CommandTransaction>> {
        let mut commands = Vec::new();
        
        for item in self.commands_tree.iter() {
            let (_key, value) = item?;
            let transaction: CommandTransaction = serde_json::from_slice(&value)?;
            
            // Only include committed commands
            if matches!(transaction.state, CommandState::Committed) {
                commands.push(transaction);
            }
        }
        
        // Sort by created_at to maintain chronological order
        commands.sort_by_key(|t| t.created_at);
        
        Ok(commands)
    }
    
    /// Check if content is already pending
    pub fn is_content_pending(&self, content_hash: &str) -> Result<bool> {
        if let Some(tx_id_bytes) = self.content_hash_index.get(content_hash.as_bytes())? {
            let tx_id = String::from_utf8_lossy(&tx_id_bytes);
            if let Ok(transaction) = self.get_command(&tx_id) {
                return Ok(matches!(transaction.state, CommandState::Active));
            }
        }
        Ok(false)
    }
    
    /// Update a command as deferred (for freeze mechanism)
    pub fn update_command_deferred(&self, id: &str, reason: &str) -> Result<()> {
        let mut transaction = self.get_command(id)?;
        
        // Update deferred reason
        transaction.deferred_reason = Some(reason.to_string());
        transaction.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        // Keep state as Active (not Committed or Aborted)
        // This ensures it will be retried during recovery
        
        // Update in database
        let tx_bytes = serde_json::to_vec(&transaction)?;
        self.commands_tree.insert(id.as_bytes(), tx_bytes)?;
        self.commands_tree.flush()?;
        
        Ok(())
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
    
    fn create_test_log() -> (CommandLog, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let log = CommandLog::new(temp_dir.path()).unwrap();
        (log, temp_dir)
    }
    
    #[test]
    fn test_append_and_get_command() {
        let (log, _temp_dir) = create_test_log();
        
        let command = Command::Graph(GraphCommand::CreateBlock {
            graph_id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("test-page".to_string()),
            properties: None,
        });
        
        let transaction = CommandTransaction::new(command);
        let tx_id = log.append_command(transaction.clone()).unwrap();
        
        let retrieved = log.get_command(&tx_id).unwrap();
        assert_eq!(retrieved.id, tx_id);
        assert_eq!(retrieved.state, CommandState::Active);
        assert!(retrieved.content_hash.is_some());
    }
    
    #[test]
    fn test_update_command_state() {
        let (log, _temp_dir) = create_test_log();
        
        let command = Command::Graph(GraphCommand::CreateBlock {
            graph_id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            content: "Test content".to_string(),
            parent_id: None,
            page_name: Some("test-page".to_string()),
            properties: None,
        });
        
        let transaction = CommandTransaction::new(command);
        let tx_id = log.append_command(transaction).unwrap();
        
        // Update to Committed
        log.update_command_state(&tx_id, CommandState::Committed).unwrap();
        let final_state = log.get_command(&tx_id).unwrap();
        assert_eq!(final_state.state, CommandState::Committed);
    }
    
    #[test]
    fn test_pending_commands() {
        let (log, _temp_dir) = create_test_log();
        
        // Create multiple commands
        for i in 0..3 {
            let command = Command::Graph(GraphCommand::CreateBlock {
                graph_id: uuid::Uuid::new_v4(),
                agent_id: uuid::Uuid::new_v4(),
                content: format!("Content {}", i),
                parent_id: None,
                page_name: Some("test-page".to_string()),
                properties: None,
            });
            log.append_command(CommandTransaction::new(command)).unwrap();
        }
        
        let pending = log.list_pending_commands().unwrap();
        assert_eq!(pending.len(), 3);
        
        // Commit one command
        log.update_command_state(&pending[0].id, CommandState::Committed).unwrap();
        
        let remaining_pending = log.list_pending_commands().unwrap();
        assert_eq!(remaining_pending.len(), 2);
    }
}