//! One session's machine: `apply(event, now)` and `tick(now)` return the Attention effects, `view(now)` the status.
//! Pure and deterministic, so a transition table can test it (decision 0187).
use std::collections::BTreeMap;

use termenbox_core::events::{AskKind, FailureCause, SubReason};

use super::event::{AskOutcome, Event, Outcome, Seen, SessionSource, Source};

/// Asks tracked per session; past this new ones are not tracked.
const MAX_ASKS: usize = 32;
/// A screen must disagree this many scans in a row before it closes anything; a transcript once.
pub const DISAGREE_SCANS: u8 = 2;
/// Events newer than this are never contradicted by an observation, and a tool event this soon after the turn ended
/// is a late delivery. Longer than the transcript's lag behind a hook (fact 0072: up to 400 ms).
pub const GRACE_MS: u64 = 4_000;
/// No progress proof for this long shows `working(quiet)` (decision 0188).
pub const QUIET_MS: u64 = 60_000;
/// `retrying` for this long raises a silent info item (decision 0192).
pub const RETRY_INFO_MS: u64 = 120_000;
/// An `AskContent` key that names no ask: the text belongs to the one open ask that has none yet. A late
/// `Notification` carries words but no key (fact 0072).
pub const ANY_ASK: &str = "";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Working,
    NeedsYou,
    Finished,
    Failed,
    Idle,
    Unknown,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Working,
        Status::NeedsYou,
        Status::Finished,
        Status::Failed,
        Status::Idle,
        Status::Unknown,
    ];

    /// Position in the roll-up order, worst first (decision 0187); `Unknown` is not ranked.
    fn rank(self) -> Option<u8> {
        match self {
            Status::NeedsYou => Some(0),
            Status::Failed => Some(1),
            Status::Finished => Some(2),
            Status::Idle => Some(3),
            Status::Working => Some(4),
            Status::Unknown => None,
        }
    }
}

/// The workspace status: the worst of its tabs, `Unknown` ignored.
pub fn rollup(tabs: &[Status]) -> Status {
    tabs.iter()
        .copied()
        .filter_map(|s| s.rank().map(|r| (r, s)))
        .min_by_key(|(r, _)| *r)
        .map_or(Status::Unknown, |(_, s)| s)
}

/// A silent note (decision 0193).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InfoKind {
    BackgroundDone,
    SubagentDone,
    StuckRetrying,
}

