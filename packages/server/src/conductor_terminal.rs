//! Terminals opened on a node through the conductor (step 2.6, `docs/plans/assets/control-plane/conductor.md`
//! section 6). Bytes cross the node link only between an explicit open and its close, tagged with a ticket
//! (`conductor-links-carry-attention-metadata-only`). The conductor does not scrub or store them: each open terminal
//! passes through a bounded buffer, and a device that cannot keep up loses its terminal rather than growing memory.
use std::collections::HashMap;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::RngCore;
use tokio::sync::mpsc;

/// Open terminals per device.
pub const PER_DEVICE: usize = 4;
/// Chunks buffered per open terminal; a node sends at most `CHUNK` bytes at a time, so this is 64 KiB.
pub const BUFFER_CHUNKS: usize = 16;
pub const CHUNK: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The node is unknown, not enrolled or its link is down.
    NodeAway,
    TooMany,
}

struct Open {
    node: String,
    device: u64,
    out: mpsc::Sender<Vec<u8>>,
}

/// The tickets of open terminals. A ticket belongs to one node and one device connection: another node's output and
/// another device's keystrokes for it are ignored.
#[derive(Default)]
pub struct Terminals {
    open: Mutex<HashMap<String, Open>>,
}

impl Terminals {
    /// Registers a terminal for `device` on `node`. The receiver yields its output and ends when the terminal does.
    pub fn open(
        &self,
        device: u64,
        node: &str,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), OpenError> {
        let mut open = self.open.lock().unwrap();
        if open.values().filter(|o| o.device == device).count() >= PER_DEVICE {
            return Err(OpenError::TooMany);
        }
        let mut raw = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut raw);
        let ticket: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let (out, rx) = mpsc::channel(BUFFER_CHUNKS);
        open.insert(
            ticket.clone(),
            Open {
                node: node.to_string(),
                device,
                out,
            },
        );
        Ok((ticket, rx))
    }

    /// Whether `device` holds `ticket`, and on which node.
    pub fn node_of(&self, device: u64, ticket: &str) -> Option<String> {
        let open = self.open.lock().unwrap();
        open.get(ticket)
            .filter(|o| o.device == device)
            .map(|o| o.node.clone())
    }

    /// Output from `node` for `ticket`. Ignored unless the ticket is open on that node. Returns false when the
    /// terminal was dropped because the device's buffer was full or gone, so the caller can tell the node to stop.
    pub fn output(&self, node: &str, ticket: &str, data: &str) -> bool {
        let Ok(bytes) = B64.decode(data) else {
            return true;
        };
        let mut open = self.open.lock().unwrap();
        let Some(o) = open.get(ticket).filter(|o| o.node == node) else {
            return true;
        };
        for chunk in bytes.chunks(CHUNK) {
            if o.out.try_send(chunk.to_vec()).is_err() {
                open.remove(ticket);
                return false;
            }
        }
        true
    }

    /// Forgets `ticket` when `node` or `device` is its owner side; true if it was open.
    pub fn close(&self, ticket: &str, by_node: Option<&str>, by_device: Option<u64>) -> bool {
        let mut open = self.open.lock().unwrap();
        let allowed = open.get(ticket).is_some_and(|o| {
            by_node.is_none_or(|n| o.node == n) && by_device.is_none_or(|d| o.device == d)
        });
        allowed && open.remove(ticket).is_some()
    }

    /// Every ticket of a device that left; returns them with their nodes so each node can be told to stop.
    pub fn close_device(&self, device: u64) -> Vec<(String, String)> {
        let mut open = self.open.lock().unwrap();
        let gone: Vec<(String, String)> = open
            .iter()
            .filter(|(_, o)| o.device == device)
            .map(|(t, o)| (t.clone(), o.node.clone()))
            .collect();
        for (t, _) in &gone {
            open.remove(t);
        }
        gone
    }

    /// Every ticket on a node whose link dropped.
    pub fn close_node(&self, node: &str) {
        self.open.lock().unwrap().retain(|_, o| o.node != node);
    }

    pub fn count(&self) -> usize {
        self.open.lock().unwrap().len()
    }
}
