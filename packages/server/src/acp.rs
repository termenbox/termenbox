//! ACP sessions the server starts itself (tier T0, decisions 0054, 0103, 0104, 0111). The agent is a stdio child
//! speaking Agent Client Protocol (JSON-RPC 2.0, one message per line); the server owns the child, so a client
//! disconnecting changes nothing. There is no terminal: a session is its event stream and its answers.
//!
//! This is a small client instead of the `agent-client-protocol` crate: the crate (Apache-2.0) brings the whole
//! SDK model and its async runtime glue for the six methods and nine updates used here, which costs binary
//! size and build time on a 512 MB / 1 vCPU host (`resource-constraints`) for no gain.
//!
//! Text from the agent is untrusted (0095): lines are length-capped, chunks are buffered to a fixed size, and
//! everything that reaches the hub is cleaned by `events::sanitize_kind`. Nothing the agent says is audit-logged.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use termenbox_core::events::{
    AgentState, Confidence, Decision, DecisionOption, EventKind, PlanItem, Source, ToolPhase,
};
use termenbox_core::protocol::{AcpView, SessionInfo};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, watch};

use crate::acp_stream::{ModeVia, Stream};
use crate::backend::{valid_dir, valid_name, SessionBackend};
use crate::events::EventHub;
use crate::pty::Attachment;

/// One line from the agent is never longer than this; a longer one is dropped whole.
const MAX_LINE: usize = 1 << 20;
/// Streamed text waiting to become one message or thought event.
const MAX_CHUNK_BUFFER: usize = 4096;
/// A prompt typed by the owner.
const MAX_PROMPT: usize = 16 * 1024;
/// Permission requests open at once per session; further ones are answered `cancelled`.
const MAX_PENDING: usize = 8;
const MAX_TOOLS: usize = 64;
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Sessions at once when the config does not say. An agent child is a Node or Rust process of a few hundred MB
/// (see the measurements in the PR that added this), so the owner sets the real number for the agents they use.
const DEFAULT_MAX_SESSIONS: usize = 2;

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
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AcpConfig {
    #[serde(default = "default_max")]
    pub max_sessions: usize,
    #[serde(default)]
    pub agents: HashMap<String, AgentProfile>,
}

fn default_max() -> usize {
    DEFAULT_MAX_SESSIONS
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            agents: HashMap::new(),
        }
    }
}

