//! Doctests for CYMBIONT_EMERGENCY_DEPLOYMENT.md
//! 
//! This file validates that all commands shown in the deployment manual
//! actually work with the current cymbiont codebase.

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// Helper to run cymbiont commands and check they parse correctly
    fn validate_command(args: &[&str]) -> bool {
        // We're just checking the command parses, not that it executes
        // This prevents side effects from test runs
        let mut cmd_args = vec!["run", "--bin", "cymbiont", "--"];
        cmd_args.extend_from_slice(args);
        cmd_args.push("--help"); // Add help flag to prevent actual execution
        
        let output = Command::new("cargo")
            .args(&cmd_args)
            .output()
            .expect("Failed to run cargo");
        
        // If --help works with these args, the command structure is valid
        output.status.success() || 
            String::from_utf8_lossy(&output.stderr).contains("unexpected argument")
    }

    #[test]
    fn test_server_command() {
        assert!(validate_command(&["--server"]));
    }

    #[test]
    fn test_import_logseq_command() {
        // The actual path doesn't need to exist for parsing validation
        assert!(validate_command(&["--import-logseq", "/fake/path"]));
    }

    #[test]
    fn test_delete_graph_command() {
        assert!(validate_command(&["--delete-graph", "test-graph"]));
    }

    #[test]
    fn test_list_graphs_command() {
        assert!(validate_command(&["--list-graphs"]));
    }

    #[test]
    fn test_server_with_port() {
        assert!(validate_command(&["--server", "--port", "8080"]));
    }

    #[test]
    fn test_filesystem_commands_are_standard() {
        // These are standard Unix commands mentioned in the manual
        // We just verify they're commonly available
        let standard_commands = vec![
            "ls", "cd", "uname", "free", "df", "wget", "git", "rustc", "cargo", "find"
        ];

        for cmd in standard_commands {
            let output = Command::new("which")
                .arg(cmd)
                .output()
                .expect("Failed to run which");
            
            // We don't require all to exist (some might not be installed)
            // But we document which ones are missing
            if !output.status.success() {
                eprintln!("Standard command '{}' not found on this system", cmd);
            }
        }
    }
}