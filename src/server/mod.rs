//! Network layer for Cymbiont
//! 
//! This module contains HTTP API and WebSocket functionality for the
//! cymbiont-server binary.

pub mod http_api;
pub mod websocket;
pub mod kg_api;
pub mod server;

// Internal server utilities
pub use server::run_server_with_duration;