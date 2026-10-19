//! The agent event model (plan v6, decision 0061): what each agent does, as one small typed stream.
//! Every enum has an `Unknown` fallback so an older peer ignores kinds and values it does not know.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Working,
    NeedsInput,
    Done,
    Error,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolPhase {
    Started,
    Finished,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Hook,
    Codex,
    Scan,
    Screen,
    /// An ACP session the server started itself (tier T0, decision 0111).
    Acp,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    pub text: String,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    State {
        state: AgentState,
    },
    Tool {
        name: String,
        summary: String,
        phase: ToolPhase,
    },
    Plan {
        items: Vec<PlanItem>,
    },
    Prompt {
        id: String,
        prompt_kind: String,
        choices: Vec<String>,
        /// T0 only: the options the agent offered, answered by id (decision 0111). Absent from older servers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision: Option<Decision>,
    },
    Message {
        summary: String,
    },
    /// The current task of the session, from the first line of the prompt (decision 0095).
    Task {
        title: String,
    },
    /// Context use of an ACP session; the percentage is `used / size`.
    Usage {
        used: u64,
        size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<String>,
    },
    /// The agent's own title for the session.
    Title {
        title: String,
    },
    /// What the agent is thinking, capped like a message.
    Thought {
        summary: String,
    },
    /// The mode the agent is in now.
    Mode {
        mode: String,
    },
    /// The slash commands the agent offers.
    Commands {
        names: Vec<String>,
    },
    /// A turn (one prompt and all the agent does until it answers) opened or closed. An input to the server's
    /// reducer, which derives the state from it: never stored and never sent on to a client.
    Turn {
        open: bool,
        /// How a closed turn ended: `done`, `error` or `idle`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<AgentState>,
    },
    /// Something that waits for the human (a permission, a question, an elicitation) opened or closed, named by
    /// `key`. Like `Turn`, an input to the reducer only.
    Block {
        key: String,
        open: bool,
    },
    #[serde(other)]
    Unknown,
}

/// One option of a permission request. Only `allow_once` and `reject_once` are ever carried (decision 0104).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DecisionOption {
    pub id: String,
    pub kind: String,
    pub name: String,
}

/// What a T0 prompt asks and how it is answered: by the id of exactly one of `options`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub options: Vec<DecisionOption>,
    /// A one-line summary of the tool the agent wants to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `seq` is a per-session counter and `ts` the server's clock (ms since the epoch), both stamped by the
/// server and never taken from the source.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub session: String,
    pub seq: u64,
    pub ts: u64,
    pub source: Source,
    pub confidence: Confidence,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// The latest state of one session, for a client that has just connected.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionAgentState {
    pub session: String,
    pub state: AgentState,
    pub seq: u64,
    /// No event for a while although the state is `working`: the state may be out of date.
    pub stale: bool,
    pub ts: u64,
    /// The agent in the session (`claude`, `codex`), from the process scan; absent for a plain shell and
    /// from older servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// When the agent last answered (ms since the epoch): a finished turn, a reply message, or a working-to-idle
    /// change. Absent until it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_response_at: Option<u64>,
    /// Start of the continuous needs-input/error wait; activity and state refreshes do not advance it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<u64>,
}

/// What an agent does now (decision 0095). `Unknown` also covers a `working` signal that went quiet for 60 s.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Working,
    Blocked,
    Idle,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    NeedsInput,
    FinishedUnseen,
    Failed,
    #[serde(other)]
    Unknown,
}

/// The current piece of work of a session: the first line of the prompt, or the plan item in progress.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub title: String,
    /// ms since the epoch, on the server's clock.
    pub started_at: u64,
    pub source: Source,
}

/// A durable, server-owned item that asks for the owner (decision 0095).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    pub id: String,
    pub session: String,
    pub kind: AttentionKind,
    pub raised_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    /// The question of a `needs_input`; never sent in a push.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// The derived Activity of one session with its task, as the status channel carries it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionActivity {
    pub session: String,
    pub activity: Activity,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// What scan or screen suspect without a hook to confirm it: low confidence, never pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<AttentionKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_with_the_wire_names_the_client_reads() {
        let e = AgentEvent {
            session: "s".into(),
            seq: 3,
            ts: 9,
            source: Source::Hook,
            confidence: Confidence::High,
            kind: EventKind::Tool {
                name: "Bash".into(),
                summary: "ls".into(),
                phase: ToolPhase::Started,
            },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "tool");
        assert_eq!(v["phase"], "started");
        assert_eq!(v["source"], "hook");
        assert_eq!(serde_json::from_value::<AgentEvent>(v).unwrap(), e);
    }

    #[test]
    fn a_snapshot_from_an_older_server_still_parses_and_the_new_fields_stay_off_the_wire_when_empty(
    ) {
        let old: SessionAgentState =
            serde_json::from_str(r#"{"session":"s","state":"idle","seq":1,"stale":false,"ts":5}"#)
                .unwrap();
        assert_eq!(
            (old.agent, old.last_response_at, old.waiting_since),
            (None, None, None)
        );
        let v = serde_json::to_value(&SessionAgentState {
            session: "s".into(),
            state: AgentState::Idle,
            seq: 1,
            stale: false,
            ts: 5,
            agent: None,
            last_response_at: None,
            waiting_since: None,
        })
        .unwrap();
        assert!(
            v.get("agent").is_none()
                && v.get("last_response_at").is_none()
                && v.get("waiting_since").is_none()
        );
    }

    #[test]
    fn unknown_kinds_and_values_are_ignored_not_fatal() {
        let e: AgentEvent = serde_json::from_str(
            r#"{"session":"s","seq":1,"ts":1,"source":"telepathy","confidence":"certain","kind":"hologram","x":1}"#,
        )
        .unwrap();
        assert_eq!(e.kind, EventKind::Unknown);
        assert_eq!(e.source, Source::Unknown);
        let s: EventKind =
            serde_json::from_str(r#"{"kind":"state","state":"levitating"}"#).unwrap();
        assert_eq!(
            s,
            EventKind::State {
                state: AgentState::Unknown
            }
        );
    }

    #[test]
    fn activity_and_attention_round_trip_with_wire_names_and_ignore_unknown_values() {
        let a = Attention {
            id: "a1".into(),
            session: "s".into(),
            kind: AttentionKind::FinishedUnseen,
            raised_at: 7,
            task: Some(Task {
                title: "fix it".into(),
                started_at: 3,
                source: Source::Hook,
            }),
            prompt: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["kind"], "finished_unseen");
        assert_eq!(v["task"]["source"], "hook");
        assert!(v.get("prompt").is_none());
        assert_eq!(serde_json::from_value::<Attention>(v).unwrap(), a);
        let u: Attention =
            serde_json::from_str(r#"{"id":"a","session":"s","kind":"levitating","raised_at":1}"#)
                .unwrap();
        assert_eq!(u.kind, AttentionKind::Unknown);
        let act: SessionActivity =
            serde_json::from_str(r#"{"session":"s","activity":"dreaming","ts":1}"#).unwrap();
        assert_eq!(act.activity, Activity::Unknown);
        assert_eq!(serde_json::to_value(Activity::Blocked).unwrap(), "blocked");
    }
}
