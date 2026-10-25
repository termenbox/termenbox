//! Bridge Contract governance for one workspace (draft-05, vendored `bridge-contract`).
//!
//! A thin seam: the workspace's attach, detach and human-wait moments drive the bridge's product state machine
//! (`Attachment x Human x Lifecycle`), and the bridge's committed events project onto the existing `Block`
//! event of the reducer. Nothing here changes a wire shape (guardrail `protocol-core-stays-acp-v1`).
//!
//! The bridge keeps its log in an in-memory `MemStorage` (axiom A1 needs a durable `Storage`; see
//! `docs/plans/bridge-contract-crate.md`). `Bridge` is `Send`, the governor holds one workspace's client.

use std::sync::Arc;

use bridge_contract::{
    Attachment, Bridge, BridgeError, ClientKey, Clock, Config, Event, HmacVerifier, Human, Kind,
    MemStorage, SessionIdentity, SystemClock,
};

use crate::events::EventKind;

/// Where a workspace's client stands, as the bridge sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Attached,
    Detached,
    Resuming,
    Terminated,
}

pub struct Governor {
    bridge: Bridge,
    workspace: String,
    key_id: String,
}

impl Governor {
    /// Attaches `workspace` for `principal` with `scope`; the key registered here proves possession at
    /// re-attach (I11). `HmacVerifier` is the crate's stand-in until a real signature scheme is plugged in.
    pub fn attach(
        workspace: &str,
        principal: &str,
        scope: &[&str],
        key: &ClientKey,
    ) -> Result<Governor, BridgeError> {
        Self::attach_with(
            workspace,
            principal,
            scope,
            key,
            Arc::new(SystemClock),
            Config::default(),
        )
    }

    pub fn attach_with(
        workspace: &str,
        principal: &str,
        scope: &[&str],
        key: &ClientKey,
        clock: Arc<dyn Clock>,
        cfg: Config,
    ) -> Result<Governor, BridgeError> {
        let mut bridge = Bridge::new(MemStorage::new(), clock, Arc::new(HmacVerifier), cfg)?;
        let id = SessionIdentity {
            session_id: workspace.to_string(),
            principal_id: principal.to_string(),
            authority_scope: scope.iter().map(|s| s.to_string()).collect(),
            task_lineage: vec![],
        };
        bridge.attach(&id, key)?;
        Ok(Governor {
            bridge,
            workspace: workspace.to_string(),
            key_id: key.key_id.clone(),
        })
    }

    /// The client's connection dropped (a detach, not a kill).
    pub fn client_disconnected(&mut self, reason: &str) -> Result<(), BridgeError> {
        self.bridge
            .client_disconnect(&self.workspace, Some(&self.key_id), reason)
    }

    /// The agent waits for the human.
    pub fn human_wanted(&mut self, reason: &str) -> Result<(), BridgeError> {
        self.bridge.escalate_to_human(&self.workspace, reason)
    }

    pub fn human_answered(&mut self) -> Result<(), BridgeError> {
        self.bridge.human_resolve(&self.workspace)
    }

    /// Evaluates the detach, human and call timers; call on a tick.
    pub fn tick(&mut self) -> Result<(), BridgeError> {
        self.bridge.tick(Some(&self.workspace))
    }

    pub fn standing(&self) -> Standing {
        let cell = self.bridge.cell_of(&self.workspace);
        if cell.life == bridge_contract::Life::Terminated {
            return Standing::Terminated;
        }
        match cell.att {
            Attachment::Att => Standing::Attached,
            Attachment::Det => Standing::Detached,
            Attachment::Res => Standing::Resuming,
        }
    }

    pub fn human_pending(&self) -> bool {
        self.bridge.cell_of(&self.workspace).human == Human::Pend
    }

    /// The committed events after log position `from`, as reducer inputs. Only the human wait maps to an
    /// agent event; the rest of the governance log is audit data and stays out of the agent event stream.
    pub fn blocks_since(&self, from: u64) -> Vec<(u64, EventKind)> {
        self.bridge
            .events(&self.workspace)
            .iter()
            .filter(|e| e.sequence >= from)
            .filter_map(|e| project(&self.workspace, e).map(|k| (e.sequence, k)))
            .collect()
    }
}

fn project(workspace: &str, e: &Event) -> Option<EventKind> {
    let key = format!("bridge:{workspace}");
    match e.kind {
        Kind::HUMAN_PENDING => Some(EventKind::Block { key, open: true }),
        Kind::HUMAN_RESOLVED | Kind::HUMAN_TIMEOUT => Some(EventKind::Block { key, open: false }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_contract::FakeClock;

    fn governor(clock: Arc<FakeClock>) -> Governor {
        let key = ClientKey::new("k1", b"client-secret");
        Governor::attach_with(
            "ws1",
            "owner",
            &["read", "write"],
            &key,
            clock,
            Config::default(),
        )
        .unwrap()
    }

    #[test]
    fn disconnect_then_human_wait_projects_blocks() {
        let clock = FakeClock::new(1000.0);
        let mut g = governor(clock.clone());
        assert_eq!(g.standing(), Standing::Attached);
        g.client_disconnected("network").unwrap();
        assert_eq!(g.standing(), Standing::Detached);
        g.human_wanted("permission").unwrap();
        assert!(g.human_pending());
        g.human_answered().unwrap();
        assert!(!g.human_pending());
        let blocks = g.blocks_since(0);
        let opens: Vec<bool> = blocks
            .iter()
            .map(|(_, k)| match k {
                EventKind::Block { open, .. } => *open,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(opens, vec![true, false]);
    }

    #[test]
    fn detach_timeout_terminates() {
        let clock = FakeClock::new(1000.0);
        let mut g = governor(clock.clone());
        g.client_disconnected("network").unwrap();
        clock.advance(Config::default().t_detach + 1.0);
        g.tick().unwrap();
        assert_eq!(g.standing(), Standing::Terminated);
    }
}
