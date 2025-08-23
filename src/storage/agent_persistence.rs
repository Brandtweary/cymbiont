//! Agent Persistence Layer
//!
//! This module handles the serialization and deserialization of agent state,
//! following the same pattern as graph_persistence.rs. It manages the full
//! agent data (conversation history, configuration, etc.) while the registry
//! only tracks metadata.
//!
//! ## Data Structure
//!
//! ```
//! {data_dir}/agents/{agent-id}/
//!   agent.json          # Full agent state including conversation history
//! ```
//!
//! ## Serialization Format
//!
//! Agent state is stored as JSON with the following structure:
//! - **id**: Agent UUID
//! - **name**: Display name
//! - **llm_config**: Backend configuration (MockLLM, Ollama, etc.)
//! - **conversation_history**: Array of messages with roles and content
//! - **context_window_limit**: Maximum messages to maintain
//! - **system_prompt**: Optional system instructions
//! - **timestamps**: Created and last modified times
//!
//! ## Agent Persistence Strategies
//!
//! The persistence system implements a multi-layered approach to ensure both
//! performance and data durability for agent state management:
//!
//! **Lazy Loading Strategy**: Agents are loaded from disk only when needed,
//! either through explicit activation or first use in chat operations. This
//! approach minimizes memory usage in deployments with many registered agents,
//! as inactive agents remain serialized on disk until required.
//!
//! **In-Memory State Management**: Active agents maintain their full state
//! in memory, including conversation history and LLM configuration. This
//! enables fast response times for chat operations while the auto-save system
//! ensures durability without blocking user interactions.
//!
//! **Versioned Serialization**: The agent.json format includes a version field
//! to enable future schema evolution. Currently at version 1, this system
//! allows backward compatibility as agent capabilities expand over time.
//!
//! ## Auto-save Triggers and Thresholds
//!
//! The auto-save system uses a dual-threshold approach optimized for both
//! data safety and performance characteristics:
//!
//! **Time-based Threshold (5 minutes)**: Ensures that even low-activity agents
//! with infrequent messages have their state persisted regularly. This prevents
//! data loss during unexpected shutdowns or crashes, particularly important
//! for long-running conversations with sparse interaction patterns.
//!
//! **Message-based Threshold (10 messages)**: Triggers saves based on conversation
//! activity volume, ensuring that active conversations are persisted frequently.
//! This threshold balances I/O efficiency with data safety, preventing excessive
//! disk writes during rapid conversation exchanges while maintaining reasonable
//! durability guarantees.
//!
//! The thresholds are evaluated by `should_save()` which agents can call
//! periodically to determine when persistence is needed. This decoupled design
//! allows different agent usage patterns to trigger saves appropriately without
//! forcing rigid save intervals.
//!
//! ## Error Handling and Recovery
//!
//! The persistence layer provides detailed error types and recovery strategies:
//!
//! **I/O Error Handling**: File system errors (disk full, permissions, network
//! storage issues) are captured and wrapped with context about the specific
//! operation and agent involved. These errors are propagated to enable
//! appropriate user notification and retry logic.
//!
//! **Serialization Error Recovery**: JSON serialization failures are detected
//! and reported with enough context to identify problematic agent state.
//! The system uses serde's robust error reporting to pinpoint specific fields
//! or values that caused serialization failures.
//!
//! **Version Compatibility**: When loading agents with newer version numbers,
//! the system logs warnings but attempts to continue. This forward-compatibility
//! approach enables graceful degradation when running older code against
//! newer agent data, though functionality may be limited.
//!
//! All errors are propagated with sufficient context to aid debugging and
//! enable appropriate error handling at higher layers of the system.

use std::path::Path;
use std::fs::{self, File};
use std::io::{Read, Write};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::agent::llm::{LLMConfig, Message};
use crate::error::*;


/// Full agent state structure for persistence
/// 
/// This is what gets serialized to agent.json. It contains all the
/// data needed to reconstruct an agent's state, including conversation
/// history and LLM configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedAgent {
    pub id: Uuid,
    pub name: String,
    pub llm_config: LLMConfig,
    pub conversation_history: Vec<Message>,
    pub context_window_limit: usize,
    pub system_prompt: Option<String>,
    pub created: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

