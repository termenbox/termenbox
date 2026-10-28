//! ACP workspaces the server starts itself (tier T0, decisions 0054, 0103, 0104, 0111). The agent is a stdio child
//! speaking Agent Client Protocol (JSON-RPC 2.0, one message per line); the server owns the child, so a client
//! disconnecting changes nothing. There is no terminal: a workspace is its event stream and its answers.
//!
//! The protocol types (requests, results, `session/update` kinds, permission requests) come from the
//! `agent-client-protocol-schema` crate, built with `default-features = false` (types and serde only, protocol v1).
//! The transport is ours: the stdio line reader, the request table, the timeouts and the caps below. There is no
//! SDK runtime glue beside tokio (`resource-constraints`).
//!
//! Decoding is lenient. Agents deviate from the schema (fact 0040), so a message that does not decode into its type
//! is dropped for that message only: it is counted (`DROPPED`) and logged by kind without its content, and it never
//! fails the turn or the workspace. An update kind we do not handle stays ignored. Fields we depend on are required by
//! the schema types, everything else is optional there.
//!
//! Text from the agent is untrusted (0095): lines are length-capped, chunks are buffered to a fixed size, and
//! everything that reaches the hub is cleaned by `events::sanitize_kind`, after decoding. Nothing the agent says is
//! audit-logged.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use agent_client_protocol_schema::v1 as acp;
use agent_client_protocol_schema::{MaybeUndefined, ProtocolVersion};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use termenbox_core::events::{
    AgentState, Confidence, Decision, DecisionOption, EventKind, PlanItem, Source, ToolPhase,
};
use termenbox_core::protocol::{AcpView, WorkspaceInfo};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, watch};

use crate::acp_stream::{ModeVia, Stream};
use crate::backend::{clean_label, new_id, valid_dir, valid_name, WorkspaceBackend};
use crate::events::EventHub;
use crate::layout::Layout;
use crate::pty::Attachment;

/// A lock whose holder panicked still opens. Poisoning would hand that one session's panic to every later caller of
/// the lock, and the global workspace table is one of them (decision 0143): the session fails on its own, the rest
/// carry on.
trait LockExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// What the owner is told when a workspace's own task panicked. No panic text: that is in the server's log.
const INTERNAL_FAILURE: &str =
    "An internal error stopped this workspace. Your other workspaces are not affected.";
/// Lines waiting to be written to an agent's input. An agent that does not read its input would otherwise let this
/// grow until the server is out of memory; at the limit the workspace ends instead.
const STDIN_QUEUE: usize = 256;
/// All the conversation rings together (`max_sessions` x `event_ring_bytes`) must fit this, so a config cannot ask
/// for more than a small server can hold.
const MAX_EVENT_RING_TOTAL: usize = 256 * 1024 * 1024;
/// After the agent closes its output it gets this long to exit by itself, so its exit status is the one reported;
/// then it is killed (a child that is still running with its output closed can never answer again).
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Watches one task of a workspace. A panic in it is that workspace's failure and nobody else's (decision 0143): the
/// task's panic is logged by the hook, and `on_panic` puts the workspace in a state its owner can see and leave.
fn watch_task(
    live: Arc<Live>,
    what: &'static str,
    task: tokio::task::JoinHandle<()>,
    on_panic: fn(&Live),
) {
    tokio::spawn(async move {
        if let Err(e) = task.await {
            if e.is_panic() {
                tracing::error!(target: "acp", "the {what} task of a workspace panicked");
                // The handler must not unwind into the runtime either.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_panic(&live)));
            }
        }
    });
}

/// One line from the agent is never longer than this; a longer one is dropped whole.
const MAX_LINE: usize = 1 << 20;
/// Streamed text waiting to become one message or thought event.
const MAX_CHUNK_BUFFER: usize = 4096;
/// A prompt typed by the owner.
const MAX_PROMPT: usize = 16 * 1024;
/// Permission requests open at once per workspace; further ones are answered `cancelled`.
const MAX_PENDING: usize = 8;
const MAX_TOOLS: usize = 64;
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Workspaces at once when the config does not say. The default is 6 and there is no memory floor: an agent child is
/// a Node or Rust process of a few hundred MB, so the box sizes it (six adapters on 512 MB can hit the OOM killer).
/// `resource-constraints` covers termenbox's own processes, not the agents it starts.
const DEFAULT_MAX_SESSIONS: usize = 6;

// ---------------------------------------------------------------------------------------------------
// Allowlist config
// ---------------------------------------------------------------------------------------------------

/// How to start one agent. Written by the owner on the server; a client only ever names the agent.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentProfile {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Variables removed from the child's environment (claude's adapter refuses to start with `CLAUDECODE`).
    #[serde(default)]
    pub env_remove: Vec<String>,
    /// Advertise form elicitation in `initialize`. Without it opencode's questions go unanswered and block the turn.
    #[serde(default)]
    pub elicitation_form: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AcpConfig {
    #[serde(default = "default_max")]
    pub max_sessions: usize,
    /// Keep the built-in agents beside the ones in this file (a same-named entry in the file wins).
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub agents: HashMap<String, AgentProfile>,
    /// Bytes of agent events kept per conversation for a client that reconnects (`event_log.rs`).
    #[serde(default = "default_ring")]
    pub event_ring_bytes: usize,
}

/// The agents termenbox knows how to start (fact 0040), used when there is no file or the file sets `builtin`.
/// The adapters are not shipped: the owner installs them, and `acp_agents` lists only those on `PATH`.
pub fn builtin_agents() -> HashMap<String, AgentProfile> {
    HashMap::from([
        (
            "claude".to_string(),
            AgentProfile {
                command: "claude-agent-acp".into(),
                args: vec![],
                env: HashMap::new(),
                env_remove: vec!["CLAUDECODE".into()],
                elicitation_form: false,
            },
        ),
        (
            "opencode".to_string(),
            AgentProfile {
                command: "opencode".into(),
                args: vec!["acp".into()],
                env: HashMap::new(),
                env_remove: vec![],
                elicitation_form: true,
            },
        ),
    ])
}

/// Whether `command` can be started: a path (it has a `/`) must be an executable file, a bare name must be one in
/// some `PATH` entry. Checked on every call, so an install shows up without a restart.
pub fn on_path(command: &str) -> bool {
    fn executable(p: &std::path::Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    if command.contains('/') {
        return executable(std::path::Path::new(command));
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| executable(&d.join(command))))
        .unwrap_or(false)
}

fn default_max() -> usize {
    DEFAULT_MAX_SESSIONS
}

fn default_ring() -> usize {
    crate::event_log::DEFAULT_BUDGET_BYTES
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            builtin: true,
            agents: builtin_agents(),
            event_ring_bytes: default_ring(),
        }
    }
}

impl AcpConfig {
    /// ```toml
    /// max_sessions = 6
    /// event_ring_bytes = 1048576 # per conversation, for a client that reconnects
    /// builtin = true # keep claude and opencode beside the agents below
    /// [agents.claude]
    /// command = "claude-agent-acp"
    /// env_remove = ["CLAUDECODE"]
    /// ```
    pub fn parse(text: &str) -> Result<Self> {
        let mut cfg: Self = toml::from_str(text).map_err(|e| anyhow!("acp agents file: {e}"))?;
        if !(1..=16).contains(&cfg.max_sessions) {
            return Err(anyhow!("max_sessions must be 1 to 16"));
        }
        if !(64 * 1024..=64 * 1024 * 1024).contains(&cfg.event_ring_bytes) {
            return Err(anyhow!("event_ring_bytes must be 65536 to 67108864"));
        }
        if cfg.max_sessions.saturating_mul(cfg.event_ring_bytes) > MAX_EVENT_RING_TOTAL {
            return Err(anyhow!(
                "max_sessions x event_ring_bytes must be at most {} bytes",
                MAX_EVENT_RING_TOTAL
            ));
        }
        for (name, a) in &cfg.agents {
            if !valid_name(name) || a.command.is_empty() {
                return Err(anyhow!("agent `{name}` needs a plain name and a command"));
            }
        }
        if cfg.builtin {
            for (name, a) in builtin_agents() {
                cfg.agents.entry(name).or_insert(a);
            }
        }
        Ok(cfg)
    }

    /// No file means the built-in agents. A file is the whole allowlist unless it sets `builtin = true`.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(anyhow!("cannot read {}: {e}", path.display())),
        }
    }
}

