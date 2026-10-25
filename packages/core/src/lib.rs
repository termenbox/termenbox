//! Termenbox Core - Shared Protocol Types
//!
//! Minimal JSON-serializable types for WebSocket protocol + HTTP registration.

pub mod bundle;
pub mod events;
pub mod ext;
pub mod protocol;

pub use protocol::*;
