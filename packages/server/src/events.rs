//! The agent event hub (plan v6, P.2): sanitises, stamps and stores what agents report, and fans it out.
//! Agent text is untrusted: it is cleaned before it is stored, logged or sent anywhere.
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use termenbox_core::events::{
    Activity, AgentEvent, AgentState, Attention, AttentionKind, Confidence, EventKind, PlanItem,
    Source, Task, ToolPhase, WorkspaceActivity, WorkspaceAgentState,
};
use tokio::sync::broadcast;

use crate::reducer::{Edge, Reducer, Screen};
use crate::tabstatus::TabStatus;

pub const RING: usize = 50;
pub const MAX_SESSIONS: usize = 20;
/// How long an answer counts as the cause of a question clearing.
const ANSWERER_MS: u64 = 10 * 60 * 1000;
pub const SUMMARY_CHARS: usize = 120;
pub const PLAN_ITEM_CHARS: usize = 200;
pub const PLAN_ITEMS: usize = 20;
pub const STALE_AFTER_MS: u64 = 60_000;
/// Attention items kept in memory; the oldest go first.
pub const MAX_ATTENTION: usize = 200;
pub const TASK_CHARS: usize = 120;

// ---------------------------------------------------------------------------------------------------
// Sanitiser
// ---------------------------------------------------------------------------------------------------

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}

fn scrub_token(tok: &str) -> Option<&'static str> {
    let core = tok.trim_matches(|c| matches!(c, '.' | ':' | '-'));
    let long = |s: &str| {
        s.len() >= 32
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "sk-"]
        .iter()
        .any(|p| core.starts_with(p) && core.len() >= p.len() + 20)
    {
        return Some("[token]");
    }
    // certificate: two long base64url halves joined by a dot
    if let Some((a, b)) = core.split_once('.') {
        let half = |s: &str| {
            s.len() >= 16
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        };
        if half(a) && half(b) {
            return Some("[cert]");
        }
    }
    if long(core) {
        return Some("[secret]");
    }
    let host_port = core
        .rsplit_once(':')
        .filter(|(_, p)| (2..=5).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()));
    let ip4 = |s: &str| {
        let parts: Vec<&str> = s.split('.').collect();
        parts.len() == 4
            && parts
                .iter()
                .all(|p| (1..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()))
    };
    if ip4(core) || host_port.map(|(h, _)| ip4(h)).unwrap_or(false) {
        return Some("[ip]");
    }
    if core.matches(':').count() >= 2
        && core
            .split(':')
            .all(|g| g.len() <= 4 && g.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return Some("[ip]");
    }
    if let Some((h, _)) = host_port {
        if h.contains('.')
            && h.rsplit('.')
                .next()
                .map(|t| t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()))
                .unwrap_or(false)
        {
            return Some("[host]");
        }
    }
    None
}

/// Control characters become spaces (so an escape sequence or newline cannot reshape a log line or a
/// card), secrets and addresses are replaced, whitespace is collapsed and the result is capped at `max`.
pub fn sanitize_text(input: &str, max: usize) -> String {
    let flat: String = input
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let out = scrub_spans(&flat);
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}

/// Several lines of untrusted text (a Markdown reply, a diff) for the ACP workspace view: newlines and the
/// indentation stay (a tab is two spaces), every other control character (so every escape sequence) becomes a
/// space, zero-width and bidirectional-override characters are dropped, secrets and addresses are replaced
/// exactly as in `sanitize_text`, and the result is cut at `max` characters and at most `max_lines` lines. Runs
/// of blank lines collapse to one.
pub fn sanitize_block(input: &str, max: usize, max_lines: usize) -> String {
    sanitize_block_cut(input, max, max_lines).0
}

/// `sanitize_block`, and whether anything but blank space was left out because of `max` or `max_lines`.
pub fn sanitize_block_cut(input: &str, max: usize, max_lines: usize) -> (String, bool) {
    let mut out = String::new();
    // The characters in `out`, kept as lines are added: counting `out` again after every line is quadratic.
    let mut chars = 0usize;
    let mut blank = false;
    let mut lines = 0;
    let mut cut = false;
    let text = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut rest = text.split('\n');
    let has_more = |rest: std::str::Split<'_, char>| rest.into_iter().any(|l| !l.trim().is_empty());
    while let Some(raw) = rest.next() {
        let flat: String = raw
            .replace('\t', "  ")
            .chars()
            .filter(|c| !is_invisible(*c))
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let line = scrub_spans(&flat).trim_end().to_string();
        if line.is_empty() {
            if blank {
                continue;
            }
            blank = true;
        } else {
            blank = false;
        }
        if lines == max_lines {
            cut = !line.is_empty() || has_more(rest.clone());
            break;
        }
        if lines > 0 {
            out.push('\n');
            chars += 1;
        }
        out.push_str(&line);
        chars += line.chars().count();
        lines += 1;
        if chars >= max {
            cut = chars > max || has_more(rest.clone());
            break;
        }
    }
    out.truncate(out.trim_end().len());
    (out.chars().take(max).collect(), cut)
}

/// Zero-width and bidirectional-override characters: they can reorder or hide text, so they are dropped.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

/// Replaces tokens that look like secrets or addresses, keeping every other character (and so all spacing).
fn scrub_spans(flat: &str) -> String {
    // Values that follow an authorization keyword are secrets whatever they look like.
    let mut out = String::new();
    let mut hide_next = 0u8;
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String, hide_next: &mut u8| {
        if tok.is_empty() {
            return;
        }
        let lower = tok.to_ascii_lowercase();
        let word = lower.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if *hide_next > 0 && !word.is_empty() {
            out.push_str("[secret]");
            *hide_next -= 1;
        } else if let Some(r) = scrub_token(tok) {
            out.push_str(r);
        } else {
            out.push_str(tok);
        }
        if matches!(
            word,
            "authorization"
                | "bearer"
                | "password"
                | "passwd"
                | "token"
                | "secret"
                | "apikey"
                | "api_key"
                | "--token"
                | "--password"
        ) {
            *hide_next = if word == "authorization" { 2 } else { 1 };
        }
        tok.clear();
    };
    for c in flat.chars() {
        if is_token_char(c) {
            tok.push(c);
        } else {
            flush(&mut tok, &mut out, &mut hide_next);
            out.push(c);
        }
    }
    flush(&mut tok, &mut out, &mut hide_next);
    out
}

/// An option id is echoed back to the agent unchanged, so it is kept exact or dropped: never rewritten.
fn sanitize_option_id(id: &str) -> String {
    if id.len() <= 80 && !id.chars().any(|c| c.is_control()) {
        id.to_string()
    } else {
        String::new()
    }
}

/// Cleans every text field of an event and enforces the plan limits.
pub fn sanitize_kind(kind: EventKind) -> EventKind {
    match kind {
        EventKind::Tool {
            name,
            summary,
            phase,
        } => EventKind::Tool {
            name: sanitize_text(&name, 40),
            summary: sanitize_text(&summary, SUMMARY_CHARS),
            phase,
        },
        EventKind::Message { summary } => EventKind::Message {
            summary: sanitize_text(&summary, SUMMARY_CHARS),
        },
        EventKind::Plan { items } => EventKind::Plan {
            items: items
                .into_iter()
                .take(PLAN_ITEMS)
                .map(|i| PlanItem {
                    text: sanitize_text(&i.text, PLAN_ITEM_CHARS),
                    status: sanitize_text(&i.status, 20),
                })
                .collect(),
        },
        EventKind::Prompt {
            id,
            prompt_kind,
            choices,
            decision,
        } => EventKind::Prompt {
            id: sanitize_text(&id, 40),
            prompt_kind: sanitize_text(&prompt_kind, 20),
            choices: choices
                .iter()
                .take(6)
                .map(|c| sanitize_text(c, 40))
                .collect(),
            decision: decision.map(|d| termenbox_core::events::Decision {
                // Only the two one-time kinds ever reach a client (decision 0104).
                options: d
                    .options
                    .into_iter()
                    .filter(|o| o.kind == "allow_once" || o.kind == "reject_once")
                    .take(4)
                    .map(|o| termenbox_core::events::DecisionOption {
                        id: sanitize_option_id(&o.id),
                        kind: o.kind,
                        name: sanitize_text(&o.name, 40),
                    })
                    .collect(),
                detail: d.detail.map(|t| sanitize_text(&t, SUMMARY_CHARS)),
            }),
        },
        EventKind::Title { title } => EventKind::Title {
            title: sanitize_text(&title, TASK_CHARS),
        },
        EventKind::Thought { summary } => EventKind::Thought {
            summary: sanitize_text(&summary, SUMMARY_CHARS),
        },
        EventKind::Mode { mode } => EventKind::Mode {
            mode: sanitize_text(&mode, 40),
        },
        EventKind::Commands { names } => EventKind::Commands {
            names: names
                .iter()
                .take(40)
                .map(|n| sanitize_text(n, 40))
                .collect(),
        },
        EventKind::Task { title } => EventKind::Task {
            title: sanitize_text(&title, TASK_CHARS),
        },
        other => other,
    }
}

// ---------------------------------------------------------------------------------------------------
// Hub
// ---------------------------------------------------------------------------------------------------

struct Ring {
    events: VecDeque<AgentEvent>,
    seq: u64,
    state: AgentState,
    state_seq: u64,
    last_event_at: u64,
    last_response_at: Option<u64>,
    waiting_since: Option<u64>,
    /// The latest state and when it was reported, per source tier: hook, screen, scan (decision 0095).
    by_source: [Option<(AgentState, u64)>; 3],
    task: Option<Task>,
}

/// The workspace's state from its tabs' reducers: a wait beats work, work beats a result, and with nothing live the
/// tab that spoke last decides how things ended.
fn aggregate(reducers: &HashMap<(String, String), Reducer>, workspace: &str) -> Option<AgentState> {
    let mut tabs = reducers
        .iter()
        .filter(|((w, _), _)| w == workspace)
        .map(|(_, r)| r);
    let mut working = false;
    let mut latest: Option<&Reducer> = None;
    for r in tabs.by_ref() {
        if r.blocked() {
            return Some(AgentState::NeedsInput);
        }
        working |= r.working();
        if latest.is_none_or(|l| r.last_edge_at() > l.last_edge_at()) {
            latest = Some(r);
        }
    }
    if working {
        Some(AgentState::Working)
    } else {
        latest.and_then(Reducer::state)
    }
}

/// The reducer edge an event stands for. Tool events count only from hooks: an ACP tool call is not a turn edge.
fn edge_of(source: Source, kind: &EventKind) -> Option<Edge> {
    match kind {
        EventKind::Turn { open: true, .. } => Some(Edge::TurnOpen),
        EventKind::Turn {
            open: false,
            outcome,
        } => Some(Edge::TurnClose(match outcome {
            Some(s @ (AgentState::Done | AgentState::Error | AgentState::Idle)) => *s,
            _ => AgentState::Idle,
        })),
        EventKind::Block { key, open: true } => Some(Edge::BlockOpen(key.clone())),
        EventKind::Block { key, open: false } => Some(Edge::BlockClose(key.clone())),
        EventKind::Tool {
            phase: ToolPhase::Started,
            ..
        } if source == Source::Hook => Some(Edge::ToolStart),
        EventKind::Tool {
            name,
            phase: ToolPhase::Finished,
            ..
        } if source == Source::Hook => Some(Edge::ToolEnd(name.clone())),
        _ => None,
    }
}

/// Hook and Codex report facts; screen and scan only suspect. Lower index wins.
/// The hook events after which a turn's counts are read from the transcript.
fn collects(event: &str) -> bool {
    matches!(
        event,
        "Stop" | "StopFailure" | "SessionEnd" | "UserPromptSubmit"
    )
}

fn tier(source: Source) -> Option<usize> {
    match source {
        Source::Hook | Source::Codex | Source::Acp => Some(0),
        Source::Screen => Some(1),
        Source::Scan => Some(2),
        Source::Unknown => None,
    }
}

fn activity_of(state: AgentState) -> Activity {
    match state {
        AgentState::Working => Activity::Working,
        AgentState::NeedsInput => Activity::Blocked,
        AgentState::Idle | AgentState::Done | AgentState::Error => Activity::Idle,
        AgentState::Unknown => Activity::Unknown,
    }
}

/// The Activity of the freshest signal of the highest tier that is still live. A `working` signal that has
/// been quiet for a minute is not live; if no tier is, the Activity is unknown.
fn derive_activity(r: &Ring, now: u64) -> Activity {
    for (state, ts) in r.by_source.iter().flatten() {
        let a = activity_of(*state);
        if a == Activity::Working && now.saturating_sub(*ts) > STALE_AFTER_MS {
            continue;
        }
        return a;
    }
    Activity::Unknown
}

/// What screen or scan suspect, when a live signal of theirs says the agent waits or failed.
fn derive_hint(r: &Ring, now: u64) -> Option<AttentionKind> {
    r.by_source[1..]
        .iter()
        .flatten()
        .filter(|(_, ts)| now.saturating_sub(*ts) <= STALE_AFTER_MS)
        .find_map(|(state, _)| match state {
            AgentState::NeedsInput => Some(AttentionKind::NeedsInput),
            AgentState::Error => Some(AttentionKind::Failed),
            _ => None,
        })
}

/// What a status-channel client is told as it happens.
#[derive(Clone, Debug)]
pub enum StatusChange {
    Activity(WorkspaceActivity),
    Attention(Vec<Attention>),
    /// The attention meter changed (decision 0169).
    Meter(Vec<termenbox_core::protocol::AttentionDay>),
}

