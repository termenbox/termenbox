//! Termenbox Core - Shared Protocol Types
//!
//! Minimal JSON-serializable types for WebSocket protocol + HTTP registration.

pub mod bundle;
pub mod conductor;
pub mod events;
pub mod ext;
pub mod governance;
pub mod protocol;

pub use protocol::*;