/// Ends an agent and everything it started: the agent is the leader of its own process group.
fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 1) {
        // SAFETY: kill has no memory preconditions; a negative pid is the group, and `pid > 1` rules out "everything".
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

/// Ends the agent's whole group when the reader task is over, however it ended: a panic, the exit of the agent
/// (whatever it left running goes too) or the runtime dropping the task.
struct GroupKill(Option<u32>);

impl Drop for GroupKill {
    fn drop(&mut self) {
        kill_group(self.0);
    }
}

/// A place counted against `max_sessions` while a workspace starts. Dropped on every way out of a start, a
/// cancelled one and a panicking one included, so the cap cannot shrink for good.
struct StartSlot<'a>(&'a Mutex<usize>);

impl<'a> StartSlot<'a> {
    /// Counts one more starting workspace. The caller has checked the cap under the same lock.
    fn take(counter: &'a Mutex<usize>, held: &mut usize) -> Self {
        *held += 1;
        StartSlot(counter)
    }

    /// Adopts a slot that was already counted.
    fn counted(counter: &'a Mutex<usize>) -> Self {
        StartSlot(counter)
    }
}

impl Drop for StartSlot<'_> {
    fn drop(&mut self) {
        let mut starting = self.0.lock_or_recover();
        *starting = starting.saturating_sub(1);
    }
}

/// Ends a freshly started agent unless the start finished: a start that is dropped (the connection that asked for it
/// went away) would otherwise leave an agent nobody can reach.
struct KillUnlessDone {
    live: Arc<Live>,
    done: bool,
}

impl Drop for KillUnlessDone {
    fn drop(&mut self) {
        if !self.done {
            if let Some(k) = self.live.kill.lock_or_recover().take() {
                let _ = k.send(());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// session/update -> events (a pure mapping apart from the tool and chunk memory it needs)
// ---------------------------------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum Chunk {
    Message,
    Thought,
}

/// Messages from the agent that did not decode into their schema type and were dropped (never the turn or the
/// workspace). Counted, not stored: what the agent sent is untrusted and is not logged.
pub(crate) static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Notes one dropped message by its kind (a fixed label, never agent text).
pub(crate) fn dropped(what: &str) {
    let total = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
    tracing::debug!(what, total, "dropped an ACP message that did not decode");
}

/// Decodes one value into a schema type without copying it. `None` (counted) when it does not fit.
pub(crate) fn decode<T: serde::de::DeserializeOwned>(v: &Value, what: &str) -> Option<T> {
    match T::deserialize(v) {
        Ok(t) => Some(t),
        Err(_) => {
            dropped(what);
            None
        }
    }
}

/// The wire form of a request or result built from schema types.
fn wire<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// The wire name of a tool kind (what a client shows and the history keeps).
pub(crate) fn tool_kind(k: acp::ToolKind) -> &'static str {
    match k {
        acp::ToolKind::Read => "read",
        acp::ToolKind::Edit => "edit",
        acp::ToolKind::Delete => "delete",
        acp::ToolKind::Move => "move",
        acp::ToolKind::Search => "search",
        acp::ToolKind::Execute => "execute",
        acp::ToolKind::Think => "think",
        acp::ToolKind::Fetch => "fetch",
        acp::ToolKind::SwitchMode => "switch_mode",
        _ => "other",
    }
}

/// The wire name of a tool call status.
pub(crate) fn tool_status(s: acp::ToolCallStatus) -> &'static str {
    match s {
        acp::ToolCallStatus::InProgress => "in_progress",
        acp::ToolCallStatus::Completed => "completed",
        acp::ToolCallStatus::Failed => "failed",
        _ => "pending",
    }
}

/// The wire name of a plan entry's status.
pub(crate) fn plan_status(s: &acp::PlanEntryStatus) -> &'static str {
    match s {
        acp::PlanEntryStatus::InProgress => "in_progress",
        acp::PlanEntryStatus::Completed => "completed",
        _ => "pending",
    }
}

/// The wire name of a stop reason.
pub(crate) fn stop_reason(r: acp::StopReason) -> &'static str {
    match r {
        acp::StopReason::EndTurn => "end_turn",
        acp::StopReason::MaxTokens => "max_tokens",
        acp::StopReason::MaxTurnRequests => "max_turn_requests",
        acp::StopReason::Refusal => "refusal",
        acp::StopReason::Cancelled => "cancelled",
        _ => "other",
    }
}

/// The update kinds this client handles; one that fails to decode is counted, any other is just not ours.
const HANDLED_UPDATES: [&str; 11] = [
    "agent_message_chunk",
    "agent_thought_chunk",
    "tool_call",
    "tool_call_update",
    "plan",
    "usage_update",
    "session_info_update",
    "current_mode_update",
    "config_option_update",
    "available_commands_update",
    "user_message_chunk",
];

/// An update that did not decode: counted when it is a kind we handle, ignored when it is not. Returns its kind.
pub(crate) fn drop_update(u: &Value) -> &str {
    let kind = s(u, "sessionUpdate");
    if HANDLED_UPDATES.contains(&kind) {
        dropped(kind);
    }
    kind
}

