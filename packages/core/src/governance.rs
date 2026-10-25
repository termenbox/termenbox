//! Bridge Contract governance for one workspace (draft-05, vendored `bridge-contract`).
//!
//! A thin seam: the workspace's attach, detach and human-wait moments drive the bridge's product state machine
//! (`Attachment x Human x Lifecycle`), and the bridge's committed events project onto the existing `Block`
//! event of the reducer. Nothing here changes a wire shape (guardrail `protocol-core-stays-acp-v1`).
//!
//! The bridge keeps its log in an in-memory `Storage` (axiom A1 needs a durable one; see
//! `docs/plans/bridge-contract-crate.md`) and is `!Send`: the server owns a governor on one task.

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use bridge_contract::{
    Attachment, Bridge, BridgeError, ClientKey, Clock, Config, Event, Human, Kind, SessionIdentity, Storage,
};

use crate::events::EventKind;

/// Wall clock in seconds since the epoch, the unit the bridge's timers use.
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }
}

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
}

impl Governor {
    /// Attaches `workspace` for `principal` with `scope`; `key_secret` stays with the server (I11).
    pub fn attach(
        workspace: &str,
        principal: &str,
        scope: &[&str],
        key: &ClientKey,
        mac_key: &[u8],
    ) -> Result<Governor, BridgeError> {
        Self::attach_with(workspace, principal, scope, key, mac_key, Rc::new(SystemClock), Config::default())
    }

    pub fn attach_with(
        workspace: &str,
        principal: &str,
        scope: &[&str],
        key: &ClientKey,
        mac_key: &[u8],
        clock: Rc<dyn Clock>,
        cfg: Config,
    ) -> Result<Governor, BridgeError> {
        let mut bridge = Bridge::new(Storage::new(mac_key), clock, cfg)?;
        let id = SessionIdentity {
            session_id: workspace.to_string(),
            principal_id: principal.to_string(),
            authority_scope: scope.iter().map(|s| s.to_string()).collect(),
            task_lineage: vec![],
        };
        bridge.attach(&id, key)?;
        Ok(Governor { bridge, workspace: workspace.to_string() })
    }

    /// The client's connection dropped (a detach, not a kill).
    pub fn client_disconnected(&mut self, reason: &str) -> Result<(), BridgeError> {
        self.bridge.client_disconnect(&self.workspace, reason)
    }

    /// The agent waits for the human.
    pub fn human_wanted(&mut self, reason: &str) -> Result<(), BridgeError> {
        self.bridge.escalate_to_human(&self.workspace, reason)
    }

    pub fn human_answered(&mut self) -> Result<(), BridgeError> {
        self.bridge.human_resolve(&self.workspace)
    }

    /// Evaluates the detach, human and token timers; call on a tick.
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

    fn governor(clock: Rc<FakeClock>) -> Governor {
        let key = ClientKey::new("k1", b"client-secret");
        Governor::attach_with("ws1", "owner", &["read", "write"], &key, b"mac-key-0123456789", clock, Config::default())
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
