//! Network layer for Cymbiont
//! 
//! This module contains HTTP API and WebSocket functionality for the
//! cymbiont-server binary.

pub mod http_api;
pub mod websocket;
pub mod server;
pub mod auth;

// Internal server utilities
pub use server::run_server_with_duration;