/// A workspace id as events know it: a plain name, or `manager:name` when several managers run.
pub fn valid_event_workspace(id: &str) -> bool {
    match id.split_once(':') {
        Some((kind, name)) => crate::backend::valid_name(kind) && crate::backend::valid_name(name),
        None => crate::backend::valid_name(id),
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct EventHub {
    key: Arc<[u8; 32]>,
    rings: Arc<Mutex<HashMap<String, Ring>>>,
    /// The agent the scan found in each workspace; only workspaces that have events report it.
    agents: Arc<Mutex<HashMap<String, String>>>,
    /// The transcript file a Claude Code hook named for each workspace. Untrusted: `transcript::resolve` decides
    /// whether it may be opened, and nothing here is logged.
    transcripts: Arc<Mutex<HashMap<String, String>>>,
    tx: broadcast::Sender<AgentEvent>,
    attention: Arc<Mutex<Vec<Attention>>>,
    next_attention: Arc<std::sync::atomic::AtomicU64>,
    /// The Activity last announced per workspace, so only a change is sent.
    derived: Arc<Mutex<HashMap<String, WorkspaceActivity>>>,
    /// The turn-and-obligation reducer of each workspace whose agent reports turns (hooks, ACP).
    /// One reducer per (workspace, tab); a pane the hooks could not place has the tab "".
    reducers: Arc<Mutex<HashMap<(String, String), Reducer>>>,
    status_tx: broadcast::Sender<StatusChange>,
    /// The tasks agents worked on, kept on disk (U12); absent in tests that do not need it.
    history: Option<Arc<crate::history::History>>,
    /// Where each Claude Code workspace's token counts are read from (U13).
    usage: Arc<crate::transcript::UsageTracker>,
    /// Per-tab state (decision 0149); absent in tests that do not need it.
    tab_status: Option<Arc<TabStatus>>,
    /// The attention meter (decision 0169); absent in tests that do not need it.
    meter: Option<Arc<crate::attention_meter::AttentionMeter>>,
    /// Who last answered a tab's open question: the class of the connection that sent the answer, reply or keys.
    answerers: Arc<Mutex<HashMap<(String, String), (&'static str, u64)>>>,
    /// Where a tab's agent session id is kept (decision 0152); absent in tests that do not need it.
    registry: Option<Arc<crate::registry::Registry>>,
}

impl EventHub {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: Arc::new(key),
            rings: Arc::default(),
            agents: Arc::default(),
            transcripts: Arc::default(),
            tx: broadcast::channel(256).0,
            attention: Arc::default(),
            next_attention: Arc::default(),
            derived: Arc::default(),
            reducers: Arc::default(),
            status_tx: broadcast::channel(256).0,
            history: None,
            usage: Arc::default(),
            tab_status: None,
            meter: None,
            answerers: Arc::default(),
            registry: None,
        }
    }

    /// Measures the owner's attention (decision 0169).
    pub fn with_meter(mut self, meter: Arc<crate::attention_meter::AttentionMeter>) -> Self {
        self.meter = Some(meter);
        self
    }

    /// The meter's buckets, oldest first, for a client that is new.
    pub fn meter_days(&self) -> Vec<termenbox_core::protocol::AttentionDay> {
        self.meter
            .as_ref()
            .map(|m| m.stats(now_ms()))
            .unwrap_or_default()
    }

    /// A device showed the card of attention item `id` for `ms`. Returns whether it was credited.
    pub fn card_seen(&self, id: &str, ms: u64) -> bool {
        let Some(m) = &self.meter else {
            return false;
        };
        let credited = m.card_seen(id, ms, now_ms());
        if credited {
            self.announce_meter();
        }
        credited
    }

    /// Tells the devices the meter again while a tab works, because its time grows without any card being answered.
    pub fn announce_working_time(&self) {
        if self.meter.as_ref().is_some_and(|m| m.working()) {
            self.announce_meter();
        }
    }

    fn announce_meter(&self) {
        if self.meter.is_some() {
            let _ = self.status_tx.send(StatusChange::Meter(self.meter_days()));
        }
    }

    /// A connection of `class` answered, replied or typed to `tab` of `workspace` (an empty tab means any). It is kept
    /// only while that tab has a question open, and used when the question clears.
    pub fn note_answerer(&self, workspace: &str, tab: Option<&str>, class: &'static str) {
        if self.meter.is_none() {
            return;
        }
        let tab = tab.unwrap_or("");
        let open = self.attention.lock().unwrap().iter().any(|a| {
            a.kind == AttentionKind::NeedsInput
                && a.workspace == workspace
                && (tab.is_empty() || a.tab.as_deref().unwrap_or("") == tab)
        });
        if !open {
            return;
        }
        let mut map = self.answerers.lock().unwrap();
        if map.len() >= 256 {
            map.retain(|_, (_, at)| now_ms().saturating_sub(*at) < ANSWERER_MS);
        }
        if map.len() < 256 {
            map.insert((workspace.to_string(), tab.to_string()), (class, now_ms()));
        }
    }

    /// The class that answered `a`, if a connection did within the last minutes; used once.
    fn answerer_of(&self, a: &Attention) -> Option<&'static str> {
        let mut map = self.answerers.lock().unwrap();
        let tab = a.tab.clone().unwrap_or_default();
        let key = [
            (a.workspace.clone(), tab),
            (a.workspace.clone(), String::new()),
        ]
        .into_iter()
        .find(|k| map.contains_key(k))?;
        let (class, at) = map.remove(&key)?;
        (now_ms().saturating_sub(at) < ANSWERER_MS).then_some(class)
    }

    /// These items left the list: the ones in `answered` were resolved by `layer`, the rest just went away.
    fn meter_removed(&self, gone: &[Attention], layer: Option<crate::attention_meter::Layer>) {
        let Some(m) = &self.meter else {
            return;
        };
        let now = now_ms();
        for a in gone {
            match layer {
                Some(l) => {
                    let device = (l == crate::attention_meter::Layer::Human)
                        .then(|| self.answerer_of(a))
                        .flatten();
                    m.resolved(&a.id, l, device, now)
                }
                None => m.forget(&a.id),
            }
        }
        if layer.is_some() && !gone.is_empty() {
            self.announce_meter();
        }
    }

    /// Records the tasks this hub sees end (U12).
    pub fn with_history(mut self, history: Arc<crate::history::History>) -> Self {
        self.history = Some(history);
        self
    }

    pub fn with_tab_status(mut self, status: Arc<TabStatus>) -> Self {
        self.tab_status = Some(status);
        self
    }

    pub fn with_registry(mut self, registry: Arc<crate::registry::Registry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Records the agent's own session id on the tab an event came from.
    pub fn note_agent_session(&self, workspace: &str, tab: &str, id: &str) {
        if let Some(r) = &self.registry {
            r.set_agent_session(workspace, tab, id);
        }
    }

    pub fn tab_status(&self) -> Option<Arc<TabStatus>> {
        self.tab_status.clone()
    }

    /// The hook-confirmed prompt of one tab, for routing its answer without the workspace's latest prompt.
    pub fn tab_prompt(
        &self,
        workspace: &str,
        tab: &str,
    ) -> Option<termenbox_core::events::TabPrompt> {
        self.tab_status.as_ref()?.prompt(workspace, tab)
    }

    /// A hook event from one pane: the tab the pane belongs to learns the state, prompt or reply. The attention
    /// item already carries the tab because `ingest_as_tab` fed the same tab through `settle`.
    pub fn apply_hook_tab(&self, pane: &str, workspace: &str, kinds: &[EventKind], now: u64) {
        if let Some(status) = &self.tab_status {
            status.apply_pane(pane, workspace, kinds, now);
        }
    }

    pub fn history(&self) -> Option<&Arc<crate::history::History>> {
        self.history.as_ref()
    }

    /// Counts an ACP agent reported for a turn (U13), kept against the task in flight.
    pub fn record_tokens(&self, workspace: &str, t: crate::history::Tokens) {
        if let Some(h) = &self.history {
            h.tokens(workspace, t);
        }
        // ACP names no model with its counts.
        self.meter_tokens(None, &t);
    }

    /// Task telemetry (plan step 1.4): a turn's counts go to the meter's day bucket. Numbers and a model id only.
    fn meter_tokens(&self, model: Option<&str>, t: &crate::history::Tokens) {
        if let Some(m) = &self.meter {
            m.turn_used(model, t, now_ms());
            self.announce_meter();
        }
    }

    /// The folder an agent works in, by its last component (U13).
    pub fn record_project(&self, workspace: &str, dir: &str) {
        static HOME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let dir = dir.trim_end_matches('/');
        // A workspace in the home folder would file its task under the user name.
        let home = HOME.get_or_init(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| h.trim_end_matches('/').to_string())
        });
        if home.as_deref() == Some(dir) {
            return;
        }
        let name = dir.rsplit('/').next().unwrap_or("");
        if let Some(h) = &self.history {
            h.project(workspace, name);
        }
    }

    /// What a Claude Code hook payload says about use (U13), before the event itself is ingested so that a task a `Stop`
    /// is about to end still takes its counts. Hook payloads carry no token counts, cost or limits (fact 0029): the
    /// counts come from the transcript the payload names, read from where this server first saw the workspace; the one
    /// thing a payload says itself is that a turn failed on a rate limit. The caller has already checked the token.
    pub fn note_claude_usage(&self, workspace: &str, data: &serde_json::Value) {
        self.note_claude_usage_in(crate::transcript::claude_root().as_deref(), workspace, data);
    }

    fn note_claude_usage_in(
        &self,
        root: Option<&std::path::Path>,
        workspace: &str,
        data: &serde_json::Value,
    ) {
        let Some(history) = &self.history else { return };
        if let (Some(path), Some(root)) =
            (data.get("transcript_path").and_then(|p| p.as_str()), root)
        {
            self.usage.arm(root, workspace, path);
        }
        let event = data
            .get("hook_event_name")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        if let Some(cwd) = data.get("cwd").and_then(|c| c.as_str()) {
            self.record_project(workspace, cwd);
        }
        if event == "StopFailure"
            && data.get("error").and_then(|e| e.as_str()) == Some("rate_limit")
        {
            history.rate_limited(workspace);
            tracing::info!(target: "audit", event = "agent_rate_limited", session = %workspace);
        }
        // A prompt collects too: after an interrupted turn no Stop came, and its counts must not wait for the next task.
        if collects(event) {
            if let Some(t) = self.usage.collect(workspace) {
                // Counts only (audit-logging-no-secrets).
                tracing::info!(target: "audit", event = "usage_collected", session = %workspace, input = t.input, output = t.output, cache_read = t.cache_read, cache_write = t.cache_write);
                history.tokens(workspace, t);
                self.meter_tokens(self.usage.model(workspace).as_deref(), &t);
            }
        }
    }

    /// What a fact from an agent means for the history: a state change opens or ends a task, a title names it (only the
    /// task title the agent reports; plan items are never copied). Guesses from the screen or the scan are not recorded.
    fn note_history(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: &EventKind,
        ts: u64,
    ) {
        let Some(history) = &self.history else { return };
        if tier(source) != Some(0) || confidence == Confidence::Low {
            return;
        }
        match kind {
            EventKind::State { state } => {
                let agent = self.agents.lock().unwrap().get(workspace).cloned();
                history.state(workspace, agent.as_deref(), *state, ts);
            }
            EventKind::Task { title } => history.task(workspace, title),
            // A cost the agent reported itself (ACP); the context figures are not counts of use.
            EventKind::Usage {
                cost: Some(cost), ..
            } => {
                if let Some((micros, currency)) = crate::history::parse_cost(cost) {
                    history.cost(workspace, micros, &currency);
                }
            }
            _ => {}
        }
    }

    /// The hub key lives in the state directory (0600) so workspaces that outlive a server restart keep a
    /// working token.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let path = dir.join("event.key");
        let key = match std::fs::read(&path) {
            Ok(b) if b.len() == 32 => b.try_into().unwrap(),
            _ => {
                let mut k = [0u8; 32];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut k);
                std::fs::create_dir_all(dir)?;
                write_private(&path, &k)?;
                k
            }
        };
        Ok(Self::new(key))
    }

    fn mac(&self, workspace: &str) -> Hmac<Sha256> {
        let mut m = Hmac::<Sha256>::new_from_slice(&*self.key).expect("any key length");
        m.update(b"termenbox-event-session\0");
        m.update(workspace.as_bytes());
        m
    }

    /// The token a workspace's agent tools present; valid for that workspace name only.
    pub fn token(&self, workspace: &str) -> String {
        URL_SAFE_NO_PAD.encode(self.mac(workspace).finalize().into_bytes())
    }

    pub fn token_matches(&self, workspace: &str, token: &str) -> bool {
        URL_SAFE_NO_PAD
            .decode(token)
            .map(|t| self.mac(workspace).verify_slice(&t).is_ok())
            .unwrap_or(false)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<StatusChange> {
        self.status_tx.subscribe()
    }

    /// An event reported by a workspace's own tool: accepted only with that workspace's token.
    pub fn ingest(
        &self,
        workspace: &str,
        token: &str,
        source: Source,
        confidence: Confidence,
        kinds: Vec<EventKind>,
    ) -> Result<()> {
        if !valid_event_workspace(workspace) || !self.token_matches(workspace, token) {
            tracing::info!(target: "audit", event = "agent_event_refused", session = %workspace);
            return Err(anyhow!("token does not match the workspace"));
        }
        for k in kinds {
            self.feed(workspace, source, confidence, k, now_ms(), None);
        }
        Ok(())
    }

    /// Like `ingest`, for an event whose reporter proved to belong to `target` rather than to the workspace its
    /// environment named: the token must still be the declared workspace's.
    pub fn ingest_as(
        &self,
        declared: &str,
        token: &str,
        target: &str,
        source: Source,
        confidence: Confidence,
        kinds: Vec<EventKind>,
    ) -> Result<()> {
        self.ingest_as_tab(declared, token, target, source, confidence, kinds, None)
    }

    /// `ingest_as` for a hook that proved its pane: the tab goes on every attention item the event raises, so
    /// the status snapshot, a push and an ack all name it.
    #[allow(clippy::too_many_arguments)]
    pub fn ingest_as_tab(
        &self,
        declared: &str,
        token: &str,
        target: &str,
        source: Source,
        confidence: Confidence,
        kinds: Vec<EventKind>,
        tab: Option<&str>,
    ) -> Result<()> {
        if !valid_event_workspace(target) || !self.token_matches(declared, token) {
            tracing::info!(target: "audit", event = "agent_event_refused", session = %declared);
            return Err(anyhow!("token does not match the workspace"));
        }
        for k in kinds {
            self.feed(target, source, confidence, k, now_ms(), tab);
        }
        Ok(())
    }

    /// An event from the server's own adapters (scan, screen). A repeated state is not an event.
    pub fn report(&self, workspace: &str, source: Source, confidence: Confidence, kind: EventKind) {
        self.feed(workspace, source, confidence, kind, now_ms(), None);
    }

    /// Where an agent's own events enter: a turn or obligation edge goes through the workspace's reducer, which
    /// decides the state; everything else is stored as it is.
    fn feed(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
        tab: Option<&str>,
    ) {
        let kind = sanitize_kind(kind);
        let edge = if tier(source) == Some(0) {
            edge_of(source, &kind)
        } else {
            None
        };
        let Some(edge) = edge else {
            // `Turn` and `Block` are inputs only; one that maps to no edge is dropped.
            if !matches!(kind, EventKind::Turn { .. } | EventKind::Block { .. }) {
                self.push_tab(workspace, source, confidence, kind, now, tab);
            }
            return;
        };
        if matches!(kind, EventKind::Tool { .. }) {
            self.push_tab(workspace, source, confidence, kind, now, tab);
        }
        let key = (workspace.to_string(), tab.unwrap_or("").to_string());
        let (next, prev, derived, before, after) = {
            let mut reducers = self.reducers.lock().unwrap();
            if !reducers.contains_key(&key) && reducers.len() >= MAX_SESSIONS {
                return;
            }
            let before = aggregate(&reducers, workspace);
            let r = reducers.entry(key).or_default();
            let prev = r.state();
            let next = r.apply(edge, now);
            let derived = r.state();
            (next, prev, derived, before, aggregate(&reducers, workspace))
        };
        // The turn and obligation edges are not states, so the tab hears the state the reducer holds after each of
        // them, changed or not: a second tab starting a turn while the first works changes nothing workspace-wide.
        if let (Some(tab), Some(state), Some(status)) = (tab, derived, &self.tab_status) {
            status.set_hook_state(workspace, tab, state, now);
        }
        // A tab's own change raises and clears the tab's own attention; the workspace shows the aggregate.
        if let Some(state) = next {
            self.settle_tab(workspace, tab, prev, state, source, confidence, now);
        }
        if let Some(state) = after.filter(|_| next.is_some() || before != after) {
            self.push_reduced(workspace, source, confidence, state, now, tab);
        }
    }

    /// The workspace's state, as the aggregate of its tabs, goes through the ring; attention was settled per tab.
    fn push_reduced(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        state: AgentState,
        now: u64,
        tab: Option<&str>,
    ) {
        self.push_event(
            workspace,
            source,
            confidence,
            EventKind::State { state },
            now,
            tab,
            true,
        );
    }

    /// The attention a tab's own state change raises or clears. Only a hook (a fact) raises; a reconciliation
    /// clears a wait but is no fact that something finished or failed.
    #[allow(clippy::too_many_arguments)]
    fn settle_tab(
        &self,
        workspace: &str,
        tab: Option<&str>,
        prev: Option<AgentState>,
        state: AgentState,
        source: Source,
        confidence: Confidence,
        now: u64,
    ) {
        if let Some(m) = &self.meter {
            m.tab_working(
                workspace,
                tab.unwrap_or(""),
                state == AgentState::Working,
                now_ms(),
            );
            // A tab starting or ending work changes the number; without this the Home line waits for a card.
            if state == AgentState::Working || prev == Some(AgentState::Working) {
                self.announce_meter();
            }
        }
        if state != AgentState::NeedsInput {
            self.clear_where(workspace, |a| {
                a.kind == AttentionKind::NeedsInput && a.tab.as_deref() == tab
            });
        }
        let raise = match state {
            AgentState::NeedsInput => Some(AttentionKind::NeedsInput),
            AgentState::Done => Some(AttentionKind::FinishedUnseen),
            AgentState::Idle if prev == Some(AgentState::Working) => {
                Some(AttentionKind::FinishedUnseen)
            }
            AgentState::Error => Some(AttentionKind::Failed),
            _ => None,
        };
        if let Some(k) = raise.filter(|_| confidence == Confidence::High) {
            self.raise(workspace, k, source, now, tab);
        }
        self.refresh(now);
    }

    /// What the screen of an agent workspace shows now, against what the reducer believes. Closes an obligation or a
    /// turn that ended without telling anyone (Esc sends no hook); never opens one.
    pub fn reconcile(&self, workspace: &str, tab: Option<&str>, screen: Screen) {
        self.reconcile_at(workspace, tab, screen, now_ms());
    }

    fn reconcile_at(&self, workspace: &str, tab: Option<&str>, screen: Screen, now: u64) {
        let key = (workspace.to_string(), tab.unwrap_or("").to_string());
        let (next, prev, after) = {
            let mut reducers = self.reducers.lock().unwrap();
            let Some(r) = reducers.get_mut(&key) else {
                return;
            };
            let prev = r.state();
            let next = r.reconcile(screen, now);
            (next, prev, aggregate(&reducers, workspace))
        };
        if let Some(state) = next {
            if let (Some(tab), Some(status)) = (tab, &self.tab_status) {
                status.set_hook_state(workspace, tab, state, now);
            }
            // A reconciliation is a fact about the screen, not a finished turn: it clears a wait but raises nothing.
            self.settle_tab(
                workspace,
                tab,
                prev,
                state,
                Source::Hook,
                Confidence::Medium,
                now,
            );
            if let Some(state) = after {
                self.push_reduced(workspace, Source::Hook, Confidence::Medium, state, now, tab);
            }
        }
    }

    /// Whether a fact about this workspace (an open turn or a wait for the human) must not be overridden by a guess.
    fn pinned(&self, workspace: &str, now: u64) -> bool {
        self.reducers
            .lock()
            .unwrap()
            .iter()
            .any(|((w, _), r)| w == workspace && r.pins_at(now))
    }

    pub fn push(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
    ) {
        self.push_tab(workspace, source, confidence, kind, now, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_tab(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
        tab: Option<&str>,
    ) {
        self.push_event(workspace, source, confidence, kind, now, tab, false);
    }

    /// `reduced`: the state is the aggregate of the workspace's reducers, whose tabs already settled their attention.
    #[allow(clippy::too_many_arguments)]
    fn push_event(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
        tab: Option<&str>,
        reduced: bool,
    ) {
        let kind = sanitize_kind(kind);
        if matches!(kind, EventKind::Unknown) {
            return;
        }
        // A guess from the screen or the scan never overrides an open turn or a wait for the human: the state
        // those describe ends with an event or with the screen reconciliation, not with a clock.
        if matches!(kind, EventKind::State { .. })
            && matches!(tier(source), Some(1 | 2))
            && self.pinned(workspace, now)
        {
            return;
        }
        let mut rings = self.rings.lock().unwrap();
        if !rings.contains_key(workspace) && rings.len() >= MAX_SESSIONS {
            if let Some(oldest) = rings
                .iter()
                .min_by_key(|(_, r)| r.last_event_at)
                .map(|(k, _)| k.clone())
            {
                rings.remove(&oldest);
            }
        }
        let ring = rings.entry(workspace.to_string()).or_insert_with(|| Ring {
            events: VecDeque::new(),
            seq: 0,
            state: AgentState::Unknown,
            state_seq: 0,
            last_event_at: now,
            last_response_at: None,
            waiting_since: None,
            by_source: [None; 3],
            task: None,
        });
        // The raw signal of each source is kept whatever the merged state does with it.
        let mut prev_hook = None;
        if let EventKind::State { state } = &kind {
            if let Some(t) = tier(source) {
                if t == 0 {
                    prev_hook = ring.by_source[0].map(|(s, _)| s);
                }
                ring.by_source[t] = Some((*state, now));
            }
        }
        match &kind {
            EventKind::Task { title } if tier(source) == Some(0) => {
                ring.task = Some(Task {
                    title: title.clone(),
                    started_at: now,
                    source,
                });
            }
            EventKind::Plan { items } if tier(source) == Some(0) => {
                if let Some(item) = items.iter().find(|i| i.status.contains("progress")) {
                    ring.task = Some(Task {
                        title: item.text.clone(),
                        started_at: ring.task.as_ref().map(|t| t.started_at).unwrap_or(now),
                        source,
                    });
                }
            }
            _ => {}
        }
        if let EventKind::State { state } = &kind {
            // The same state again only refreshes the clock; a low-confidence source never overrides a
            // fresher state from a precise one within a minute.
            if *state == ring.state {
                ring.last_event_at = now;
                drop(rings);
                // The merged state may already say this because a screen guess got there first; the agent's own
                // report still opens or ends the task.
                self.note_history(workspace, source, confidence, &kind, now);
                self.settle(
                    workspace, source, confidence, &kind, prev_hook, now, tab, reduced,
                );
                return;
            }
            if confidence == Confidence::Low && !matches!(source, Source::Screen | Source::Hook) {
                if let Some(last) = ring
                    .events
                    .iter()
                    .rev()
                    .find(|e| matches!(e.kind, EventKind::State { .. }))
                {
                    if last.confidence != Confidence::Low
                        && now.saturating_sub(last.ts) < STALE_AFTER_MS
                    {
                        drop(rings);
                        self.settle(
                            workspace, source, confidence, &kind, prev_hook, now, tab, reduced,
                        );
                        return;
                    }
                }
            }
        }
        ring.seq += 1;
        ring.last_event_at = now;
        let event = AgentEvent {
            workspace: workspace.to_string(),
            seq: ring.seq,
            ts: now,
            source,
            confidence,
            kind,
        };
        match &event.kind {
            EventKind::State { state } => {
                let was_working = ring.state == AgentState::Working;
                ring.waiting_since = if matches!(state, AgentState::NeedsInput | AgentState::Error)
                {
                    Some(ring.waiting_since.unwrap_or(event.ts))
                } else {
                    None
                };
                ring.state = *state;
                ring.state_seq = event.seq;
                // A finished turn, or the agent going quiet after working, is a response.
                if *state == AgentState::Done
                    || (was_working && matches!(state, AgentState::Idle | AgentState::NeedsInput))
                {
                    ring.last_response_at = Some(event.ts);
                }
            }
            EventKind::Message { .. } => ring.last_response_at = Some(event.ts),
            _ => {}
        }
        ring.events.push_back(event.clone());
        while ring.events.len() > RING {
            ring.events.pop_front();
        }
        drop(rings);
        self.note_history(workspace, source, confidence, &event.kind, event.ts);
        // Counts and kinds only, never content (audit-logging-no-secrets).
        tracing::info!(target: "audit", event = "agent_event", session = %event.workspace, source = ?event.source, seq = event.seq);
        self.settle(
            workspace,
            source,
            confidence,
            &event.kind,
            prev_hook,
            now,
            tab,
            reduced,
        );
        let _ = self.tx.send(event);
    }

    /// After an event: move the workspace's Attention items by fact, then announce a changed Activity.
    /// Only a hook or Codex (a fact, not a guess) raises or clears Attention.
    fn settle(
        &self,
        workspace: &str,
        source: Source,
        confidence: Confidence,
        kind: &EventKind,
        prev_hook: Option<AgentState>,
        now: u64,
        tab: Option<&str>,
        reduced: bool,
    ) {
        if tier(source) == Some(0) && confidence != Confidence::Low {
            match kind {
                EventKind::State { state } if prev_hook != Some(*state) && !reduced => {
                    if *state != AgentState::NeedsInput {
                        self.clear_where(workspace, |a| a.kind == AttentionKind::NeedsInput);
                    }
                    let raise = match state {
                        AgentState::NeedsInput => Some(AttentionKind::NeedsInput),
                        AgentState::Done => Some(AttentionKind::FinishedUnseen),
                        AgentState::Idle if prev_hook == Some(AgentState::Working) => {
                            Some(AttentionKind::FinishedUnseen)
                        }
                        AgentState::Error => Some(AttentionKind::Failed),
                        _ => None,
                    };
                    // A reconciliation clears a wait but is no fact that something finished or failed.
                    if let Some(k) = raise.filter(|_| confidence == Confidence::High) {
                        self.raise(workspace, k, source, now, tab);
                    }
                }
                // The notification text arrives right after its `needs_input`; a T0 request names its action
                // (the agent's one-line title of the tool call) in the same way.
                EventKind::Message { summary }
                | EventKind::Prompt {
                    decision:
                        Some(termenbox_core::events::Decision {
                            detail: Some(summary),
                            ..
                        }),
                    ..
                } => {
                    let mut list = self.attention.lock().unwrap();
                    if let Some(a) = list.iter_mut().find(|a| {
                        a.workspace == workspace
                            && a.kind == AttentionKind::NeedsInput
                            && a.prompt.is_none()
                    }) {
                        a.prompt = Some(summary.clone());
                        let snapshot = list.clone();
                        drop(list);
                        let _ = self.status_tx.send(StatusChange::Attention(snapshot));
                    }
                }
                _ => {}
            }
        }
        self.refresh(now);
    }

    fn raise(
        &self,
        workspace: &str,
        kind: AttentionKind,
        source: Source,
        now: u64,
        tab: Option<&str>,
    ) {
        let task = self
            .rings
            .lock()
            .unwrap()
            .get(workspace)
            .and_then(|r| r.task.clone());
        let mut list = self.attention.lock().unwrap();
        // One open item per tab and kind: a newer one replaces the older.
        let replaced: Vec<Attention> = list
            .iter()
            .filter(|a| a.workspace == workspace && a.kind == kind && a.tab.as_deref() == tab)
            .cloned()
            .collect();
        list.retain(|a| !(a.workspace == workspace && a.kind == kind && a.tab.as_deref() == tab));
        let id = self
            .next_attention
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        list.push(Attention {
            id: format!("a{id}"),
            workspace: workspace.to_string(),
            tab: tab.map(str::to_string),
            kind,
            raised_at: now,
            task,
            prompt: None,
        });
        let mut dropped = replaced;
        while list.len() > MAX_ATTENTION {
            dropped.push(list.remove(0));
        }
        let snapshot = list.clone();
        drop(list);
        if let Some(m) = &self.meter {
            self.meter_removed(&dropped, None);
            m.raised(&format!("a{id}"), now_ms());
        }
        // Workspace, kind and source only (audit-logging-no-secrets).
        tracing::info!(target: "audit", event = "attention_raised", session = %workspace, kind = ?kind, source = ?source);
        let _ = self.status_tx.send(StatusChange::Attention(snapshot));
    }

    /// A closed tab must not keep its prompt or a pending attention that a push could still follow.
    pub fn forget_tab(&self, workspace: &str, tab: &str) {
        if let Some(status) = &self.tab_status {
            status.forget_tab(workspace, tab);
        }
        // The tab was closed: its items went away, nobody resolved them.
        self.clear_where_as(workspace, |a| a.tab.as_deref() == Some(tab), None);
        if let Some(m) = &self.meter {
            m.close_where(now_ms(), |w, t| w == workspace && t == tab);
        }
        let after = {
            let mut reducers = self.reducers.lock().unwrap();
            if reducers
                .remove(&(workspace.to_string(), tab.to_string()))
                .is_none()
            {
                return;
            }
            aggregate(&reducers, workspace)
        };
        if let Some(state) = after {
            self.push_reduced(
                workspace,
                Source::Hook,
                Confidence::Medium,
                state,
                now_ms(),
                None,
            );
        }
    }

    /// The agent moved on: the items were resolved, by the owner (an answer, in the terminal or from a card).
    fn clear_where(&self, workspace: &str, pred: impl Fn(&Attention) -> bool) {
        self.clear_where_as(workspace, pred, Some(crate::attention_meter::Layer::Human));
    }

    /// `layer` is who resolved the removed items for the meter; `None` when they just went away.
    fn clear_where_as(
        &self,
        workspace: &str,
        pred: impl Fn(&Attention) -> bool,
        layer: Option<crate::attention_meter::Layer>,
    ) {
        let mut list = self.attention.lock().unwrap();
        let before = list.len();
        let gone: Vec<Attention> = list
            .iter()
            .filter(|a| a.workspace == workspace && pred(a))
            .cloned()
            .collect();
        list.retain(|a| !(a.workspace == workspace && pred(a)));
        if list.len() != before {
            let snapshot = list.clone();
            drop(list);
            self.meter_removed(&gone, layer);
            tracing::info!(target: "audit", event = "attention_cleared", session = %workspace, by = "fact");
            let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        }
    }

    /// A device shows the item's workspace in the foreground, or the owner dismisses it. A `needs_input`
    /// stays until the prompt is really gone. Returns whether an item was cleared.
    pub fn ack(&self, attention_id: &str) -> bool {
        let mut list = self.attention.lock().unwrap();
        let Some(pos) = list
            .iter()
            .position(|a| a.id == attention_id && a.kind != AttentionKind::NeedsInput)
        else {
            return false;
        };
        let gone = list.remove(pos);
        let snapshot = list.clone();
        drop(list);
        self.meter_removed(
            std::slice::from_ref(&gone),
            Some(crate::attention_meter::Layer::Human),
        );
        // Looking at a finished or failed tab ends the result it showed (decision 0095: a result is not a state).
        if let (Some(tab), Some(status)) = (&gone.tab, &self.tab_status) {
            status.clear_result(&gone.workspace, tab);
        }
        tracing::info!(target: "audit", event = "attention_cleared", session = %gone.workspace, by = "ack");
        let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        true
    }

    pub fn attention_items(&self) -> Vec<Attention> {
        self.attention.lock().unwrap().clone()
    }

    /// Recomputes every workspace's Activity and announces the ones that changed. Called after each event and
    /// on a timer, so a silent `working` decays to `unknown`.
    pub fn refresh(&self, now: u64) {
        let open: Vec<String> = self
            .attention
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.workspace.clone())
            .collect();
        let rings = self.rings.lock().unwrap();
        let agents = self.agents.lock().unwrap();
        let mut derived = self.derived.lock().unwrap();
        let mut changed = Vec::new();
        for (workspace, r) in rings.iter() {
            let next = WorkspaceActivity {
                workspace: workspace.clone(),
                activity: derive_activity(r, now),
                ts: now,
                task: r.task.clone(),
                agent: agents.get(workspace).cloned(),
                hint: if open.contains(workspace) {
                    None
                } else {
                    derive_hint(r, now)
                },
            };
            let same = derived.get(workspace).is_some_and(|d| {
                (d.activity, &d.task, &d.agent, d.hint)
                    == (next.activity, &next.task, &next.agent, next.hint)
            });
            if !same {
                derived.insert(workspace.clone(), next.clone());
                changed.push(next);
            }
        }
        drop((derived, agents, rings));
        for a in changed {
            let _ = self.status_tx.send(StatusChange::Activity(a));
        }
    }

    /// The Activity of every workspace, for a client that has just connected.
    pub fn activities(&self) -> Vec<WorkspaceActivity> {
        self.activities_at(now_ms())
    }

    pub fn activities_at(&self, now: u64) -> Vec<WorkspaceActivity> {
        self.refresh(now);
        let mut out: Vec<_> = self.derived.lock().unwrap().values().cloned().collect();
        out.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        out
    }

    /// Forget a workspace that no longer exists.
    /// Only ids that start with `prefix` are considered (one per workspace manager when several run).
    pub fn forget_except(&self, prefix: &str, live: &[String]) {
        if let Some(history) = &self.history {
            history.retain_live(prefix, live, now_ms());
        }
        self.agents
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(prefix) || live.contains(k));
        self.transcripts
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(prefix) || live.contains(k));
        self.rings
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(prefix) || live.contains(k));
        self.derived
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(prefix) || live.contains(k));
        self.reducers
            .lock()
            .unwrap()
            .retain(|(w, _), _| !w.starts_with(prefix) || live.contains(w));
        if let Some(m) = &self.meter {
            m.close_where(now_ms(), |w, _| {
                w.starts_with(prefix) && !live.iter().any(|l| l == w)
            });
        }
        let mut list = self.attention.lock().unwrap();
        let before = list.len();
        let gone: Vec<Attention> = list
            .iter()
            .filter(|a| a.workspace.starts_with(prefix) && !live.contains(&a.workspace))
            .cloned()
            .collect();
        list.retain(|a| !a.workspace.starts_with(prefix) || live.contains(&a.workspace));
        if list.len() != before {
            let snapshot = list.clone();
            drop(list);
            self.meter_removed(&gone, None);
            let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        }
    }

    /// Remembers the transcript path a hook of `session` named (only after its token was accepted).
    pub fn set_transcript(&self, workspace: &str, path: &str) {
        if path.is_empty() || path.len() > 1024 || !valid_event_workspace(workspace) {
            return;
        }
        let mut map = self.transcripts.lock().unwrap();
        if !map.contains_key(workspace) && map.len() >= MAX_SESSIONS * 4 {
            return;
        }
        if map.get(workspace).map(String::as_str) != Some(path) {
            map.insert(workspace.to_string(), path.to_string());
        }
    }

    pub fn transcript(&self, workspace: &str) -> Option<String> {
        self.transcripts.lock().unwrap().get(workspace).cloned()
    }

    /// Records which agent the process scan found in `session`.
    pub fn set_agent(&self, workspace: &str, agent: &str) {
        let mut agents = self.agents.lock().unwrap();
        if agents.get(workspace).map(String::as_str) != Some(agent) {
            agents.insert(workspace.to_string(), agent.to_string());
        }
    }

    /// The agent name for a workspace, if known.
    pub fn agent(&self, workspace: &str) -> Option<String> {
        self.agents.lock().unwrap().get(workspace).cloned()
    }

    pub fn snapshot(&self) -> Vec<WorkspaceAgentState> {
        self.snapshot_at(now_ms())
    }

    pub fn snapshot_at(&self, now: u64) -> Vec<WorkspaceAgentState> {
        let rings = self.rings.lock().unwrap();
        let agents = self.agents.lock().unwrap();
        let mut out: Vec<_> = rings
            .iter()
            .map(|(workspace, r)| WorkspaceAgentState {
                workspace: workspace.clone(),
                state: r.state,
                seq: r.seq,
                stale: r.state == AgentState::Working
                    && now.saturating_sub(r.last_event_at) > STALE_AFTER_MS,
                ts: r.last_event_at,
                agent: agents.get(workspace).cloned(),
                last_response_at: r.last_response_at,
                waiting_since: r.waiting_since,
            })
            .collect();
        out.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        out
    }

    /// The prompt a one-tap answer may target: the workspace's latest `prompt` event, unless the agent has
    /// moved on (a later state other than `needs_input`).
    pub fn current_prompt(&self, workspace: &str) -> Option<AgentEvent> {
        let rings = self.rings.lock().unwrap();
        let ring = rings.get(workspace)?;
        let prompt = ring
            .events
            .iter()
            .rev()
            .find(|e| matches!(e.kind, EventKind::Prompt { .. }))?;
        let moved_on = ring.events.iter().any(|e| {
            e.seq > prompt.seq
                && matches!(e.kind, EventKind::State { state } if state != AgentState::NeedsInput)
        });
        (!moved_on).then(|| prompt.clone())
    }

    /// Source and confidence of the workspace's latest state event, and when it happened.
    pub fn last_state_event(&self, workspace: &str) -> Option<(Source, Confidence, u64)> {
        let rings = self.rings.lock().unwrap();
        let ring = rings.get(workspace)?;
        ring.events
            .iter()
            .rev()
            .find(|e| matches!(e.kind, EventKind::State { .. }))
            .map(|e| (e.source, e.confidence, e.ts))
    }

    /// The agent's own hook reported `needs_input` within the last minute and nothing newer has changed the state.
    pub fn hook_says_needs_input(&self, workspace: &str) -> bool {
        let now = now_ms();
        matches!(
            self.last_state_event(workspace),
            Some((Source::Hook, Confidence::High, ts)) if now.saturating_sub(ts) < STALE_AFTER_MS
        ) && self
            .snapshot_at(now)
            .iter()
            .any(|s| s.workspace == workspace && s.state == AgentState::NeedsInput)
    }

    /// Events of `session` after `seq`, oldest first (only what the ring still holds).
    pub fn since(&self, workspace: &str, seq: u64) -> Vec<AgentEvent> {
        self.rings
            .lock()
            .unwrap()
            .get(workspace)
            .map(|r| r.events.iter().filter(|e| e.seq > seq).cloned().collect())
            .unwrap_or_default()
    }

    pub fn workspaces_with_events(&self) -> usize {
        self.rings.lock().unwrap().len()
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

/// Unix socket paths are limited to about 108 bytes; a long state directory falls back to a private
/// directory (0700) under the temp dir.
pub fn socket_path(state_dir: &Path) -> std::path::PathBuf {
    socket_in(state_dir, "events.sock")
}

/// A socket file named `name` in the state directory, or in a private one under the temp directory when the path
/// would be too long for a Unix socket.
pub fn socket_in(state_dir: &Path, name: &str) -> std::path::PathBuf {
    let preferred = state_dir.join(name);
    if preferred.as_os_str().len() <= 100 {
        return preferred;
    }
    let dir = std::env::temp_dir().join(format!("termenbox-{}", unsafe { libc::geteuid() }));
    let _ = std::fs::create_dir_all(&dir);
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir.join(name)
}

/// What a workspace's shell gets so its tools can report: who it is, its token and where to send.
#[derive(Clone)]
pub struct EventEnv {
    pub hub: EventHub,
    pub socket: std::path::PathBuf,
}

impl EventEnv {
    pub fn vars(&self, workspace: &str) -> Vec<String> {
        vec![
            format!("TERMENBOX_SESSION={workspace}"),
            format!("TERMENBOX_EVENT_TOKEN={}", self.hub.token(workspace)),
            format!("TERMENBOX_EVENT_SOCKET={}", self.socket.display()),
        ]
    }
}

// ---------------------------------------------------------------------------------------------------
// Ingest socket
// ---------------------------------------------------------------------------------------------------

/// One line a tool (via `termenbox-server hook`) writes: which workspace, its token, which adapter, and the
/// tool's own JSON, untouched.
#[derive(serde::Deserialize)]
struct Wire {
    #[serde(rename = "session")]
    workspace: String,
    token: String,
    adapter: String,
    data: serde_json::Value,
    /// Parent process ids of the reporter, nearest first (absent from an older hook).
    #[serde(default)]
    ancestors: Vec<u32>,
    /// The tmux pane the hook ran in (`%12`); absent outside tmux or from an older hook.
    #[serde(default)]
    pane: String,
}

const MAX_LINE: u64 = 64 * 1024;

/// A Unix socket in the state directory, mode 0600, never TCP. Every connection carries one line and is
/// answered with nothing; refusals only show in the audit log.
pub async fn serve_socket(
    hub: EventHub,
    path: std::path::PathBuf,
    resolver: Arc<crate::owner::Resolver>,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    // Connections only read and parse; one worker handles the events in the order they came in, so a tool's hooks keep
    // their order even when one of them waits on a transcript read.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Hooked>(256);
    tokio::spawn({
        let hub = hub.clone();
        async move {
            // Events whose workspace is not known yet (a shared service, before its first prompt shows where it is).
            let mut pending: VecDeque<(Hooked, u64)> = VecDeque::new();
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                tokio::select! {
                    item = rx.recv() => {
                        let Some(item) = item else { break };
                        if handle_hooked(&hub, &resolver, item, &mut pending).await {
                            retry_pending(&hub, &resolver, &mut pending).await;
                        }
                    }
                    _ = tick.tick() => retry_pending(&hub, &resolver, &mut pending).await,
                }
            }
        }
    });
    loop {
        let (stream, _) = listener.accept().await?;
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut line = String::new();
            let mut r = BufReader::new(stream.take(MAX_LINE));
            let read =
                tokio::time::timeout(std::time::Duration::from_secs(2), r.read_line(&mut line))
                    .await;
            if !matches!(read, Ok(Ok(n)) if n > 0) {
                return;
            }
            let Ok(w) = serde_json::from_str::<Wire>(&line) else {
                return;
            };
            let Some((source, confidence, kinds)) = crate::adapters::adapt(&w.adapter, &w.data)
            else {
                return;
            };
            let _ = tx.send((w, source, confidence, kinds)).await;
        });
    }
}