/// The commands of an `available_commands_update`. The schema requires a `description`; agents leave it out and
/// nothing here depends on it, so a missing one reads as empty. An element that is not a command is skipped.
pub(crate) fn available_commands(u: &Value) -> Vec<acp::AvailableCommand> {
    u.get("availableCommands")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let mut c = c.as_object()?.clone();
                    c.entry("description").or_insert_with(|| json!(""));
                    acp::AvailableCommand::deserialize(Value::Object(c)).ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `toolCall` of a `session/request_permission`, decoded on its own so that a request whose tool call is odd
/// is still shown (with what could be read) and answered.
pub(crate) fn permission_tool(params: &Value) -> acp::ToolCallUpdate {
    params
        .get("toolCall")
        .and_then(|t| decode(t, "permission tool call"))
        .unwrap_or_else(|| acp::ToolCallUpdate::new("", acp::ToolCallUpdateFields::new()))
}

/// Turns `session/update` payloads into the agent event model. Streamed chunks are joined into one event,
/// flushed when another kind arrives or the turn ends; tool calls are merged by `toolCallId`.
#[derive(Default)]
pub struct UpdateMapper {
    tools: HashMap<String, (String, String)>,
    buf: Option<(Chunk, String)>,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn first_line(t: &str) -> String {
    t.lines().next().unwrap_or("").to_string()
}

/// Cuts at a char boundary at most `max` bytes in.
fn cap(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl UpdateMapper {
    pub fn flush(&mut self) -> Option<EventKind> {
        let (kind, text) = self.buf.take()?;
        if text.trim().is_empty() {
            return None;
        }
        Some(match kind {
            Chunk::Message => EventKind::Message { summary: text },
            Chunk::Thought => EventKind::Thought { summary: text },
        })
    }

    fn chunk(&mut self, kind: Chunk, c: &acp::ContentChunk) -> Vec<EventKind> {
        let mut out = Vec::new();
        if self.buf.as_ref().map(|(k, _)| *k != kind).unwrap_or(false) {
            out.extend(self.flush());
        }
        if let acp::ContentBlock::Text(t) = &c.content {
            let buf = &mut self.buf.get_or_insert((kind, String::new())).1;
            let room = MAX_CHUNK_BUFFER.saturating_sub(buf.len());
            buf.push_str(cap(&t.text, room));
        }
        out
    }

    pub fn map(&mut self, u: &Value) -> Vec<EventKind> {
        let update = match acp::SessionUpdate::deserialize(u) {
            Ok(update) => update,
            Err(_) => {
                // Dropped for this message only. A kind that is not a chunk still ends the chunk in progress.
                let kind = drop_update(u);
                if matches!(kind, "agent_message_chunk" | "agent_thought_chunk") {
                    return Vec::new();
                }
                return self.flush().into_iter().collect();
            }
        };
        match &update {
            acp::SessionUpdate::AgentMessageChunk(c) => return self.chunk(Chunk::Message, c),
            acp::SessionUpdate::AgentThoughtChunk(c) => return self.chunk(Chunk::Thought, c),
            _ => {}
        }
        let mut out: Vec<EventKind> = self.flush().into_iter().collect();
        match update {
            acp::SessionUpdate::ToolCall(t) => {
                let id = t.tool_call_id.to_string();
                // The schema reads a missing `kind` as `other`; the event keeps saying `tool` for an agent that sends none
                // (goose does), so the history is unchanged.
                let name = match u.get("kind") {
                    Some(Value::String(_)) => tool_kind(t.kind),
                    _ => "tool",
                }
                .to_string();
                let summary = first_line(&t.title);
                if self.tools.len() >= MAX_TOOLS {
                    self.tools.clear();
                }
                let done = matches!(
                    t.status,
                    acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
                );
                out.push(EventKind::Tool {
                    name: name.clone(),
                    summary: summary.clone(),
                    phase: ToolPhase::Started,
                });
                if done {
                    out.push(EventKind::Tool {
                        name,
                        summary,
                        phase: ToolPhase::Finished,
                    });
                } else {
                    self.tools.insert(id, (name, summary));
                }
            }
            acp::SessionUpdate::ToolCallUpdate(t) => {
                let id = t.tool_call_id.to_string();
                let (name, mut summary) = self
                    .tools
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| ("tool".into(), String::new()));
                if let Some(title) = t.fields.title.as_deref().filter(|t| !t.is_empty()) {
                    summary = first_line(title);
                }
                if matches!(
                    t.fields.status,
                    Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
                ) {
                    self.tools.remove(&id);
                    out.push(EventKind::Tool {
                        name,
                        summary,
                        phase: ToolPhase::Finished,
                    });
                } else if let Some(t) = self.tools.get_mut(&id) {
                    t.1 = summary;
                }
            }
            acp::SessionUpdate::Plan(p) => {
                let items: Vec<PlanItem> = p
                    .entries
                    .iter()
                    .take(crate::events::PLAN_ITEMS)
                    .map(|e| PlanItem {
                        text: e.content.clone(),
                        status: plan_status(&e.status).to_string(),
                    })
                    .collect();
                if !items.is_empty() {
                    out.push(EventKind::Plan { items });
                }
            }
            acp::SessionUpdate::UsageUpdate(u) => {
                let cost = u.cost.map(|c| format!("{} {}", c.amount, c.currency));
                out.push(EventKind::Usage {
                    used: u.used,
                    size: u.size,
                    cost,
                });
            }
            acp::SessionUpdate::SessionInfoUpdate(i) => {
                if let MaybeUndefined::Value(title) = i.title {
                    out.push(EventKind::Title { title });
                }
            }
            acp::SessionUpdate::CurrentModeUpdate(m) => out.push(EventKind::Mode {
                mode: m.current_mode_id.to_string(),
            }),
            acp::SessionUpdate::AvailableCommandsUpdate(_) => out.push(EventKind::Commands {
                names: available_commands(u).into_iter().map(|c| c.name).collect(),
            }),
            _ => {}
        }
        out
    }
}

/// The options of a `session/request_permission` that may be shown: `allow_once` and `reject_once` only
/// (decision 0104); `allow_always`, `reject_always` and anything unknown are dropped. Each option is decoded on its
/// own, so one the schema does not know costs only itself.
pub fn permission_options(params: &Value) -> Vec<DecisionOption> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|o| acp::PermissionOption::deserialize(o).ok())
                .filter(|o| {
                    matches!(
                        o.kind,
                        acp::PermissionOptionKind::AllowOnce
                            | acp::PermissionOptionKind::RejectOnce
                    )
                })
                .filter(|o| !o.option_id.0.is_empty())
                .map(|o| DecisionOption {
                    id: o.option_id.to_string(),
                    kind: match o.kind {
                        acp::PermissionOptionKind::AllowOnce => "allow_once",
                        _ => "reject_once",
                    }
                    .into(),
                    name: o.name,
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------
// One workspace: the child, its two pipes and the state they share
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceStatus {
    Running,
    /// Closed by the owner, or not resumable after a server restart.
    Ended,
    /// The agent process died; the text is its exit status.
    Failed(String),
}

struct Perm {
    rpc_id: Value,
    allowed: Vec<String>,
}

struct Inner {
    status: WorkspaceStatus,
    next_id: u64,
    waiting: HashMap<u64, oneshot::Sender<std::result::Result<Value, Value>>>,
    perms: HashMap<String, Perm>,
    perm_seq: u64,
    mapper: UpdateMapper,
    acp_id: String,
    turn: bool,
    /// `session/load` replays history as updates; those are not news.
    replaying: bool,
    /// What the workspace view shows (T0): kept here, bounded, and handed to a client on `acp_open`.
    stream: Stream,
    /// The conversation as scrubbed `session/update` frames with a sequence number, for a client that resumes.
    log: crate::event_log::EventLog,
}

struct Live {
    event_name: String,
    /// The folder the agent runs in (its last component is kept in the history).
    dir: String,
    hub: Option<EventHub>,
    inner: Mutex<Inner>,
    /// The stream's revision, for the connections that show this workspace.
    changed: watch::Sender<u64>,
    out: mpsc::Sender<String>,
    /// Set when the input queue overflowed: the workspace then ends as failed, not as closed.
    overflowed: AtomicBool,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    /// Writes the state file (set by the backend). A turn opening or closing changes what a restart interrupts, so
    /// it is written then, not only when a workspace is made or closed.
    saver: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Live {
    fn persist(&self) {
        let save = self.saver.lock_or_recover().clone();
        if let Some(save) = save {
            save();
        }
    }

    fn emit(&self, kind: EventKind) {
        if let Some(hub) = &self.hub {
            hub.report(&self.event_name, Source::Acp, Confidence::High, kind);
        }
    }

    /// Runs `f` on the stream and wakes the connections that show this workspace.
    fn stream<R>(&self, f: impl FnOnce(&mut Stream) -> R) -> R {
        let (r, rev) = {
            let mut g = self.inner.lock_or_recover();
            let r = f(&mut g.stream);
            (r, g.stream.rev())
        };
        self.changed.send_replace(rev);
        r
    }

    /// The turn opens at `session/prompt` and closes with its response; the state is derived from this and from
    /// the open requests by the hub's reducer, the same one the hooks feed.
    fn turn_open(&self) {
        self.emit(EventKind::Turn {
            open: true,
            outcome: None,
        });
    }

    fn turn_close(&self, outcome: AgentState) {
        self.emit(EventKind::Turn {
            open: false,
            outcome: Some(outcome),
        });
    }

    fn block(&self, key: &str, open: bool) {
        self.emit(EventKind::Block {
            key: key.to_string(),
            open,
        });
    }

    fn send(&self, v: Value) {
        if let Err(mpsc::error::TrySendError::Full(_)) = self.out.try_send(v.to_string()) {
            // The agent is not reading. Ending it here is the cost of a bounded queue; the first overflow does it.
            if !self.overflowed.swap(true, Ordering::SeqCst) {
                tracing::warn!(target: "acp", "an agent stopped reading its input; ending its workspace");
                if let Some(k) = self.kill.lock_or_recover().take() {
                    let _ = k.send(());
                }
            }
        }
    }

    fn respond(&self, id: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut g = self.inner.lock_or_recover();
            if g.status != WorkspaceStatus::Running {
                return Err(anyhow!("the agent is not running"));
            }
            g.next_id += 1;
            let id = g.next_id;
            g.waiting.insert(id, tx);
            id
        };
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = match timeout {
            Some(t) => match tokio::time::timeout(t, rx).await {
                Ok(reply) => reply,
                Err(_) => {
                    // The reply may still come; with nobody waiting for it the entry would stay for good.
                    self.inner.lock_or_recover().waiting.remove(&id);
                    return Err(anyhow!("the agent did not answer {method}"));
                }
            },
            None => rx.await,
        };
        match reply {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => {
                let message = acp::Error::deserialize(&e)
                    .map(|e| e.message)
                    .unwrap_or_default();
                Err(anyhow!("{method}: {}", cap(&message, 200)))
            }
            Err(_) => Err(anyhow!("the agent exited")),
        }
    }

    fn on_message(&self, v: Value) {
        // Tests: a frame that panics the reader task, as a bug in the mapping of a hostile frame would.
        #[cfg(test)]
        if v.get("method").and_then(Value::as_str) == Some("test/panic") {
            panic!("test panic in the reader task");
        }
        let method = s(&v, "method").to_string();
        let id = v.get("id").cloned();
        match (method.as_str(), id) {
            ("", Some(id)) => {
                let Some(n) = id.as_u64() else { return };
                let tx = self.inner.lock_or_recover().waiting.remove(&n);
                if let Some(tx) = tx {
                    let _ = tx.send(match v.get("error") {
                        Some(e) => Err(e.clone()),
                        None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                    });
                }
            }
            ("session/request_permission", Some(id)) => self.on_permission(id, &v["params"]),
            // fs and terminal are not advertised, and extension requests are not ours to answer.
            (_, Some(id)) => self.send(json!({
                "jsonrpc": "2.0", "id": id,
                "error": wire(&acp::Error::method_not_found())
            })),
            ("session/update", None) => self.on_update(&v["params"]["update"]),
            _ => {}
        }
    }

    fn on_update(&self, update: &Value) {
        let (kinds, rev) = {
            let mut g = self.inner.lock_or_recover();
            if g.replaying {
                return;
            }
            g.stream.update(update);
            g.log.push(update);
            (g.mapper.map(update), g.stream.rev())
        };
        self.changed.send_replace(rev);
        for k in kinds {
            self.emit(k);
        }
    }

    fn on_permission(&self, rpc_id: Value, params: &Value) {
        let options = permission_options(params);
        let prompt_id = {
            let mut g = self.inner.lock_or_recover();
            if g.perms.len() >= MAX_PENDING {
                drop(g);
                return self.respond(&rpc_id, cancelled());
            }
            g.perm_seq += 1;
            let id = format!("acp-{}", g.perm_seq);
            g.perms.insert(
                id.clone(),
                Perm {
                    rpc_id,
                    allowed: options.iter().map(|o| o.id.clone()).collect(),
                },
            );
            id
        };
        self.stream(|st| st.permission(&prompt_id, params, options.clone()));
        let detail = Some(first_line(
            permission_tool(params)
                .fields
                .title
                .as_deref()
                .unwrap_or(""),
        ))
        .filter(|d| !d.is_empty());
        self.block(&prompt_id, true);
        self.emit(EventKind::Prompt {
            id: prompt_id,
            prompt_kind: "permission".into(),
            choices: options.iter().map(|o| o.name.clone()).collect(),
            decision: Some(Decision { options, detail }),
        });
    }

    /// Answers every open request `cancelled`.
    fn cancel_pending(&self) {
        let perms: Vec<Perm> = self
            .inner
            .lock_or_recover()
            .perms
            .drain()
            .map(|(_, p)| p)
            .collect();
        for p in perms {
            self.respond(&p.rpc_id, cancelled());
        }
        self.stream(|st| st.cancel_open_permissions());
    }

    /// The process is gone (or was told to go): nothing answers any more.
    fn finish(&self, status: WorkspaceStatus) {
        let was = {
            let mut g = self.inner.lock_or_recover();
            g.waiting.clear();
            g.perms.clear();
            std::mem::replace(&mut g.status, status.clone())
        };
        if was == WorkspaceStatus::Running {
            self.stream(|st| match &status {
                WorkspaceStatus::Failed(text) => st.status("failed", Some(text)),
                _ => st.status("ended", None),
            });
            self.turn_close(match status {
                WorkspaceStatus::Failed(_) => AgentState::Error,
                _ => AgentState::Done,
            });
        }
    }
}

/// The answer to a permission request that nobody is going to take.
fn cancelled() -> Value {
    wire(&acp::RequestPermissionResponse::new(
        acp::RequestPermissionOutcome::Cancelled,
    ))
}

/// Reads one line of at most `max` bytes. `Ok(None)` is end of input; `Ok(Some(None))` a line that was too long
/// and has been skipped to its end.
async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    r: &mut R,
    max: usize,
) -> std::io::Result<Option<Option<Vec<u8>>>> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return Ok(if line.is_empty() && !too_long {
                None
            } else {
                Some((!too_long).then_some(line))
            });
        }
        let (take, done) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        if !too_long {
            if line.len() + take > max {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(&buf[..take]);
            }
        }
        r.consume(take);
        if done {
            return Ok(Some((!too_long).then_some(line)));
        }
    }
}

