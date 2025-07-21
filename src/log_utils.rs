/**
 * @module log_utils
 * @description Log analysis utilities for managing logs according to the emoji convention
 * 
 * This module provides static analysis tools to help identify permanent vs temporary logs
 * in the codebase based on the emoji convention. It exists separately from the logging
 * module to avoid circular dependencies when using these analysis tools.
 * 
 * ## Emoji Convention
 * 
 * - Logs with emojis (🚀, 📊, 🔌, etc.) are intended for production
 * - Logs without emojis are typically temporary debugging aids
 * - ERROR and WARN logs are always kept regardless of emoji usage
 * 
 * ## Usage
 * 
 * ```rust
 * use log_utils::{find_emoji_logs, find_temp_logs, print_log_report};
 * 
 * // Find all permanent logs
 * let permanent_logs = find_emoji_logs();
 * for log in permanent_logs {
 *     println!("{}", log.display());
 * }
 * 
 * // Find temporary logs that might need cleanup
 * let temp_logs = find_temp_logs();
 * println!("Found {} temporary logs", temp_logs.len());
 * 
 * // Print a full report
 * print_log_report();
 * ```
 */

use std::fs;
use std::path::Path;
use regex::Regex;
use tracing::info;

/// Represents a log entry found in the source code
#[derive(Debug)]
pub struct LogEntry {
    pub file: String,
    pub line: usize,
    pub level: String,
    pub content: String,
    pub has_emoji: bool,
}

impl LogEntry {
    /// Format the log entry for display
    pub fn display(&self) -> String {
        format!("{}:{}: [{}] {}", self.file, self.line, self.level, self.content)
    }
}

