//! The conductor link (decision 0170, `docs/plans/assets/control-plane/conductor.md`): what a node and the
//! conductor say to each other. Attention metadata only (`conductor-links-carry-attention-metadata-only`): terminal
//! bytes cross only between an explicit `open_terminal` and its close, tagged with the ticket, and agent text is
//! capped and scrubbed. Every answer and acknowledgement names the `actor` and the
//! `owner` (`operator-actions-carry-an-actor-id-apart-from-the-owner`); a message without them does not parse.
//! Unknown variants fall back to `Unknown` so an older peer ignores what it does not know.
use serde::{Deserialize, Serialize};

use crate::events::{AskKind, AttentionKind, AttentionTier, FailureCause};

pub const MAX_PROMPT: usize = 512;
pub const MAX_TASK: usize = 160;
pub const MAX_NAME: usize = 64;
pub const MAX_DETAIL: usize = 256;
/// Rows in one `usage` message.
pub const MAX_USAGE_ROWS: usize = 64;

/// `s` without control characters, cut to at most `max` characters.
pub fn cap(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// One open ask of a node, as the conductor merges it. Identity at the conductor is
/// `(node, epoch, id)`; the node's own counter `id` restarts at 1 with every `epoch`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    pub id: String,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    pub kind: AttentionKind,
    pub raised_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
    /// Stable features for the class key (agent, tool kind, command shape), so clustering never reads the prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_features: Vec<String>,
    /// Task title the card shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The question; the only agent text on the link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// How loud the item is (decision 0193). Absent from an older node: it then means an ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<AttentionTier>,
    /// What a `needs_you` item waits for (decision 0190).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<AskKind>,
    /// Why a `failed` result ended (decision 0192).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<FailureCause>,
}

impl Obligation {
    /// A `result` waits to be viewed and never counts as an ask; everything else is an ask.
    pub fn is_ask(&self) -> bool {
        !matches!(self.tier, Some(AttentionTier::Result | AttentionTier::Info))
    }

    pub fn capped(mut self) -> Self {
        self.workspace = cap(&self.workspace, MAX_NAME);
        self.task = self.task.map(|t| cap(&t, MAX_TASK));
        self.prompt = self.prompt.map(|p| cap(&p, MAX_PROMPT));
        self.class_features = self
            .class_features
            .iter()
            .take(8)
            .map(|f| cap(f, MAX_NAME))
            .collect();
        self
    }
}

/// The state of one workspace, for the node summary.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceState {
    pub workspace: String,
    pub activity: crate::events::Activity,
}

/// Usage of one tab since the last report; numbers only.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageRow {
    pub workspace: String,
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cost_micro: u64,
    #[serde(default)]
    pub working_s: u64,
}

/// How an obligation left the node's list.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClearedBy {
    Fact,
    Ack,
    Answer,
    Rule,
    #[serde(other)]
    Unknown,
}