fn spawn_agent(
    profile: &AgentProfile,
    agent: &str,
    dir: &str,
    name: &str,
    hub: Option<EventHub>,
    event_name: String,
    event_ring_bytes: usize,
) -> Result<Arc<Live>> {
    let mut cmd = Command::new(&profile.command);
    cmd.args(&profile.args)
        .envs(crate::locale::utf8_env())
        .envs(&profile.env)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        // Its own group, so ending the agent ends what it started, and so a signal for the server's group misses it.
        .process_group(0);
    // SAFETY: only prctl, which is async-signal-safe. The agent dies with the server, so a restart never finds two
    // agents for one conversation. The signal is tied to the thread that started it, which for this runtime is a
    // worker that lives as long as the process.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    for k in &profile.env_remove {
        cmd.env_remove(k);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("cannot start the agent for {name}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let pid = child.id();
    let (out, mut out_rx) = mpsc::channel::<String>(STDIN_QUEUE);
    let (kill_tx, mut kill_rx) = oneshot::channel::<()>();
    let live = Arc::new(Live {
        saver: Mutex::new(None),
        event_name,
        dir: dir.to_string(),
        hub,
        changed: watch::channel(0).0,
        inner: Mutex::new(Inner {
            status: WorkspaceStatus::Running,
            next_id: 0,
            waiting: HashMap::new(),
            perms: HashMap::new(),
            perm_seq: 0,
            mapper: UpdateMapper::default(),
            acp_id: String::new(),
            turn: false,
            replaying: false,
            stream: Stream::new(agent, dir, crate::events::now_ms()),
            log: crate::event_log::EventLog::new("", crate::events::now_ms(), event_ring_bytes),
        }),
        out,
        overflowed: AtomicBool::new(false),
        kill: Mutex::new(Some(kill_tx)),
    });
    let l = live.clone();
    let writer = tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err()
                || stdin.write_all(b"\n").await.is_err()
                || stdin.flush().await.is_err()
            {
                // An agent nobody can write to is over: fail the workspace and let the reader task kill the child.
                l.finish(WorkspaceStatus::Failed("the agent stopped reading".into()));
                if let Some(kill) = l.kill.lock_or_recover().take() {
                    let _ = kill.send(());
                }
                break;
            }
        }
    });
    watch_task(live.clone(), "writer", writer, |l| {
        l.finish(WorkspaceStatus::Failed(INTERNAL_FAILURE.into()))
    });
    let l = live.clone();
    let reader_task = tokio::spawn(async move {
        let _group = GroupKill(pid);
        let mut reader = BufReader::new(stdout);
        loop {
            tokio::select! {
                _ = &mut kill_rx => {
                    kill_group(pid);
                    let _ = child.kill().await;
                    l.finish(if l.overflowed.load(Ordering::SeqCst) {
                        WorkspaceStatus::Failed("the agent stopped reading its input".into())
                    } else {
                        WorkspaceStatus::Ended
                    });
                    return;
                }
                line = read_bounded_line(&mut reader, MAX_LINE) => match line {
                    Ok(Some(Some(bytes))) => {
                        // Anything that is not a JSON object (a banner, a log line) is not ours.
                        if let Ok(v @ Value::Object(_)) = serde_json::from_slice::<Value>(&bytes) {
                            l.on_message(v);
                        }
                    }
                    Ok(Some(None)) => {}
                    _ => break,
                }
            }
        }
        let exited = match tokio::time::timeout(EXIT_GRACE, child.wait()).await {
            Ok(r) => r,
            Err(_) => {
                kill_group(pid);
                let _ = child.kill().await;
                child.wait().await
            }
        };
        let status = match exited {
            Ok(st) => WorkspaceStatus::Failed(st.to_string()),
            Err(e) => WorkspaceStatus::Failed(e.to_string()),
        };
        l.finish(status);
    });
    // The child lives in the reader task: when that task panics the child is dropped, and `kill_on_drop` ends it.
    watch_task(live.clone(), "reader", reader_task, |l| {
        l.finish(WorkspaceStatus::Failed(INTERNAL_FAILURE.into()))
    });
    Ok(live)
}

