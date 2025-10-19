//! Cymbiont Rust MCP Server - Entry point

mod client;
mod config;
mod error;
mod graphiti_launcher;
mod mcp_tools;
mod types;

use client::GraphitiClient;
use config::Config;
use mcp_tools::CymbiontService;
use rmcp::ServiceExt;
use tokio::io::{stdin, stdout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Config::load().unwrap_or_else(|_| {
        eprintln!("Warning: Failed to load config.yaml, using defaults");
        Config::default()
    });

    // Initialize file-only logging with verbosity monitoring (CRITICAL for MCP mode)
    let verbosity_config = autodebugger::VerbosityConfig {
        info_threshold: config.verbosity.info_threshold,
        debug_threshold: config.verbosity.debug_threshold,
        trace_threshold: config.verbosity.trace_threshold,
    };

    let file_config = autodebugger::RotatingFileConfig {
        log_directory: config.logging.log_directory.clone(),
        filename: "cymbiont_mcp.log".to_string(),
        max_files: config.logging.max_files,
        max_size_mb: config.logging.max_size_mb as u64,
        console_output: config.logging.console_output, // FALSE for MCP mode
        truncate_on_limit: true,
    };

    let verbosity_layer = autodebugger::init_logging_with_file(
        Some(&config.logging.level),
        Some(verbosity_config),
        None, // No custom output
        file_config,
    );

    tracing::info!("Cymbiont MCP server starting (version {})", env!("CARGO_PKG_VERSION"));

    // Construct Graphiti log path (in same directory as Cymbiont logs)
    let graphiti_log_path = std::path::PathBuf::from(&config.logging.log_directory)
        .join("graphiti_latest.log");

    // Ensure Graphiti backend is running (launch if needed, intentional resource leak)
    graphiti_launcher::ensure_graphiti_running(
        &config.graphiti.base_url,
        &config.graphiti.server_path,
        &graphiti_log_path,
    )
    .await?;

    // Create Graphiti HTTP client
    let client = GraphitiClient::new(&config.graphiti)?;
    tracing::info!("Graphiti client initialized (base_url: {})", config.graphiti.base_url);

    // Initialize document sync if corpus path is configured
    let sync_enabled = if let Some(corpus_path) = &config.corpus.path {
        tracing::info!("Corpus path configured: {}", corpus_path);

        // Start document sync watcher (hourly sync)
        match client
            .start_sync(
                corpus_path,
                config.corpus.sync_interval_hours,
                &config.graphiti.default_group_id,
            )
            .await
        {
            Ok(msg) => tracing::info!("Document sync watcher started: {}", msg),
            Err(e) => tracing::error!("Failed to start document sync watcher: {} (continuing without sync)", e),
        }

        // Trigger immediate sync on startup
        match client.trigger_sync().await {
            Ok(msg) => tracing::info!("Initial document sync triggered: {}", msg),
            Err(e) => tracing::error!("Failed to trigger initial sync: {}", e),
        }

        true
    } else {
        tracing::warn!("No corpus path configured - document sync disabled");
        tracing::warn!("To enable document sync, set 'corpus.path' in config.yaml");
        false
    };

    // Create Cymbiont MCP service
    let service = CymbiontService::new(client.clone(), config);

    // Start MCP server with stdio transport
    tracing::info!("Starting MCP server with stdio transport");
    let transport = (stdin(), stdout());
    let server = service.serve(transport).await?;

    // Wait for server shutdown
    let _quit_reason = server.waiting().await?;

    // Graceful shutdown: stop document sync if it was started
    if sync_enabled {
        tracing::info!("Shutting down document sync...");
        match client.stop_sync().await {
            Ok(msg) => tracing::info!("{}", msg),
            Err(e) => tracing::error!("Failed to stop document sync: {}", e),
        }
    }

    // Check for excessive logging and report
    if let Some(report) = verbosity_layer.check_and_report() {
        tracing::warn!("{}", report);
    }

    Ok(())
}
