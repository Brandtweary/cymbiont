//! WAL-Based Validation System
//! 
//! Direct validation against the Write-Ahead Log (sled database) instead of JSON files.
//! This eliminates timing issues and validates the actual source of truth.
//! 
//! ## Architecture
//! 
//! The WAL validator reads transactions directly from the sled database and validates
//! that expected operations were logged. This is more reliable than checking JSON files
//! which may not be written immediately or at all (for closed graphs/inactive agents).
//! 
//! ## Usage Example
//! 
//! ```rust
//! let mut validator = WalValidator::new(&test_env.data_dir);
//! 
//! // Set up expectations
//! validator.expect_create_page("test-page", None);
//! validator.expect_create_block("block-id", "content", Some("test-page"));
//! validator.expect_update_block("block-id", "new content");
//! 
//! // Run operations...
//! 
//! // Validate all expectations were met
//! validator.validate_all();
//! ```

use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
// use chrono::{DateTime, Utc}; // Not needed yet

// Import the operation types from the main codebase
// We need to match the exact structure used in transaction_log.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionState {
    Active,
    Committed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Graph(GraphOperation),
    Agent(AgentOperation),
    Registry(RegistryOperation),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphOperation {
    CreateBlock {
        graph_id: Uuid,
        agent_id: Uuid,
        content: String,
        parent_id: Option<String>,
        page_name: Option<String>,
        properties: Option<Value>,
    },
    UpdateBlock {
        graph_id: Uuid,
        agent_id: Uuid,
        block_id: String,
        content: String,
        properties: Option<Value>,
    },
    DeleteBlock {
        graph_id: Uuid,
        agent_id: Uuid,
        block_id: String,
    },
    CreatePage {
        graph_id: Uuid,
        agent_id: Uuid,
        page_name: String,
        properties: Option<Value>,
    },
    DeletePage {
        graph_id: Uuid,
        agent_id: Uuid,
        page_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOperation {
    AddMessage {
        agent_id: Uuid,
        message: Value, // Contains role, content, timestamp, etc.
    },
    ClearHistory {
        agent_id: Uuid,
    },
    SetLLMConfig {
        agent_id: Uuid,
        config: Value,
    },
    SetSystemPrompt {
        agent_id: Uuid,
        prompt: String,
    },
    SetDefaultGraph {
        agent_id: Uuid,
        graph_id: Option<Uuid>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryOperation {
    Graph(GraphRegistryOp),
    Agent(AgentRegistryOp),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphRegistryOp {
    RegisterGraph {
        graph_id: Uuid,
        name: String,
        path: String,
    },
    RemoveGraph {
        graph_id: Uuid,
    },
    OpenGraph {
        graph_id: Uuid,
    },
    CloseGraph {
        graph_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRegistryOp {
    RegisterAgent {
        agent_id: Uuid,
        name: String,
        is_prime: bool,
    },
    RemoveAgent {
        agent_id: Uuid,
    },
    ActivateAgent {
        agent_id: Uuid,
    },
    DeactivateAgent {
        agent_id: Uuid,
    },
    AuthorizeAgent {
        agent_id: Uuid,
        graph_id: Uuid,
    },
    DeauthorizeAgent {
        agent_id: Uuid,
        graph_id: Uuid,
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
}

/// Read-only access to the WAL database - temporary helper
struct WalReader;

impl WalReader {
    /// Open the WAL database for reading and return all transactions
    fn read_all_transactions(data_dir: &Path) -> Result<Vec<Transaction>, String> {
        let wal_path = data_dir.join("transaction_log");
        
        // Open sled database
        let config = sled::Config::new()
            .path(&wal_path);
            
        let db = config.open()
            .map_err(|e| format!("Failed to open WAL database: {}", e))?;
        
        let transactions_tree = db.open_tree("transactions")
            .map_err(|e| format!("Failed to open transactions tree: {}", e))?;
        
        let mut transactions = Vec::new();
        
        for item in transactions_tree.iter() {
            if let Ok((_key, value)) = item {
                if let Ok(transaction) = serde_json::from_slice::<Transaction>(&value) {
                    transactions.push(transaction);
                }
            }
        }
        
        // Sort by created_at to maintain chronological order
        transactions.sort_by_key(|t| t.created_at);
        Ok(transactions)
    }
    
    /// Read only committed transactions  
    fn read_committed_transactions(data_dir: &Path) -> Result<Vec<Transaction>, String> {
        let all = Self::read_all_transactions(data_dir)?;
        Ok(all.into_iter()
            .filter(|t| matches!(t.state, TransactionState::Committed))
            .collect())
    }
    
    /// Find all transactions for a specific graph
    fn find_by_graph(transactions: &[Transaction], graph_id: Uuid) -> Vec<Transaction> {
        transactions
            .iter()
            .filter(|t| {
                match &t.operation {
                    Operation::Graph(op) => match op {
                        GraphOperation::CreateBlock { graph_id: gid, .. } |
                        GraphOperation::UpdateBlock { graph_id: gid, .. } |
                        GraphOperation::DeleteBlock { graph_id: gid, .. } |
                        GraphOperation::CreatePage { graph_id: gid, .. } |
                        GraphOperation::DeletePage { graph_id: gid, .. } => *gid == graph_id,
                    },
                    Operation::Registry(RegistryOperation::Graph(op)) => match op {
                        GraphRegistryOp::RegisterGraph { graph_id: gid, .. } |
                        GraphRegistryOp::RemoveGraph { graph_id: gid } |
                        GraphRegistryOp::OpenGraph { graph_id: gid } |
                        GraphRegistryOp::CloseGraph { graph_id: gid } => *gid == graph_id,
                    },
                    _ => false,
                }
            })
            .cloned()
            .collect()
    }
    
}

/// Expected graph operation for validation
#[derive(Debug, Clone)]
pub struct ExpectedGraphOp {
    pub op_type: GraphOpType,
    pub content: Option<String>,
    pub page_name: Option<String>,
    pub block_id: Option<String>,
    pub properties: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphOpType {
    CreatePage,
    CreateBlock,
    UpdateBlock,
    DeleteBlock,
    DeletePage,
}

/// Expected agent operation for validation
#[derive(Debug, Clone)]
pub struct ExpectedAgentOp {
    pub op_type: AgentOpType,
    pub agent_id: Uuid,
    pub details: AgentOpDetails,
}

#[derive(Debug, Clone)]
pub enum AgentOpType {
    RegisterAgent,
    RemoveAgent,
    ActivateAgent,
    DeactivateAgent,
    // Note: AddMessage operations are validated separately through validate_conversation()
    // because they require special handling for ordering and content pattern matching
    ClearHistory,
    AuthorizeAgent,
    DeauthorizeAgent,
}

#[derive(Debug, Clone)]
pub enum AgentOpDetails {
    Register { name: String, is_prime: bool },
    // Note: Message variant removed - messages are validated through expected_conversations
    Authorization { graph_id: Uuid },
    Simple, // For operations with no additional details
}

/// Pattern matching for message content
#[derive(Debug, Clone)]
pub enum MessagePattern {
    Exact(String),
    Contains(String),
}

impl MessagePattern {
    pub fn matches(&self, actual: &str) -> bool {
        match self {
            MessagePattern::Exact(expected) => actual == expected,
            MessagePattern::Contains(substring) => actual.contains(substring),
        }
    }
}

/// Main WAL validator combining graph and agent validation
pub struct WalValidator {
    data_dir: PathBuf,  // Just store the path - don't open database yet
    
    // Graph expectations
    expected_graph_ops: Vec<ExpectedGraphOp>,
    deleted_nodes: HashSet<String>,
    
    // Agent expectations
    expected_agent_ops: Vec<ExpectedAgentOp>,
    deleted_agents: HashSet<Uuid>,
    expected_conversations: HashMap<Uuid, Vec<(String, MessagePattern)>>, // (role, pattern)
    
    // Registry expectations (still JSON-based)
    expected_authorizations: HashSet<(Uuid, Uuid)>,
    expected_prime_agent: Option<Uuid>,
}

impl WalValidator {
    /// Create a new WAL validator for the given data directory
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),  // Just store the path
            expected_graph_ops: Vec::new(),
            deleted_nodes: HashSet::new(),
            expected_agent_ops: Vec::new(),
            deleted_agents: HashSet::new(),
            expected_conversations: HashMap::new(),
            expected_authorizations: HashSet::new(),
            expected_prime_agent: None,
        }
    }
    
    // ===== Graph Validation Methods (adapted from GraphValidationFixture) =====
    
    /// Record that a page will be created
    pub fn expect_create_page(&mut self, name: &str, properties: Option<Value>) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            op_type: GraphOpType::CreatePage,
            content: Some(name.to_string()),
            page_name: None,
            block_id: None,
            properties,
        });
        self.deleted_nodes.remove(name);
        self
    }
    
    /// Record that a block will be created
    pub fn expect_create_block(
        &mut self,
        block_id: &str,
        content: &str,
        page_name: Option<&str>
    ) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            op_type: GraphOpType::CreateBlock,
            content: Some(content.to_string()),
            page_name: page_name.map(|s| s.to_string()),
            block_id: Some(block_id.to_string()),
            properties: None,
        });
        self.deleted_nodes.remove(block_id);
        self
    }
    
    /// Record that a block's content will be updated
    pub fn expect_update_block(&mut self, block_id: &str, new_content: &str) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            op_type: GraphOpType::UpdateBlock,
            content: Some(new_content.to_string()),
            page_name: None,
            block_id: Some(block_id.to_string()),
            properties: None,
        });
        self
    }
    
    /// Record that a block will be deleted
    pub fn expect_delete_block(&mut self, block_id: &str) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            op_type: GraphOpType::DeleteBlock,
            block_id: Some(block_id.to_string()),
            content: None,
            page_name: None,
            properties: None,
        });
        self.deleted_nodes.insert(block_id.to_string());
        self
    }
    
    /// Record that a page will be deleted
    pub fn expect_delete_page(&mut self, page_name: &str) -> &mut Self {
        self.expected_graph_ops.push(ExpectedGraphOp {
            op_type: GraphOpType::DeletePage,
            block_id: None,
            content: Some(page_name.to_string()), // For pages, we use content field for name
            page_name: None,
            properties: None,
        });
        self.deleted_nodes.insert(page_name.to_string());
        self
    }
    
    /// Add expectations for the dummy graph that's imported in tests
    pub fn expect_dummy_graph(&mut self) -> &mut Self {
        // Based on the dummy_graph content, we expect certain pages to be created
        // This is simplified - in reality we'd need to parse the actual dummy graph
        self.expect_create_page("cyberorganism-test-1", None)
            .expect_create_page("cyberorganism-test-2", None)
            .expect_create_page("contents", None)
            .expect_create_page("test-websocket", None);
        self
    }
    
    // ===== Agent Validation Methods (adapted from AgentValidationFixture) =====
    
    /// Record that an agent will be created
    pub fn expect_agent_created(&mut self, id: Uuid, name: &str, is_prime: bool) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::RegisterAgent,
            agent_id: id,
            details: AgentOpDetails::Register {
                name: name.to_string(),
                is_prime,
            },
        });
        
        if is_prime {
            self.expected_prime_agent = Some(id);
        }
        
        self.deleted_agents.remove(&id);
        self
    }
    
    /// Record that an agent will be deleted
    pub fn expect_agent_deleted(&mut self, id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::RemoveAgent,
            agent_id: *id,
            details: AgentOpDetails::Simple,
        });
        self.deleted_agents.insert(*id);
        self.expected_conversations.remove(id);
        self.expected_authorizations.retain(|(aid, _)| aid != id);
        self
    }
    
    /// Record that an agent will be activated
    pub fn expect_agent_activated(&mut self, id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::ActivateAgent,
            agent_id: *id,
            details: AgentOpDetails::Simple,
        });
        self
    }
    
    /// Record that an agent will be deactivated
    pub fn expect_agent_deactivated(&mut self, id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::DeactivateAgent,
            agent_id: *id,
            details: AgentOpDetails::Simple,
        });
        self
    }
    
    /// Record that a user message will be added
    pub fn expect_user_message(&mut self, agent_id: &Uuid, content: MessagePattern) -> &mut Self {
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push(("user".to_string(), content));
        self
    }
    
    /// Record that an assistant message will be added
    pub fn expect_assistant_message(&mut self, agent_id: &Uuid, content: MessagePattern) -> &mut Self {
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push(("assistant".to_string(), content));
        self
    }
    
    /// Record that a tool message will be added
    pub fn expect_tool_message(&mut self, agent_id: &Uuid, tool: &str, result: MessagePattern) -> &mut Self {
        // For tool messages, we encode the tool name in the role string
        // This will be properly validated in validate_conversation
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push((format!("tool:{}", tool), result));
        self
    }
    
    /// Record that an agent's chat will be reset
    pub fn expect_chat_reset(&mut self, agent_id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::ClearHistory,
            agent_id: *agent_id,
            details: AgentOpDetails::Simple,
        });
        self.expected_conversations.insert(*agent_id, Vec::new());
        self
    }
    
    /// Record that an agent will be authorized for a graph
    pub fn expect_authorization(&mut self, agent_id: &Uuid, graph_id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::AuthorizeAgent,
            agent_id: *agent_id,
            details: AgentOpDetails::Authorization { graph_id: *graph_id },
        });
        self.expected_authorizations.insert((*agent_id, *graph_id));
        self
    }
    
    /// Record that an agent will be deauthorized from a graph
    pub fn expect_deauthorization(&mut self, agent_id: &Uuid, graph_id: &Uuid) -> &mut Self {
        self.expected_agent_ops.push(ExpectedAgentOp {
            op_type: AgentOpType::DeauthorizeAgent,
            agent_id: *agent_id,
            details: AgentOpDetails::Authorization { graph_id: *graph_id },
        });
        self.expected_authorizations.remove(&(*agent_id, *graph_id));
        self
    }
    
    /// Helper to set up prime agent expectations
    pub fn expect_prime_agent(&mut self, prime_id: Uuid) -> &mut Self {
        self.expect_agent_created(prime_id, "Prime Agent", true);
        self
    }
    
    // ===== Main Validation Methods =====
    
    /// Validate all expectations against the WAL
    pub fn validate_all(&self) -> Result<(), String> {
        // NOW we open the database and read transactions
        let transactions = WalReader::read_committed_transactions(&self.data_dir)?;
        
        // Validate graph operations
        for expected in &self.expected_graph_ops {
            if !self.find_graph_operation(&transactions, expected) {
                return Err(format!(
                    "Expected graph operation not found: {:?}",
                    expected
                ));
            }
        }
        
        // Validate deleted nodes don't have create operations after delete
        for node_id in &self.deleted_nodes {
            if self.has_create_after_delete(&transactions, node_id) {
                return Err(format!(
                    "Node {} was created after being deleted",
                    node_id
                ));
            }
        }
        
        // Validate agent operations
        for expected in &self.expected_agent_ops {
            if !self.find_agent_operation(&transactions, expected) {
                return Err(format!(
                    "Expected agent operation not found: {:?}",
                    expected
                ));
            }
        }
        
        // Validate conversations
        for (agent_id, expected_messages) in &self.expected_conversations {
            self.validate_conversation(&transactions, *agent_id, expected_messages)?;
        }
        
        Ok(())
    }
    
    /// Find a graph operation in the transaction list
    fn find_graph_operation(&self, transactions: &[Transaction], expected: &ExpectedGraphOp) -> bool {
        transactions.iter().any(|tx| {
            if let Operation::Graph(op) = &tx.operation {
                match (&expected.op_type, op) {
                    (GraphOpType::CreatePage, GraphOperation::CreatePage { page_name, properties, .. }) => {
                        expected.content.as_ref() == Some(page_name) &&
                        expected.properties.as_ref() == properties.as_ref()
                    },
                    (GraphOpType::CreateBlock, GraphOperation::CreateBlock { content, page_name, .. }) => {
                        expected.content.as_ref() == Some(content) &&
                        expected.page_name.as_deref() == page_name.as_deref()
                    },
                    (GraphOpType::UpdateBlock, GraphOperation::UpdateBlock { block_id, content, .. }) => {
                        expected.block_id.as_ref() == Some(block_id) &&
                        expected.content.as_ref() == Some(content)
                    },
                    (GraphOpType::DeleteBlock, GraphOperation::DeleteBlock { block_id, .. }) => {
                        expected.block_id.as_ref() == Some(block_id)
                    },
                    (GraphOpType::DeletePage, GraphOperation::DeletePage { page_name, .. }) => {
                        expected.content.as_ref() == Some(page_name)
                    },
                    _ => false,
                }
            } else {
                false
            }
        })
    }
    
    /// Find an agent operation in the transaction list
    fn find_agent_operation(&self, transactions: &[Transaction], expected: &ExpectedAgentOp) -> bool {
        transactions.iter().any(|tx| {
            match &tx.operation {
                Operation::Registry(RegistryOperation::Agent(op)) => {
                    match (&expected.op_type, &expected.details, op) {
                        (AgentOpType::RegisterAgent, AgentOpDetails::Register { name, is_prime }, 
                         AgentRegistryOp::RegisterAgent { agent_id, name: n, is_prime: p }) => {
                            *agent_id == expected.agent_id && n == name && *p == *is_prime
                        },
                        (AgentOpType::RemoveAgent, _, AgentRegistryOp::RemoveAgent { agent_id }) => {
                            *agent_id == expected.agent_id
                        },
                        (AgentOpType::ActivateAgent, _, AgentRegistryOp::ActivateAgent { agent_id }) => {
                            *agent_id == expected.agent_id
                        },
                        (AgentOpType::DeactivateAgent, _, AgentRegistryOp::DeactivateAgent { agent_id }) => {
                            *agent_id == expected.agent_id
                        },
                        (AgentOpType::AuthorizeAgent, AgentOpDetails::Authorization { graph_id }, 
                         AgentRegistryOp::AuthorizeAgent { agent_id, graph_id: gid }) => {
                            *agent_id == expected.agent_id && gid == graph_id
                        },
                        (AgentOpType::DeauthorizeAgent, AgentOpDetails::Authorization { graph_id },
                         AgentRegistryOp::DeauthorizeAgent { agent_id, graph_id: gid }) => {
                            *agent_id == expected.agent_id && gid == graph_id
                        },
                        _ => false,
                    }
                },
                Operation::Agent(op) => {
                    match (&expected.op_type, op) {
                        (AgentOpType::ClearHistory, AgentOperation::ClearHistory { agent_id }) => {
                            *agent_id == expected.agent_id
                        },
                        _ => false,
                    }
                },
                _ => false,
            }
        })
    }
    
    /// Check if a node has create operations after being deleted
    fn has_create_after_delete(&self, transactions: &[Transaction], node_id: &str) -> bool {
        let mut deleted_at = None;
        
        // Find when it was deleted
        for tx in transactions {
            if let Operation::Graph(op) = &tx.operation {
                match op {
                    GraphOperation::DeleteBlock { block_id, .. } if block_id == node_id => {
                        deleted_at = Some(tx.created_at);
                    },
                    GraphOperation::DeletePage { page_name, .. } if page_name == node_id => {
                        deleted_at = Some(tx.created_at);
                    },
                    _ => {},
                }
            }
        }
        
        // If deleted, check for creates after that time
        if let Some(delete_time) = deleted_at {
            for tx in transactions {
                if tx.created_at > delete_time {
                    if let Operation::Graph(op) = &tx.operation {
                        match op {
                            GraphOperation::CreateBlock { .. } => {
                                // Check if this creates the same block ID
                                // Note: We'd need the block ID in the response to properly track this
                                // For now, this is a simplified check
                            },
                            GraphOperation::CreatePage { page_name, .. } if page_name == node_id => {
                                return true;
                            },
                            _ => {},
                        }
                    }
                }
            }
        }
        
        false
    }
    
    /// Validate conversation messages for an agent
    fn validate_conversation(
        &self,
        transactions: &[Transaction],
        agent_id: Uuid,
        expected_messages: &[(String, MessagePattern)]
    ) -> Result<(), String> {
        let agent_messages: Vec<_> = transactions
            .iter()
            .filter_map(|tx| {
                if let Operation::Agent(AgentOperation::AddMessage { agent_id: aid, message }) = &tx.operation {
                    if *aid == agent_id {
                        Some((tx.created_at, message))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        
        // Sort by timestamp to ensure correct order
        let mut sorted_messages = agent_messages;
        sorted_messages.sort_by_key(|(timestamp, _)| *timestamp);
        
        if sorted_messages.len() != expected_messages.len() {
            return Err(format!(
                "Agent {} has {} messages, expected {}",
                agent_id, sorted_messages.len(), expected_messages.len()
            ));
        }
        
        for (idx, ((_, msg), (expected_role, expected_pattern))) in 
            sorted_messages.iter().zip(expected_messages.iter()).enumerate() 
        {
            let role = msg["role"].as_str().unwrap_or("");
            
            // Handle tool messages specially - validate both role and tool name
            if expected_role.starts_with("tool:") {
                if role != "tool" {
                    return Err(format!(
                        "Message {} for agent {}: expected tool message, got role '{}'",
                        idx, agent_id, role
                    ));
                }
                
                // Extract expected tool name from "tool:toolname" format
                let expected_tool = &expected_role[5..];
                let actual_tool = msg["name"].as_str().unwrap_or("");
                if actual_tool != expected_tool {
                    return Err(format!(
                        "Message {} for agent {}: expected tool '{}', got '{}'",
                        idx, agent_id, expected_tool, actual_tool
                    ));
                }
                
                // For tool messages, validate the result.message field
                let result_message = msg["result"]["message"].as_str().unwrap_or("");
                if !expected_pattern.matches(result_message) {
                    return Err(format!(
                        "Message {} for agent {}: tool result doesn't match pattern",
                        idx, agent_id
                    ));
                }
            } else {
                // For user and assistant messages, validate role and content normally
                if role != expected_role {
                    return Err(format!(
                        "Message {} for agent {}: expected role '{}', got '{}'",
                        idx, agent_id, expected_role, role
                    ));
                }
                
                let content = msg["content"].as_str().unwrap_or("");
                if !expected_pattern.matches(content) {
                    return Err(format!(
                        "Message {} for agent {}: content doesn't match pattern",
                        idx, agent_id
                    ));
                }
            }
        }
        
        Ok(())
    }
    
    /// Validate graph state by searching for content in WAL
    pub fn validate_graph_with_content_checks(
        &self,
        graph_id: &str,
        expected_blocks: &[(&str, Option<&str>)]
    ) -> Result<(), String> {
        let graph_uuid = Uuid::parse_str(graph_id)
            .map_err(|e| format!("Invalid graph ID: {}", e))?;
        
        // Open database and read transactions
        let all_transactions = WalReader::read_committed_transactions(&self.data_dir)?;
        let transactions = WalReader::find_by_graph(&all_transactions, graph_uuid);
        
        for (content, page_name) in expected_blocks {
            let found = transactions.iter().any(|tx| {
                if let Operation::Graph(GraphOperation::CreateBlock { 
                    content: c, 
                    page_name: p, 
                    .. 
                }) = &tx.operation {
                    c == content && p.as_deref() == *page_name
                } else {
                    false
                }
            });
            
            if !found {
                return Err(format!(
                    "Block with content '{}' on page {:?} not found in WAL",
                    content, page_name
                ));
            }
        }
        
        // Also validate using base expectations
        self.validate_all()?;
        
        Ok(())
    }
    
}