impl AcpConfig {
    /// ```toml
    /// max_sessions = 2
    /// [agents.claude]
    /// command = "claude-agent-acp"
    /// env_remove = ["CLAUDECODE"]
    /// ```
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| anyhow!("acp agents file: {e}"))?;
        if !(1..=16).contains(&cfg.max_sessions) {
            return Err(anyhow!("max_sessions must be 1 to 16"));
        }
        for (name, a) in &cfg.agents {
            if !valid_name(name) || a.command.is_empty() {
                return Err(anyhow!("agent `{name}` needs a plain name and a command"));
            }
        }
        Ok(cfg)
    }

    /// A missing file is an empty allowlist: no ACP agent can be started.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(anyhow!("cannot read {}: {e}", path.display())),
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

    fn chunk(&mut self, kind: Chunk, u: &Value) -> Vec<EventKind> {
        let mut out = Vec::new();
        if self.buf.as_ref().map(|(k, _)| *k != kind).unwrap_or(false) {
            out.extend(self.flush());
        }
        let content = u.get("content").unwrap_or(&Value::Null);
        if s(content, "type") == "text" {
            let buf = &mut self.buf.get_or_insert((kind, String::new())).1;
            let room = MAX_CHUNK_BUFFER.saturating_sub(buf.len());
            buf.push_str(cap(s(content, "text"), room));
        }
        out
    }

    pub fn map(&mut self, u: &Value) -> Vec<EventKind> {
        let kind = s(u, "sessionUpdate");
        match kind {
            "agent_message_chunk" => return self.chunk(Chunk::Message, u),
            "agent_thought_chunk" => return self.chunk(Chunk::Thought, u),
            _ => {}
        }
        let mut out: Vec<EventKind> = self.flush().into_iter().collect();
        match kind {
            "tool_call" => {
                let id = s(u, "toolCallId").to_string();
                let name = match s(u, "kind") {
                    "" => "tool".to_string(),
                    k => k.to_string(),
                };
                let summary = first_line(s(u, "title"));
                if self.tools.len() >= MAX_TOOLS {
                    self.tools.clear();
                }
                let done = matches!(s(u, "status"), "completed" | "failed");
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
            "tool_call_update" => {
                let id = s(u, "toolCallId");
                let (name, mut summary) = self
                    .tools
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| ("tool".into(), String::new()));
                if !s(u, "title").is_empty() {
                    summary = first_line(s(u, "title"));
                }
                if matches!(s(u, "status"), "completed" | "failed") {
                    self.tools.remove(id);
                    out.push(EventKind::Tool {
                        name,
                        summary,
                        phase: ToolPhase::Finished,
                    });
                } else if let Some(t) = self.tools.get_mut(id) {
                    t.1 = summary;
                }
            }
            "plan" => {
                let items: Vec<PlanItem> = u
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .take(crate::events::PLAN_ITEMS)
                            .map(|e| PlanItem {
                                text: s(e, "content").to_string(),
                                status: s(e, "status").to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !items.is_empty() {
                    out.push(EventKind::Plan { items });
                }
            }
            "usage_update" => {
                if let (Some(used), Some(size)) = (
                    u.get("used").and_then(Value::as_u64),
                    u.get("size").and_then(Value::as_u64),
                ) {
                    let cost = u.get("cost").filter(|c| c.is_object()).map(|c| {
                        format!(
                            "{} {}",
                            c.get("amount").map(|a| a.to_string()).unwrap_or_default(),
                            s(c, "currency")
                        )
                    });
                    out.push(EventKind::Usage { used, size, cost });
                }
            }
            "session_info_update" => {
                if let Some(title) = u.get("title").and_then(Value::as_str) {
                    out.push(EventKind::Title {
                        title: title.to_string(),
                    });
                }
            }
            "current_mode_update" => out.push(EventKind::Mode {
                mode: s(u, "currentModeId").to_string(),
            }),
            "available_commands_update" => out.push(EventKind::Commands {
                names: u
                    .get("availableCommands")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|c| s(c, "name").to_string()).collect())
                    .unwrap_or_default(),
            }),
            _ => {}
        }
        out
    }
}

