//! Agent Validation Helpers
//! 
//! Automated test validation for agent state after operations.
//! Critical focus: Message ordering validation to ensure conversation integrity.
//! 
//! ## Usage Example
//! 
//! ```rust
//! use crate::common::agent_validation::AgentValidationFixture;
//! 
//! #[test]
//! fn test_agent_chat_commands() {
//!     let mut fixture = AgentValidationFixture::new();
//!     
//!     // Set up prime agent expectations
//!     let prime_id = fixture.expect_prime_agent();
//!     fixture.expect_authorization(&prime_id, &graph_id);
//!     
//!     // Send chat message
//!     send_command(json!({
//!         "type": "agent_chat",
//!         "message": "Hello agent"
//!     }));
//!     
//!     // Expect the conversation sequence
//!     fixture.expect_user_message(&prime_id, MessagePattern::Exact("Hello agent".to_string()));
//!     fixture.expect_assistant_message(&prime_id, MessagePattern::Contains("Hello".to_string()));
//!     
//!     // Validate everything
//!     fixture.validate_all(&data_dir);
//! }
//! ```

use std::fs;
use std::path::Path;
use std::collections::{HashMap, HashSet};
use serde_json::Value;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Validates that the agent registry has the correct schema and structure
pub fn validate_agent_registry_schema(data_dir: &Path) -> HashMap<String, Value> {
    let registry_path = data_dir.join("agent_registry.json");
    
    // Load and parse registry
    let registry_content = fs::read_to_string(&registry_path)
        .expect("Failed to read agent registry");
    let registry: Value = serde_json::from_str(&registry_content)
        .expect("Failed to parse agent registry");
    
    // Validate top-level structure
    assert!(registry["agents"].is_object(), "Registry must have 'agents' object");
    assert!(registry["active_agents"].is_array(), "Registry must have 'active_agents' array");
    
    // Extract agents for further validation
    let agents = registry["agents"].as_object().unwrap();
    let mut agent_map = HashMap::new();
    
    for (id_str, agent_info) in agents {
        // Validate agent entry schema
        assert_eq!(agent_info["id"].as_str(), Some(id_str.as_str()), 
            "Agent ID mismatch in registry");
        assert!(agent_info["name"].is_string(), 
            "Agent must have 'name' field");
        assert!(agent_info["created"].is_string(), 
            "Agent must have 'created' field");
        assert!(agent_info["last_active"].is_string(), 
            "Agent must have 'last_active' field");
        assert!(agent_info["data_path"].is_string(), 
            "Agent must have 'data_path' field");
        assert!(agent_info["authorized_graphs"].is_array(), 
            "Agent must have 'authorized_graphs' array");
        assert!(agent_info["is_prime"].is_boolean(), 
            "Agent must have 'is_prime' field");
        
        agent_map.insert(id_str.clone(), agent_info.clone());
    }
    
    // Validate active_agents references valid agents
    let active_agents = registry["active_agents"].as_array().unwrap();
    for active_id in active_agents {
        let id_str = active_id.as_str()
            .expect("active_agents must contain strings");
        assert!(agents.contains_key(id_str), 
            "active_agents references non-existent agent: {}", id_str);
    }
    
    // Validate prime agent uniqueness
    let prime_agents: Vec<_> = agents.values()
        .filter(|a| a["is_prime"].as_bool() == Some(true))
        .collect();
    assert!(prime_agents.len() <= 1, 
        "Multiple prime agents found, should be at most one");
    
    agent_map
}

/// Core agent state validator
pub struct AgentValidator {
    pub agent_data: Value,
    pub agent_id: Uuid,
    pub conversation_history: Vec<Value>,
}

impl AgentValidator {
    /// Load an agent from disk
    pub fn load(data_dir: &Path, agent_id: &Uuid) -> Self {
        let agent_path = data_dir.join("agents")
            .join(agent_id.to_string())
            .join("agent.json");
        
        let agent_content = fs::read_to_string(&agent_path)
            .unwrap_or_else(|_| panic!("Failed to read agent file for {}", agent_id));
        
        let agent_data: Value = serde_json::from_str(&agent_content)
            .unwrap_or_else(|_| panic!("Failed to parse agent JSON for {}", agent_id));
        
        // Validate basic structure
        assert_eq!(
            agent_data["id"].as_str(),
            Some(agent_id.to_string().as_str()),
            "Agent ID mismatch in file"
        );
        
        let conversation_history = agent_data["conversation_history"]
            .as_array()
            .expect("Agent must have conversation_history array")
            .to_vec();
        
        Self {
            agent_data,
            agent_id: *agent_id,
            conversation_history,
        }
    }
    