/// No file system and no terminal: the agent runs its own tools (decision 0111).
fn initialize_request(elicitation_form: bool) -> acp::InitializeRequest {
    let mut caps = acp::ClientCapabilities::new()
        .fs(acp::FileSystemCapabilities::new()
            .read_text_file(false)
            .write_text_file(false))
        .terminal(false);
    if elicitation_form {
        // Serialises as `"elicitation": {"form": {}}`.
        caps = caps.elicitation(
            acp::ElicitationCapabilities::new().form(acp::ElicitationFormCapabilities::new()),
        );
    }
    acp::InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(caps)
        .client_info(acp::Implementation::new(
            "termenbox",
            env!("CARGO_PKG_VERSION"),
        ))
}

/// The `initialize` params. The schema always writes `auth: {terminal: false}` (the default); it is left out, as it
/// has always been, so the agents are sent the same bytes as before.
fn initialize_params(req: &acp::InitializeRequest) -> Value {
    let mut params = wire(req);
    if let Some(caps) = params
        .get_mut("clientCapabilities")
        .and_then(Value::as_object_mut)
    {
        caps.remove("auth");
    }
    params
}

/// `initialize`, then `session/new` or (after a restart) `session/resume` / `session/load`.
async fn handshake(
    live: &Live,
    dir: &str,
    resume: Option<&str>,
    elicitation_form: bool,
) -> Result<String> {
    let init = live
        .request(
            "initialize",
            initialize_params(&initialize_request(elicitation_form)),
            Some(SETUP_TIMEOUT),
        )
        .await?;
    // An `initialize` result that does not decode offers nothing: the workspace can be new, but not resumed.
    let caps = decode::<acp::InitializeResponse>(&init, "initialize result")
        .map(|r| r.agent_capabilities)
        .unwrap_or_default();
    match resume {
        None => {
            let r = live
                .request(
                    "session/new",
                    wire(&acp::NewSessionRequest::new(dir)),
                    Some(SETUP_TIMEOUT),
                )
                .await?;
            let new: acp::NewSessionResponse = decode(&r, "session/new result")
                .ok_or_else(|| anyhow!("the agent's session/new answer has no workspace id"))?;
            live.stream(|st| st.workspace_state(&r));
            Ok(new.session_id.to_string())
        }
        Some(id) if caps.session_capabilities.resume.is_some() => {
            // `mcpServers` is always sent (the schema leaves an empty list out), as every agent has been sent it.
            let mut params = wire(&acp::ResumeSessionRequest::new(id.to_string(), dir));
            params["mcpServers"] = json!([]);
            let r = live
                .request("session/resume", params, Some(SETUP_TIMEOUT))
                .await?;
            live.stream(|st| st.workspace_state(&r));
            Ok(id.to_string())
        }
        Some(id) if caps.load_session => {
            live.inner.lock_or_recover().replaying = true;
            let r = live
                .request(
                    "session/load",
                    wire(&acp::LoadSessionRequest::new(id.to_string(), dir)),
                    Some(SETUP_TIMEOUT),
                )
                .await;
            let r = {
                let mut g = live.inner.lock_or_recover();
                g.replaying = false;
                g.mapper = UpdateMapper::default();
                r?
            };
            live.stream(|st| st.workspace_state(&r));
            Ok(id.to_string())
        }
        Some(_) => Err(anyhow!("the agent can neither resume nor load a workspace")),
    }
}

// ---------------------------------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------------------------------

/// Agents running now in a locked workspace table.
fn running_in(workspaces: &HashMap<String, Entry>) -> usize {
    workspaces
        .values()
        .filter(|e| {
            e.live
                .as_ref()
                .map(|l| l.inner.lock_or_recover().status == WorkspaceStatus::Running)
                .unwrap_or(false)
        })
        .count()
}

