/**
 * @module logging
 * @description Custom logging configuration for clean console output
 * 
 * This module provides a custom tracing formatter that improves log readability by
 * conditionally displaying file location information. The goal is to reduce visual
 * noise in the console while preserving critical debugging information for errors.
 * 
 * ## Design Philosophy
 * 
 * Standard tracing output includes file:line information for all log levels, which
 * creates visual clutter during normal operation. This module implements a custom
 * formatter that only shows location information for ERROR and WARN levels, where
 * the specific code location is most valuable for debugging.
 * 
 * ## Emoji Convention for Permanent Logs
 * 
 * The codebase uses emojis to distinguish permanent production logs from temporary
 * debugging logs. This convention applies to INFO, DEBUG, and TRACE levels:
 * - Logs with emojis (🚀, 📊, 🔌, etc.) are intended for production
 * - Logs without emojis are typically temporary debugging aids
 * - ERROR and WARN logs are always kept regardless of emoji usage
 * 
 * This makes it easy to identify and clean up temporary logs before commits while
 * preserving important operational logs.
 * 
 * ## ConditionalLocationFormatter
 * 
 * A custom FormatEvent implementation that:
 * - Shows file:line for ERROR and WARN levels only
 * - Omits location information for INFO, DEBUG, and TRACE
 * - Preserves all other formatting (timestamps, levels, messages)
 * 
 * Example output:
 * ```
 * ERROR pkm_knowledge_graph::api:310: Failed to parse block data
 * WARN  pkm_knowledge_graph::utils:351: Could not parse datetime
 * INFO  pkm_knowledge_graph: 🚀 Server listening on 127.0.0.1:3000
 * DEBUG pkm_knowledge_graph: 📦 Processing batch of 25 pages
 * ```
 * 
 * ## Usage
 * 
 * The formatter is automatically applied when calling `init_logging()` in main.rs.
 * It integrates with the tracing ecosystem and respects RUST_LOG environment
 * variable settings.
 * 
 * ## Log Analysis Utilities
 * 
 * Log analysis utilities are provided in the separate `log_utils` module to avoid
 * circular dependencies. See `log_utils` for:
 * - Finding permanent logs with emojis
 * - Finding temporary logs without emojis
 * - Generating log analysis reports
 * 
 * These utilities can be run via CLI flags on the backend server.
 */

use tracing::{Level};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Custom formatter that conditionally shows file:line only for ERROR and WARN levels
pub struct ConditionalLocationFormatter;

impl<S, N> FormatEvent<S, N> for ConditionalLocationFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();
        let level = metadata.level();
        
        // Format level
        write!(&mut writer, "{}", level)?;
        
        // Only show module target and file:line for ERROR and WARN levels
        if matches!(level, &Level::ERROR | &Level::WARN) {
            write!(&mut writer, " {}", metadata.target())?;
            if let (Some(file), Some(line)) = (metadata.file(), metadata.line()) {
                write!(&mut writer, " {}:{}", file, line)?;
            }
        }
        
        write!(&mut writer, ": ")?;
        
        // Format all the spans in the event's span context
        if let Some(scope) = ctx.event_scope() {
            let mut first = true;
            for span in scope.from_root() {
                if !first {
                    write!(&mut writer, ":")?;
                }
                first = false;
                write!(writer, "{}", span.name())?;
                
                let ext = span.extensions();
                if let Some(fields) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    if !fields.is_empty() {
                        write!(writer, "{{{}}}", fields)?;
                    }
                }
            }
            write!(writer, " ")?;
        }
        
        // Write the event fields
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        
        writeln!(writer)
    }
}

/// Initialize the tracing subscriber with custom formatting
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .event_format(ConditionalLocationFormatter)
        .init();
}