    /// Assert that a specific message matches expectations
    pub fn assert_message_at(&self, index: usize, expected: &ExpectedMessage) {
        let message = self.conversation_history.get(index)
            .unwrap_or_else(|| panic!(
                "No message at index {} (agent has {} messages)", 
                index, self.conversation_history.len()
            ));
        
        // Debug logging
        tracing::debug!("Checking message at index {}: role={:?}, expected={:?}", 
            index, 
            message["role"].as_str(),
            match expected {
                ExpectedMessage::User { .. } => "user",
                ExpectedMessage::Assistant { .. } => "assistant", 
                ExpectedMessage::Tool { .. } => "tool",
            }
        );
        
        match expected {
            ExpectedMessage::User { content, .. } => {
                assert_eq!(message["role"].as_str(), Some("user"),
                    "Expected user message at index {}", index);
                
                let actual_content = message["content"].as_str()
                    .expect("Message must have content");
                content.assert_matches(actual_content, &format!("User message at index {}", index));
            },
            ExpectedMessage::Assistant { content, .. } => {
                assert_eq!(message["role"].as_str(), Some("assistant"),
                    "Expected assistant message at index {}", index);
                
                let actual_content = message["content"].as_str()
                    .expect("Message must have content");
                content.assert_matches(actual_content, &format!("Assistant message at index {}", index));
            },
            ExpectedMessage::Tool { name, result_pattern } => {
                assert_eq!(message["role"].as_str(), Some("tool"),
                    "Expected tool message at index {}", index);
                assert_eq!(message["name"].as_str(), Some(name.as_str()),
                    "Expected tool '{}' at index {}", name, index);
                
                // Tool result is an AgentContext object with success, message, and optional data
                let result = &message["result"];
                assert!(result.is_object(), "Tool result must be an object at index {}", index);
                
                // Extract the message field from the result for pattern matching
                let actual_message = result["message"].as_str()
                    .expect("Tool result must have message field");
                result_pattern.assert_matches(actual_message, &format!("Tool result at index {}", index));
            },
        }
    }
    
    /// Validate that messages are properly ordered
    pub fn assert_message_ordering(&self) -> Result<(), String> {
        let validator = MessageOrderValidator::new(self.conversation_history.clone());
        validator.validate_timestamp_ordering()?;
        validator.validate_message_structure()?;
        validator.validate_integrity()?;
        Ok(())
    }
    
    /// Validate agent data fields match expectations
    pub fn assert_agent_fields(&self, expected: &ExpectedAgentState) {
        // Validate name
        assert_eq!(
            self.agent_data["name"].as_str(),
            Some(expected.name.as_str()),
            "Agent name mismatch for {}",
            self.agent_id
        );
        
        // Validate system prompt if present
        if let Some(ref prompt) = expected.system_prompt {
            assert_eq!(
                self.agent_data["system_prompt"].as_str(),
                Some(prompt.as_str()),
                "System prompt mismatch for {}",
                self.agent_id
            );
        }
        
        // Validate LLM config is MockLLM
        assert_eq!(
            self.agent_data["llm_config"]["type"].as_str(),
            Some("Mock"),
            "Agent {} should use MockLLM for testing",
            self.agent_id
        );
    }
    
    
    /// Get the number of messages in conversation history
    pub fn get_message_count(&self) -> usize {
        self.conversation_history.len()
    }
}

/// Expected message for validation
#[derive(Debug, Clone)]
pub enum ExpectedMessage {
    User { 
        content: MessagePattern,
    },
    Assistant { 
        content: MessagePattern,
    },
    Tool { 
        name: String, 
        result_pattern: MessagePattern,
    },
}

/// Pattern matching for message content
#[derive(Debug, Clone)]
pub enum MessagePattern {
    Exact(String),
    Contains(String),
}

impl MessagePattern {
    /// Assert that the pattern matches the actual content
    pub fn assert_matches(&self, actual: &str, context: &str) {
        match self {
            MessagePattern::Exact(expected) => {
                assert_eq!(actual, expected, "{}: Content mismatch", context);
            },
            MessagePattern::Contains(substring) => {
                assert!(actual.contains(substring), 
                    "{}: Expected content to contain '{}', got: '{}'" , context, substring, actual);
            },
        }
    }
}

