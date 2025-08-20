// CLI Command Registry Module
//
// This module defines the registry of all CLI commands as a simple enum.
// It's separated from the main CLI module to allow easy importing in tests
// without pulling in execution dependencies (AppState, GraphOps, etc.).

/// Registry of all CLI commands with their metadata
#[derive(Debug, Clone)]
pub enum CliCommand {
    ImportLogseq { path: String },
    DeleteGraph { identifier: String },
    CreateAgent { name: String, description: Option<String> },
    DeleteAgent { identifier: String },
    ActivateAgent { identifier: String },
    DeactivateAgent { identifier: String },
    AgentInfo { identifier: String },
    AuthorizeAgent { agent: String, graph: String },
    DeauthorizeAgent { agent: String, graph: String },
}

impl CliCommand {
    /// Returns whether each command variant has been integration tested
    /// Developers must update this when adding new commands and tests
    #[cfg(test)]
    pub fn is_tested(command_name: &str) -> bool {
        match command_name {
            "import_logseq" => true,      // Tested in cli_commands.rs
            "delete_graph" => true,        // Tested in cli_commands.rs  
            "create_agent" => true,        // Tested in cli_commands.rs
            "delete_agent" => true,        // Tested in cli_commands.rs
            "activate_agent" => true,      // Tested in cli_commands.rs
            "deactivate_agent" => true,    // Tested in cli_commands.rs
            "agent_info" => true,          // Tested in cli_commands.rs
            "authorize_agent" => true,     // Tested in cli_commands.rs
            "deauthorize_agent" => true,   // Tested in cli_commands.rs
            _ => false,
        }
    }
    
    /// Get all command names for iteration
    #[cfg(test)]
    pub fn all_commands() -> Vec<&'static str> {
        vec![
            "import_logseq",
            "delete_graph",
            "create_agent",
            "delete_agent",
            "activate_agent",
            "deactivate_agent",
            "agent_info",
            "authorize_agent",
            "deauthorize_agent",
        ]
    }
    
    /// Parse a command from JSON parameters (for WebSocket bridge)
    pub fn from_json(command_name: &str, params: &serde_json::Value) -> Result<Self, String> {
        match command_name {
            "import_logseq" => Ok(Self::ImportLogseq {
                path: params["path"].as_str()
                    .ok_or("Missing path")?.to_string()
            }),
            "delete_graph" => Ok(Self::DeleteGraph {
                identifier: params["identifier"].as_str()
                    .ok_or("Missing identifier")?.to_string()
            }),
            "create_agent" => Ok(Self::CreateAgent {
                name: params["name"].as_str()
                    .ok_or("Missing name")?.to_string(),
                description: params["description"].as_str().map(|s| s.to_string()),
            }),
            "delete_agent" => Ok(Self::DeleteAgent {
                identifier: params["identifier"].as_str()
                    .ok_or("Missing identifier")?.to_string()
            }),
            "activate_agent" => Ok(Self::ActivateAgent {
                identifier: params["identifier"].as_str()
                    .ok_or("Missing identifier")?.to_string()
            }),
            "deactivate_agent" => Ok(Self::DeactivateAgent {
                identifier: params["identifier"].as_str()
                    .ok_or("Missing identifier")?.to_string()
            }),
            "agent_info" => Ok(Self::AgentInfo {
                identifier: params["identifier"].as_str()
                    .ok_or("Missing identifier")?.to_string()
            }),
            "authorize_agent" => Ok(Self::AuthorizeAgent {
                agent: params["agent"].as_str()
                    .ok_or("Missing agent")?.to_string(),
                graph: params["graph"].as_str()
                    .ok_or("Missing graph")?.to_string(),
            }),
            "deauthorize_agent" => Ok(Self::DeauthorizeAgent {
                agent: params["agent"].as_str()
                    .ok_or("Missing agent")?.to_string(),
                graph: params["graph"].as_str()
                    .ok_or("Missing graph")?.to_string(),
            }),
            _ => Err(format!("Unknown command: {}", command_name)),
        }
    }
    
}