/// The options of a `session/request_permission` that may be shown: `allow_once` and `reject_once` only
/// (decision 0104); `allow_always`, `reject_always` and anything unknown are dropped.
pub fn permission_options(params: &Value) -> Vec<DecisionOption> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|o| matches!(s(o, "kind"), "allow_once" | "reject_once"))
                .filter(|o| !s(o, "optionId").is_empty())
                .map(|o| DecisionOption {
                    id: s(o, "optionId").to_string(),
                    kind: s(o, "kind").to_string(),
                    name: s(o, "name").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------
// One session: the child, its two pipes and the state they share
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
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
    status: SessionStatus,
    next_id: u64,
    waiting: HashMap<u64, oneshot::Sender<std::result::Result<Value, Value>>>,
    perms: HashMap<String, Perm>,
    perm_seq: u64,
    mapper: UpdateMapper,
    acp_id: String,
    turn: bool,
    /// `session/load` replays history as updates; those are not news.
    replaying: bool,
    /// What the session view shows (T0): kept here, bounded, and handed to a client on `acp_open`.
    stream: Stream,
}

struct Live {
    event_name: String,
    /// The folder the agent runs in (its last component is kept in the history).
    dir: String,
    hub: Option<EventHub>,
    inner: Mutex<Inner>,
    /// The stream's revision, for the connections that show this session.
    changed: watch::Sender<u64>,
    out: mpsc::UnboundedSender<String>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
}

impl Live {
    fn emit(&self, kind: EventKind) {
        if let Some(hub) = &self.hub {
            hub.report(&self.event_name, Source::Acp, Confidence::High, kind);
        }
    }

    /// Runs `f` on the stream and wakes the connections that show this session.
    fn stream<R>(&self, f: impl FnOnce(&mut Stream) -> R) -> R {
        let (r, rev) = {
            let mut g = self.inner.lock().unwrap();
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
        let _ = self.out.send(v.to_string());
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
            let mut g = self.inner.lock().unwrap();
            if g.status != SessionStatus::Running {
                return Err(anyhow!("the agent is not running"));
            }
            g.next_id += 1;
            let id = g.next_id;
            g.waiting.insert(id, tx);
            id
        };
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = match timeout {
            Some(t) => tokio::time::timeout(t, rx)
                .await
                .map_err(|_| anyhow!("the agent did not answer {method}"))?,
            None => rx.await,
        };
        match reply {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(anyhow!("{method}: {}", cap(s(&e, "message"), 200))),
            Err(_) => Err(anyhow!("the agent exited")),
        }
    }

    fn on_message(&self, v: Value) {
        let method = s(&v, "method").to_string();
        let id = v.get("id").cloned();
        match (method.as_str(), id) {
            ("", Some(id)) => {
                let Some(n) = id.as_u64() else { return };
                let tx = self.inner.lock().unwrap().waiting.remove(&n);
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
                "error": {"code": -32601, "message": "Method not found"}
            })),
            ("session/update", None) => self.on_update(&v["params"]["update"]),
            _ => {}
        }
    }

    fn on_update(&self, update: &Value) {
        let (kinds, rev) = {
            let mut g = self.inner.lock().unwrap();
            if g.replaying {
                return;
            }
            g.stream.update(update);
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
            let mut g = self.inner.lock().unwrap();
            if g.perms.len() >= MAX_PENDING {
                drop(g);
                return self.respond(&rpc_id, json!({"outcome": {"outcome": "cancelled"}}));
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
        let tool = params.get("toolCall").unwrap_or(&Value::Null);
        let detail = Some(first_line(s(tool, "title"))).filter(|d| !d.is_empty());
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
            .lock()
            .unwrap()
            .perms
            .drain()
            .map(|(_, p)| p)
            .collect();
        for p in perms {
            self.respond(&p.rpc_id, json!({"outcome": {"outcome": "cancelled"}}));
        }
        self.stream(|st| st.cancel_open_permissions());
    }

    /// The process is gone (or was told to go): nothing answers any more.
    fn finish(&self, status: SessionStatus) {
        let was = {
            let mut g = self.inner.lock().unwrap();
            g.waiting.clear();
            g.perms.clear();
            std::mem::replace(&mut g.status, status.clone())
        };
        if was == SessionStatus::Running {
            self.stream(|st| match &status {
                SessionStatus::Failed(text) => st.status("failed", Some(text)),
                _ => st.status("ended", None),
            });
            self.turn_close(match status {
                SessionStatus::Failed(_) => AgentState::Error,
                _ => AgentState::Done,
            });
        }
    }
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
) -> Result<Arc<Live>> {
    let mut cmd = Command::new(&profile.command);
    cmd.args(&profile.args)
        .envs(crate::locale::utf8_env())
        .envs(&profile.env)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for k in &profile.env_remove {
        cmd.env_remove(k);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("cannot start the agent for {name}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let (out, mut out_rx) = mpsc::unbounded_channel::<String>();
    let (kill_tx, mut kill_rx) = oneshot::channel::<()>();
    let live = Arc::new(Live {
        event_name,
        dir: dir.to_string(),
        hub,
        changed: watch::channel(0).0,
        inner: Mutex::new(Inner {
            status: SessionStatus::Running,
            next_id: 0,
            waiting: HashMap::new(),
            perms: HashMap::new(),
            perm_seq: 0,
            mapper: UpdateMapper::default(),
            acp_id: String::new(),
            turn: false,
            replaying: false,
            stream: Stream::new(agent, dir, crate::events::now_ms()),
        }),
        out,
        kill: Mutex::new(Some(kill_tx)),
    });
    tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err()
                || stdin.write_all(b"\n").await.is_err()
                || stdin.flush().await.is_err()
            {
                break;
            }
        }
    });
    let l = live.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            tokio::select! {
                _ = &mut kill_rx => {
                    let _ = child.kill().await;
                    l.finish(SessionStatus::Ended);
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
        let status = match child.wait().await {
            Ok(st) => SessionStatus::Failed(st.to_string()),
            Err(e) => SessionStatus::Failed(e.to_string()),
        };
        l.finish(status);
    });
    Ok(live)
}