/// Expected agent state
#[derive(Debug, Clone)]
pub struct ExpectedAgentState {
    pub name: String,
    pub is_active: bool,
    pub is_prime: bool,
    pub min_message_count: usize,
    pub max_message_count: Option<usize>,
    pub system_prompt: Option<String>,
}

/// Main test fixture for tracking expected agent transformations
pub struct AgentValidationFixture {
    /// Expected agents after all operations (keyed by agent_id)
    pub expected_agents: HashMap<Uuid, ExpectedAgentState>,
    
    /// Expected conversation sequences per agent (ORDER MATTERS!)
    pub expected_conversations: HashMap<Uuid, Vec<ExpectedMessage>>,
    
    /// Deleted agents that should NOT exist
    pub deleted_agents: HashSet<Uuid>,
    
    /// Expected authorizations (agent_id, graph_id pairs)
    pub expected_authorizations: HashSet<(Uuid, Uuid)>,
    
    /// Track which agent should be prime
    pub expected_prime_agent: Option<Uuid>,
}

impl AgentValidationFixture {
    pub fn new() -> Self {
        Self {
            expected_agents: HashMap::new(),
            expected_conversations: HashMap::new(),
            deleted_agents: HashSet::new(),
            expected_authorizations: HashSet::new(),
            expected_prime_agent: None,
        }
    }
    
    // Agent lifecycle expectations
    pub fn expect_agent_created(&mut self, id: Uuid, name: &str, is_prime: bool) -> &mut Self {
        self.expected_agents.insert(id, ExpectedAgentState {
            name: name.to_string(),
            is_active: true,  // New agents start active
            is_prime,
            min_message_count: 0,
            max_message_count: None,
            system_prompt: if is_prime {
                Some("You are the prime agent, a helpful assistant with full access to knowledge graphs.".to_string())
            } else {
                Some("An intelligent assistant".to_string())
            },
        });
        
        if is_prime {
            self.expected_prime_agent = Some(id);
        }
        
        // Remove from deleted set if it was there
        self.deleted_agents.remove(&id);
        self
    }
    
    pub fn expect_agent_deleted(&mut self, id: &Uuid) -> &mut Self {
        self.expected_agents.remove(id);
        self.expected_conversations.remove(id);
        self.deleted_agents.insert(*id);
        
        // Remove all authorizations for this agent
        self.expected_authorizations.retain(|(agent_id, _)| agent_id != id);
        self
    }
    
    pub fn expect_agent_activated(&mut self, id: &Uuid) -> &mut Self {
        if let Some(agent) = self.expected_agents.get_mut(id) {
            agent.is_active = true;
        }
        self
    }
    
    pub fn expect_agent_deactivated(&mut self, id: &Uuid) -> &mut Self {
        if let Some(agent) = self.expected_agents.get_mut(id) {
            agent.is_active = false;
        }
        self
    }
    
    // Message expectations (ORDER CRITICAL)
    pub fn expect_user_message(&mut self, agent_id: &Uuid, content: MessagePattern) -> &mut Self {
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push(ExpectedMessage::User { 
                content,
            });
        self
    }
    
    pub fn expect_assistant_message(&mut self, agent_id: &Uuid, content: MessagePattern) -> &mut Self {
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push(ExpectedMessage::Assistant { 
                content,
            });
        self
    }
    
    pub fn expect_tool_message(&mut self, agent_id: &Uuid, tool: &str, result: MessagePattern) -> &mut Self {
        self.expected_conversations
            .entry(*agent_id)
            .or_insert_with(Vec::new)
            .push(ExpectedMessage::Tool { 
                name: tool.to_string(), 
                result_pattern: result 
            });
        self
    }
    
    
    pub fn expect_chat_reset(&mut self, agent_id: &Uuid) -> &mut Self {
        // Clear expected conversation for this agent
        self.expected_conversations.insert(*agent_id, Vec::new());
        
        // Update message count expectations
        if let Some(agent) = self.expected_agents.get_mut(agent_id) {
            agent.min_message_count = 0;
            agent.max_message_count = Some(0);
        }
        self
    }
    
    // Authorization expectations
    pub fn expect_authorization(&mut self, agent_id: &Uuid, graph_id: &Uuid) -> &mut Self {
        self.expected_authorizations.insert((*agent_id, *graph_id));
        self
    }
    
