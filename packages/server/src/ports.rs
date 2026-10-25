//! The ports of the workspace protocol (decision 0136): an agent adapter, a window host and a transport. Each is a
//! seam where an implementation can be replaced without touching the others: a new agent integration, an own PTY host
//! instead of tmux, another carrier than WebRTC or WebSocket. Nothing implements or uses them yet; step 3.1 puts the
//! tmux and ACP-child hosts behind `WindowHost`, steps 4.x the three adapters behind `AgentAdapter`, and 3.3/3.4 the
//! three channels behind `Transport`.
//!
//! Messages cross the ports as `Frame`s, one JSON-RPC 2.0 message in ACP v1 (extensions in `_meta`, see
//! `termenbox_core::ext`). Typed decoding stays lenient and per message (decision 0111).
#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

/// One JSON-RPC 2.0 message of the protocol.
pub type Frame = Value;

/// How much an adapter can know about the agent (decision 0103). It never invents content a lower tier cannot see.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The agent speaks ACP itself.
    T0,
    /// Structured hook events.
    T1,
    /// Hook events plus transcript.
    T2,
    /// Scanning the screen.
    T3,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    /// Hosts one agent conversation, one ACP `sessionId`.
    Agent,
    /// A shell or any other program on the terminal byte channel.
    Terminal,
}

/// What a new window should be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSpec {
    pub kind: WindowKind,
    pub name: String,
    pub dir: Option<String>,
    /// The agent profile or program to start; the host decides what it may run (allowlist stays server-side).
    pub command: Option<String>,
}

/// A window as the host reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowEntry {
    /// Stable within the workspace for the window's life (a tmux pane id, an ACP child's id).
    pub id: String,
    pub kind: WindowKind,
    pub name: String,
    pub focused: bool,
    /// The ACP `sessionId` of an agent window once it has one.
    pub session_id: Option<String>,
}

/// Where windows live: tmux, an ACP child process, or something of our own later. Session code does not know which.
#[async_trait]
pub trait WindowHost: Send + Sync {
    async fn create(&self, workspace: &str, spec: WindowSpec) -> Result<WindowEntry>;
    async fn list(&self, workspace: &str) -> Result<Vec<WindowEntry>>;
    async fn focus(&self, workspace: &str, window: &str) -> Result<()>;
    async fn close(&self, workspace: &str, window: &str) -> Result<()>;
    async fn resize(&self, workspace: &str, window: &str, cols: u16, rows: u16) -> Result<()>;
}

/// Turns one agent window into the protocol, and the protocol back into input for the agent. The only place that
/// knows a particular agent.
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn tier(&self) -> Tier;
    /// Frames from the client for the agent window (`session/prompt`, `session/cancel`, a permission answer, ...).
    async fn send(&self, window: &str, frame: Frame) -> Result<()>;
    /// The frames the agent window produces (`session/update`, `session/request_permission`, ...). Ends when the
    /// window does.
    async fn subscribe(&self, window: &str) -> Result<mpsc::Receiver<Frame>>;
}

/// The carrier to one client, with the three channels of decision 0139. A transport may map them onto one connection
/// but must never let a bulk send hold up the control channel (guardrail `control-never-waits-behind-bulk`).
#[async_trait]
pub trait Transport: Send + Sync {
    /// Requests, responses and everything small and urgent.
    async fn send_control(&self, frame: Frame) -> Result<()>;
    /// Terminal output of one window.
    async fn send_bytes(&self, window: &str, data: &[u8]) -> Result<()>;
    /// Agent events of one conversation.
    async fn send_event(&self, session: &str, frame: Frame) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use termenbox_core::ext::{route, Negotiated, Registry, Route};

    /// An in-memory host: windows in a vector, the first agent window gets a session id.
    #[derive(Default)]
    struct FakeHost {
        windows: Mutex<Vec<WindowEntry>>,
    }

    #[async_trait]
    impl WindowHost for FakeHost {
        async fn create(&self, _workspace: &str, spec: WindowSpec) -> Result<WindowEntry> {
            let mut w = self.windows.lock().unwrap();
            let entry = WindowEntry {
                id: format!("%{}", w.len()),
                kind: spec.kind,
                name: spec.name,
                focused: w.is_empty(),
                session_id: (spec.kind == WindowKind::Agent).then(|| format!("sess-{}", w.len())),
            };
            w.push(entry.clone());
            Ok(entry)
        }
        async fn list(&self, _workspace: &str) -> Result<Vec<WindowEntry>> {
            Ok(self.windows.lock().unwrap().clone())
        }
        async fn focus(&self, _workspace: &str, window: &str) -> Result<()> {
            for w in self.windows.lock().unwrap().iter_mut() {
                w.focused = w.id == window;
            }
            Ok(())
        }
        async fn close(&self, _workspace: &str, window: &str) -> Result<()> {
            self.windows.lock().unwrap().retain(|w| w.id != window);
            Ok(())
        }
        async fn resize(
            &self,
            _workspace: &str,
            _window: &str,
            _cols: u16,
            _rows: u16,
        ) -> Result<()> {
            Ok(())
        }
    }

