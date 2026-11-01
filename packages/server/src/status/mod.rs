//! The per-session status machine (decisions 0187 to 0193): typed events and a clock in, a status and Attention
//! effects out, no I/O. Evidence sources emit `Event`s; nothing sets a status directly.
//!
//! Not yet read by the server: the adapters (B.3), the transcript (B.4) and the pane watcher (B.5) feed it, and B.6
//! retires the paths it replaces.
#![allow(dead_code)]

pub mod event;
pub mod machine;

#[cfg(test)]
mod transitions;