/// How many events wait for a workspace, and for how long.
const MAX_PENDING: usize = 64;
const PENDING_MS: u64 = 30_000;

fn report_of(w: &Wire, kinds: &[EventKind]) -> crate::owner::Report {
    crate::owner::Report {
        adapter: w.adapter.clone(),
        declared: w.workspace.clone(),
        agent_id: crate::adapters::agent_session_id(&w.adapter, &w.data),
        ancestors: w.ancestors.clone(),
        turn: kinds.iter().find_map(|k| match k {
            EventKind::Turn { open, .. } => Some(*open),
            _ => None,
        }),
        prompt: kinds.iter().find_map(|k| match k {
            EventKind::Task { title } => Some(title.clone()),
            _ => None,
        }),
    }
}

/// Delivers one event, parks it, or drops it. True when it was delivered, so parked events may now be placeable.
async fn handle_hooked(
    hub: &EventHub,
    resolver: &crate::owner::Resolver,
    item: Hooked,
    pending: &mut VecDeque<(Hooked, u64)>,
) -> bool {
    use crate::owner::Verdict;
    let (w, source, confidence, kinds) = &item;
    if !hub.token_matches(&w.workspace, &w.token) {
        tracing::info!(target: "audit", event = "agent_event_refused", session = %w.workspace);
        return false;
    }
    match resolver.resolve(&report_of(w, kinds), now_ms()).await {
        Verdict::Deliver(workspace) => {
            deliver(
                hub,
                &item.0,
                *source,
                *confidence,
                kinds.clone(),
                &workspace,
            )
            .await;
            true
        }
        Verdict::Pending => {
            if pending.len() >= MAX_PENDING {
                pending.pop_front();
            }
            pending.push_back((item, now_ms()));
            false
        }
        Verdict::Drop(why) => {
            tracing::info!(target: "audit", event = "agent_event_unplaced", session = %w.workspace, adapter = %w.adapter, reason = why);
            false
        }
    }
}

