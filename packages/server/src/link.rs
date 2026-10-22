//! What the session logic sees of a connection, whatever carries it. Two lanes (decision 0096): JSON text
//! messages (status, control, everything but the terminal) and, once the client asked for it, raw terminal
//! bytes. A client that did not ask keeps one JSON pipe, with output as base64 inside it. Transports
//! (WebSocket, WebRTC data channels) are adapters that pump a real connection into a `Link`; dropping the
//! outgoing side tells the adapter to close gracefully.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use termenbox_core::protocol::ServerMessage;
use tokio::sync::{mpsc, Notify};

/// First byte of a binary frame that carries terminal bytes; re-exported from termenbox-core for convenience.
pub use termenbox_core::protocol::LANE_TERM;

/// What the client sent: a JSON message, or terminal input bytes from the `term` lane.
#[derive(Debug, PartialEq, Eq)]
pub enum Inbound {
    Json(String),
    Term(Vec<u8>),
}

/// Outgoing half. `wake` lets a transport that owns its own event loop (WebRTC) notice new output.
#[derive(Clone)]
pub struct LinkTx {
    status: mpsc::Sender<String>,
    term: mpsc::Sender<Vec<u8>>,
    lanes: Arc<AtomicBool>,
    wake: Option<Arc<Notify>>,
}

/// A reserved place for the next terminal output, so the session loop can wait for room without holding up
/// status messages that use the other queue.
pub enum OutPermit<'a> {
    Json(mpsc::Permit<'a, String>, Option<Arc<Notify>>),
    Term(mpsc::Permit<'a, Vec<u8>>, Option<Arc<Notify>>),
}

impl OutPermit<'_> {
    pub fn send(self, bytes: Vec<u8>) {
        let wake = match self {
            OutPermit::Json(p, wake) => {
                let msg = ServerMessage::Output {
                    data: B64.encode(bytes),
                };
                p.send(serde_json::to_string(&msg).unwrap());
                wake
            }
            OutPermit::Term(p, wake) => {
                p.send(bytes);
                wake
            }
        };
        if let Some(wake) = wake {
            wake.notify_one();
        }
    }
}

impl LinkTx {
    /// False when the transport is gone.
    pub async fn send(&self, text: String) -> bool {
        if self.status.send(text).await.is_err() {
            return false;
        }
        if let Some(wake) = &self.wake {
            wake.notify_one();
        }
        true
    }

    /// From now on terminal output travels as raw bytes on its own lane.
    pub fn enable_lanes(&self) {
        self.lanes.store(true, Ordering::Release);
    }

    pub fn lanes(&self) -> bool {
        self.lanes.load(Ordering::Acquire)
    }

    /// Waits for room for one output chunk; `None` when the transport is gone.
    pub async fn reserve_output(&self) -> Option<OutPermit<'_>> {
        if self.lanes() {
            let p = self.term.reserve().await.ok()?;
            Some(OutPermit::Term(p, self.wake.clone()))
        } else {
            let p = self.status.reserve().await.ok()?;
            Some(OutPermit::Json(p, self.wake.clone()))
        }
    }
}

pub struct Link {
    pub rx: mpsc::Receiver<Inbound>,
    pub tx: LinkTx,
    /// The real client address when the transport knows it, for the audit log.
    pub peer_ip: Option<String>,
}

/// The transport's side of a `Link`.
pub struct LinkEnd {
    pub to_session: mpsc::Sender<Inbound>,
    pub from_session: mpsc::Receiver<String>,
    pub term_from_session: mpsc::Receiver<Vec<u8>>,
}

pub fn pair(peer_ip: Option<String>, wake: Option<Arc<Notify>>) -> (Link, LinkEnd) {
    let (in_tx, in_rx) = mpsc::channel(32);
    let (out_tx, out_rx) = mpsc::channel(64);
    let (term_tx, term_rx) = mpsc::channel(64);
    (
        Link {
            rx: in_rx,
            tx: LinkTx {
                status: out_tx,
                term: term_tx,
                lanes: Arc::default(),
                wake,
            },
            peer_ip,
        },
        LinkEnd {
            to_session: in_tx,
            from_session: out_rx,
            term_from_session: term_rx,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn output_is_base64_json_until_the_lanes_are_on_then_raw_bytes() {
        let (l, mut end) = pair(None, None);
        l.tx.reserve_output().await.unwrap().send(b"hi".to_vec());
        let json = end.from_session.recv().await.unwrap();
        assert_eq!(json, r#"{"type":"output","data":"aGk="}"#);
        l.tx.enable_lanes();
        l.tx.reserve_output().await.unwrap().send(b"hi".to_vec());
        assert_eq!(end.term_from_session.recv().await.unwrap(), b"hi");
        assert!(end.from_session.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_full_terminal_queue_does_not_hold_up_status() {
        let (l, mut end) = pair(None, None);
        l.tx.enable_lanes();
        for _ in 0..64 {
            l.tx.reserve_output().await.unwrap().send(vec![0; 8]);
        }
        // The terminal queue is full: a new reservation waits, a status message does not.
        let blocked =
            tokio::time::timeout(std::time::Duration::from_millis(50), l.tx.reserve_output()).await;
        assert!(blocked.is_err());
        assert!(l.tx.send("status".into()).await);
        assert_eq!(end.from_session.recv().await.unwrap(), "status");
    }
}