    /// Answers a prompt with one `session/update`, the way a T0 adapter would, and carries a tier extension.
    struct FakeAdapter {
        out: Mutex<Vec<Frame>>,
    }

    #[async_trait]
    impl AgentAdapter for FakeAdapter {
        fn name(&self) -> &str {
            "fake"
        }
        fn tier(&self) -> Tier {
            Tier::T0
        }
        async fn send(&self, _window: &str, frame: Frame) -> Result<()> {
            if frame["method"] == "session/prompt" {
                self.out.lock().unwrap().push(json!({
                    "jsonrpc": "2.0", "method": "session/update",
                    "params": {"sessionId": frame["params"]["sessionId"], "update": {
                        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"},
                        "_meta": {"termenbox.dev/tier": {"tier": "T0"}}}}
                }));
            }
            Ok(())
        }
        async fn subscribe(&self, _window: &str) -> Result<mpsc::Receiver<Frame>> {
            let (tx, rx) = mpsc::channel(8);
            for f in self.out.lock().unwrap().drain(..) {
                tx.try_send(f)?;
            }
            Ok(rx)
        }
    }

    #[derive(Default)]
    struct FakeTransport {
        control: Mutex<Vec<Frame>>,
        bytes: Mutex<Vec<(String, Vec<u8>)>>,
        events: Mutex<Vec<(String, Frame)>>,
    }

    #[async_trait]
    impl Transport for FakeTransport {
        async fn send_control(&self, frame: Frame) -> Result<()> {
            self.control.lock().unwrap().push(frame);
            Ok(())
        }
        async fn send_bytes(&self, window: &str, data: &[u8]) -> Result<()> {
            self.bytes
                .lock()
                .unwrap()
                .push((window.into(), data.to_vec()));
            Ok(())
        }
        async fn send_event(&self, session: &str, frame: Frame) -> Result<()> {
            self.events.lock().unwrap().push((session.into(), frame));
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_ports_compose_through_fakes() {
        let host: Box<dyn WindowHost> = Box::new(FakeHost::default());
        let adapter: Box<dyn AgentAdapter> = Box::new(FakeAdapter {
            out: Mutex::default(),
        });
        let transport = FakeTransport::default();

        // A workspace with a terminal and an agent window; focus moves; the agent window has a session id.
        let shell = host
            .create(
                "w",
                WindowSpec {
                    kind: WindowKind::Terminal,
                    name: "sh".into(),
                    dir: None,
                    command: None,
                },
            )
            .await
            .unwrap();
        let agent = host
            .create(
                "w",
                WindowSpec {
                    kind: WindowKind::Agent,
                    name: "claude".into(),
                    dir: Some("/src".into()),
                    command: Some("claude".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            (shell.focused, agent.focused, shell.session_id.clone()),
            (true, false, None)
        );
        host.focus("w", &agent.id).await.unwrap();
        host.resize("w", &agent.id, 100, 30).await.unwrap();
        let listed = host.list("w").await.unwrap();
        assert_eq!(
            listed.iter().map(|w| w.focused).collect::<Vec<_>>(),
            [false, true]
        );
        let session = agent.session_id.clone().unwrap();
        assert_eq!(adapter.tier(), Tier::T0);

        // Negotiate with a peer that offers the tier extension, then run a prompt through the adapter to the events channel.
        let tier_registry = Registry::new().with_name("tier", 1);
        let mut caps = serde_json::Map::new();
        tier_registry.advertise(&mut caps);
        let negotiated = tier_registry.negotiate(Some(&caps));
        let prompt = json!({"jsonrpc": "2.0", "id": 1, "method": "session/prompt", "params": {"sessionId": session}});
        assert_eq!(
            route(prompt["method"].as_str().unwrap(), true, &negotiated),
            Route::Core
        );
        transport
            .send_control(json!({"jsonrpc": "2.0", "id": 1, "result": {}}))
            .await
            .unwrap();
        adapter.send(&agent.id, prompt).await.unwrap();
        let mut rx = adapter.subscribe(&agent.id).await.unwrap();
        while let Ok(frame) = rx.try_recv() {
            transport.send_event(&session, frame).await.unwrap();
        }
        transport.send_bytes(&shell.id, b"$ ").await.unwrap();

        assert_eq!(transport.control.lock().unwrap().len(), 1);
        assert_eq!(
            transport.bytes.lock().unwrap()[0],
            ("%0".to_string(), b"$ ".to_vec())
        );
        let events = transport.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "sess-1");
        assert_eq!(
            events[0].1["params"]["update"]["_meta"]["termenbox.dev/tier"]["tier"],
            "T0"
        );

        host.close("w", &agent.id).await.unwrap();
        assert_eq!(host.list("w").await.unwrap().len(), 1);
        assert!(!Negotiated::default().supports("tier"));
    }
}