/// Parked events are tried again, oldest first, so their order is kept; those that wait too long are dropped.
async fn retry_pending(
    hub: &EventHub,
    resolver: &crate::owner::Resolver,
    pending: &mut VecDeque<(Hooked, u64)>,
) {
    use crate::owner::Verdict;
    let now = now_ms();
    let mut keep = VecDeque::new();
    let mut placed_any = true;
    while placed_any {
        placed_any = false;
        while let Some((item, since)) = pending.pop_front() {
            if now.saturating_sub(since) > PENDING_MS {
                tracing::info!(target: "audit", event = "agent_event_unplaced", session = %item.0.workspace, adapter = %item.0.adapter, reason = "no workspace found for it in time");
                continue;
            }
            let (w, source, confidence, kinds) = &item;
            match resolver.resolve(&report_of(w, kinds), now).await {
                Verdict::Deliver(workspace) => {
                    deliver(
                        hub,
                        &item.0,
                        *source,
                        *confidence,
                        kinds.clone(),
                        &workspace,
                    )
                    .await;
                    placed_any = true;
                }
                Verdict::Pending => keep.push_back((item, since)),
                Verdict::Drop(_) => {}
            }
        }
        std::mem::swap(pending, &mut keep);
        if !placed_any {
            break;
        }
    }
}