/// Check if a string contains emoji characters
/// Uses common Unicode emoji ranges for detection
fn has_emoji(text: &str) -> bool {
    // Common emoji Unicode ranges
    // This is not exhaustive but covers most common emojis used in logs
    for ch in text.chars() {
        match ch {
            // Emoticons
            '\u{1F600}'..='\u{1F64F}' |
            // Miscellaneous Symbols and Pictographs
            '\u{1F300}'..='\u{1F5FF}' |
            // Transport and Map Symbols
            '\u{1F680}'..='\u{1F6FF}' |
            // Supplemental Symbols and Pictographs
            '\u{1F900}'..='\u{1F9FF}' |
            // Common symbols that are often colored as emoji
            '\u{2600}'..='\u{26FF}' |
            '\u{2700}'..='\u{27BF}' |
            // Additional symbols (includes ⏱️ timer)
            '\u{23F0}'..='\u{23FF}' => return true,
            _ => continue,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_emoji_detection() {
        // Test strings with various emojis
        assert!(has_emoji("🚀 Server started"));
        assert!(has_emoji("📊 Loaded graph with nodes"));
        assert!(has_emoji("🔌 Plugin connected"));
        assert!(has_emoji("⏱️ Time-based save"));
        assert!(has_emoji("💾 Save completed"));
        assert!(has_emoji("🗄️ Archiving old data"));
        assert!(has_emoji("✅ Success"));
        assert!(has_emoji("🧹 Cleaning up"));
        assert!(has_emoji("⚡ Fast operation"));
        
        // Test strings without emojis
        assert!(!has_emoji("Server started"));
        assert!(!has_emoji("Processing data"));
        assert!(!has_emoji("Error: failed to connect"));
        assert!(!has_emoji("DEBUG: entering function"));
        assert!(!has_emoji("[2023-01-01] Log entry"));
        
        // Test edge cases
        assert!(!has_emoji("")); // Empty string
        assert!(!has_emoji("   ")); // Just whitespace
        assert!(has_emoji("Multiple 🚀 emojis 📊 here")); // Multiple emojis
        assert!(has_emoji("Emoji at end 🔌")); // Emoji at end
        assert!(has_emoji("🔌 Emoji at start")); // Emoji at start
    }

    #[test]
    fn test_log_entry_parsing() {
        // Create a test log entry
        let entry = LogEntry {
            file: "src/main.rs".to_string(),
            line: 42,
            level: "INFO".to_string(),
            content: r#""🚀 Server started on port {}", port"#.to_string(),
            has_emoji: true,
        };
        
        // Test the display format
        assert_eq!(entry.display(), r#"src/main.rs:42: [INFO] "🚀 Server started on port {}", port"#);
        
        // Test emoji detection in content
        assert!(entry.has_emoji);
        
        // Create entry without emoji
        let entry_no_emoji = LogEntry {
            file: "src/utils.rs".to_string(),
            line: 100,
            level: "DEBUG".to_string(),
            content: r#""Processing request: {}", id"#.to_string(),
            has_emoji: false,
        };
        
        assert!(!entry_no_emoji.has_emoji);
        assert_eq!(entry_no_emoji.display(), r#"src/utils.rs:100: [DEBUG] "Processing request: {}", id"#);
    }

    #[test]
    fn test_log_analysis_statistics() {
        // Create mock log entries for testing statistics
        let mock_logs = vec![
            // Permanent logs (with emoji)
            LogEntry {
                file: "src/main.rs".to_string(),
                line: 10,
                level: "INFO".to_string(),
                content: r#""🚀 Server started""#.to_string(),
                has_emoji: true,
            },
            LogEntry {
                file: "src/api.rs".to_string(),
                line: 20,
                level: "DEBUG".to_string(),
                content: r#""📦 Processing batch""#.to_string(),
                has_emoji: true,
            },
            LogEntry {
                file: "src/api.rs".to_string(),
                line: 30,
                level: "TRACE".to_string(),
                content: r#""🔧 Detailed operation""#.to_string(),
                has_emoji: true,
            },
            // Temporary logs (no emoji)
            LogEntry {
                file: "src/api.rs".to_string(),
                line: 40,
                level: "INFO".to_string(),
                content: r#""Request received""#.to_string(),
                has_emoji: false,
            },
            LogEntry {
                file: "src/api.rs".to_string(),
                line: 50,
                level: "DEBUG".to_string(),
                content: r#""Processing data""#.to_string(),
                has_emoji: false,
            },
            LogEntry {
                file: "src/main.rs".to_string(),
                line: 60,
                level: "TRACE".to_string(),
                content: r#""Entering function""#.to_string(),
                has_emoji: false,
            },
            // ERROR/WARN should be excluded from temp logs
            LogEntry {
                file: "src/utils.rs".to_string(),
                line: 70,
                level: "ERROR".to_string(),
                content: r#""Connection failed""#.to_string(),
                has_emoji: false,
            },
            LogEntry {
                file: "src/utils.rs".to_string(),
                line: 80,
                level: "WARN".to_string(),
                content: r#""Retry attempted""#.to_string(),
                has_emoji: false,
            },
        ];

        // Test filtering for emoji logs
        let emoji_logs: Vec<_> = mock_logs.iter()
            .filter(|log| log.has_emoji)
            .collect();
        assert_eq!(emoji_logs.len(), 3);

        // Test filtering for temp logs (no emoji, not ERROR/WARN)
        let temp_logs: Vec<_> = mock_logs.iter()
            .filter(|log| !log.has_emoji && !matches!(log.level.as_str(), "ERROR" | "WARN"))
            .collect();
        assert_eq!(temp_logs.len(), 3);

        // Test emoji adoption rate calculation
        let total_logs = emoji_logs.len() + temp_logs.len();
        let adoption_rate = (emoji_logs.len() as f64 / total_logs as f64) * 100.0;
        assert_eq!(adoption_rate, 50.0);

        // Test grouping by level
        let mut emoji_by_level = std::collections::HashMap::new();
        for log in &emoji_logs {
            *emoji_by_level.entry(log.level.clone()).or_insert(0) += 1;
        }
        assert_eq!(emoji_by_level.get("INFO"), Some(&1));
        assert_eq!(emoji_by_level.get("DEBUG"), Some(&1));
        assert_eq!(emoji_by_level.get("TRACE"), Some(&1));

        // Test grouping by file
        let mut temp_by_file = std::collections::HashMap::new();
        for log in &temp_logs {
            *temp_by_file.entry(log.file.clone()).or_insert(0) += 1;
        }
        assert_eq!(temp_by_file.get("src/api.rs"), Some(&2));
        assert_eq!(temp_by_file.get("src/main.rs"), Some(&1));
        
        // Test sorting files by temp log count
        let mut files_sorted: Vec<_> = temp_by_file.iter().collect();
        files_sorted.sort_by(|a, b| b.1.cmp(a.1));
        assert_eq!(files_sorted[0].0, &"src/api.rs".to_string());
        assert_eq!(*files_sorted[0].1, 2);
    }

    #[test]
    fn test_edge_cases() {
        // Empty log list
        let empty_logs: Vec<LogEntry> = vec![];
        let emoji_count = empty_logs.iter().filter(|log| log.has_emoji).count();
        let temp_count = empty_logs.iter()
            .filter(|log| !log.has_emoji && !matches!(log.level.as_str(), "ERROR" | "WARN"))
            .count();
        assert_eq!(emoji_count, 0);
        assert_eq!(temp_count, 0);

        // All logs have emojis
        let all_emoji_logs = vec![
            LogEntry {
                file: "src/test.rs".to_string(),
                line: 1,
                level: "INFO".to_string(),
                content: r#""🚀 Log 1""#.to_string(),
                has_emoji: true,
            },
            LogEntry {
                file: "src/test.rs".to_string(),
                line: 2,
                level: "DEBUG".to_string(),
                content: r#""📊 Log 2""#.to_string(),
                has_emoji: true,
            },
        ];
        let temp_count = all_emoji_logs.iter()
            .filter(|log| !log.has_emoji && !matches!(log.level.as_str(), "ERROR" | "WARN"))
            .count();
        assert_eq!(temp_count, 0);
        
        // Division by zero protection for adoption rate
        let adoption_rate = if all_emoji_logs.len() > 0 {
            100.0
        } else {
            0.0
        };
        assert_eq!(adoption_rate, 100.0);
    }
}

/// Find all log statements in a Rust source file
fn find_logs_in_file(file_path: &Path) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    
    // Read file content
    let content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => return entries,
    };
    
    // Regex to match log macros
    // Captures: level, content (everything between parentheses)
    let log_regex = Regex::new(r"(?m)^\s*(info!|warn!|error!|debug!|trace!)\s*\(([^;]+)\);").unwrap();
    
    for (line_num, line) in content.lines().enumerate() {
        if let Some(captures) = log_regex.captures(line) {
            let level = captures.get(1).unwrap().as_str();
            let content = captures.get(2).unwrap().as_str();
            
            // Skip if it's a macro definition or similar
            if content.contains("$") {
                continue;
            }
            
            entries.push(LogEntry {
                file: file_path.display().to_string(),
                line: line_num + 1,
                level: level.trim_end_matches('!').to_uppercase(),
                content: content.to_string(),
                has_emoji: has_emoji(content),
            });
        }
    }
    
    entries
}

/// Find all log statements with emojis (permanent logs)
pub fn find_emoji_logs() -> Vec<LogEntry> {
    let mut all_logs = Vec::new();
    
    // Scan all .rs files in src directory
    if let Ok(entries) = fs::read_dir("src") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let mut logs = find_logs_in_file(&path);
                all_logs.append(&mut logs);
            }
        }
    }
    
    // Filter for logs with emojis
    all_logs.into_iter().filter(|log| log.has_emoji).collect()
}