/// Writes the workspaces to the state file: running ones with their ACP id and whether a turn is open, the others with
/// why they are not running.
fn write_state(path: &std::path::Path, workspaces: &Mutex<HashMap<String, Entry>>) {
    // The run marker keeps who is live (or being brought back), for the next start to judge a crash loop by.
    crate::crash::set_live(
        workspaces
            .lock_or_recover()
            .iter()
            .filter(|(_, e)| e.live.is_some() || e.restoring)
            .map(|(name, _)| name.clone())
            .collect(),
    );
    let mut saved: Vec<Saved> = workspaces
        .lock_or_recover()
        .iter()
        .filter_map(|(name, e)| {
            let (acp_id, reason, retry, turn_open) = match &e.live {
                Some(l) => {
                    let g = l.inner.lock_or_recover();
                    // A crashed or ended agent is not kept, as before.
                    if g.status != WorkspaceStatus::Running {
                        return None;
                    }
                    (g.acp_id.clone(), None, false, g.stream.working())
                }
                // Not running: kept with why, so a restart or a cap does not lose the workspace.
                None => (
                    e.acp_id.clone(),
                    e.reason.clone(),
                    e.retry || e.restoring,
                    e.turn_open,
                ),
            };
            Some(Saved {
                name: name.clone(),
                label: e.label.clone(),
                agent: e.agent.clone(),
                dir: e.dir.clone(),
                acp_id,
                reason,
                retry,
                turn_open,
                layout: e.layout.clone(),
            })
        })
        .collect();
    saved.sort_by(|a, b| a.name.cmp(&b.name));
    if let Ok(text) = serde_json::to_string(&saved) {
        // Whole or not at all: a crash in the middle of a plain write would leave a file no restore can read.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Saved {
    /// The id; for a workspace an older server made, the name its owner typed.
    name: String,
    /// What clients show. Absent in state an older server wrote: the id is the name.
    #[serde(default)]
    label: Option<String>,
    agent: String,
    dir: String,
    /// Empty for an entry that never started (it was over the cap at the restart).
    acp_id: String,
    /// Why this entry has no running agent; shown as the workspace's "Stopped: ..." line.
    #[serde(default)]
    reason: Option<String>,
    /// Tried again at the next restore.
    #[serde(default)]
    retry: bool,
    /// A turn was running when this was saved: a restart interrupted it.
    #[serde(default)]
    turn_open: bool,
    /// The windows and focus (step 3.1). Absent in state an older server wrote: one agent window.
    #[serde(default)]
    layout: Layout,
}

struct Entry {
    /// What clients show, when it is not the id.
    label: Option<String>,
    agent: String,
    dir: String,
    /// `None` for a workspace with no running agent: not restored yet, not restorable, or over the cap.
    live: Option<Arc<Live>>,
    /// Why there is no agent (shown to the owner).
    reason: Option<String>,
    /// A restore is starting this workspace's agent now; the entry holds its name and slot meanwhile.
    restoring: bool,
    /// For an entry with no live agent, what `save` writes back: the ACP id to resume, whether the next restore
    /// tries again, and whether a turn was open.
    acp_id: String,
    retry: bool,
    turn_open: bool,
    /// The windows of the workspace; kept across a restore, which replaces the placeholder entry by a running one.
    layout: Layout,
}

impl Entry {
    fn running(
        label: Option<String>,
        agent: &str,
        dir: &str,
        live: Arc<Live>,
        layout: Layout,
    ) -> Self {
        Self {
            label,
            agent: agent.to_string(),
            dir: dir.to_string(),
            live: Some(live),
            reason: None,
            restoring: false,
            acp_id: String::new(),
            retry: false,
            turn_open: false,
            layout,
        }
    }

    /// An entry without a running agent. `retry`: the next restore tries to start it again.
    fn stopped(s: &Saved, reason: String, retry: bool) -> Self {
        Self {
            label: s.label.clone(),
            agent: s.agent.clone(),
            dir: s.dir.clone(),
            live: None,
            reason: Some(reason),
            restoring: false,
            acp_id: s.acp_id.clone(),
            retry,
            turn_open: s.turn_open,
            layout: s.layout.clone(),
        }
    }
}

/// How often the ACP supervisor beats.
const HEARTBEAT_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

pub struct AcpBackend {
    config: AcpConfig,
    hub: Option<EventHub>,
    state_file: Option<PathBuf>,
    /// The file an older server wrote its state to (`acp-sessions.json`); read when `state_file` does not exist yet.
    legacy_state_file: Option<PathBuf>,
    event_prefix: String,
    workspaces: Arc<Mutex<HashMap<String, Entry>>>,
    /// Workspaces being started, counted against the cap so two creates cannot both pass it.
    starting: Mutex<usize>,
    /// Names closed while a restore was still starting them: the restore ends what it started instead of keeping it.
    closed_while_restoring: Mutex<HashSet<String>>,
    /// Workspaces live at the last two unclean exits: a restore leaves them stopped (decision 0143).
    safe_mode: HashSet<String>,
    /// New workspaces get a random id and the client's name as a label. Tests that address a workspace by the name
    /// they typed turn it off.
    mint_ids: bool,
}

impl AcpBackend {
    pub fn new(config: AcpConfig) -> Self {
        Self {
            config,
            hub: None,
            state_file: None,
            legacy_state_file: None,
            event_prefix: String::new(),
            workspaces: Arc::new(Mutex::new(HashMap::new())),
            starting: Mutex::new(0),
            closed_while_restoring: Mutex::new(HashSet::new()),
            safe_mode: HashSet::new(),
            mint_ids: true,
        }
    }

    #[cfg(test)]
    fn with_typed_ids(mut self) -> Self {
        self.mint_ids = false;
        self
    }

    pub fn with_events(mut self, hub: EventHub) -> Self {
        self.hub = Some(hub);
        self
    }

    /// Where workspace ids are kept (names, directories, ACP ids: no text) to come back after a restart.
    /// The ids a restore must not resume (`crash::safe_mode_ids`).
    pub fn with_safe_mode(mut self, ids: HashSet<String>) -> Self {
        self.safe_mode = ids;
        self
    }

    pub fn with_state_file(mut self, path: PathBuf) -> Self {
        self.state_file = Some(path);
        self
    }

    /// The state file's old name, read once when the new one is missing; saves go to the new one only.
    pub fn with_legacy_state_file(mut self, path: PathBuf) -> Self {
        self.legacy_state_file = Some(path);
        self
    }

    pub fn with_event_prefix(mut self, prefix: &str) -> Self {
        self.event_prefix = prefix.to_string();
        self
    }

    #[allow(dead_code)] // the wire step that carries a tap or an interrupt from a client calls this
    pub fn status(&self, id: &str) -> Option<WorkspaceStatus> {
        let g = self.workspaces.lock_or_recover();
        let e = g.get(id)?;
        Some(match &e.live {
            Some(l) => l.inner.lock_or_recover().status.clone(),
            None if e.restoring => WorkspaceStatus::Running,
            None => WorkspaceStatus::Failed(e.reason.clone().unwrap_or_default()),
        })
    }

    fn live(&self, id: &str) -> Result<Arc<Live>> {
        let g = self.workspaces.lock_or_recover();
        let e = g.get(id).ok_or_else(|| anyhow!("Unknown workspace"))?;
        match &e.live {
            Some(l) => Ok(l.clone()),
            None if e.restoring => Err(anyhow!("This workspace is still being restored")),
            None => Err(anyhow!("Unknown workspace")),
        }
    }

    /// The windows of workspace `id` and its ACP `sessionId`.
    pub fn layout(&self, id: &str) -> Result<(Layout, Option<String>)> {
        let g = self.workspaces.lock_or_recover();
        let e = g.get(id).ok_or_else(|| anyhow!("Unknown workspace"))?;
        let session = match &e.live {
            Some(l) => l.inner.lock_or_recover().acp_id.clone(),
            None => e.acp_id.clone(),
        };
        Ok((e.layout.clone(), Some(session).filter(|s| !s.is_empty())))
    }

    /// Changes the windows of workspace `id` and writes the state file; an error from `f` leaves them as they were.
    pub fn edit_layout(&self, id: &str, f: impl FnOnce(&mut Layout) -> Result<()>) -> Result<()> {
        {
            let mut g = self.workspaces.lock_or_recover();
            let e = g.get_mut(id).ok_or_else(|| anyhow!("Unknown workspace"))?;
            let mut next = e.layout.clone();
            f(&mut next)?;
            e.layout = next;
        }
        self.save();
        Ok(())
    }

    fn save(&self) {
        let Some(path) = &self.state_file else { return };
        write_state(path, &self.workspaces);
    }

    fn running(&self) -> usize {
        running_in(&self.workspaces.lock_or_recover())
    }

    async fn start(
        &self,
        name: &str,
        label: Option<String>,
        agent: &str,
        dir: &str,
        resume: Option<&str>,
        interrupted: bool,
    ) -> Result<()> {
        let profile = self
            .config
            .agents
            .get(agent)
            .ok_or_else(|| anyhow!("Unknown agent"))?;
        let live = spawn_agent(
            profile,
            agent,
            dir,
            name,
            self.hub.clone(),
            format!("{}{name}", self.event_prefix),
            self.config.event_ring_bytes,
        )?;
        // A start that is dropped before it finishes (its connection went away) ends the agent it began.
        let mut unfinished = KillUnlessDone {
            live: live.clone(),
            done: false,
        };
        let id = match handshake(&live, dir, resume, profile.elicitation_form).await {
            Ok(id) => id,
            Err(e) => {
                if let Some(k) = live.kill.lock_or_recover().take() {
                    let _ = k.send(());
                }
                // The agent refused `session/new` (gemini: -32000 when not logged in): say what to do about it.
                let msg = e.to_string();
                if let (None, Some(why)) = (resume, msg.strip_prefix("session/new: ")) {
                    return Err(anyhow!(
                        "{agent} would not start: {why}. Log in to it on the server, then try again."
                    ));
                }
                return Err(e);
            }
        };
        {
            let mut g = live.inner.lock_or_recover();
            g.log.set_session(&id);
            g.acp_id = id;
        }
        if let Some(path) = self.state_file.clone() {
            let workspaces = self.workspaces.clone();
            *live.saver.lock_or_recover() = Some(Arc::new(move || write_state(&path, &workspaces)));
        }
        if resume.is_some() {
            // `session/load` replays are not shown (they would arrive as one burst), so say what is missing.
            live.stream(|st| {
                st.notice(
                    "info",
                    "Restored after a server restart. Earlier output is not shown.",
                )
            });
        }
        if interrupted {
            // The turn that was running when the server stopped is gone with the old agent process.
            live.stream(|st| st.notice("error", "A server restart interrupted the running turn."));
        }
        live.turn_close(if interrupted {
            AgentState::Error
        } else {
            AgentState::Idle
        });
        // The agent is known from the start (there is no process to scan): Home and the tabs show its icon.
        if let Some(hub) = &self.hub {
            hub.set_agent(&format!("{}{name}", self.event_prefix), agent);
        }
        let mut workspaces = self.workspaces.lock_or_recover();
        // A restore started this workspace from a placeholder that holds its saved windows.
        let placeholder = workspaces.get(name);
        let layout = placeholder.map(|e| e.layout.clone()).unwrap_or_default();
        let label = placeholder.and_then(|e| e.label.clone()).or(label);
        workspaces.insert(
            name.to_string(),
            Entry::running(label, agent, dir, live, layout),
        );
        unfinished.done = true;
        Ok(())
    }

    /// After a restart: brings back the saved workspaces with `session/resume` or `session/load`. A workspace the
    /// agent cannot take back stays in the list with the reason (the view says "Stopped: ...") until it is closed
    /// or the next restore starts it; one over the cap waits the same way.
    ///
    /// Every saved name gets a placeholder entry at once, under the same locks `create_with` takes, so while the
    /// (slow) starts run neither its name nor its slot can be taken by a new workspace.
    pub async fn restore(&self) {
        let Some(path) = &self.state_file else { return };
        let Ok(text) = std::fs::read_to_string(path).or_else(|e| match &self.legacy_state_file {
            Some(old) if !path.exists() => std::fs::read_to_string(old),
            _ => Err(e),
        }) else {
            return;
        };
        let saved: Vec<Saved> = serde_json::from_str(&text).unwrap_or_default();
        let mut todo: Vec<(Saved, StartSlot<'_>)> = Vec::new();
        {
            // The same order as `create_with`: `starting`, then `workspaces`.
            let mut starting = self.starting.lock_or_recover();
            let mut workspaces = self.workspaces.lock_or_recover();
            let mut free = self
                .config
                .max_sessions
                .saturating_sub(running_in(&workspaces) + *starting);
            for s in saved {
                if !valid_name(&s.name) {
                    // Not shown: a hostile name is never put in a list. Logged once, without the name.
                    tracing::warn!("dropped a saved ACP workspace with an invalid name");
                    continue;
                }
                if workspaces.contains_key(&s.name) {
                    continue;
                }
                if !valid_dir(&s.dir) {
                    let why = "The saved directory is not valid".to_string();
                    workspaces.insert(s.name.clone(), Entry::stopped(&s, why, false));
                } else if s.reason.is_some() && !s.retry {
                    // Stopped for a reason a retry will not change: kept as it was.
                    let why = s.reason.clone().unwrap_or_default();
                    workspaces.insert(s.name.clone(), Entry::stopped(&s, why, false));
                } else if self.safe_mode.contains(&s.name) {
                    let why = "Not resumed: the server stopped unexpectedly twice while this was running. Open it to resume.".to_string();
                    workspaces.insert(s.name.clone(), Entry::stopped(&s, why, true));
                } else if free == 0 {
                    let why = format!(
                        "Not restored: at most {} agent workspaces run at once",
                        self.config.max_sessions
                    );
                    workspaces.insert(s.name.clone(), Entry::stopped(&s, why, true));
                } else {
                    free -= 1;
                    // Counted here under the lock; released when this start is over, however it ends.
                    *starting += 1;
                    let slot = StartSlot::counted(&self.starting);
                    let mut e = Entry::stopped(&s, String::new(), true);
                    e.reason = None;
                    e.restoring = true;
                    workspaces.insert(s.name.clone(), e);
                    todo.push((s, slot));
                }
            }
        }
        for (s, slot) in todo {
            // An entry that never started has no conversation to resume: it starts afresh.
            let resume = (!s.acp_id.is_empty()).then_some(s.acp_id.as_str());
            let started = self
                .start(&s.name, None, &s.agent, &s.dir, resume, s.turn_open)
                .await;
            // Closed by its owner while it was starting: what started is ended, and nothing is put back.
            if self
                .closed_while_restoring
                .lock_or_recover()
                .remove(&s.name)
            {
                if started.is_ok() {
                    let entry = self.workspaces.lock_or_recover().remove(&s.name);
                    if let Some(k) = entry
                        .and_then(|e| e.live)
                        .and_then(|l| l.kill.lock_or_recover().take())
                    {
                        let _ = k.send(());
                    }
                }
                drop(slot);
                self.save();
                continue;
            }
            if let Err(e) = started {
                let why = format!("Could not restore: {e}");
                self.workspaces
                    .lock_or_recover()
                    .insert(s.name.clone(), Entry::stopped(&s, why, true));
                if let Some(hub) = &self.hub {
                    hub.report(
                        &format!("{}{}", self.event_prefix, s.name),
                        Source::Acp,
                        Confidence::High,
                        EventKind::Turn {
                            open: false,
                            outcome: Some(AgentState::Error),
                        },
                    );
                }
            }
            drop(slot);
            self.save();
        }
        self.save();
    }

    /// Runs `restore` on its own task. A panic in it must not leave the workspaces it was bringing back "restoring"
    /// for ever: they are shown as stopped, with the reason, and can be started again.
    pub fn spawn_restore(self: &Arc<Self>) {
        let me = self.clone();
        let task = tokio::spawn(async move { me.restore().await });
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = task.await {
                if e.is_panic() {
                    tracing::error!(target: "acp", "restoring the workspaces panicked");
                    let mut workspaces = me.workspaces.lock_or_recover();
                    for entry in workspaces.values_mut().filter(|e| e.restoring) {
                        entry.restoring = false;
                        entry.reason = Some("Could not restore: an internal error".into());
                    }
                }
            }
        });
    }

    /// Beats `/health` every few seconds, through a probe of the workspace table on a blocking thread. A lock that
    /// is never released, or a runtime with no free worker, stops the beat, and `/health` turns 503 (decision 0143).
    /// One probe at a time: a stuck one is not joined by more.
    pub fn spawn_heartbeat(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let probe = me.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    drop(probe.workspaces.lock_or_recover());
                })
                .await;
                crate::health::HEARTBEATS.beat_acp();
            }
        });
    }

    /// The owner's tap on a permission prompt. `option_id` must be one the agent offered and that was shown
    /// (`allow_once` or `reject_once`); it is sent back exactly as it came.
    pub async fn answer_permission(
        &self,
        id: &str,
        prompt_id: &str,
        option_id: &str,
    ) -> Result<()> {
        let live = self.live(id)?;
        {
            let mut g = live.inner.lock_or_recover();
            let perm = g
                .perms
                .get(prompt_id)
                .ok_or_else(|| anyhow!("That question is no longer open"))?;
            if !perm.allowed.iter().any(|a| a == option_id) {
                return Err(anyhow!("No such choice"));
            }
            let perm = g.perms.remove(prompt_id).unwrap();
            live.respond(
                &perm.rpc_id,
                wire(&acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        option_id.to_string(),
                    )),
                )),
            );
        }
        live.stream(|st| st.permission_answered(prompt_id, option_id));
        // Resolved: the turn resumes unless another request is still open.
        live.block(prompt_id, false);
        Ok(())
    }

    /// The workspace view's stream (T0). A workspace with no running agent (being restored, over the cap, or one the
    /// agent could not take back) is shown as such, with the reason.
    fn stream_view(
        &self,
        id: &str,
        after: Option<u64>,
        epoch: Option<u64>,
        info_seen: u64,
    ) -> Result<(AcpView, u64)> {
        let (live, agent, dir, restoring, reason) = {
            let g = self.workspaces.lock_or_recover();
            let e = g.get(id).ok_or_else(|| anyhow!("Unknown workspace"))?;
            (
                e.live.clone(),
                e.agent.clone(),
                e.dir.clone(),
                e.restoring,
                e.reason.clone(),
            )
        };
        match live {
            Some(l) => {
                let mut g = l.inner.lock_or_recover();
                Ok(g.stream.view(id, after, epoch, info_seen))
            }
            None => {
                let mut st = Stream::new(&agent, &dir, 0);
                if restoring {
                    st.notice(
                        "info",
                        "Restoring after a server restart. Open this workspace again in a moment.",
                    );
                } else {
                    // A failure, not "Done": the view says "Stopped: <reason>" and Close works.
                    let why = reason
                        .as_deref()
                        .unwrap_or("The agent could not be restored");
                    st.status("failed", Some(why));
                }
                Ok(st.view(id, after, epoch, info_seen))
            }
        }
    }

    /// `session/set_mode`, or `session/set_config_option` for an agent that lists its modes only as a config option.
    /// Only a mode the agent itself listed is sent.
    pub async fn set_mode(&self, id: &str, mode: &str) -> Result<()> {
        let live = self.live(id)?;
        let (via, ids, acp_id) = {
            let g = live.inner.lock_or_recover();
            (
                g.stream.mode_via().clone(),
                g.stream.mode_ids(),
                g.acp_id.clone(),
            )
        };
        if !ids.iter().any(|m| m == mode) {
            return Err(anyhow!("The agent does not offer that mode"));
        }
        match via {
            ModeVia::Modes => {
                live.request(
                    "session/set_mode",
                    wire(&acp::SetSessionModeRequest::new(
                        acp_id.clone(),
                        mode.to_string(),
                    )),
                    Some(SETUP_TIMEOUT),
                )
                .await?
            }
            ModeVia::Config(config_id) => {
                live.request(
                    "session/set_config_option",
                    wire(&acp::SetSessionConfigOptionRequest::new(
                        acp_id.clone(),
                        config_id.clone(),
                        acp::SessionConfigOptionValue::value_id(mode.to_string()),
                    )),
                    Some(SETUP_TIMEOUT),
                )
                .await?
            }
            ModeVia::None => return Err(anyhow!("This agent has no modes")),
        };
        live.stream(|st| st.set_mode(mode));
        Ok(())
    }

    /// Interrupt: open requests are answered `cancelled`, then `session/cancel` ends the turn.
    pub async fn interrupt(&self, id: &str) -> Result<()> {
        let live = self.live(id)?;
        live.cancel_pending();
        let acp_id = live.inner.lock_or_recover().acp_id.clone();
        live.send(json!({
            "jsonrpc": "2.0", "method": "session/cancel",
            "params": wire(&acp::CancelNotification::new(acp_id.clone()))
        }));
        Ok(())
    }
}

