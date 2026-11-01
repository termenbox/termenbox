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
    /// An ACP workspace the server started itself (tier T0, decision 0111).
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
    /// The current task of the workspace, from the first line of the prompt (decision 0095).
    Task {
        title: String,
    },
    /// Context use of an ACP workspace; the percentage is `used / size`.
    Usage {
        used: u64,
        size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<String>,
    },
    /// The agent's own title for the workspace.
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

/// `seq` is a per-workspace counter and `ts` the server's clock (ms since the epoch), both stamped by the
/// server and never taken from the source.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub workspace: String,
    pub seq: u64,
    pub ts: u64,
    pub source: Source,
    pub confidence: Confidence,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// The latest state of one workspace, for a client that has just connected.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAgentState {
    pub workspace: String,
    pub state: AgentState,
    pub seq: u64,
    /// No event for a while although the state is `working`: the state may be out of date.
    pub stale: bool,
    pub ts: u64,
    /// The agent in the workspace (`claude`, `codex`), from the process scan; absent for a plain shell and
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

/// The prompt one tab waits on (decision 0149). The choices are labels only; the keys stay on the server.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TabPrompt {
    pub id: String,
    pub kind: String,
    pub choices: Vec<String>,
}

/// What one tab's agent does now. The workspace-level `WorkspaceAgentState` stays the aggregate Home and push use.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TabAgentState {
    pub workspace: String,
    pub tab: String,
    pub state: AgentState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_response_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<TabPrompt>,
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

/// Why a `working` session is working (decision 0188); the tool name rides beside it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubReason {
    Inferring,
    Tool,
    Subagent,
    Compacting,
    Retrying,
    /// No progress proof for the quiet window.
    Quiet,
    #[serde(other)]
    Unknown,
}

/// What a `needs_you` item waits for (decision 0190).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    Permission,
    Question,
    Plan,
    Trust,
    Elicitation,
    #[serde(other)]
    Unknown,
}

/// Why a turn ended badly (decision 0192).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCause {
    RateLimit,
    Auth,
    ApiError,
    Context,
    Crashed,
    #[serde(other)]
    Unknown,
}

/// How loud an Attention item is (decision 0193): `needs_you` and `result` alert, `info` is silent.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttentionTier {
    NeedsYou,
    Result,
    Info,
    #[serde(other)]
    Unknown,
}

/// The current piece of work of a workspace: the first line of the prompt, or the plan item in progress.
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
    pub workspace: String,
    /// The tab that asked, when a hook proved which one (decision 0149); old items and workspace-level hints have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    pub kind: AttentionKind,
    pub raised_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    /// The question of a `needs_input`; never sent in a push.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Absent from an older server; the client then derives the tier from `kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<AttentionTier>,
    /// The typed kind of a `needs_you` item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<AskKind>,
    /// The cause of a `failed` result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<FailureCause>,
}

/// The derived Activity of one workspace with its task, as the status channel carries it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceActivity {
    pub workspace: String,
    pub activity: Activity,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// What scan or screen suspect without a hook to confirm it: low confidence, never pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<AttentionKind>,
    /// Why it is `working`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_reason: Option<SubReason>,
    /// Background work still running after the turn ended (the `bg: N` badge).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bg: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_with_the_wire_names_the_client_reads() {
        let e = AgentEvent {
            workspace: "s".into(),
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
        let old: WorkspaceAgentState = serde_json::from_str(
            r#"{"workspace":"s","state":"idle","seq":1,"stale":false,"ts":5}"#,
        )
        .unwrap();
        assert_eq!(
            (old.agent, old.last_response_at, old.waiting_since),
            (None, None, None)
        );
        let v = serde_json::to_value(&WorkspaceAgentState {
            workspace: "s".into(),
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
            r#"{"workspace":"s","seq":1,"ts":1,"source":"telepathy","confidence":"certain","kind":"hologram","x":1}"#,
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
            workspace: "s".into(),
            tab: None,
            kind: AttentionKind::FinishedUnseen,
            raised_at: 7,
            task: Some(Task {
                title: "fix it".into(),
                started_at: 3,
                source: Source::Hook,
            }),
            prompt: None,
            tier: None,
            ask: None,
            cause: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["kind"], "finished_unseen");
        assert_eq!(v["task"]["source"], "hook");
        assert!(v.get("prompt").is_none());
        assert_eq!(serde_json::from_value::<Attention>(v).unwrap(), a);
        let u: Attention =
            serde_json::from_str(r#"{"id":"a","workspace":"s","kind":"levitating","raised_at":1}"#)
                .unwrap();
        assert_eq!(u.kind, AttentionKind::Unknown);
        let act: WorkspaceActivity =
            serde_json::from_str(r#"{"workspace":"s","activity":"dreaming","ts":1}"#).unwrap();
        assert_eq!(act.activity, Activity::Unknown);
        assert_eq!(serde_json::to_value(Activity::Blocked).unwrap(), "blocked");
    }

    #[test]
    fn every_new_status_type_round_trips_and_ignores_an_unknown_value() {
        fn check<
            T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
        >(
            v: T,
            wire: &str,
            unknown: T,
        ) {
            assert_eq!(serde_json::to_value(&v).unwrap(), wire);
            assert_eq!(serde_json::from_value::<T>(wire.into()).unwrap(), v);
            assert_eq!(
                serde_json::from_value::<T>("levitating".into()).unwrap(),
                unknown
            );
        }
        check(SubReason::Retrying, "retrying", SubReason::Unknown);
        check(SubReason::Quiet, "quiet", SubReason::Unknown);
        check(AskKind::Elicitation, "elicitation", AskKind::Unknown);
        check(FailureCause::RateLimit, "rate_limit", FailureCause::Unknown);
        check(FailureCause::ApiError, "api_error", FailureCause::Unknown);
        check(AttentionTier::NeedsYou, "needs_you", AttentionTier::Unknown);
        check(AttentionTier::Info, "info", AttentionTier::Unknown);
    }

    #[test]
    fn the_new_attention_and_activity_fields_default_for_an_older_peer_and_stay_off_the_wire_when_empty(
    ) {
        let a: Attention = serde_json::from_str(
            r#"{"id":"a","workspace":"s","kind":"needs_input","raised_at":1}"#,
        )
        .unwrap();
        assert_eq!((a.tier, a.ask, a.cause), (None, None, None));
        let act: WorkspaceActivity =
            serde_json::from_str(r#"{"workspace":"s","activity":"idle","ts":1}"#).unwrap();
        assert_eq!((act.sub_reason, act.bg), (None, 0));
        let v = serde_json::to_value(&act).unwrap();
        assert!(v.get("sub_reason").is_none() && v.get("bg").is_none());
        let full: Attention = serde_json::from_str(
            r#"{"id":"a","workspace":"s","kind":"failed","raised_at":1,"tier":"result","cause":"crashed","ask":"trust"}"#,
        )
        .unwrap();
        assert_eq!(
            (full.tier, full.cause, full.ask),
            (
                Some(AttentionTier::Result),
                Some(FailureCause::Crashed),
                Some(AskKind::Trust)
            )
        );
    }
}