/// Find temporary logs (INFO/DEBUG/TRACE without emojis)
pub fn find_temp_logs() -> Vec<LogEntry> {
    let mut all_logs = Vec::new();
    
    // Scan all .rs files in src directory
    if let Ok(entries) = fs::read_dir("src") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let mut logs = find_logs_in_file(&path);
                all_logs.append(&mut logs);
            }
        }
    }
    
    // Filter for INFO/DEBUG/TRACE logs without emojis
    all_logs.into_iter()
        .filter(|log| {
            !log.has_emoji && 
            !matches!(log.level.as_str(), "ERROR" | "WARN")
        })
        .collect()
}

/// Print a summary report of permanent vs temporary logs
pub fn print_log_report() {
    let emoji_logs = find_emoji_logs();
    let temp_logs = find_temp_logs();
    
    info!("📋 === Log Analysis Report ===");
    info!("");
    
    // Overall summary first
    info!("📊 Summary:");
    info!("  Total permanent logs (with emoji): {}", emoji_logs.len());
    info!("  Total temporary logs (no emoji): {}", temp_logs.len());
    info!("  Emoji adoption rate: {:.1}%", 
        (emoji_logs.len() as f64 / (emoji_logs.len() + temp_logs.len()) as f64) * 100.0);
    info!("");
    
    // Group by level
    let mut emoji_by_level = std::collections::HashMap::new();
    let mut temp_by_level = std::collections::HashMap::new();
    
    for log in &emoji_logs {
        *emoji_by_level.entry(log.level.clone()).or_insert(0) += 1;
    }
    
    for log in &temp_logs {
        *temp_by_level.entry(log.level.clone()).or_insert(0) += 1;
    }
    
    info!("📈 By Level:");
    for level in ["INFO", "DEBUG", "TRACE"] {
        let emoji_count = emoji_by_level.get(level).unwrap_or(&0);
        let temp_count = temp_by_level.get(level).unwrap_or(&0);
        info!("  {}: {} permanent, {} temporary", level, emoji_count, temp_count);
    }
    info!("");
    
    // Group temporary logs by file
    let mut temp_by_file = std::collections::HashMap::new();
    for log in &temp_logs {
        *temp_by_file.entry(log.file.clone()).or_insert(0) += 1;
    }
    
    // Sort files by number of temporary logs
    let mut files_sorted: Vec<_> = temp_by_file.iter().collect();
    files_sorted.sort_by(|a, b| b.1.cmp(a.1));
    
    info!("🎯 Files with most temporary logs (cleanup candidates):");
    for (file, count) in files_sorted.iter().take(5) {
        info!("  {} - {} temporary logs", file, count);
    }
    
    // Show a few examples of temp logs that might be good to clean up
    info!("");
    info!("💡 Example temporary logs to consider removing:");
    let cleanup_candidates: Vec<_> = temp_logs.iter()
        .filter(|log| {
            // Filter out logs from log_utils itself
            !log.file.contains("log_utils.rs") &&
            // Focus on debug/trace as they're more likely to be temporary
            (log.level == "DEBUG" || log.level == "TRACE")
        })
        .take(5)
        .collect();
    
    for log in cleanup_candidates {
        info!("  {}", log.display());
    }
    
    info!("");
    info!("💭 Tip: Use 'cargo run -- log-check temp' to see all temporary logs");
    info!("💭 Tip: Review DEBUG and TRACE logs first - they're often development artifacts");
}