/// `initialize`, then `session/new` or (after a restart) `session/resume` / `session/load`.
async fn handshake(live: &Live, dir: &str, resume: Option<&str>) -> Result<String> {
    let init = live
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                // No file system and no terminal: the agent runs its own tools (decision 0111).
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": "termenbox", "version": env!("CARGO_PKG_VERSION")},
            }),
            Some(SETUP_TIMEOUT),
        )
        .await?;
    let caps = &init["agentCapabilities"];
    let params = |id: &str| json!({"sessionId": id, "cwd": dir, "mcpServers": []});
    match resume {
        None => {
            let r = live
                .request(
                    "session/new",
                    json!({"cwd": dir, "mcpServers": []}),
                    Some(SETUP_TIMEOUT),
                )
                .await?;
            live.stream(|st| st.session_state(&r));
            Ok(s(&r, "sessionId").to_string())
        }
        Some(id) if caps["sessionCapabilities"].get("resume").is_some() => {
            let r = live
                .request("session/resume", params(id), Some(SETUP_TIMEOUT))
                .await?;
            live.stream(|st| st.session_state(&r));
            Ok(id.to_string())
        }
        Some(id) if caps["loadSession"].as_bool() == Some(true) => {
            live.inner.lock().unwrap().replaying = true;
            let r = live
                .request("session/load", params(id), Some(SETUP_TIMEOUT))
                .await;
            let r = {
                let mut g = live.inner.lock().unwrap();
                g.replaying = false;
                g.mapper = UpdateMapper::default();
                r?
            };
            live.stream(|st| st.session_state(&r));
            Ok(id.to_string())
        }
        Some(_) => Err(anyhow!("the agent can neither resume nor load a session")),
    }
}

// ---------------------------------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Saved {
    name: String,
    agent: String,
    dir: String,
    acp_id: String,
}

struct Entry {
    agent: String,
    dir: String,
    /// `None` for a session that could not be brought back after a restart.
    live: Option<Arc<Live>>,
}

pub struct AcpBackend {
    config: AcpConfig,
    hub: Option<EventHub>,
    state_file: Option<PathBuf>,
    event_prefix: String,
    sessions: Mutex<HashMap<String, Entry>>,
    /// Sessions being started, counted against the cap so two creates cannot both pass it.
    starting: Mutex<usize>,
}

impl AcpBackend {
    pub fn new(config: AcpConfig) -> Self {
        Self {
            config,
            hub: None,
            state_file: None,
            event_prefix: String::new(),
            sessions: Mutex::new(HashMap::new()),
            starting: Mutex::new(0),
        }
    }

    pub fn with_events(mut self, hub: EventHub) -> Self {
        self.hub = Some(hub);
        self
    }

    /// Where session ids are kept (names, directories, ACP ids: no text) to come back after a restart.
    pub fn with_state_file(mut self, path: PathBuf) -> Self {
        self.state_file = Some(path);
        self
    }

    pub fn with_event_prefix(mut self, prefix: &str) -> Self {
        self.event_prefix = prefix.to_string();
        self
    }

    #[allow(dead_code)] // the wire step that carries a tap or an interrupt from a client calls this
    pub fn status(&self, id: &str) -> Option<SessionStatus> {
        let g = self.sessions.lock().unwrap();
        let e = g.get(id)?;
        Some(match &e.live {
            Some(l) => l.inner.lock().unwrap().status.clone(),
            None => SessionStatus::Ended,
        })
    }

