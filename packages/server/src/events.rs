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
    SessionActivity, SessionAgentState, Source, Task, ToolPhase,
};
use tokio::sync::broadcast;

use crate::reducer::{Edge, Reducer, Screen};

pub const RING: usize = 50;
pub const MAX_SESSIONS: usize = 20;
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

/// Several lines of untrusted text (a Markdown reply, a diff) for the ACP session view: newlines and the
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
    Activity(SessionActivity),
    Attention(Vec<Attention>),
}

/// A session id as events know it: a plain name, or `manager:name` when several managers run.
pub fn valid_event_session(id: &str) -> bool {
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
    /// The agent the scan found in each session; only sessions that have events report it.
    agents: Arc<Mutex<HashMap<String, String>>>,
    /// The transcript file a Claude Code hook named for each session. Untrusted: `transcript::resolve` decides
    /// whether it may be opened, and nothing here is logged.
    transcripts: Arc<Mutex<HashMap<String, String>>>,
    tx: broadcast::Sender<AgentEvent>,
    attention: Arc<Mutex<Vec<Attention>>>,
    next_attention: Arc<std::sync::atomic::AtomicU64>,
    /// The Activity last announced per session, so only a change is sent.
    derived: Arc<Mutex<HashMap<String, SessionActivity>>>,
    /// The turn-and-obligation reducer of each session whose agent reports turns (hooks, ACP).
    reducers: Arc<Mutex<HashMap<String, Reducer>>>,
    status_tx: broadcast::Sender<StatusChange>,
    /// The tasks agents worked on, kept on disk (U12); absent in tests that do not need it.
    history: Option<Arc<crate::history::History>>,
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
        }
    }

    /// Records the tasks this hub sees end (U12).
    pub fn with_history(mut self, history: Arc<crate::history::History>) -> Self {
        self.history = Some(history);
        self
    }

    pub fn history(&self) -> Option<&Arc<crate::history::History>> {
        self.history.as_ref()
    }

    /// What a fact from an agent means for the history: a state change opens or ends a task, a title names it (only the
    /// task title the agent reports; plan items are never copied). Guesses from the screen or the scan are not recorded.
    fn note_history(
        &self,
        session: &str,
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
                let agent = self.agents.lock().unwrap().get(session).cloned();
                history.state(session, agent.as_deref(), *state, ts);
            }
            EventKind::Task { title } => history.task(session, title),
            _ => {}
        }
    }

    /// The hub key lives in the state directory (0600) so sessions that outlive a server restart keep a
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

    fn mac(&self, session: &str) -> Hmac<Sha256> {
        let mut m = Hmac::<Sha256>::new_from_slice(&*self.key).expect("any key length");
        m.update(b"termenbox-event-session\0");
        m.update(session.as_bytes());
        m
    }

    /// The token a session's agent tools present; valid for that session name only.
    pub fn token(&self, session: &str) -> String {
        URL_SAFE_NO_PAD.encode(self.mac(session).finalize().into_bytes())
    }

    pub fn token_matches(&self, session: &str, token: &str) -> bool {
        URL_SAFE_NO_PAD
            .decode(token)
            .map(|t| self.mac(session).verify_slice(&t).is_ok())
            .unwrap_or(false)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<StatusChange> {
        self.status_tx.subscribe()
    }

    /// An event reported by a session's own tool: accepted only with that session's token.
    pub fn ingest(
        &self,
        session: &str,
        token: &str,
        source: Source,
        confidence: Confidence,
        kinds: Vec<EventKind>,
    ) -> Result<()> {
        if !valid_event_session(session) || !self.token_matches(session, token) {
            tracing::info!(target: "audit", event = "agent_event_refused", session = %session);
            return Err(anyhow!("token does not match the session"));
        }
        for k in kinds {
            self.feed(session, source, confidence, k, now_ms());
        }
        Ok(())
    }

    /// An event from the server's own adapters (scan, screen). A repeated state is not an event.
    pub fn report(&self, session: &str, source: Source, confidence: Confidence, kind: EventKind) {
        self.feed(session, source, confidence, kind, now_ms());
    }

    /// Where an agent's own events enter: a turn or obligation edge goes through the session's reducer, which
    /// decides the state; everything else is stored as it is.
    fn feed(
        &self,
        session: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
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
                self.push(session, source, confidence, kind, now);
            }
            return;
        };
        if matches!(kind, EventKind::Tool { .. }) {
            self.push(session, source, confidence, kind, now);
        }
        let next = {
            let mut reducers = self.reducers.lock().unwrap();
            if !reducers.contains_key(session) && reducers.len() >= MAX_SESSIONS {
                return;
            }
            reducers
                .entry(session.to_string())
                .or_default()
                .apply(edge, now)
        };
        if let Some(state) = next {
            self.push(session, source, confidence, EventKind::State { state }, now);
        }
    }

    /// What the screen of an agent session shows now, against what the reducer believes. Closes an obligation or a
    /// turn that ended without telling anyone (Esc sends no hook); never opens one.
    pub fn reconcile(&self, session: &str, screen: Screen) {
        self.reconcile_at(session, screen, now_ms());
    }

    fn reconcile_at(&self, session: &str, screen: Screen, now: u64) {
        let next = {
            let mut reducers = self.reducers.lock().unwrap();
            reducers
                .get_mut(session)
                .and_then(|r| r.reconcile(screen, now))
        };
        if let Some(state) = next {
            // A reconciliation is a fact about the screen, not a finished turn: it clears a wait but raises nothing.
            self.push(
                session,
                Source::Hook,
                Confidence::Medium,
                EventKind::State { state },
                now,
            );
        }
    }

    /// Whether a fact about this session (an open turn or a wait for the human) must not be overridden by a guess.
    fn pinned(&self, session: &str, now: u64) -> bool {
        self.reducers
            .lock()
            .unwrap()
            .get(session)
            .is_some_and(|r| r.pins_at(now))
    }

    pub fn push(
        &self,
        session: &str,
        source: Source,
        confidence: Confidence,
        kind: EventKind,
        now: u64,
    ) {
        let kind = sanitize_kind(kind);
        if matches!(kind, EventKind::Unknown) {
            return;
        }
        // A guess from the screen or the scan never overrides an open turn or a wait for the human: the state
        // those describe ends with an event or with the screen reconciliation, not with a clock.
        if matches!(kind, EventKind::State { .. })
            && matches!(tier(source), Some(1 | 2))
            && self.pinned(session, now)
        {
            return;
        }
        let mut rings = self.rings.lock().unwrap();
        if !rings.contains_key(session) && rings.len() >= MAX_SESSIONS {
            if let Some(oldest) = rings
                .iter()
                .min_by_key(|(_, r)| r.last_event_at)
                .map(|(k, _)| k.clone())
            {
                rings.remove(&oldest);
            }
        }
        let ring = rings.entry(session.to_string()).or_insert_with(|| Ring {
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
                self.note_history(session, source, confidence, &kind, now);
                self.settle(session, source, confidence, &kind, prev_hook, now);
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
                        self.settle(session, source, confidence, &kind, prev_hook, now);
                        return;
                    }
                }
            }
        }
        ring.seq += 1;
        ring.last_event_at = now;
        let event = AgentEvent {
            session: session.to_string(),
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
        self.note_history(session, source, confidence, &event.kind, event.ts);
        // Counts and kinds only, never content (audit-logging-no-secrets).
        tracing::info!(target: "audit", event = "agent_event", session = %event.session, source = ?event.source, seq = event.seq);
        self.settle(session, source, confidence, &event.kind, prev_hook, now);
        let _ = self.tx.send(event);
    }

    /// After an event: move the session's Attention items by fact, then announce a changed Activity.
    /// Only a hook or Codex (a fact, not a guess) raises or clears Attention.
    fn settle(
        &self,
        session: &str,
        source: Source,
        confidence: Confidence,
        kind: &EventKind,
        prev_hook: Option<AgentState>,
        now: u64,
    ) {
        if tier(source) == Some(0) && confidence != Confidence::Low {
            match kind {
                EventKind::State { state } if prev_hook != Some(*state) => {
                    if *state != AgentState::NeedsInput {
                        self.clear_where(session, |a| a.kind == AttentionKind::NeedsInput);
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
                        self.raise(session, k, source, now);
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
                        a.session == session
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

    fn raise(&self, session: &str, kind: AttentionKind, source: Source, now: u64) {
        let task = self
            .rings
            .lock()
            .unwrap()
            .get(session)
            .and_then(|r| r.task.clone());
        let mut list = self.attention.lock().unwrap();
        // One open item per session and kind: a newer one replaces the older.
        list.retain(|a| !(a.session == session && a.kind == kind));
        let id = self
            .next_attention
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        list.push(Attention {
            id: format!("a{id}"),
            session: session.to_string(),
            kind,
            raised_at: now,
            task,
            prompt: None,
        });
        while list.len() > MAX_ATTENTION {
            list.remove(0);
        }
        let snapshot = list.clone();
        drop(list);
        // Session, kind and source only (audit-logging-no-secrets).
        tracing::info!(target: "audit", event = "attention_raised", session = %session, kind = ?kind, source = ?source);
        let _ = self.status_tx.send(StatusChange::Attention(snapshot));
    }

    fn clear_where(&self, session: &str, pred: impl Fn(&Attention) -> bool) {
        let mut list = self.attention.lock().unwrap();
        let before = list.len();
        list.retain(|a| !(a.session == session && pred(a)));
        if list.len() != before {
            let snapshot = list.clone();
            drop(list);
            tracing::info!(target: "audit", event = "attention_cleared", session = %session, by = "fact");
            let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        }
    }

    /// A device shows the item's session in the foreground, or the owner dismisses it. A `needs_input`
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
        tracing::info!(target: "audit", event = "attention_cleared", session = %gone.session, by = "ack");
        let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        true
    }

    pub fn attention_items(&self) -> Vec<Attention> {
        self.attention.lock().unwrap().clone()
    }

    /// Recomputes every session's Activity and announces the ones that changed. Called after each event and
    /// on a timer, so a silent `working` decays to `unknown`.
    pub fn refresh(&self, now: u64) {
        let open: Vec<String> = self
            .attention
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.session.clone())
            .collect();
        let rings = self.rings.lock().unwrap();
        let agents = self.agents.lock().unwrap();
        let mut derived = self.derived.lock().unwrap();
        let mut changed = Vec::new();
        for (session, r) in rings.iter() {
            let next = SessionActivity {
                session: session.clone(),
                activity: derive_activity(r, now),
                ts: now,
                task: r.task.clone(),
                agent: agents.get(session).cloned(),
                hint: if open.contains(session) {
                    None
                } else {
                    derive_hint(r, now)
                },
            };
            let same = derived.get(session).is_some_and(|d| {
                (d.activity, &d.task, &d.agent, d.hint)
                    == (next.activity, &next.task, &next.agent, next.hint)
            });
            if !same {
                derived.insert(session.clone(), next.clone());
                changed.push(next);
            }
        }
        drop((derived, agents, rings));
        for a in changed {
            let _ = self.status_tx.send(StatusChange::Activity(a));
        }
    }

    /// The Activity of every session, for a client that has just connected.
    pub fn activities(&self) -> Vec<SessionActivity> {
        self.activities_at(now_ms())
    }

    pub fn activities_at(&self, now: u64) -> Vec<SessionActivity> {
        self.refresh(now);
        let mut out: Vec<_> = self.derived.lock().unwrap().values().cloned().collect();
        out.sort_by(|a, b| a.session.cmp(&b.session));
        out
    }

    /// Forget a session that no longer exists.
    /// Only ids that start with `prefix` are considered (one per session manager when several run).
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
            .retain(|k, _| !k.starts_with(prefix) || live.contains(k));
        let mut list = self.attention.lock().unwrap();
        let before = list.len();
        list.retain(|a| !a.session.starts_with(prefix) || live.contains(&a.session));
        if list.len() != before {
            let snapshot = list.clone();
            drop(list);
            let _ = self.status_tx.send(StatusChange::Attention(snapshot));
        }
    }

    /// Remembers the transcript path a hook of `session` named (only after its token was accepted).
    pub fn set_transcript(&self, session: &str, path: &str) {
        if path.is_empty() || path.len() > 1024 || !valid_event_session(session) {
            return;
        }
        let mut map = self.transcripts.lock().unwrap();
        if !map.contains_key(session) && map.len() >= MAX_SESSIONS * 4 {
            return;
        }
        if map.get(session).map(String::as_str) != Some(path) {
            map.insert(session.to_string(), path.to_string());
        }
    }

    pub fn transcript(&self, session: &str) -> Option<String> {
        self.transcripts.lock().unwrap().get(session).cloned()
    }

    /// Records which agent the process scan found in `session`.
    pub fn set_agent(&self, session: &str, agent: &str) {
        let mut agents = self.agents.lock().unwrap();
        if agents.get(session).map(String::as_str) != Some(agent) {
            agents.insert(session.to_string(), agent.to_string());
        }
    }

    pub fn snapshot(&self) -> Vec<SessionAgentState> {
        self.snapshot_at(now_ms())
    }

    pub fn snapshot_at(&self, now: u64) -> Vec<SessionAgentState> {
        let rings = self.rings.lock().unwrap();
        let agents = self.agents.lock().unwrap();
        let mut out: Vec<_> = rings
            .iter()
            .map(|(session, r)| SessionAgentState {
                session: session.clone(),
                state: r.state,
                seq: r.seq,
                stale: r.state == AgentState::Working
                    && now.saturating_sub(r.last_event_at) > STALE_AFTER_MS,
                ts: r.last_event_at,
                agent: agents.get(session).cloned(),
                last_response_at: r.last_response_at,
                waiting_since: r.waiting_since,
            })
            .collect();
        out.sort_by(|a, b| a.session.cmp(&b.session));
        out
    }

    /// The prompt a one-tap answer may target: the session's latest `prompt` event, unless the agent has
    /// moved on (a later state other than `needs_input`).
    pub fn current_prompt(&self, session: &str) -> Option<AgentEvent> {
        let rings = self.rings.lock().unwrap();
        let ring = rings.get(session)?;
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

    /// Source and confidence of the session's latest state event, and when it happened.
    pub fn last_state_event(&self, session: &str) -> Option<(Source, Confidence, u64)> {
        let rings = self.rings.lock().unwrap();
        let ring = rings.get(session)?;
        ring.events
            .iter()
            .rev()
            .find(|e| matches!(e.kind, EventKind::State { .. }))
            .map(|e| (e.source, e.confidence, e.ts))
    }

    /// The agent's own hook reported `needs_input` within the last minute and nothing newer has changed the state.
    pub fn hook_says_needs_input(&self, session: &str) -> bool {
        let now = now_ms();
        matches!(
            self.last_state_event(session),
            Some((Source::Hook, Confidence::High, ts)) if now.saturating_sub(ts) < STALE_AFTER_MS
        ) && self
            .snapshot_at(now)
            .iter()
            .any(|s| s.session == session && s.state == AgentState::NeedsInput)
    }

    /// Events of `session` after `seq`, oldest first (only what the ring still holds).
    pub fn since(&self, session: &str, seq: u64) -> Vec<AgentEvent> {
        self.rings
            .lock()
            .unwrap()
            .get(session)
            .map(|r| r.events.iter().filter(|e| e.seq > seq).cloned().collect())
            .unwrap_or_default()
    }

    pub fn sessions_with_events(&self) -> usize {
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
    let preferred = state_dir.join("events.sock");
    if preferred.as_os_str().len() <= 100 {
        return preferred;
    }
    let dir = std::env::temp_dir().join(format!("termenbox-{}", unsafe { libc::geteuid() }));
    let _ = std::fs::create_dir_all(&dir);
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir.join("events.sock")
}

/// What a session's shell gets so its tools can report: who it is, its token and where to send.
#[derive(Clone)]
pub struct EventEnv {
    pub hub: EventHub,
    pub socket: std::path::PathBuf,
}

impl EventEnv {
    pub fn vars(&self, session: &str) -> Vec<String> {
        vec![
            format!("TERMENBOX_SESSION={session}"),
            format!("TERMENBOX_EVENT_TOKEN={}", self.hub.token(session)),
            format!("TERMENBOX_EVENT_SOCKET={}", self.socket.display()),
        ]
    }
}

// ---------------------------------------------------------------------------------------------------
// Ingest socket
// ---------------------------------------------------------------------------------------------------

/// One line a tool (via `termenbox-server hook`) writes: which session, its token, which adapter, and the
/// tool's own JSON, untouched.
#[derive(serde::Deserialize)]
struct Wire {
    session: String,
    token: String,
    adapter: String,
    data: serde_json::Value,
}

const MAX_LINE: u64 = 64 * 1024;

/// A Unix socket in the state directory, mode 0600, never TCP. Every connection carries one line and is
/// answered with nothing; refusals only show in the audit log.
pub async fn serve_socket(hub: EventHub, path: std::path::PathBuf) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    loop {
        let (stream, _) = listener.accept().await?;
        let hub = hub.clone();
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
            if hub
                .ingest(&w.session, &w.token, source, confidence, kinds)
                .is_ok()
                && w.adapter == "claude"
            {
                if let Some(path) = w.data.get("transcript_path").and_then(|p| p.as_str()) {
                    hub.set_transcript(&w.session, path);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// The hook command a tool runs: `termenbox-server --hook <adapter>`
// ---------------------------------------------------------------------------------------------------

/// Copies the tool's JSON from stdin to the ingest socket. It never fails and never waits long: whatever
/// goes wrong (no socket, server down, slow disk), it returns, because the agent must not be held up by us.
pub fn run_hook(adapter: &str) {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    run_hook_with(
        adapter,
        env("TERMENBOX_SESSION"),
        env("TERMENBOX_EVENT_TOKEN"),
        env("TERMENBOX_EVENT_SOCKET"),
    );
}

pub fn run_hook_with(
    adapter: &str,
    session: Option<String>,
    token: Option<String>,
    socket: Option<String>,
) {
    use std::io::{Read, Write};
    let (Some(session), Some(token), Some(socket)) = (session, token, socket) else {
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
        let line = serde_json::json!({"session": session, "token": token, "adapter": adapter, "data": data}).to_string() + "\n";
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

    fn activity(h: &EventHub, session: &str, now: u64) -> Option<SessionActivity> {
        h.activities_at(now)
            .into_iter()
            .find(|a| a.session == session)
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
    fn forgetting_a_session_drops_its_attention() {
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
    fn event_session_ids_may_carry_a_manager_prefix() {
        assert!(valid_event_session("work") && valid_event_session("tmux:work"));
        assert!(
            !valid_event_session("a:b:c")
                && !valid_event_session(":x")
                && !valid_event_session("x:")
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
                .map(|s| s.session.as_str())
                .collect::<Vec<_>>(),
            ["cmd:b"]
        );
    }

    #[test]
    fn a_token_only_works_for_its_own_session() {
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
        assert_eq!(h.snapshot()[0].session, "one");
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
        assert_eq!(h.sessions_with_events(), MAX_SESSIONS);
    }

    #[test]
    fn unknown_kinds_are_ignored_and_repeated_states_are_not_events() {
        let h = hub();
        h.push("a", Source::Hook, Confidence::High, EventKind::Unknown, 1);
        assert_eq!(h.sessions_with_events(), 0);
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
        tokio::spawn(serve_socket(h.clone(), sock.clone()));
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
        let send = |session: &str, token: String| {
            let line = serde_json::json!({"session": session, "token": token, "adapter": "claude",
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
        assert_eq!(h.sessions_with_events(), 0);
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

    /// 20 sessions at 20 events per second for a minute of events, processed in one go: memory is bounded
    /// by the rings and the CPU cost is tiny (the budget is 5% of one core, i.e. 3 s per minute).
    #[test]
    fn load_20_sessions_at_20_events_per_second_for_a_minute() {
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
        assert_eq!(h.sessions_with_events(), 20);
        assert!(took.as_secs_f64() < 3.0, "24000 events took {took:?}");
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
            h.feed("s", src, conf, k, now);
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
        h.reconcile_at("s", gone, 3_000);
        h.reconcile_at("s", gone, 5_000);
        assert_eq!(
            now_state(&h),
            AgentState::NeedsInput,
            "inside the grace window"
        );
        h.reconcile_at("s", gone, 7_000);
        h.reconcile_at("s", gone, 9_000);
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
            h.reconcile_at("s", shown, t);
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
    fn two_open_requests_keep_an_acp_session_blocked_until_both_are_answered() {
        let h = EventHub::new([1; 32]);
        let ev = |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now);
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
        let ev = |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now);
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
        let acp = |kind: EventKind, now: u64| h.feed("a", Source::Acp, Confidence::High, kind, now);
        // The order the ACP session uses: the turn opens, then the prompt names the task.
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
    fn a_session_that_goes_away_ends_its_task_as_stopped() {
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
}