#[async_trait]
impl WorkspaceBackend for AcpBackend {
    fn name(&self) -> &str {
        "acp"
    }

    async fn list(&self) -> Result<Vec<WorkspaceInfo>> {
        let mut v: Vec<WorkspaceInfo> = self
            .workspaces
            .lock_or_recover()
            .iter()
            .map(|(name, e)| WorkspaceInfo {
                id: name.clone(),
                name: e.label.clone().unwrap_or_else(|| name.clone()).into(),
                agent: Some(e.agent.clone()),
                backend: None,
                dir: e.dir.clone(),
                ..Default::default()
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(v)
    }

    async fn create(&self, _name: &str) -> Result<()> {
        Err(anyhow!("Choose an agent for this workspace"))
    }

    async fn create_with(
        &self,
        name: &str,
        _backend: Option<&str>,
        dir: Option<&str>,
        template: Option<&str>,
    ) -> Result<()> {
        // The id is ours; what the client typed is the label.
        let label = clean_label(name)?.ok_or_else(|| anyhow!("A workspace needs a name"))?;
        let id = if self.mint_ids {
            new_id()
        } else {
            name.to_string()
        };
        let name = id.as_str();
        // The client names an agent; the command line comes only from the owner's allowlist.
        let agent = template
            .filter(|t| self.config.agents.contains_key(*t))
            .ok_or_else(|| anyhow!("Unknown agent"))?;
        if !on_path(&self.config.agents[agent].command) {
            return Err(anyhow!("That agent is not installed on this server"));
        }
        let dir = match dir.filter(|d| !d.is_empty()) {
            Some(d) if valid_dir(d) => d.to_string(),
            Some(_) => return Err(anyhow!("The directory must be an absolute path")),
            None => std::env::var("HOME").unwrap_or_else(|_| "/".into()),
        };
        if self.workspaces.lock_or_recover().contains_key(name) {
            return Err(anyhow!("A workspace with that id exists"));
        }
        let slot;
        {
            let mut starting = self.starting.lock_or_recover();
            if self.running() + *starting >= self.config.max_sessions {
                return Err(anyhow!(
                    "At most {} agent workspaces run at once on this server",
                    self.config.max_sessions
                ));
            }
            // A restore holds this lock while it takes the ids it is bringing back.
            if self.workspaces.lock_or_recover().contains_key(name) {
                return Err(anyhow!("A workspace with that id exists"));
            }
            slot = Some(StartSlot::take(&self.starting, &mut starting));
        }
        let r = self
            .start(name, Some(label), agent, &dir, None, false)
            .await;
        drop(slot);
        r?;
        self.save();
        Ok(())
    }

    fn attach(&self, _id: &str, _cols: u16, _rows: u16) -> Result<Attachment> {
        Err(anyhow!("This workspace has no terminal"))
    }

    async fn rename(&self, id: &str, name: &str) -> Result<()> {
        let label = clean_label(name)?;
        self.workspaces
            .lock_or_recover()
            .get_mut(id)
            .ok_or_else(|| anyhow!("Unknown workspace"))?
            .label = label;
        self.save();
        Ok(())
    }

    /// The owner's next prompt.
    async fn send_text(&self, id: &str, text: &str) -> Result<()> {
        let live = self.live(id)?;
        let text = text.trim_end_matches('\r');
        if text.trim().is_empty() || text.len() > MAX_PROMPT {
            return Err(anyhow!("The message is empty or too long"));
        }
        let acp_id = {
            let mut g = live.inner.lock_or_recover();
            if g.status != WorkspaceStatus::Running {
                return Err(anyhow!("The agent is not running"));
            }
            if g.turn {
                return Err(anyhow!("The agent is still answering"));
            }
            g.turn = true;
            g.acp_id.clone()
        };
        // The turn opens first: the history gives a title only to a task that is already open.
        live.turn_open();
        live.emit(EventKind::Task {
            title: first_line(text),
        });
        live.stream(|st| st.user(text));
        live.persist();
        if let Some(hub) = &live.hub {
            hub.record_project(&live.event_name, &live.dir);
        }
        let text = text.to_string();
        let watcher = live.clone();
        let task = tokio::spawn(async move {
            let r = live
                .request(
                    "session/prompt",
                    wire(&acp::PromptRequest::new(
                        acp_id.clone(),
                        vec![acp::ContentBlock::Text(acp::TextContent::new(text.clone()))],
                    )),
                    None,
                )
                .await;
            let tail = {
                let mut g = live.inner.lock_or_recover();
                g.turn = false;
                // A turn that ended drops whatever it had open.
                g.perms.clear();
                g.mapper.flush()
            };
            if let Some(k) = tail {
                live.emit(k);
            }
            // What the agent says this turn used, as it says it. `thoughtTokens` is reported apart from the output
            // count and is not added to it. `usage` is read from the raw result: the schema types it only behind
            // `unstable_end_turn_token_usage`, which stays off.
            if let (Ok(v), Some(hub)) = (&r, &live.hub) {
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64);
                    hub.record_tokens(
                        &live.event_name,
                        crate::history::Tokens {
                            input: n("inputTokens"),
                            output: n("outputTokens"),
                            cache_read: n("cachedReadTokens"),
                            cache_write: n("cachedWriteTokens"),
                        },
                    );
                }
            }
            if live.inner.lock_or_recover().status != WorkspaceStatus::Running {
                return;
            }
            // A result whose stop reason does not decode ends the turn as `Idle`, without a notice.
            let stop = r
                .as_ref()
                .ok()
                .and_then(|v| decode::<acp::PromptResponse>(v, "session/prompt result"))
                .map(|p| stop_reason(p.stop_reason));
            live.stream(|st| match &r {
                Ok(_) => st.turn_end(stop, None),
                Err(e) => st.turn_end(None, Some(&e.to_string())),
            });
            live.persist();
            live.turn_close(match r {
                Ok(_) if stop == Some("end_turn") => AgentState::Done,
                Ok(_) => AgentState::Idle,
                Err(_) => AgentState::Error,
            });
        });
        // A panic in the turn must not leave the workspace "still answering" for good.
        watch_task(watcher, "prompt", task, |l| {
            l.inner.lock_or_recover().turn = false;
            l.stream(|st| {
                st.notice(
                    "error",
                    "The reply stopped because of an internal error. Send the message again.",
                )
            });
            l.turn_close(AgentState::Error);
        });
        Ok(())
    }