async fn deliver(
    hub: &EventHub,
    w: &Wire,
    source: Source,
    confidence: Confidence,
    kinds: Vec<EventKind>,
    workspace: &str,
) {
    if w.adapter == "claude" {
        // File reads: off the async threads.
        let (h, s, data) = (hub.clone(), workspace.to_string(), w.data.clone());
        let _ = tokio::task::spawn_blocking(move || h.note_claude_usage(&s, &data)).await;
    }
    let for_tab = kinds.clone();
    let tab = hub
        .tab_status()
        .and_then(|status| status.tab_of(&w.pane, workspace));
    if hub
        .ingest_as_tab(
            &w.workspace,
            &w.token,
            workspace,
            source,
            confidence,
            kinds,
            tab.as_deref(),
        )
        .is_ok()
    {
        hub.apply_hook_tab(&w.pane, workspace, &for_tab, now_ms());
        if let (Some(tab), Some(id)) = (
            tab.as_deref(),
            crate::adapters::agent_session_id(&w.adapter, &w.data),
        ) {
            hub.note_agent_session(workspace, tab, &id);
        }
        if w.adapter == "claude" {
            if let Some(path) = w.data.get("transcript_path").and_then(|p| p.as_str()) {
                hub.set_transcript(workspace, path);
            }
            // A task this event opened takes its folder now; one it ended took it above.
            if let Some(cwd) = w.data.get("cwd").and_then(|c| c.as_str()) {
                hub.record_project(workspace, cwd);
            }
        }
    }
}

/// One accepted hook line, parsed and adapted, waiting for the worker.
type Hooked = (Wire, Source, Confidence, Vec<EventKind>);

// ---------------------------------------------------------------------------------------------------
// The hook command a tool runs: `termenbox-server --hook <adapter>`
// ---------------------------------------------------------------------------------------------------

/// Copies the tool's JSON from stdin to the ingest socket. It never fails and never waits long: whatever
/// goes wrong (no socket, server down, slow disk), it returns, because the agent must not be held up by us.
pub fn run_hook(adapter: &str) {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    run_hook_at(
        adapter,
        env("TERMENBOX_SESSION"),
        env("TERMENBOX_EVENT_TOKEN"),
        env("TERMENBOX_EVENT_SOCKET"),
        env("TMUX_PANE"),
    );
}