/// Which Attention item an effect is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemId {
    Ask(String),
    Result,
    Info(InfoKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A new item. `needs_you` and `result` alert, `info` is silent.
    RaiseAsk {
        key: String,
        kind: AskKind,
    },
    RaiseFinished,
    RaiseFailed(FailureCause),
    RaiseInfo(InfoKind),
    /// The text of an open ask arrived: fill the card in place, no alert.
    FillAsk {
        key: String,
        text: String,
        options: Vec<String>,
    },
    Clear(ItemId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Unseen {
    Finished,
    Failed(FailureCause),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub kind: AskKind,
    pub text: Option<String>,
    pub options: Vec<String>,
}

/// What every surface reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub status: Status,
    pub sub_reason: Option<SubReason>,
    /// The running tool when `sub_reason` is `Tool`.
    pub tool: Option<String>,
    pub cause: Option<FailureCause>,
    pub bg: u32,
    /// The kind of the first open ask.
    pub ask: Option<AskKind>,
    pub asks: usize,
    /// A blocked screen with no ask to back it: a hint, never an alert (decision 0189).
    pub hint_blocked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Machine {
    present: bool,
    turn_open: bool,
    title: Option<String>,
    tools: Vec<String>,
    subagents: u32,
    compacting: bool,
    retrying_since: Option<u64>,
    asks: BTreeMap<String, Ask>,
    unseen: Option<Unseen>,
    infos: Vec<InfoKind>,
    bg: u32,
    last_proof: u64,
    last_event_at: u64,
    last_turn_end_at: Option<u64>,
    hint_blocked: bool,
    idle_scans: u8,
    working_scans: u8,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> Status {
        if !self.asks.is_empty() {
            Status::NeedsYou
        } else if self.turn_open {
            Status::Working
        } else {
            match &self.unseen {
                Some(Unseen::Finished) => Status::Finished,
                Some(Unseen::Failed(_)) => Status::Failed,
                None if self.present => Status::Idle,
                None => Status::Unknown,
            }
        }
    }

    pub fn view(&self, now: u64) -> View {
        let status = self.status();
        let (sub_reason, tool) = if status == Status::Working {
            self.sub_reason(now)
        } else {
            (None, None)
        };
        View {
            status,
            sub_reason,
            tool,
            cause: match (status, &self.unseen) {
                (Status::Failed, Some(Unseen::Failed(c))) => Some(*c),
                _ => None,
            },
            bg: self.bg,
            ask: self.asks.values().next().map(|a| a.kind),
            asks: self.asks.len(),
            hint_blocked: self.hint_blocked && self.asks.is_empty(),
        }
    }

    pub fn ask(&self, key: &str) -> Option<&Ask> {
        self.asks.get(key)
    }

    fn sub_reason(&self, now: u64) -> (Option<SubReason>, Option<String>) {
        if self.retrying_since.is_some() {
            (Some(SubReason::Retrying), None)
        } else if self.compacting {
            (Some(SubReason::Compacting), None)
        } else if let Some(t) = self.tools.last() {
            (Some(SubReason::Tool), Some(t.clone()))
        } else if self.subagents > 0 {
            (Some(SubReason::Subagent), None)
        } else if now.saturating_sub(self.last_proof) >= QUIET_MS {
            (Some(SubReason::Quiet), None)
        } else {
            (Some(SubReason::Inferring), None)
        }
    }

    fn raise_info(&mut self, kind: InfoKind, out: &mut Vec<Effect>) {
        if !self.infos.contains(&kind) {
            self.infos.push(kind);
            out.push(Effect::RaiseInfo(kind));
        }
    }

    fn close_asks(&mut self, out: &mut Vec<Effect>) {
        for key in std::mem::take(&mut self.asks).into_keys() {
            out.push(Effect::Clear(ItemId::Ask(key)));
        }
    }

    fn end_turn(&mut self, now: u64, out: &mut Vec<Effect>) {
        self.close_asks(out);
        self.turn_open = false;
        self.tools.clear();
        self.compacting = false;
        self.retrying_since = None;
        self.last_turn_end_at = Some(now);
    }

    fn clear_unseen(&mut self, out: &mut Vec<Effect>) {
        if self.unseen.take().is_some() {
            out.push(Effect::Clear(ItemId::Result));
        }
    }

    fn reset(&mut self, out: &mut Vec<Effect>) {
        self.end_turn(0, out);
        self.last_turn_end_at = None;
        self.subagents = 0;
        self.bg = 0;
        self.title = None;
    }

    /// Time passing with no event: the stuck-retry notice.
    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if self
            .retrying_since
            .is_some_and(|t| now.saturating_sub(t) >= RETRY_INFO_MS)
            && self.turn_open
        {
            self.raise_info(InfoKind::StuckRetrying, &mut out);
        }
        out
    }

    pub fn apply(&mut self, event: Event, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        let observed = matches!(event, Event::Observe { .. });
        if !observed {
            self.idle_scans = 0;
            self.working_scans = 0;
            self.hint_blocked = false;
        }
        let touches = !matches!(
            event,
            Event::Observe { .. } | Event::Viewed | Event::Dismiss | Event::Progress
        );
        match event {
            Event::SessionStart(SessionSource::Compact) => {
                self.last_proof = now;
            }
            Event::SessionStart(_) => {
                self.reset(&mut out);
                self.present = true;
                self.last_proof = now;
            }
            Event::SessionEnd => {
                self.end_turn(now, &mut out);
                self.subagents = 0;
                self.bg = 0;
            }
            Event::TurnStart { title } => {
                self.present = true;
                self.close_asks(&mut out);
                self.turn_open = true;
                self.title = title;
                self.tools.clear();
                self.retrying_since = None;
                self.last_proof = now;
            }
            Event::TurnEnd { outcome, bg } => {
                self.present = true;
                self.bg = bg;
                // A duplicate `Stop` after the turn ended changes nothing.
                if self.turn_open || !self.asks.is_empty() {
                    self.end_turn(now, &mut out);
                    match outcome {
                        Outcome::Done => {
                            self.unseen = Some(Unseen::Finished);
                            out.push(Effect::RaiseFinished);
                        }
                        Outcome::Failed(cause) => {
                            self.unseen = Some(Unseen::Failed(cause));
                            out.push(Effect::RaiseFailed(cause));
                        }
                        Outcome::Interrupted => {}
                    }
                }
            }
            Event::ToolStart { name } => {
                self.present = true;
                let late = self
                    .last_turn_end_at
                    .is_some_and(|t| now.saturating_sub(t) < GRACE_MS);
                if !self.turn_open && !late {
                    self.turn_open = true;
                }
                if self.turn_open {
                    self.tools.push(name);
                }
                self.last_proof = now;
            }
            Event::ToolEnd { name } => {
                if let Some(i) = self.tools.iter().rposition(|t| *t == name) {
                    self.tools.remove(i);
                }
                self.last_proof = now;
            }
            Event::AskOpen { key, kind } => {
                self.present = true;
                self.turn_open = true;
                if !self.asks.contains_key(&key) && self.asks.len() < MAX_ASKS {
                    self.asks.insert(
                        key.clone(),
                        Ask {
                            kind,
                            text: None,
                            options: Vec::new(),
                        },
                    );
                    out.push(Effect::RaiseAsk { key, kind });
                }
                self.last_proof = now;
            }
            Event::AskContent { key, text, options } => {
                // Content attaches to the open ask of the same key only; a late one for a closed ask is dropped.
                // `ANY_ASK` resolves to the only open ask without text, and to nothing when that is ambiguous.
                let key = if key == ANY_ASK {
                    let mut bare = self.asks.iter().filter(|(_, a)| a.text.is_none());
                    match (bare.next(), bare.next()) {
                        (Some((k, _)), None) => k.clone(),
                        _ => return out,
                    }
                } else {
                    key
                };
                if let Some(a) = self.asks.get_mut(&key) {
                    if a.text.as_deref() != Some(text.as_str()) || a.options != options {
                        a.text = Some(text.clone());
                        a.options = options.clone();
                        out.push(Effect::FillAsk { key, text, options });
                    }
                }
            }
            Event::AskClose { key, outcome } => {
                if self.asks.remove(&key).is_some() {
                    out.push(Effect::Clear(ItemId::Ask(key)));
                    if outcome == AskOutcome::Escaped && self.asks.is_empty() {
                        // Esc on the dialog stops the turn (catalogue 4): idle, not failed.
                        self.end_turn(now, &mut out);
                    }
                }
                self.last_proof = now;
            }
            Event::SubagentStart => {
                self.subagents += 1;
                self.last_proof = now;
            }
            Event::SubagentEnd => {
                self.subagents = self.subagents.saturating_sub(1);
                if !self.turn_open {
                    self.raise_info(InfoKind::SubagentDone, &mut out);
                }
                self.last_proof = now;
            }
            Event::Background { count } => {
                if count < self.bg && !self.turn_open {
                    self.raise_info(InfoKind::BackgroundDone, &mut out);
                }
                self.bg = count;
            }
            Event::CompactStart => {
                self.compacting = true;
                self.last_proof = now;
            }
            Event::CompactEnd => {
                self.compacting = false;
                self.last_proof = now;
            }
            Event::Retry { active } => {
                if !active {
                    self.retrying_since = None;
                } else if self.retrying_since.is_none() && self.turn_open {
                    self.retrying_since = Some(now);
                }
            }
            Event::Progress => self.last_proof = now,
            Event::Observe { source, seen } => self.observe(source, seen, now, &mut out),
            Event::ProcessGone => {
                if self.turn_open || !self.asks.is_empty() {
                    self.end_turn(now, &mut out);
                    let cause = FailureCause::Crashed;
                    self.unseen = Some(Unseen::Failed(cause));
                    out.push(Effect::RaiseFailed(cause));
                } else {
                    self.end_turn(now, &mut out);
                }
                self.present = false;
                self.subagents = 0;
                self.bg = 0;
            }
            Event::Viewed | Event::Dismiss => {
                self.clear_unseen(&mut out);
                for kind in std::mem::take(&mut self.infos) {
                    out.push(Effect::Clear(ItemId::Info(kind)));
                }
            }
        }
        if touches {
            self.last_event_at = now;
        }
        out
    }

    /// A screen, transcript or scan reading, which only corrects the machine after it has disagreed long enough.
    fn observe(&mut self, source: Source, seen: Seen, now: u64, out: &mut Vec<Effect>) {
        if now.saturating_sub(self.last_event_at) < GRACE_MS {
            return;
        }
        let needed = if source == Source::Transcript {
            1
        } else {
            DISAGREE_SCANS
        };
        match seen {
            Seen::Blocked => {
                self.idle_scans = 0;
                self.working_scans = 0;
                // Nothing backs it: a hint only, no ask and no alert.
                self.hint_blocked = self.asks.is_empty();
            }
            Seen::Idle => {
                self.working_scans = 0;
                self.hint_blocked = false;
                if self.turn_open || !self.asks.is_empty() {
                    self.idle_scans += 1;
                    if self.idle_scans >= needed {
                        // The turn or the wait ended without an event: Esc.
                        self.end_turn(now, out);
                        self.idle_scans = 0;
                    }
                } else {
                    self.idle_scans = 0;
                }
            }
            Seen::Working => {
                self.idle_scans = 0;
                self.hint_blocked = false;
                if !self.turn_open && self.present {
                    self.working_scans += 1;
                    if self.working_scans >= needed {
                        self.turn_open = true;
                        self.last_proof = now;
                        self.working_scans = 0;
                    }
                }
            }
        }
    }
}
