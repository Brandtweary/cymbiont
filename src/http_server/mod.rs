//! Network layer for Cymbiont
//!
//! This module contains HTTP API and WebSocket functionality for the
//! cymbiont-server binary.

#![allow(clippy::module_inception)]

pub mod auth;
pub mod http_api;
pub mod server;
pub mod websocket;
pub mod websocket_commands;
pub mod websocket_utils;

// Internal server utilities
pub use server::{cleanup_server_info, start_server};