    pub fn expect_deauthorization(&mut self, agent_id: &Uuid, graph_id: &Uuid) -> &mut Self {
        self.expected_authorizations.remove(&(*agent_id, *graph_id));
        self
    }
    
    /// Helper to set up prime agent expectations
    /// Takes the actual prime agent ID from the system
    pub fn expect_prime_agent(&mut self, prime_id: Uuid) -> &mut Self {
        self.expect_agent_created(prime_id, "Prime Agent", true);
        self
    }
    
    /// Main validation entry point
    pub fn validate_all(&self, data_dir: &Path) {
        // First validate the registry structure
        let registry_agents = validate_agent_registry_schema(data_dir);
        
        // Load the full registry for detailed checks
        let registry_path = data_dir.join("agent_registry.json");
        let registry_content = fs::read_to_string(&registry_path)
            .expect("Failed to read agent registry");
        let registry: Value = serde_json::from_str(&registry_content)
            .expect("Failed to parse agent registry");
        
        // Validate each expected agent
        for (agent_id, expected_state) in &self.expected_agents {
            // Check registry entry
            let id_str = agent_id.to_string();
            assert!(registry_agents.contains_key(&id_str),
                "Expected agent {} not found in registry", id_str);
            
            let agent_info = &registry_agents[&id_str];
            assert_eq!(agent_info["name"].as_str(), Some(expected_state.name.as_str()),
                "Agent name mismatch for {}", id_str);
            assert_eq!(agent_info["is_prime"].as_bool(), Some(expected_state.is_prime),
                "Agent prime status mismatch for {}", id_str);
            
            // Check active status
            let active_agents = registry["active_agents"].as_array().unwrap();
            let is_active = active_agents.iter()
                .any(|id| id.as_str() == Some(&id_str));
            assert_eq!(is_active, expected_state.is_active,
                "Agent active status mismatch for {}", id_str);
            
            // Load and validate agent file
            let validator = AgentValidator::load(data_dir, agent_id);
            
            // Validate message count
            let msg_count = validator.get_message_count();
            assert!(msg_count >= expected_state.min_message_count,
                "Agent {} has {} messages, expected at least {}",
                id_str, msg_count, expected_state.min_message_count);
            
            if let Some(max) = expected_state.max_message_count {
                assert!(msg_count <= max,
                    "Agent {} has {} messages, expected at most {}",
                    id_str, msg_count, max);
            }
            
            // Validate agent fields
            validator.assert_agent_fields(expected_state);
            
            // Validate conversation sequence if specified
            if let Some(expected_messages) = self.expected_conversations.get(agent_id) {
                self.validate_message_sequence(&validator, expected_messages);
            }
            
            // Validate message ordering
            validator.assert_message_ordering()
                .unwrap_or_else(|e| panic!("Message ordering validation failed for {}: {}", id_str, e));
        }
        
        // Validate deleted agents don't exist
        for agent_id in &self.deleted_agents {
            let id_str = agent_id.to_string();
            assert!(!registry_agents.contains_key(&id_str),
                "Deleted agent {} should not exist in registry", id_str);
            
            let agent_path = data_dir.join("agents")
                .join(&id_str)
                .join("agent.json");
            assert!(!agent_path.exists(),
                "Deleted agent {} should not have data file", id_str);
        }
        
        // Validate authorizations
        self.validate_authorizations(&registry);
        
        // Validate prime agent
        if let Some(expected_prime) = self.expected_prime_agent {
            let prime_id = registry["prime_agent_id"].as_str()
                .map(|s| Uuid::parse_str(s).ok())
                .flatten();
            assert_eq!(prime_id, Some(expected_prime),
                "Prime agent mismatch in registry");
        }
    }
    
    /// Validate message sequence matches expectations
    fn validate_message_sequence(&self, validator: &AgentValidator, expected: &[ExpectedMessage]) {
        tracing::debug!("Validating message sequence for agent {}: {} actual messages, {} expected",
            validator.agent_id, validator.conversation_history.len(), expected.len());
        
        assert_eq!(validator.conversation_history.len(), expected.len(),
            "Agent {} has {} messages, expected {}",
            validator.agent_id, validator.conversation_history.len(), expected.len());
        
        for (index, expected_msg) in expected.iter().enumerate() {
            validator.assert_message_at(index, expected_msg);
        }
    }
    
