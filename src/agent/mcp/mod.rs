//! MCP (Model Context Protocol) Server Implementation
//!
//! This module provides a JSON-RPC 2.0 compliant MCP server that exposes
//! Cymbiont's knowledge graph operations to AI agents like Claude Code.
//! It runs parallel to the existing WebSocket agent system and does not
//! replace it.
//!
//! ## Architecture
//!
//! The MCP server operates as a peer to other interfaces (HTTP, WebSocket),
//! allowing multiple AI systems to interact with Cymbiont simultaneously.
//! All tools are sourced from the canonical `agent/tools.rs` module.
//!
//! ## Components
//!
//! - **server**: MCP server implementation with stdio communication
//! - **protocol**: JSON-RPC 2.0 message types and MCP-specific methods
//!
//! ## Communication
//!
//! The server communicates via JSON-RPC 2.0 over stdio:
//! - Reads requests from stdin
//! - Writes responses to stdout
//! - Logs all debugging information to stderr
//!
//! ## Tool Mapping
//!
//! MCP tools are mapped to internal tool names:
//! - `cymbiont_add_block` → `add_block`
//! - `cymbiont_update_block` → `update_block`
//! - etc.

pub mod protocol;
pub mod server;