    fn acp_agents(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .config
            .agents
            .iter()
            .filter(|(_, a)| on_path(&a.command))
            .map(|(name, _)| name.clone())
            .collect();
        v.sort();
        v
    }

    fn acp_watch(&self, id: &str) -> Result<watch::Receiver<u64>> {
        let g = self.workspaces.lock_or_recover();
        let e = g.get(id).ok_or_else(|| anyhow!("Unknown workspace"))?;
        Ok(match &e.live {
            Some(l) => l.changed.subscribe(),
            // Nothing will ever change: the sender is gone, which the connection reads as "no more updates".
            None => watch::channel(0).1,
        })
    }

    fn acp_view(
        &self,
        id: &str,
        after: Option<u64>,
        epoch: Option<u64>,
        info_seen: u64,
    ) -> Result<(AcpView, u64)> {
        self.stream_view(id, after, epoch, info_seen)
    }

    fn acp_events(
        &self,
        id: &str,
        after: Option<u64>,
        epoch: Option<u64>,
    ) -> Result<crate::event_log::Replay> {
        let live = self.live(id)?;
        let g = live.inner.lock_or_recover();
        Ok(g.log.resume(after, epoch))
    }

    async fn acp_prompt(&self, id: &str, text: &str) -> Result<()> {
        self.send_text(id, text).await
    }

    async fn acp_interrupt(&self, id: &str) -> Result<()> {
        self.interrupt(id).await
    }

    async fn acp_answer(&self, id: &str, prompt_id: &str, option_id: &str) -> Result<()> {
        self.answer_permission(id, prompt_id, option_id).await
    }

    async fn acp_mode(&self, id: &str, mode: &str) -> Result<()> {
        self.set_mode(id, mode).await
    }

    async fn kill(&self, id: &str) -> Result<()> {
        let entry = self
            .workspaces
            .lock_or_recover()
            .remove(id)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        if entry.restoring {
            // Its agent is still being started by `restore`, which will find this and end it.
            self.closed_while_restoring
                .lock_or_recover()
                .insert(id.to_string());
        }
        if let Some(live) = entry.live {
            if let Some(k) = live.kill.lock_or_recover().take() {
                let _ = k.send(());
            }
        }
        self.save();
        Ok(())
    }
}

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;