    fn live(&self, id: &str) -> Result<Arc<Live>> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .and_then(|e| e.live.clone())
            .ok_or_else(|| anyhow!("Unknown session"))
    }

    fn save(&self) {
        let Some(path) = &self.state_file else { return };
        let saved: Vec<Saved> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(name, e)| {
                let l = e.live.as_ref()?;
                let g = l.inner.lock().unwrap();
                (g.status == SessionStatus::Running).then(|| Saved {
                    name: name.clone(),
                    agent: e.agent.clone(),
                    dir: e.dir.clone(),
                    acp_id: g.acp_id.clone(),
                })
            })
            .collect();
        if let Ok(text) = serde_json::to_string(&saved) {
            let _ = std::fs::write(path, text);
        }
    }

    fn running(&self) -> usize {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|e| {
                e.live
                    .as_ref()
                    .map(|l| l.inner.lock().unwrap().status == SessionStatus::Running)
                    .unwrap_or(false)
            })
            .count()
    }

    async fn start(&self, name: &str, agent: &str, dir: &str, resume: Option<&str>) -> Result<()> {
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
        )?;
        let id = match handshake(&live, dir, resume).await {
            Ok(id) => id,
            Err(e) => {
                if let Some(k) = live.kill.lock().unwrap().take() {
                    let _ = k.send(());
                }
                return Err(e);
            }
        };
        live.inner.lock().unwrap().acp_id = id;
        if resume.is_some() {
            // `session/load` replays are not shown (they would arrive as one burst), so say what is missing.
            live.stream(|st| {
                st.notice(
                    "info",
                    "Restored after a server restart. Earlier output is not shown.",
                )
            });
        }
        live.turn_close(AgentState::Idle);
        // The agent is known from the start (there is no process to scan): Home and the tabs show its icon.
        if let Some(hub) = &self.hub {
            hub.set_agent(&format!("{}{name}", self.event_prefix), agent);
        }
        self.sessions.lock().unwrap().insert(
            name.to_string(),
            Entry {
                agent: agent.to_string(),
                dir: dir.to_string(),
                live: Some(live),
            },
        );
        Ok(())
    }

    /// After a restart: brings back the saved sessions with `session/resume` or `session/load`; a session the
    /// agent cannot take back is marked ended (it stays in the list until closed).
    pub async fn restore(&self) {
        let Some(path) = &self.state_file else { return };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let saved: Vec<Saved> = serde_json::from_str(&text).unwrap_or_default();
        for s in saved.into_iter().take(self.config.max_sessions) {
            if !valid_name(&s.name) || !valid_dir(&s.dir) {
                continue;
            }
            if self
                .start(&s.name, &s.agent, &s.dir, Some(&s.acp_id))
                .await
                .is_err()
            {
                self.sessions.lock().unwrap().insert(
                    s.name.clone(),
                    Entry {
                        agent: s.agent,
                        dir: s.dir,
                        live: None,
                    },
                );
                if let Some(hub) = &self.hub {
                    hub.report(
                        &format!("{}{}", self.event_prefix, s.name),
                        Source::Acp,
                        Confidence::High,
                        EventKind::Turn {
                            open: false,
                            outcome: Some(AgentState::Done),
                        },
                    );
                }
            }
        }
        self.save();
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
            let mut g = live.inner.lock().unwrap();
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
                json!({"outcome": {"outcome": "selected", "optionId": option_id}}),
            );
        }
        live.stream(|st| st.permission_answered(prompt_id, option_id));
        // Resolved: the turn resumes unless another request is still open.
        live.block(prompt_id, false);
        Ok(())
    }

    /// The session view's stream (T0). A session that could not be brought back after a restart has no agent, so it
    /// is shown as ended.
    fn stream_view(
        &self,
        id: &str,
        after: Option<u64>,
        epoch: Option<u64>,
        info_seen: u64,
    ) -> Result<(AcpView, u64)> {
        let (live, agent, dir) = {
            let g = self.sessions.lock().unwrap();
            let e = g.get(id).ok_or_else(|| anyhow!("Unknown session"))?;
            (e.live.clone(), e.agent.clone(), e.dir.clone())
        };
        match live {
            Some(l) => {
                let mut g = l.inner.lock().unwrap();
                Ok(g.stream.view(id, after, epoch, info_seen))
            }
            None => {
                let mut st = Stream::new(&agent, &dir, 0);
                st.status("ended", None);
                Ok(st.view(id, after, epoch, info_seen))
            }
        }
    }

    /// `session/set_mode`, or `session/set_config_option` for an agent that lists its modes only as a config option.
    /// Only a mode the agent itself listed is sent.
    pub async fn set_mode(&self, id: &str, mode: &str) -> Result<()> {
        let live = self.live(id)?;
        let (via, ids, acp_id) = {
            let g = live.inner.lock().unwrap();
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
                    json!({"sessionId": acp_id, "modeId": mode}),
                    Some(SETUP_TIMEOUT),
                )
                .await?
            }
            ModeVia::Config(config_id) => {
                live.request(
                    "session/set_config_option",
                    json!({"sessionId": acp_id, "configId": config_id, "value": mode}),
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
        let acp_id = live.inner.lock().unwrap().acp_id.clone();
        live.send(
            json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": acp_id}}),
        );
        Ok(())
    }
}

#[async_trait]
impl SessionBackend for AcpBackend {
    fn name(&self) -> &str {
        "acp"
    }