pub fn run_hook_at(
    adapter: &str,
    workspace: Option<String>,
    token: Option<String>,
    socket: Option<String>,
    pane: Option<String>,
) {
    use std::io::{Read, Write};
    let (Some(workspace), Some(token), Some(socket)) = (workspace, token, socket) else {
        return;
    };
    // The deadline covers the whole run, including a stdin that never closes.
    let adapter = adapter.to_string();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut raw = String::new();
        let mut stdin = std::io::stdin().take(MAX_LINE);
        let _ = stdin.read_to_string(&mut raw);
        let data =
            serde_json::from_str::<serde_json::Value>(&raw).unwrap_or(serde_json::Value::Null);
        let ancestors = crate::owner::own_ancestors();
        let line = serde_json::json!({"session": workspace, "token": token, "adapter": adapter, "data": data, "ancestors": ancestors, "pane": pane.unwrap_or_default()}).to_string() + "\n";
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&socket) {
            let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
            let _ = s.write_all(line.as_bytes());
        }
        let _ = done_tx.send(());
    });
    let _ = done_rx.recv_timeout(std::time::Duration::from_millis(1500));
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_block_keeps_lines_and_indentation_but_not_escapes_secrets_or_runs_of_blank_lines() {
        let out = sanitize_block(
            "# T\n\n\n\n    code\tx\u{1b}[31m red\r\nkey ghp_abcdefghijklmnopqrstuvwxyz0123 \u{202e}end\n\n",
            1000,
            50,
        );
        assert_eq!(out, "# T\n\n    code  x [31m red\nkey [token] end");
        assert!(!out.contains('\u{1b}') && !out.contains('\u{202e}'));
        assert_eq!(sanitize_block("a\nb\nc\nd", 1000, 2), "a\nb");
        assert_eq!(sanitize_block("abcdef", 3, 9), "abc");
        assert_eq!(sanitize_block("", 10, 10), "");
    }

    #[test]
    fn a_block_says_whether_it_was_cut() {
        let cut = |t: &str, max, lines| sanitize_block_cut(t, max, lines).1;
        assert!(!cut("abc", 3, 5));
        assert!(
            !cut("abc\n\n  \n", 3, 5),
            "trailing blank space is not text"
        );
        assert!(cut("abcd", 3, 5));
        assert!(cut("ab\ncd", 3, 5), "the cap falls inside a later line");
        assert!(
            cut("abc\ndef", 3, 5),
            "the cap falls at a line end with text after it"
        );
        assert!(!cut("a\nb", 100, 2));
        assert!(cut("a\nb\nc", 100, 2));
        assert!(cut("a\nb\n\nc", 100, 2), "text after a blank line is cut");
        assert!(!cut("a\nb\n\n", 100, 2));
    }

    /// A long block costs one pass, not one per line (the character count is kept, not redone).
    #[test]
    fn a_block_of_many_short_lines_is_cut_in_linear_time() {
        let text = "x\n".repeat(150_000);
        let start = std::time::Instant::now();
        let (out, cut) = sanitize_block_cut(&text, 10_000_000, 1_000_000);
        assert!(!cut && out.chars().count() == 299_999);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            start.elapsed()
        );
    }

    use super::*;

    fn hub() -> EventHub {
        EventHub::new([7; 32])
    }
    fn state(s: AgentState) -> EventKind {
        EventKind::State { state: s }
    }

    fn activity(h: &EventHub, workspace: &str, now: u64) -> Option<WorkspaceActivity> {
        h.activities_at(now)
            .into_iter()
            .find(|a| a.workspace == workspace)
    }

    #[test]
    fn a_scan_event_cannot_overwrite_a_hooks_needs_input() {
        let h = hub();
        h.push(
            "s",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1_000,
        );
        h.push(
            "s",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Working),
            2_000,
        );
        h.push(
            "s",
            Source::Screen,
            Confidence::Low,
            state(AgentState::Idle),
            3_000,
        );
        let a = activity(&h, "s", 4_000).unwrap();
        assert_eq!(a.activity, Activity::Blocked);
        let items = h.attention_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, AttentionKind::NeedsInput);
    }

    #[test]
    fn scan_and_screen_only_hint_and_never_raise_attention() {
        let h = hub();
        h.push(
            "s",
            Source::Screen,
            Confidence::Low,
            state(AgentState::NeedsInput),
            1_000,
        );
        h.push(
            "s",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Working),
            1_500,
        );
        h.push(
            "s",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            2_000,
        );
        assert!(h.attention_items().is_empty());
        let a = activity(&h, "s", 2_500).unwrap();
        assert_eq!(a.activity, Activity::Blocked);
        assert_eq!(a.hint, Some(AttentionKind::NeedsInput));
    }

    #[test]
    fn silence_decays_working_to_unknown_but_not_idle() {
        let h = hub();
        h.push(
            "w",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            1_000,
        );
        h.push(
            "i",
            Source::Hook,
            Confidence::High,
            state(AgentState::Idle),
            1_000,
        );
        assert_eq!(
            activity(&h, "w", 30_000).unwrap().activity,
            Activity::Working
        );
        assert_eq!(
            activity(&h, "w", 62_000).unwrap().activity,
            Activity::Unknown
        );
        assert_eq!(activity(&h, "i", 600_000).unwrap().activity, Activity::Idle);
        // A fresh lower-tier signal takes over from a hook that went quiet.
        h.push(
            "w",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Working),
            61_000,
        );
        assert_eq!(
            activity(&h, "w", 62_000).unwrap().activity,
            Activity::Working
        );
    }

    #[test]
    fn ack_clears_finished_unseen_but_not_needs_input() {
        let h = hub();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            1_000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Done),
            2_000,
        );
        h.push(
            "b",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            2_000,
        );
        let items = h.attention_items();
        assert_eq!(items.len(), 2);
        let id_of = |k| items.iter().find(|i| i.kind == k).unwrap().id.clone();
        assert!(!h.ack(&id_of(AttentionKind::NeedsInput)));
        assert!(h.ack(&id_of(AttentionKind::FinishedUnseen)));
        let left = h.attention_items();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].kind, AttentionKind::NeedsInput);
        assert!(!h.ack("nope"));
    }

    #[test]
    fn needs_input_clears_by_fact_and_failures_are_raised() {
        let h = hub();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1_000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            EventKind::Message {
                summary: "Claude needs your permission".into(),
            },
            1_001,
        );
        assert_eq!(
            h.attention_items()[0].prompt.as_deref(),
            Some("Claude needs your permission")
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            2_000,
        );
        assert!(h.attention_items().is_empty());
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Error),
            3_000,
        );
        assert_eq!(h.attention_items()[0].kind, AttentionKind::Failed);
    }

    #[test]
    fn a_hooks_needs_input_is_raised_even_when_a_scan_already_said_it() {
        let h = hub();
        h.push(
            "a",
            Source::Screen,
            Confidence::Low,
            state(AgentState::NeedsInput),
            1_000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            2_000,
        );
        assert_eq!(h.attention_items().len(), 1);
    }

    #[test]
    fn the_task_title_is_sanitised_and_keeps_its_start() {
        let h = hub();
        h.push(
            "t",
            Source::Hook,
            Confidence::High,
            EventKind::Task {
                title: format!(
                    "fix\u{1b}[31m login {}{} {}",
                    "ghp_",
                    "a".repeat(36),
                    "x".repeat(200)
                ),
            },
            5_000,
        );
        let t = activity(&h, "t", 6_000).unwrap().task.unwrap();
        assert_eq!(t.started_at, 5_000);
        assert!(!t.title.contains('\u{1b}') && !t.title.contains("ghp_"));
        assert!(t.title.chars().count() <= TASK_CHARS);
        h.push(
            "t",
            Source::Hook,
            Confidence::High,
            EventKind::Plan {
                items: vec![
                    PlanItem {
                        text: "write tests".into(),
                        status: "completed".into(),
                    },
                    PlanItem {
                        text: "wire the hub".into(),
                        status: "in_progress".into(),
                    },
                ],
            },
            7_000,
        );
        let t = activity(&h, "t", 8_000).unwrap().task.unwrap();
        assert_eq!((t.title.as_str(), t.started_at), ("wire the hub", 5_000));
    }

    #[test]
    fn status_changes_are_announced_once() {
        let h = hub();
        let mut rx = h.subscribe_status();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            1_000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            1_500,
        );
        let mut activities = 0;
        while let Ok(c) = rx.try_recv() {
            if matches!(c, StatusChange::Activity(_)) {
                activities += 1;
            }
        }
        assert_eq!(activities, 1);
    }

    #[test]
    fn a_hook_event_keeps_the_tab_on_the_attention_it_raises() {
        let h = hub();
        h.push_tab(
            "w",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1_000,
            Some("t2"),
        );
        let items = h.attention_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tab.as_deref(), Some("t2"));
        h.push(
            "w",
            Source::Hook,
            Confidence::High,
            state(AgentState::Idle),
            1_500,
        );
        h.push(
            "w",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            2_000,
        );
        assert_eq!(h.attention_items()[0].tab, None);
    }

    #[test]
    fn forgetting_a_tab_drops_its_attention_and_prompt() {
        let status = Arc::new(TabStatus::default());
        let h = EventHub::new([7; 32]).with_tab_status(status.clone());
        status.set_pane("%1", "w", "t2");
        status.apply_pane(
            "%1",
            "w",
            &[EventKind::Prompt {
                id: "p1".into(),
                prompt_kind: "yes_no".into(),
                choices: vec!["Yes".into(), "No".into()],
                decision: None,
            }],
            1_000,
        );
        h.push_tab(
            "w",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1_000,
            Some("t2"),
        );
        assert_eq!(h.attention_items()[0].tab.as_deref(), Some("t2"));
        assert!(status.prompt("w", "t2").is_some());

        h.forget_tab("w", "t2");

        assert!(
            h.attention_items().is_empty(),
            "a closed tab keeps no attention"
        );
        assert!(
            status.prompt("w", "t2").is_none(),
            "a closed tab keeps no prompt"
        );
        assert!(status.snapshot().is_empty(), "a closed tab keeps no state");
    }

    #[test]
    fn forgetting_a_workspace_drops_its_attention() {
        let h = hub();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1_000,
        );
        h.forget_except("", &[]);
        assert!(h.attention_items().is_empty() && h.activities().is_empty());
    }

    #[test]
    fn a_long_state_directory_gets_a_socket_path_that_fits() {
        let short = socket_path(Path::new("/var/lib/termenbox"));
        assert_eq!(short, Path::new("/var/lib/termenbox/events.sock"));
        let long = socket_path(&Path::new("/tmp").join("x".repeat(120)));
        assert!(
            long.as_os_str().len() <= 100 && long.ends_with("events.sock"),
            "{long:?}"
        );
    }

    #[test]
    fn event_workspace_ids_may_carry_a_manager_prefix() {
        assert!(valid_event_workspace("work") && valid_event_workspace("tmux:work"));
        assert!(
            !valid_event_workspace("a:b:c")
                && !valid_event_workspace(":x")
                && !valid_event_workspace("x:")
        );
    }

    #[test]
    fn forgetting_is_limited_to_one_managers_prefix() {
        let h = hub();
        h.push(
            "tmux:a",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            1,
        );
        h.push(
            "cmd:b",
            Source::Screen,
            Confidence::Low,
            state(AgentState::Idle),
            1,
        );
        h.forget_except("tmux:", &[]);
        assert_eq!(
            h.snapshot()
                .iter()
                .map(|s| s.workspace.as_str())
                .collect::<Vec<_>>(),
            ["cmd:b"]
        );
    }

    #[test]
    fn a_token_only_works_for_its_own_workspace() {
        let h = hub();
        let t = h.token("one");
        assert!(h.token_matches("one", &t));
        assert!(!h.token_matches("two", &t));
        assert!(!h.token_matches("one", "garbage"));
        assert!(h
            .ingest(
                "two",
                &t,
                Source::Hook,
                Confidence::High,
                vec![state(AgentState::Working)]
            )
            .is_err());
        assert!(h
            .ingest(
                "one",
                &t,
                Source::Hook,
                Confidence::High,
                vec![state(AgentState::Working)]
            )
            .is_ok());
        assert_eq!(h.snapshot().len(), 1);
        assert_eq!(h.snapshot()[0].workspace, "one");
        assert!(
            EventHub::new([8; 32]).token("one") != t,
            "tokens depend on the key"
        );
    }

    #[test]
    fn control_characters_and_secrets_are_removed() {
        let dirty = "ls\x1b[31m\nrm -rf /\r\n curl -H \"Authorization: Bearer abcDEF123\" http://10.0.0.5:8080/x \
                     ghp_abcdefghijklmnopqrstuvwxyz0123456789 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \
                     my.host.example.com:4443 2001:db8:0:0:0:0:0:1";
        let clean = sanitize_text(dirty, 500);
        for bad in [
            "\x1b",
            "\n",
            "\r",
            "abcDEF123",
            "ghp_abc",
            "10.0.0.5",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "my.host.example.com",
            "2001:db8",
        ] {
            assert!(!clean.contains(bad), "{bad:?} leaked: {clean}");
        }
        assert!(clean.contains("[token]") && clean.contains("[secret]") && clean.contains("[ip]"));
        assert!(clean.starts_with("ls"));
    }

    #[test]
    fn text_is_capped_by_characters() {
        assert_eq!(sanitize_text(&"é ".repeat(300), 120).chars().count(), 120);
        let EventKind::Plan { items } = sanitize_kind(EventKind::Plan {
            items: (0..30)
                .map(|i| PlanItem {
                    text: format!("item {i} {}", "x ".repeat(300)),
                    status: "todo".into(),
                })
                .collect(),
        }) else {
            panic!()
        };
        assert_eq!(items.len(), PLAN_ITEMS);
        assert!(items
            .iter()
            .all(|i| i.text.chars().count() <= PLAN_ITEM_CHARS));
    }

    #[test]
    fn rings_stay_bounded_and_seq_is_stamped_by_the_server() {
        let h = hub();
        for i in 0..200 {
            h.push(
                "a",
                Source::Hook,
                Confidence::High,
                EventKind::Message {
                    summary: format!("m{i}"),
                },
                1000 + i,
            );
        }
        let all = h.since("a", 0);
        assert_eq!(all.len(), RING);
        assert_eq!(all.last().unwrap().seq, 200);
        assert_eq!(all[0].seq, 151);
        assert_eq!(h.since("a", 195).len(), 5);
        for i in 0..40 {
            h.push(
                &format!("s{i}"),
                Source::Hook,
                Confidence::High,
                state(AgentState::Idle),
                5000 + i,
            );
        }
        assert_eq!(h.workspaces_with_events(), MAX_SESSIONS);
    }

    #[test]
    fn unknown_kinds_are_ignored_and_repeated_states_are_not_events() {
        let h = hub();
        h.push("a", Source::Hook, Confidence::High, EventKind::Unknown, 1);
        assert_eq!(h.workspaces_with_events(), 0);
        h.push(
            "a",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            1,
        );
        h.push(
            "a",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            2,
        );
        assert_eq!(h.since("a", 0).len(), 1);
    }

    #[test]
    fn waiting_since_survives_activity_and_same_state_refreshes() {
        let h = hub();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            1000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            EventKind::Message {
                summary: "waiting".into(),
            },
            2000,
        );
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            3000,
        );
        assert_eq!(h.snapshot_at(4000)[0].waiting_since, Some(1000));
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Error),
            5000,
        );
        assert_eq!(h.snapshot_at(5000)[0].waiting_since, Some(1000));
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            6000,
        );
        assert_eq!(h.snapshot_at(6000)[0].waiting_since, None);
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::NeedsInput),
            7000,
        );
        assert_eq!(h.snapshot_at(7000)[0].waiting_since, Some(7000));
    }

    #[test]
    fn a_working_state_with_no_news_is_reported_stale_and_a_precise_source_beats_the_scan() {
        let h = hub();
        h.push(
            "a",
            Source::Hook,
            Confidence::High,
            state(AgentState::Working),
            1_000,
        );
        assert!(!h.snapshot_at(30_000)[0].stale);
        assert!(h.snapshot_at(70_000)[0].stale);
        assert_eq!(h.snapshot_at(70_000)[0].state, AgentState::Working);
        h.push(
            "a",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            20_000,
        );
        assert_eq!(
            h.snapshot_at(21_000)[0].state,
            AgentState::Working,
            "the scan must not override a fresh hook"
        );
        h.push(
            "a",
            Source::Scan,
            Confidence::Low,
            state(AgentState::Idle),
            90_000,
        );
        assert_eq!(h.snapshot_at(91_000)[0].state, AgentState::Idle);
    }

    #[tokio::test]
    async fn the_socket_is_private_and_a_wrong_token_changes_nothing() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncWriteExt;
        let dir = std::env::temp_dir().join(format!("tm-ev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("events.sock");
        let h = hub();
        tokio::spawn(serve_socket(
            h.clone(),
            sock.clone(),
            Arc::new(crate::owner::Resolver::default()),
        ));
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let send = |workspace: &str, token: String| {
            let line = serde_json::json!({"session": workspace, "token": token, "adapter": "claude",
                "data": {"hook_event_name": "UserPromptSubmit", "transcript_path": "/home/u/.claude/projects/p/s.jsonl", "prompt": "fix the login\nthe second line stays in the hook payload"}})
            .to_string()
                + "\n";
            let sock = sock.clone();
            async move {
                let mut s = tokio::net::UnixStream::connect(sock).await.unwrap();
                s.write_all(line.as_bytes()).await.unwrap();
            }
        };
        send("one", "wrong".into()).await;
        send("one", h.token("two")).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(h.workspaces_with_events(), 0);
        assert_eq!(
            h.transcript("one"),
            None,
            "a refused hook names no transcript"
        );
        send("one", h.token("one")).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            h.transcript("one").as_deref(),
            Some("/home/u/.claude/projects/p/s.jsonl")
        );
        let ev = h.since("one", 0);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].kind, state(AgentState::Working));
        assert_eq!(
            ev[1].kind,
            EventKind::Task {
                title: "fix the login".into()
            }
        );
        assert!(
            !serde_json::to_string(&ev).unwrap().contains("second line"),
            "only the first prompt line leaves the hook payload"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 20 workspaces at 20 events per second for a minute of events, processed in one go: memory is bounded by the rings and the CPU cost is tiny (the budget is 5% of one core, i.e. 3 s per minute). The bound is generous so a loaded CI runner where the test competes for CPU with parallel tests still passes; a regression to a quadratic cost still misses it by an order of magnitude.
    #[test]
    fn load_20_workspaces_at_20_events_per_second_for_a_minute() {
        let h = hub();
        let started = std::time::Instant::now();
        for sec in 0..60u64 {
            for n in 0..20 {
                for e in 0..20u64 {
                    let kind = if e % 2 == 0 {
                        EventKind::Tool { name: "Bash".into(), summary: "cargo test --workspace -- --nocapture token=abcdefghijklmnopqrstuvwxyz0123456789".into(), phase: termenbox_core::events::ToolPhase::Started }
                    } else {
                        EventKind::Message {
                            summary: "working on the next step of the plan".into(),
                        }
                    };
                    h.push(
                        &format!("s{n}"),
                        Source::Hook,
                        Confidence::High,
                        kind,
                        1_000 + sec * 1000 + e,
                    );
                }
            }
        }
        let took = started.elapsed();
        assert_eq!(h.workspaces_with_events(), 20);
        assert!(took.as_secs_f64() < 10.0, "24000 events took {took:?}");
        let bytes: usize = (0..20)
            .map(|n| {
                serde_json::to_string(&h.since(&format!("s{n}"), 0))
                    .unwrap()
                    .len()
            })
            .sum();
        assert!(bytes < 500_000, "rings hold {bytes} bytes");
        eprintln!("24000 events in {took:?}, rings {bytes} bytes");
    }

    // ---- the turn-and-obligation reducer, end to end: recorded hook payloads -> adapter -> hub ----

    fn hook(h: &EventHub, name: &str, extra: serde_json::Value, now: u64) {
        let mut d =
            serde_json::json!({"hook_event_name": name, "tool_name": "Bash", "prompt": "do it"});
        for (k, v) in extra.as_object().unwrap() {
            d[k] = v.clone();
        }
        let (src, conf, kinds) = crate::adapters::claude(&d).unwrap_or_else(|| panic!("{name}"));
        for k in kinds {
            h.feed("s", src, conf, k, now, None);
        }
    }

    fn now_state(h: &EventHub) -> AgentState {
        h.snapshot_at(u64::MAX / 2)[0].state
    }

    fn quiet() -> serde_json::Value {
        serde_json::json!({})
    }

    fn ask() -> serde_json::Value {
        serde_json::json!({"tool_name": "AskUserQuestion"})
    }

    #[test]
    fn a_question_is_needs_input_from_the_first_event_and_a_late_notification_changes_nothing() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        assert_eq!(now_state(&h), AgentState::Working);
        hook(&h, "PreToolUse", ask(), 2_000);
        assert_eq!(
            now_state(&h),
            AgentState::NeedsInput,
            "no waiting for the notification"
        );
        assert_eq!(h.attention_items().len(), 1);
        hook(&h, "PermissionRequest", ask(), 2_050);
        hook(
            &h,
            "Notification",
            serde_json::json!({"notification_type": "permission_prompt", "message": "Claude needs your permission"}),
            8_000,
        );
        assert_eq!(now_state(&h), AgentState::NeedsInput);
        assert_eq!(h.attention_items().len(), 1, "one wait, one item");
        assert_eq!(h.activities_at(9_000)[0].activity, Activity::Blocked);
    }

    #[test]
    fn answering_resumes_work_and_the_wait_is_cleared_for_good() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PreToolUse", ask(), 2_000);
        hook(&h, "PostToolUse", ask(), 12_000);
        assert_eq!(now_state(&h), AgentState::Working);
        assert!(h.attention_items().is_empty(), "the wait was answered");
        hook(&h, "Stop", quiet(), 14_000);
        assert_eq!(now_state(&h), AgentState::Done);
    }

    #[test]
    fn a_notification_after_the_dialog_was_answered_does_not_block_again() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PreToolUse", quiet(), 1_100);
        hook(&h, "PermissionRequest", quiet(), 1_200);
        hook(&h, "PostToolUse", quiet(), 3_000);
        hook(
            &h,
            "Notification",
            serde_json::json!({"notification_type": "permission_prompt", "message": "m"}),
            7_000,
        );
        assert_eq!(now_state(&h), AgentState::Working);
    }

    #[test]
    fn a_guess_from_the_scan_never_overrides_a_wait_however_long_it_lasts() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PreToolUse", ask(), 2_000);
        for t in [10_000, 70_000, 400_000, 900_000] {
            h.feed(
                "s",
                Source::Scan,
                Confidence::Low,
                EventKind::State {
                    state: AgentState::Idle,
                },
                t,
                None,
            );
            assert_eq!(now_state(&h), AgentState::NeedsInput, "at {t}");
        }
        assert_eq!(h.activities_at(900_000)[0].activity, Activity::Blocked);
    }

    #[test]
    fn a_long_tool_is_still_working_after_the_old_sixty_second_window() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PreToolUse", quiet(), 2_000);
        h.feed(
            "s",
            Source::Scan,
            Confidence::Low,
            EventKind::State {
                state: AgentState::Idle,
            },
            200_000,
            None,
        );
        assert_eq!(now_state(&h), AgentState::Working);
    }

    #[test]
    fn esc_on_a_dialog_sends_no_hook_and_the_screen_closes_the_wait() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PreToolUse", ask(), 2_000);
        let gone = Screen {
            working: false,
            idle: true,
            blocked: false,
        };
        h.reconcile_at("s", None, gone, 3_000);
        h.reconcile_at("s", None, gone, 5_000);
        assert_eq!(
            now_state(&h),
            AgentState::NeedsInput,
            "inside the grace window"
        );
        h.reconcile_at("s", None, gone, 7_000);
        h.reconcile_at("s", None, gone, 9_000);
        assert_eq!(now_state(&h), AgentState::Idle);
        assert!(h.attention_items().is_empty());
        // Nothing "finished": the owner interrupted it.
        assert!(h
            .attention_items()
            .iter()
            .all(|a| a.kind != AttentionKind::FinishedUnseen));
    }

    #[test]
    fn a_dialog_that_is_still_on_screen_is_never_closed_by_the_screen() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PermissionRequest", quiet(), 2_000);
        let shown = Screen {
            working: false,
            idle: false,
            blocked: true,
        };
        for t in (10_000..500_000).step_by(2_000) {
            h.reconcile_at("s", None, shown, t);
        }
        assert_eq!(now_state(&h), AgentState::NeedsInput);
    }

    #[test]
    fn idle_prompt_ends_a_turn_that_never_sent_stop_and_raises_nothing() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(
            &h,
            "Notification",
            serde_json::json!({"notification_type": "idle_prompt", "message": "waiting"}),
            61_000,
        );
        assert_eq!(now_state(&h), AgentState::Idle);
        assert!(h
            .attention_items()
            .iter()
            .all(|a| a.kind != AttentionKind::NeedsInput));
    }

    #[test]
    fn a_failed_turn_is_an_error_and_a_new_prompt_starts_clean() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PermissionRequest", quiet(), 1_500);
        hook(
            &h,
            "StopFailure",
            serde_json::json!({"error": "rate_limit"}),
            2_000,
        );
        assert_eq!(now_state(&h), AgentState::Error);
        hook(&h, "UserPromptSubmit", quiet(), 3_000);
        assert_eq!(now_state(&h), AgentState::Working);
    }

    #[test]
    fn turn_and_block_inputs_are_never_stored_or_sent_on() {
        let h = EventHub::new([1; 32]);
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        hook(&h, "PermissionRequest", quiet(), 1_500);
        assert!(h
            .since("s", 0)
            .iter()
            .all(|e| !matches!(e.kind, EventKind::Turn { .. } | EventKind::Block { .. })));
    }

    #[test]
    fn two_open_requests_keep_an_acp_workspace_blocked_until_both_are_answered() {
        let h = EventHub::new([1; 32]);
        let ev =
            |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now, None);
        let block = |key: &str, open: bool| EventKind::Block {
            key: key.into(),
            open,
        };
        ev(
            EventKind::Turn {
                open: true,
                outcome: None,
            },
            1_000,
        );
        ev(block("acp-1", true), 2_000);
        ev(block("acp-2", true), 2_100);
        ev(block("acp-1", false), 3_000);
        assert_eq!(h.snapshot_at(4_000)[0].state, AgentState::NeedsInput);
        ev(block("acp-2", false), 3_500);
        assert_eq!(h.snapshot_at(4_000)[0].state, AgentState::Working);
    }

    #[test]
    fn a_t0_request_names_its_action_on_the_open_attention_item() {
        use termenbox_core::events::{Decision, DecisionOption};
        let h = EventHub::new([1; 32]);
        let ev =
            |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now, None);
        ev(
            EventKind::Turn {
                open: true,
                outcome: None,
            },
            1_000,
        );
        ev(
            EventKind::Block {
                key: "acp-1".into(),
                open: true,
            },
            2_000,
        );
        ev(
            EventKind::Prompt {
                id: "acp-1".into(),
                prompt_kind: "permission".into(),
                choices: vec!["Yes".into()],
                decision: Some(Decision {
                    options: vec![DecisionOption {
                        id: "y".into(),
                        kind: "allow_once".into(),
                        name: "Yes".into(),
                    }],
                    detail: Some("Write a.txt".into()),
                }),
            },
            2_050,
        );
        let items = h.attention_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, AttentionKind::NeedsInput);
        assert_eq!(items[0].prompt.as_deref(), Some("Write a.txt"));
    }

    // ---- history (U12): what the hub records of a task ----

    fn hub_with_history() -> (EventHub, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("termenbox-events-{}", uuid::Uuid::new_v4()));
        let history =
            crate::history::History::open(&dir, crate::history::Limits::new(90, 20), 0).unwrap();
        (EventHub::new([1; 32]).with_history(Arc::new(history)), dir)
    }

    #[test]
    fn a_hooked_task_lands_in_the_history_with_its_wait_and_without_the_prompt_body() {
        let (h, dir) = hub_with_history();
        let prompt = "Fix the login test\nthe secret plan is in the second line";
        hook(
            &h,
            "UserPromptSubmit",
            serde_json::json!({ "prompt": prompt }),
            1_000,
        );
        hook(&h, "PreToolUse", ask(), 2_000);
        hook(&h, "PostToolUse", ask(), 12_000);
        hook(&h, "Stop", quiet(), 14_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].task.as_deref(), Some("Fix the login test"));
        assert_eq!(
            runs[0].outcome,
            Some(termenbox_core::protocol::RunOutcome::Finished)
        );
        assert_eq!(runs[0].blocked_ms, 10_000);
        assert_eq!(
            (runs[0].started_at, runs[0].ended_at),
            (1_000, Some(14_000))
        );
        h.history().unwrap().wait_idle();
        let file = std::fs::read_to_string(dir.join("history.jsonl")).unwrap();
        assert!(!file.contains("second line") && !file.contains("secret plan"));
    }

    #[test]
    fn a_task_the_screen_guessed_first_is_still_recorded() {
        let (h, _dir) = hub_with_history();
        // The scan saw the agent working before its hook said so.
        h.push(
            "s",
            Source::Screen,
            Confidence::Medium,
            EventKind::State {
                state: AgentState::Working,
            },
            500,
        );
        hook(
            &h,
            "UserPromptSubmit",
            serde_json::json!({ "prompt": "Fix the login test" }),
            1_000,
        );
        hook(&h, "Stop", quiet(), 4_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 5_000).runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].task.as_deref(), Some("Fix the login test"));
        assert_eq!(
            runs[0].outcome,
            Some(termenbox_core::protocol::RunOutcome::Finished)
        );
    }

    #[test]
    fn an_acp_task_keeps_its_title() {
        let (h, _dir) = hub_with_history();
        let acp =
            |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now, None);
        // The order the ACP workspace uses: the turn opens, then the prompt names the task.
        acp(
            EventKind::Turn {
                open: true,
                outcome: None,
            },
            1_000,
        );
        acp(
            EventKind::Task {
                title: "Add a retry to the uploader".into(),
            },
            1_000,
        );
        acp(
            EventKind::Turn {
                open: false,
                outcome: Some(AgentState::Done),
            },
            3_000,
        );
        let runs = h.history().unwrap().query(0, u64::MAX, 4_000).runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].task.as_deref(), Some("Add a retry to the uploader"));
    }

    #[test]
    fn a_plan_item_never_becomes_the_title() {
        let (h, dir) = hub_with_history();
        hook(
            &h,
            "UserPromptSubmit",
            serde_json::json!({ "prompt": "Tidy the repo" }),
            1_000,
        );
        h.push(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::Plan {
                items: vec![PlanItem {
                    text: "edit /home/me/secret-project/src/lib.rs".into(),
                    status: "in_progress".into(),
                }],
            },
            2_000,
        );
        hook(&h, "Stop", quiet(), 3_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 4_000).runs;
        assert_eq!(runs[0].task.as_deref(), Some("Tidy the repo"));
        h.history().unwrap().wait_idle();
        let file = std::fs::read_to_string(dir.join("history.jsonl")).unwrap();
        assert!(!file.contains("secret-project"));
    }

    #[test]
    fn a_guess_from_the_screen_is_not_a_task() {
        let (h, _dir) = hub_with_history();
        let state = |s: AgentState, now: u64| {
            h.push(
                "s",
                Source::Screen,
                Confidence::Medium,
                EventKind::State { state: s },
                now,
            )
        };
        state(AgentState::Working, 1_000);
        state(AgentState::Idle, 5_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 6_000).runs;
        assert!(runs.is_empty());
    }

    #[test]
    fn a_workspace_that_goes_away_ends_its_task_as_stopped() {
        let (h, _dir) = hub_with_history();
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        h.forget_except("", &[]);
        let runs = h.history().unwrap().query(0, u64::MAX, u64::MAX / 2).runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].outcome,
            Some(termenbox_core::protocol::RunOutcome::Stopped)
        );
    }

    // ---- usage (U13): the counts a hook-driven task takes ----

    /// What the socket handler does with one Claude Code hook payload, in its order.
    fn claude_payload(h: &EventHub, root: &std::path::Path, data: serde_json::Value, now: u64) {
        h.note_claude_usage_in(Some(root), "s", &data);
        let (src, conf, kinds) = crate::adapters::claude(&data).unwrap();
        for k in kinds {
            h.feed("s", src, conf, k, now, None);
        }
        if let Some(cwd) = data.get("cwd").and_then(|c| c.as_str()) {
            h.record_project("s", cwd);
        }
    }

    #[test]
    fn a_hooked_task_takes_the_transcript_counts_the_folder_and_a_rate_limit() {
        let (h, dir) = hub_with_history();
        let root = dir.join(".claude");
        let file = root.join("projects/p/s.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("projects/p/s.jsonl");
        let common = |name: &str| {
            serde_json::json!({
                "hook_event_name": name, "transcript_path": path, "cwd": "/home/u/work/termenbox",
                "prompt": "Fix the login test", "error": "rate_limit",
            })
        };
        claude_payload(&h, &root, common("UserPromptSubmit"), 1_000);
        std::fs::write(
            &file,
            include_str!("../testdata/transcripts/claude-usage.jsonl"),
        )
        .unwrap();
        claude_payload(&h, &root, common("StopFailure"), 9_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].project.as_deref(), Some("termenbox"));
        let u = runs[0].usage.as_ref().unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (Some(5), Some(1575)));
        assert_eq!(
            (u.cache_read_tokens, u.cache_write_tokens),
            (Some(67914), Some(15592))
        );
        assert_eq!((u.cost_micros, u.rate_limited), (None, 1));
        // The folder is a name, not a path, and no transcript text reached the store.
        h.history().unwrap().wait_idle();
        let file = std::fs::read_to_string(dir.join("history.jsonl")).unwrap();
        assert!(
            !file.contains("/home/u")
                && !file.contains("All green")
                && !file.contains("Running the tests")
        );
    }

    #[test]
    fn an_interrupted_turn_does_not_leak_into_the_next_task() {
        let (h, dir) = hub_with_history();
        let root = dir.join(".claude");
        let file = root.join("projects/p/s.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("projects/p/s.jsonl");
        let d = |name: &str| serde_json::json!({"hook_event_name": name, "transcript_path": path, "prompt": "x"});
        let usage = include_str!("../testdata/transcripts/claude-usage.jsonl");
        let (first, second) = usage.split_at(usage.find("{\"parentUuid\":\"u2\"").unwrap());
        claude_payload(&h, &root, d("UserPromptSubmit"), 1_000);
        std::fs::write(&file, first).unwrap();
        // Esc: the turn stops without a Stop hook, and the task ends as stopped.
        h.forget_except("", &[]);
        claude_payload(&h, &root, d("UserPromptSubmit"), 100_000);
        std::fs::write(&file, format!("{first}{second}")).unwrap();
        claude_payload(&h, &root, d("Stop"), 110_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 200_000).runs;
        let last = runs.iter().max_by_key(|r| r.started_at).unwrap();
        let u = last.usage.as_ref().unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (Some(3), Some(264)));
    }

    #[test]
    fn a_folder_that_only_shares_the_name_of_the_home_folder_is_recorded() {
        let (h, _dir) = hub_with_history();
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        let home = std::env::var("HOME").unwrap();
        let name = home
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        h.record_project("s", &format!("/srv/{name}"));
        h.feed(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Done,
            },
            2_000,
            None,
        );
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        assert_eq!(runs[0].project.as_deref(), Some(name.as_str()));
    }

    #[test]
    fn a_workspace_in_the_home_folder_is_not_filed_under_the_user_name() {
        let (h, _dir) = hub_with_history();
        hook(&h, "UserPromptSubmit", quiet(), 1_000);
        let home = std::env::var("HOME").unwrap();
        h.record_project("s", &home);
        h.record_project("s", "/work/termenbox");
        h.record_project("s", "/work/termenbox/src");
        h.feed(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Done,
            },
            2_000,
            None,
        );
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        assert_eq!(runs[0].project.as_deref(), Some("termenbox"));
    }

    #[test]
    fn a_hook_task_whose_transcript_is_unreadable_has_no_usage_not_zeros() {
        let (h, dir) = hub_with_history();
        let root = dir.join(".claude");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let d = |name: &str| serde_json::json!({"hook_event_name": name, "transcript_path": root.join("none.jsonl"), "prompt": "x"});
        claude_payload(&h, &root, d("UserPromptSubmit"), 1_000);
        claude_payload(&h, &root, d("Stop"), 3_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        assert!(runs[0].usage.is_none());
    }

    #[test]
    fn an_acp_agent_cost_and_turn_tokens_are_kept_as_the_agent_said_them() {
        let (h, _dir) = hub_with_history();
        let turn = |open: bool, now: u64| {
            h.feed(
                "acp",
                Source::Acp,
                Confidence::High,
                EventKind::Turn {
                    open,
                    outcome: (!open).then_some(AgentState::Done),
                },
                now,
                None,
            )
        };
        turn(true, 1_000);
        h.feed(
            "acp",
            Source::Acp,
            Confidence::High,
            EventKind::Usage {
                used: 10,
                size: 100,
                cost: Some("0.25 USD".into()),
            },
            2_000,
            None,
        );
        h.record_tokens(
            "acp",
            crate::history::Tokens {
                input: Some(1472),
                output: Some(35),
                cache_read: Some(13008),
                cache_write: None,
            },
        );
        turn(false, 3_000);
        let runs = h.history().unwrap().query(0, u64::MAX, 20_000).runs;
        let u = runs[0].usage.as_ref().unwrap();
        assert_eq!(
            (u.input_tokens, u.cache_read_tokens, u.cache_write_tokens),
            (Some(1472), Some(13008), None)
        );
        assert_eq!(
            (u.cost_micros, u.cost_currency.as_deref()),
            (Some(250_000), Some("USD"))
        );
    }

    // ---- the tab a hook ran in hears what the reducer derives (decisions 0122, 0149) ----

    fn tab_hub() -> (EventHub, Arc<crate::tabstatus::TabStatus>) {
        let status = Arc::new(crate::tabstatus::TabStatus::default());
        let h = EventHub::new([1; 32]).with_tab_status(status.clone());
        status.set_pane("%1", "s", "t1");
        status.set_pane("%2", "s", "t2");
        (h, status)
    }

    /// One recorded Claude Code hook payload, delivered the way `deliver` does for a pane.
    fn tab_hook(h: &EventHub, tab: &str, name: &str, extra: serde_json::Value, now: u64) {
        let mut d =
            serde_json::json!({"hook_event_name": name, "tool_name": "Bash", "prompt": "do it"});
        for (k, v) in extra.as_object().unwrap() {
            d[k] = v.clone();
        }
        let (src, conf, kinds) = crate::adapters::claude(&d).unwrap_or_else(|| panic!("{name}"));
        let pane = if tab == "t1" { "%1" } else { "%2" };
        for k in kinds.clone() {
            h.feed("s", src, conf, k, now, Some(tab));
        }
        h.apply_hook_tab(pane, "s", &kinds, now);
    }

    fn tab_state(status: &crate::tabstatus::TabStatus, tab: &str) -> AgentState {
        status
            .snapshot()
            .into_iter()
            .find(|s| s.tab == tab)
            .map_or(AgentState::Unknown, |s| s.state)
    }

    #[test]
    fn a_hook_turn_works_the_tab_and_its_end_finishes_it() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        assert_eq!(tab_state(&status, "t1"), AgentState::Working);
        tab_hook(&h, "t1", "Stop", quiet(), 5_000);
        assert_eq!(tab_state(&status, "t1"), AgentState::Done);
    }

    #[test]
    fn a_question_makes_the_tab_wait_and_the_answer_resumes_it() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        assert_eq!(tab_state(&status, "t1"), AgentState::NeedsInput);
        tab_hook(&h, "t1", "PostToolUse", ask(), 9_000);
        assert_eq!(tab_state(&status, "t1"), AgentState::Working);
    }

    #[test]
    fn attention_meter_counts_a_question_its_answer_and_working_time() {
        let (h, _status) = tab_hub();
        let h = h.with_meter(Arc::new(crate::attention_meter::AttentionMeter::in_memory()));
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        let id = h.attention_items()[0].id.clone();
        assert!(h.card_seen(&id, 3_000));
        tab_hook(&h, "t1", "PostToolUse", ask(), 9_000);
        let days = h.meter_days();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].raised, 1);
        assert_eq!(days[0].by_layer["human"], 1);
        assert_eq!(days[0].human_ms, 3_000);
        assert!(
            !h.card_seen("a999", 1_000),
            "an unknown card is not credited"
        );
    }

    #[test]
    fn attention_meter_tells_the_devices_when_a_tab_starts_and_stops_working() {
        let (h, _status) = tab_hub();
        let h = h.with_meter(Arc::new(crate::attention_meter::AttentionMeter::in_memory()));
        let mut rx = h.subscribe_status();
        let meters = |rx: &mut broadcast::Receiver<StatusChange>| {
            std::iter::from_fn(|| rx.try_recv().ok())
                .filter(|c| matches!(c, StatusChange::Meter(_)))
                .count()
        };
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        assert_eq!(meters(&mut rx), 1, "start of work");
        h.announce_working_time();
        assert_eq!(meters(&mut rx), 1, "still working");
        tab_hook(&h, "t1", "Stop", quiet(), 5_000);
        assert_eq!(meters(&mut rx), 1, "end of work");
        h.announce_working_time();
        assert_eq!(meters(&mut rx), 0, "nothing is working");
    }

    #[test]
    fn attention_meter_credits_the_answer_to_the_class_of_the_connection_that_gave_it() {
        let (h, _status) = tab_hub();
        let h = h.with_meter(Arc::new(crate::attention_meter::AttentionMeter::in_memory()));
        // A keystroke before any question is open is not an answer.
        h.note_answerer("s", Some("t1"), "desktop");
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        h.note_answerer("s", Some("t1"), "desktop");
        h.note_answerer("s", Some("t1"), "phone");
        tab_hook(&h, "t1", "PostToolUse", ask(), 9_000);
        // A second question, answered with no connection noted, is `other`.
        tab_hook(&h, "t1", "PermissionRequest", ask(), 10_000);
        tab_hook(&h, "t1", "PostToolUse", ask(), 19_000);
        let d = &h.meter_days()[0];
        assert_eq!(d.by_device.get("phone"), Some(&1));
        assert_eq!(d.by_device.get("desktop"), None);
        assert_eq!(d.by_device.get("other"), Some(&1));
    }

    #[test]
    fn attention_meter_does_not_count_a_closed_tab_as_an_answer() {
        let (h, _status) = tab_hub();
        let h = h.with_meter(Arc::new(crate::attention_meter::AttentionMeter::in_memory()));
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        h.forget_tab("s", "t1");
        let days = h.meter_days();
        assert_eq!(days[0].raised, 1);
        assert!(days[0].by_layer.is_empty());
    }

    #[test]
    fn a_failed_turn_shows_error_until_the_next_prompt() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(
            &h,
            "t1",
            "StopFailure",
            serde_json::json!({"error": "rate_limit"}),
            5_000,
        );
        assert_eq!(tab_state(&status, "t1"), AgentState::Error);
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 60_000);
        assert_eq!(
            tab_state(&status, "t1"),
            AgentState::Working,
            "a new prompt ends the error"
        );
    }

    #[test]
    fn seeing_a_failed_tab_clears_its_error() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(
            &h,
            "t1",
            "StopFailure",
            serde_json::json!({"error": "rate_limit"}),
            5_000,
        );
        let item = h
            .attention_items()
            .into_iter()
            .find(|a| a.kind == AttentionKind::Failed);
        let item = item.expect("a failed turn raises attention");
        assert_eq!(item.tab.as_deref(), Some("t1"));
        assert!(h.ack(&item.id));
        assert_eq!(
            tab_state(&status, "t1"),
            AgentState::Idle,
            "the error stayed after the owner saw it"
        );
    }

    #[test]
    fn one_tabs_finish_does_not_end_the_other_tabs_turn() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_500);
        tab_hook(&h, "t2", "Stop", quiet(), 5_000);
        assert_eq!(tab_state(&status, "t2"), AgentState::Done);
        assert_eq!(
            tab_state(&status, "t1"),
            AgentState::Working,
            "t1 is still inferring"
        );
    }

    #[test]
    fn a_second_tab_starting_a_turn_works_too() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_500);
        assert_eq!(tab_state(&status, "t2"), AgentState::Working);
    }

    // ---- several tabs in one workspace: each tab has its own turn and waits (decision 0149) ----

    fn items(h: &EventHub, kind: AttentionKind) -> Vec<Option<String>> {
        let mut tabs: Vec<_> = h
            .attention_items()
            .into_iter()
            .filter(|a| a.kind == kind)
            .map(|a| a.tab)
            .collect();
        tabs.sort();
        tabs
    }

    fn some(tabs: &[&str]) -> Vec<Option<String>> {
        tabs.iter().map(|t| Some(t.to_string())).collect()
    }

    fn workspace_state(h: &EventHub, at: u64) -> AgentState {
        h.snapshot_at(at)[0].state
    }

    #[test]
    fn the_workspace_works_while_any_tab_works() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_500);
        tab_hook(&h, "t2", "Stop", quiet(), 5_000);
        assert_eq!(
            workspace_state(&h, 5_100),
            AgentState::Working,
            "t1 still works"
        );
        tab_hook(&h, "t1", "Stop", quiet(), 9_000);
        assert_eq!(workspace_state(&h, 9_100), AgentState::Done);
    }

    #[test]
    fn a_wait_beats_work_across_tabs_and_ends_with_its_own_tab() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_500);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        assert_eq!(workspace_state(&h, 2_100), AgentState::NeedsInput);
        tab_hook(&h, "t2", "Stop", quiet(), 5_000);
        assert_eq!(
            workspace_state(&h, 5_100),
            AgentState::NeedsInput,
            "t2 ending does not answer t1"
        );
        tab_hook(&h, "t1", "PostToolUse", ask(), 9_000);
        assert_eq!(workspace_state(&h, 9_100), AgentState::Working);
    }

    #[test]
    fn another_tabs_finish_does_not_clear_a_tabs_question() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 2_500);
        tab_hook(&h, "t2", "Stop", quiet(), 5_000);
        assert_eq!(
            items(&h, AttentionKind::NeedsInput),
            some(&["t1"]),
            "t1 still asks"
        );
        assert_eq!(items(&h, AttentionKind::FinishedUnseen), some(&["t2"]));
    }

    #[test]
    fn each_tab_raises_and_clears_its_own_question() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_100);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        tab_hook(&h, "t2", "PermissionRequest", ask(), 2_100);
        assert_eq!(items(&h, AttentionKind::NeedsInput), some(&["t1", "t2"]));
        tab_hook(&h, "t1", "PostToolUse", ask(), 9_000);
        assert_eq!(
            items(&h, AttentionKind::NeedsInput),
            some(&["t2"]),
            "only t1 was answered"
        );
    }

    #[test]
    fn two_tabs_finishing_one_after_the_other_each_raise_an_item() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_100);
        tab_hook(&h, "t1", "Stop", quiet(), 5_000);
        tab_hook(&h, "t2", "Stop", quiet(), 6_000);
        assert_eq!(
            items(&h, AttentionKind::FinishedUnseen),
            some(&["t1", "t2"])
        );
    }

    #[test]
    fn acknowledging_one_tabs_failure_leaves_the_others() {
        let (h, status) = tab_hub();
        for (tab, at) in [("t1", 1_000), ("t2", 1_100)] {
            tab_hook(&h, tab, "UserPromptSubmit", quiet(), at);
            tab_hook(
                &h,
                tab,
                "StopFailure",
                serde_json::json!({"error": "rate_limit"}),
                at + 4_000,
            );
        }
        let failed = h
            .attention_items()
            .into_iter()
            .filter(|a| a.kind == AttentionKind::Failed);
        let first = failed
            .clone()
            .find(|a| a.tab.as_deref() == Some("t1"))
            .expect("t1 failed");
        assert_eq!(failed.count(), 2, "one item per failed tab");
        assert!(h.ack(&first.id));
        assert_eq!(tab_state(&status, "t1"), AgentState::Idle);
        assert_eq!(tab_state(&status, "t2"), AgentState::Error);
    }

    #[test]
    fn the_screen_closes_one_tabs_turn_and_not_the_others() {
        let (h, status) = tab_hub();
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_100);
        let idle = Screen {
            working: false,
            idle: true,
            blocked: false,
        };
        h.reconcile_at("s", Some("t1"), idle, 10_000);
        h.reconcile_at("s", Some("t1"), idle, 12_000);
        assert_eq!(
            workspace_state(&h, 12_100),
            AgentState::Working,
            "t2 still works"
        );
        h.reconcile_at("s", Some("t2"), idle, 14_000);
        h.reconcile_at("s", Some("t2"), idle, 16_000);
        assert_eq!(workspace_state(&h, 16_100), AgentState::Idle);
        assert_eq!(tab_state(&status, "t1"), AgentState::Idle);
    }

    #[test]
    fn closing_a_tab_drops_its_wait_from_the_workspace() {
        let (h, _) = tab_hub();
        tab_hook(&h, "t2", "UserPromptSubmit", quiet(), 1_000);
        tab_hook(&h, "t1", "UserPromptSubmit", quiet(), 1_100);
        tab_hook(&h, "t1", "PermissionRequest", ask(), 2_000);
        assert_eq!(workspace_state(&h, 2_100), AgentState::NeedsInput);
        h.forget_tab("s", "t1");
        assert_eq!(
            workspace_state(&h, 3_000),
            AgentState::Working,
            "only t2 is left"
        );
    }
}