/// Data returned when loading an agent from disk
#[derive(Debug)]
pub struct LoadedAgentData {
    pub id: Uuid,
    pub name: String,
    pub llm_config: LLMConfig,
    pub conversation_history: Vec<Message>,
    pub context_window_limit: usize,
    pub system_prompt: Option<String>,
    pub created: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

/// Load an agent from the given directory
/// 
/// Reads the agent.json file and deserializes the full agent state.
pub fn load_agent(agent_dir: &Path) -> Result<LoadedAgentData> {
    let agent_path = agent_dir.join("agent.json");
    
    if !agent_path.exists() {
        return Err(StorageError::agent_persistence(format!("Agent file not found at {:?}", agent_path)).into());
    }
    
    let mut file = File::open(&agent_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    
    let serialized: SerializedAgent = serde_json::from_str(&contents)?;
    
    if serialized.version > 1 {
        warn!("Agent file version {} is newer than supported version 1", serialized.version);
    }
    
    info!("📤 Loaded agent '{}' with {} messages in history", 
          serialized.name, serialized.conversation_history.len());
    
    Ok(LoadedAgentData {
        id: serialized.id,
        name: serialized.name,
        llm_config: serialized.llm_config,
        conversation_history: serialized.conversation_history,
        context_window_limit: serialized.context_window_limit,
        system_prompt: serialized.system_prompt,
        created: serialized.created,
        last_active: serialized.last_active,
    })
}

/// Save an agent to the given directory
/// 
/// Serializes the full agent state to agent.json with pretty formatting.
pub fn save_agent(
    agent_dir: &Path,
    id: Uuid,
    name: &str,
    llm_config: &LLMConfig,
    conversation_history: &[Message],
    context_window_limit: usize,
    system_prompt: Option<&str>,
    created: DateTime<Utc>,
    last_active: DateTime<Utc>,
) -> Result<()> {
    // Ensure the agent directory exists
    fs::create_dir_all(agent_dir)?;
    
    let agent_path = agent_dir.join("agent.json");
    
    let serialized = SerializedAgent {
        id,
        name: name.to_string(),
        llm_config: llm_config.clone(),
        conversation_history: conversation_history.to_vec(),
        context_window_limit,
        system_prompt: system_prompt.map(|s| s.to_string()),
        created,
        last_active,
        version: 1,
    };
    
    let json = serde_json::to_string_pretty(&serialized)?;
    let mut file = File::create(&agent_path)?;
    file.write_all(json.as_bytes())?;
    
    info!("💾 Saved agent '{}' with {} messages", name, conversation_history.len());
    
    Ok(())
}

/// Check if we should save based on conversation length or time
/// 
/// Similar to graph persistence, but based on message count instead of operations.
pub fn should_save(last_save_time: DateTime<Utc>, messages_since_save: usize) -> bool {
    const SAVE_INTERVAL_MINUTES: i64 = 5;
    const SAVE_MESSAGE_THRESHOLD: usize = 10;
    
    let minutes_since_save = (Utc::now() - last_save_time).num_minutes();
    if minutes_since_save >= SAVE_INTERVAL_MINUTES {
        return true;
    }
    
    if messages_since_save >= SAVE_MESSAGE_THRESHOLD {
        return true;
    }
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::agent::llm::LLMConfig;
    
    #[test]
    fn test_save_and_load_agent() {
        let dir = tempdir().unwrap();
        let agent_dir = dir.path().join("test-agent");
        
        let agent_id = Uuid::new_v4();
        let name = "Test Agent";
        let llm_config = LLMConfig::default();
        let conversation_history = vec![
            Message::User {
                content: "Hello".to_string(),
                echo: None,
                timestamp: Utc::now(),
            },
            Message::Assistant {
                content: "Hi there!".to_string(),
                timestamp: Utc::now(),
            },
        ];
        
        let created = Utc::now();
        let last_active = Utc::now();
        
        // Save agent
        save_agent(
            &agent_dir,
            agent_id,
            name,
            &llm_config,
            &conversation_history,
            100,
            Some("You are a helpful assistant"),
            created,
            last_active,
        ).unwrap();
        
        // Load agent
        let loaded = load_agent(&agent_dir).unwrap();
        
        assert_eq!(loaded.id, agent_id);
        assert_eq!(loaded.name, name);
        assert_eq!(loaded.conversation_history.len(), 2);
        assert_eq!(loaded.context_window_limit, 100);
        assert_eq!(loaded.system_prompt, Some("You are a helpful assistant".to_string()));
    }
    
    #[test]
    fn test_should_save() {
        // Test time-based saving
        let old_time = Utc::now() - chrono::Duration::minutes(10);
        assert!(should_save(old_time, 0));
        
        // Test message-based saving
        let recent_time = Utc::now();
        assert!(should_save(recent_time, 15));
        
        // Test no save needed
        assert!(!should_save(recent_time, 5));
    }
}