    async fn list(&self) -> Result<Vec<SessionInfo>> {
        let mut v: Vec<SessionInfo> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(name, e)| SessionInfo {
                id: name.clone(),
                name: name.clone(),
                agent: Some(e.agent.clone()),
                backend: None,
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(v)
    }

    async fn create(&self, _name: &str) -> Result<()> {
        Err(anyhow!("Choose an agent for this session"))
    }

    async fn create_with(
        &self,
        name: &str,
        _backend: Option<&str>,
        dir: Option<&str>,
        template: Option<&str>,
    ) -> Result<()> {
        if !valid_name(name) {
            return Err(anyhow!(
                "Session names use letters, digits, - and _ (max 64)"
            ));
        }
        // The client names an agent; the command line comes only from the owner's allowlist.
        let agent = template
            .filter(|t| self.config.agents.contains_key(*t))
            .ok_or_else(|| anyhow!("Unknown agent"))?;
        let dir = match dir.filter(|d| !d.is_empty()) {
            Some(d) if valid_dir(d) => d.to_string(),
            Some(_) => return Err(anyhow!("The directory must be an absolute path")),
            None => std::env::var("HOME").unwrap_or_else(|_| "/".into()),
        };
        if self.sessions.lock().unwrap().contains_key(name) {
            return Err(anyhow!("A session with that name exists"));
        }
        {
            let mut starting = self.starting.lock().unwrap();
            if self.running() + *starting >= self.config.max_sessions {
                return Err(anyhow!(
                    "At most {} agent sessions run at once on this server",
                    self.config.max_sessions
                ));
            }
            *starting += 1;
        }
        let r = self.start(name, agent, &dir, None).await;
        *self.starting.lock().unwrap() -= 1;
        r?;
        self.save();
        Ok(())
    }

    fn attach(&self, _id: &str, _cols: u16, _rows: u16) -> Result<Attachment> {
        Err(anyhow!("This session has no terminal"))
    }

    /// The owner's next prompt.
    async fn send_text(&self, id: &str, text: &str) -> Result<()> {
        let live = self.live(id)?;
        let text = text.trim_end_matches('\r');
        if text.trim().is_empty() || text.len() > MAX_PROMPT {
            return Err(anyhow!("The message is empty or too long"));
        }
        let acp_id = {
            let mut g = live.inner.lock().unwrap();
            if g.status != SessionStatus::Running {
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
        if let Some(hub) = &live.hub {
            hub.record_project(&live.event_name, &live.dir);
        }
        let text = text.to_string();
        tokio::spawn(async move {
            let r = live
                .request(
                    "session/prompt",
                    json!({"sessionId": acp_id, "prompt": [{"type": "text", "text": text}]}),
                    None,
                )
                .await;
            let tail = {
                let mut g = live.inner.lock().unwrap();
                g.turn = false;
                // A turn that ended drops whatever it had open.
                g.perms.clear();
                g.mapper.flush()
            };
            if let Some(k) = tail {
                live.emit(k);
            }
            // What the agent says this turn used, as it says it. `thoughtTokens` is reported apart from the output
            // count and is not added to it.
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
            if live.inner.lock().unwrap().status != SessionStatus::Running {
                return;
            }
            live.stream(|st| match &r {
                Ok(v) => st.turn_end(v["stopReason"].as_str(), None),
                Err(e) => st.turn_end(None, Some(&e.to_string())),
            });
            live.turn_close(match r {
                Ok(v) if s(&v, "stopReason") == "end_turn" => AgentState::Done,
                Ok(_) => AgentState::Idle,
                Err(_) => AgentState::Error,
            });
        });
        Ok(())
    }

    fn acp_agents(&self) -> Vec<String> {
        let mut v: Vec<String> = self.config.agents.keys().cloned().collect();
        v.sort();
        v
    }

    fn acp_watch(&self, id: &str) -> Result<watch::Receiver<u64>> {
        let g = self.sessions.lock().unwrap();
        let e = g.get(id).ok_or_else(|| anyhow!("Unknown session"))?;
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
            .sessions
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| anyhow!("Unknown session"))?;
        if let Some(live) = entry.live {
            if let Some(k) = live.kill.lock().unwrap().take() {
                let _ = k.send(());
            }
        }
        self.save();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