/// Node to conductor. `epoch` is picked by the node at process start; `seq` counts messages within it, so the
/// conductor applies in order, ignores a duplicate and asks for a `resync` on a gap.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeMessage {
    Hello {
        v: u32,
        /// The node's server id (ed25519 public key, base64url).
        node: String,
        name: String,
        epoch: u64,
        version: String,
        #[serde(default)]
        caps: Vec<String>,
        /// The one-time enrolment secret, first connection only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enrol: Option<String>,
        /// The node's signature over the conductor's challenge nonce, base64url.
        #[serde(default)]
        sig: String,
    },
    Snapshot {
        epoch: u64,
        items: Vec<Obligation>,
        #[serde(default)]
        workspaces: Vec<WorkspaceState>,
    },
    Raised {
        epoch: u64,
        seq: u64,
        obligation: Obligation,
    },
    Cleared {
        epoch: u64,
        seq: u64,
        id: String,
        by: ClearedBy,
    },
    /// The text of an open item arrived or changed after it was raised (decision 0190); same id, never an alert.
    Filled {
        epoch: u64,
        seq: u64,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask: Option<AskKind>,
    },
    Acked {
        epoch: u64,
        seq: u64,
        id: String,
        actor: String,
        owner: String,
    },
    Answered {
        epoch: u64,
        seq: u64,
        id: String,
        actor: String,
        owner: String,
        /// Device class (`phone`, `desktop`, `other`); never a name.
        device: String,
        option: String,
    },
    Usage {
        epoch: u64,
        seq: u64,
        rows: Vec<UsageRow>,
    },
    Incident {
        epoch: u64,
        seq: u64,
        cause: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        #[serde(default)]
        detail: String,
    },
    Health {
        epoch: u64,
        seq: u64,
        agents: u32,
        working: u32,
        waiting: u32,
        #[serde(default)]
        /// One-minute load average in hundredths.
        load: u32,
        #[serde(default)]
        queue_dropped: u32,
    },
    /// Sent to a standby conductor on dialling it: this node could not reach the active conductor `active` (its
    /// server id). One of the two views the standby needs before it takes over.
    PeerReport { active: String },
    /// Bytes of an open terminal (base64), from the node. Only after an `open_terminal` of the same `ticket`.
    TerminalOutput { ticket: String, data: String },
    /// The node ended an open terminal (the tab closed, the open was refused); nothing follows on this ticket.
    TerminalClosed { ticket: String },
    #[serde(other)]
    Unknown,
}

impl NodeMessage {
    /// The message with every free-text field capped and scrubbed and lists bounded; the conductor applies this to
    /// what it receives, a node applies it before it sends.
    pub fn capped(self) -> Self {
        match self {
            NodeMessage::Hello {
                v,
                node,
                name,
                epoch,
                version,
                caps,
                enrol,
                sig,
            } => NodeMessage::Hello {
                v,
                node,
                name: cap(&name, MAX_NAME),
                epoch,
                version: cap(&version, MAX_NAME),
                caps: caps.iter().take(16).map(|c| cap(c, MAX_NAME)).collect(),
                enrol,
                sig,
            },
            NodeMessage::Snapshot {
                epoch,
                items,
                workspaces,
            } => NodeMessage::Snapshot {
                epoch,
                items: items.into_iter().map(Obligation::capped).collect(),
                workspaces,
            },
            NodeMessage::Raised {
                epoch,
                seq,
                obligation,
            } => NodeMessage::Raised {
                epoch,
                seq,
                obligation: obligation.capped(),
            },
            NodeMessage::Filled {
                epoch,
                seq,
                id,
                prompt,
                ask,
            } => NodeMessage::Filled {
                epoch,
                seq,
                id,
                prompt: prompt.map(|p| cap(&p, MAX_PROMPT)),
                ask,
            },
            NodeMessage::Usage {
                epoch,
                seq,
                mut rows,
            } => {
                rows.truncate(MAX_USAGE_ROWS);
                NodeMessage::Usage { epoch, seq, rows }
            }
            NodeMessage::Incident {
                epoch,
                seq,
                cause,
                workspace,
                detail,
            } => NodeMessage::Incident {
                epoch,
                seq,
                cause: cap(&cause, MAX_NAME),
                workspace: workspace.map(|w| cap(&w, MAX_NAME)),
                detail: cap(&detail, MAX_DETAIL),
            },
            other => other,
        }
    }
}

/// Whether the owner has tapped a node in.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeStanding {
    Pending,
    Enrolled,
}

/// What the owner's device sees of one node of the fleet.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub node: String,
    pub name: String,
    pub standing: NodeStanding,
    pub connected: bool,
    /// Since when the node has been silent, ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silent_since: Option<u64>,
    pub open: u32,
    pub agents: u32,
    pub working: u32,
    pub waiting: u32,
    /// Items the conductor had to drop for this node's byte budget.
    pub dropped: u32,
}

/// One row of the merged stream, as a device sees it: the `card` id is the handle for an answer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CardInfo {
    pub card: String,
    pub node: String,
    pub node_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silent_since: Option<u64>,
    pub obligation: Obligation,
}

/// Where an answer goes on the node.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AnswerTarget {
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    pub prompt_id: String,
}