    /// Validate authorization relationships
    fn validate_authorizations(&self, registry: &Value) {
        let agents = registry["agents"].as_object().unwrap();
        
        for (agent_id, graph_id) in &self.expected_authorizations {
            let agent_id_str = agent_id.to_string();
            let graph_id_str = graph_id.to_string();
            
            // Check agent has the graph in its authorized list
            if let Some(agent_info) = agents.get(&agent_id_str) {
                let authorized_graphs = agent_info["authorized_graphs"]
                    .as_array()
                    .expect("Agent must have authorized_graphs array");
                
                let has_auth = authorized_graphs.iter()
                    .any(|g| g.as_str() == Some(&graph_id_str));
                
                assert!(has_auth,
                    "Agent {} should be authorized for graph {}",
                    agent_id_str, graph_id_str);
            } else {
                panic!("Agent {} not found when checking authorizations", agent_id_str);
            }
        }
    }
}

/// Message ordering validator (THE CRITICAL PIECE)
pub struct MessageOrderValidator {
    messages: Vec<Value>,
}

impl MessageOrderValidator {
    pub fn new(messages: Vec<Value>) -> Self {
        Self { messages }
    }
    
    /// Validate timestamps are strictly increasing
    pub fn validate_timestamp_ordering(&self) -> Result<(), String> {
        if self.messages.len() < 2 {
            return Ok(());  // Nothing to validate
        }
        
        let mut last_timestamp: Option<DateTime<Utc>> = None;
        
        for (index, message) in self.messages.iter().enumerate() {
            let timestamp_str = message["timestamp"].as_str()
                .ok_or_else(|| format!("Message at index {} missing timestamp", index))?;
            
            let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
                .map_err(|e| format!("Invalid timestamp at index {}: {}", index, e))?
                .with_timezone(&Utc);
            
            // Check it's not in the future
            if timestamp > Utc::now() {
                return Err(format!("Message at index {} has future timestamp: {}", index, timestamp));
            }
            
            // Check ordering
            if let Some(last) = last_timestamp {
                if timestamp < last {
                    return Err(format!(
                        "Message timestamps out of order at index {}: {} < {}",
                        index, timestamp, last
                    ));
                }
                
                // Warn if timestamps are identical (shouldn't happen in practice)
                if timestamp == last {
                    return Err(format!(
                        "Duplicate timestamp at index {}: {}",
                        index, timestamp
                    ));
                }
            }
            
            last_timestamp = Some(timestamp);
        }
        
        Ok(())
    }
    
    /// Validate basic message structure (simplified - no flow assumptions)
    pub fn validate_message_structure(&self) -> Result<(), String> {
        for (index, message) in self.messages.iter().enumerate() {
            let role = message["role"].as_str()
                .ok_or_else(|| format!("Message at index {} missing role", index))?;
            
            match role {
                "user" | "assistant" => {
                    // Just ensure basic fields exist
                },
                "tool" => {
                    // Tool messages should have required fields
                    if message["name"].as_str().is_none() {
                        return Err(format!("Tool message at index {} missing name", index));
                    }
                    if message["result"].is_null() {
                        return Err(format!("Tool message at index {} missing result", index));
                    }
                },
                _ => {
                    return Err(format!("Unknown message role '{}' at index {}", role, index));
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for duplicates or corruption
    pub fn validate_integrity(&self) -> Result<(), String> {
        // Check for exact duplicate messages (shouldn't happen)
        for i in 0..self.messages.len() {
            for j in (i + 1)..self.messages.len() {
                if self.messages[i] == self.messages[j] {
                    return Err(format!(
                        "Duplicate messages found at indices {} and {}",
                        i, j
                    ));
                }
            }
        }
        
        // Check each message has required fields
        for (index, message) in self.messages.iter().enumerate() {
            if message["role"].is_null() {
                return Err(format!("Message at index {} missing role", index));
            }
            
            let role = message["role"].as_str().unwrap();
            match role {
                "user" | "assistant" => {
                    if message["content"].is_null() {
                        return Err(format!("{} message at index {} missing content", role, index));
                    }
                },
                "tool" => {
                    if message["name"].is_null() || message["result"].is_null() {
                        return Err(format!("Tool message at index {} missing required fields", index));
                    }
                },
                _ => {}
            }
            
            if message["timestamp"].is_null() {
                return Err(format!("Message at index {} missing timestamp", index));
            }
        }
        
        Ok(())
    }
}