/// Conductor to node.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConductorMessage {
    Welcome {
        epoch_ack: u64,
        heartbeat_s: u32,
        /// The conductor's own epoch (standby takeover raises it); a node ignores a lower one.
        #[serde(default)]
        conductor_epoch: u64,
        /// The standby conductor's signed pairing bundle, so a node knows where to dial when the active is gone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        standby: Option<String>,
    },
    Resync,
    Answer {
        target: AnswerTarget,
        id: String,
        actor: String,
        owner: String,
        device: String,
        option: String,
        /// Redelivery after a reconnect applies once.
        idem: String,
    },
    Ack {
        id: String,
        actor: String,
        owner: String,
        idem: String,
    },
    OpenTerminal {
        ticket: String,
        workspace: String,
        tab: String,
        cols: u16,
        rows: u16,
    },
    CloseTerminal {
        ticket: String,
    },
    /// Keystrokes (base64) for an open terminal.
    TerminalInput {
        ticket: String,
        data: String,
    },
    ResizeTerminal {
        ticket: String,
        cols: u16,
        rows: u16,
    },
    Disable,
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obligation() -> Obligation {
        Obligation {
            id: "a1".into(),
            workspace: "api".into(),
            tab: Some("t1".into()),
            kind: AttentionKind::NeedsInput,
            raised_at: 1_700_000_000_000,
            urgency: None,
            class_features: vec!["claude".into(), "bash".into()],
            task: Some("fix the build".into()),
            prompt: Some("Run cargo test?".into()),
            tier: Some(AttentionTier::NeedsYou),
            ask: Some(AskKind::Permission),
            cause: None,
        }
    }

    fn round<T: Serialize + for<'a> Deserialize<'a> + PartialEq + std::fmt::Debug>(m: T) {
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<T>(&json).unwrap(), m, "{json}");
    }

    #[test]
    fn conductor_node_messages_round_trip() {
        round(NodeMessage::Hello {
            v: 1,
            node: "k".into(),
            name: "vps".into(),
            epoch: 7,
            version: "1.0".into(),
            caps: vec![],
            enrol: Some("s".into()),
            sig: "x".into(),
        });
        round(NodeMessage::Snapshot {
            epoch: 7,
            items: vec![obligation()],
            workspaces: vec![],
        });
        round(NodeMessage::Raised {
            epoch: 7,
            seq: 1,
            obligation: obligation(),
        });
        round(NodeMessage::Cleared {
            epoch: 7,
            seq: 2,
            id: "a1".into(),
            by: ClearedBy::Rule,
        });
        round(NodeMessage::Filled {
            epoch: 7,
            seq: 3,
            id: "a1".into(),
            prompt: Some("Run cargo test?".into()),
            ask: Some(AskKind::Permission),
        });
        round(NodeMessage::Acked {
            epoch: 7,
            seq: 3,
            id: "a1".into(),
            actor: "me".into(),
            owner: "me".into(),
        });
        round(NodeMessage::Answered {
            epoch: 7,
            seq: 4,
            id: "a1".into(),
            actor: "me".into(),
            owner: "me".into(),
            device: "phone".into(),
            option: "1".into(),
        });
        round(NodeMessage::Usage {
            epoch: 7,
            seq: 5,
            rows: vec![UsageRow::default()],
        });
        round(NodeMessage::Incident {
            epoch: 7,
            seq: 6,
            cause: "x".into(),
            workspace: None,
            detail: "d".into(),
        });
        round(NodeMessage::Health {
            epoch: 7,
            seq: 7,
            agents: 50,
            working: 3,
            waiting: 1,
            load: 50,
            queue_dropped: 0,
        });
    }

    #[test]
    fn conductor_messages_round_trip() {
        round(ConductorMessage::Welcome {
            epoch_ack: 7,
            heartbeat_s: 30,
            conductor_epoch: 1,
            standby: Some("bundle".into()),
        });
        round(ConductorMessage::Resync);
        round(ConductorMessage::Answer {
            target: AnswerTarget {
                workspace: "api".into(),
                tab: None,
                prompt_id: "p".into(),
            },
            id: "a1".into(),
            actor: "me".into(),
            owner: "me".into(),
            device: "desktop".into(),
            option: "2".into(),
            idem: "i".into(),
        });
        round(ConductorMessage::Ack {
            id: "a1".into(),
            actor: "me".into(),
            owner: "me".into(),
            idem: "i".into(),
        });
        round(ConductorMessage::OpenTerminal {
            ticket: "t".into(),
            workspace: "api".into(),
            tab: "t1".into(),
            cols: 80,
            rows: 24,
        });
        round(ConductorMessage::CloseTerminal { ticket: "t".into() });
        round(ConductorMessage::TerminalInput {
            ticket: "t".into(),
            data: "aGk=".into(),
        });
        round(ConductorMessage::ResizeTerminal {
            ticket: "t".into(),
            cols: 100,
            rows: 30,
        });
        round(NodeMessage::TerminalOutput {
            ticket: "t".into(),
            data: "aGk=".into(),
        });
        round(NodeMessage::TerminalClosed { ticket: "t".into() });
        round(ConductorMessage::Disable);
    }

    #[test]
    fn conductor_answer_without_actor_does_not_parse() {
        let no_actor = r#"{"type":"answer","target":{"workspace":"w","prompt_id":"p"},"id":"a1","owner":"me","device":"phone","option":"1","idem":"i"}"#;
        assert!(serde_json::from_str::<ConductorMessage>(no_actor).is_err());
        let no_owner = r#"{"type":"ack","id":"a1","actor":"me","idem":"i"}"#;
        assert!(serde_json::from_str::<ConductorMessage>(no_owner).is_err());
        let answered = r#"{"type":"answered","epoch":1,"seq":1,"id":"a1","owner":"me","device":"phone","option":"1"}"#;
        assert!(serde_json::from_str::<NodeMessage>(answered).is_err());
        let acked = r#"{"type":"acked","epoch":1,"seq":1,"id":"a1","actor":"me"}"#;
        assert!(serde_json::from_str::<NodeMessage>(acked).is_err());
    }

    #[test]
    fn conductor_peer_report_round_trips() {
        let m = NodeMessage::PeerReport { active: "k".into() };
        let t = serde_json::to_string(&m).unwrap();
        assert_eq!(t, r#"{"type":"peer_report","active":"k"}"#);
        assert_eq!(serde_json::from_str::<NodeMessage>(&t).unwrap(), m);
    }

    #[test]
    fn conductor_unknown_variants_are_ignored() {
        let m: NodeMessage = serde_json::from_str(r#"{"type":"from_the_future","x":1}"#).unwrap();
        assert_eq!(m, NodeMessage::Unknown);
        let m: ConductorMessage = serde_json::from_str(r#"{"type":"from_the_future"}"#).unwrap();
        assert_eq!(m, ConductorMessage::Unknown);
    }

    #[test]
    fn conductor_agent_text_is_capped_and_scrubbed() {
        let mut o = obligation();
        o.prompt = Some(format!("a\u{1b}[31mb\n{}", "x".repeat(2000)));
        o.task = Some("t".repeat(1000));
        let NodeMessage::Raised { obligation, .. } = (NodeMessage::Raised {
            epoch: 1,
            seq: 1,
            obligation: o,
        })
        .capped() else {
            panic!("variant changed");
        };
        let p = obligation.prompt.unwrap();
        assert_eq!(p.chars().count(), MAX_PROMPT);
        assert!(!p.chars().any(|c| c.is_control()));
        assert_eq!(obligation.task.unwrap().chars().count(), MAX_TASK);
    }

    #[test]
    fn conductor_usage_rows_are_bounded() {
        let rows = vec![UsageRow::default(); MAX_USAGE_ROWS + 10];
        let NodeMessage::Usage { rows, .. } = (NodeMessage::Usage {
            epoch: 1,
            seq: 1,
            rows,
        })
        .capped() else {
            panic!("variant changed");
        };
        assert_eq!(rows.len(), MAX_USAGE_ROWS);
    }
}
