//! The `SessionLog` port (decision 0157): how the server reads an agent's own conversation files to search them and
//! to show the turns around a hit. One adapter per agent maps that agent's files to the unified entries the ACP view
//! already renders (`user`, `message`, `thought`, `tool`, `plan`, `notice`).
//!
//! The formats belong to the vendors and change. So an adapter fails soft: a record it cannot map becomes a `notice`,
//! and a file it cannot read, or reads in a version it does not know, turns search off for that agent only
//! (`LogError::Disabled`), never a panic and never an error that reaches the others.
//!
//! What leaves this module is scrubbed (`events::sanitize_block`), tool input and output never leave it (decision
//! 0095, guardrail `session-search-is-bounded-scrubbed-and-never-logs-the-query`), and a query or snippet is never
//! put in an error, so a log line built from an error cannot carry one. Session ids are not logged either: `SessionRef`
//! prints the agent only.
//!
//! Adapters block on file reads. The caller runs them on a blocking thread, never on the control path.
#![allow(dead_code)] // wired in by the search handler (cross-session-search step 7)

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::events::sanitize_block;

#[cfg(test)]
mod contract;
#[cfg(test)]
mod stub;
#[cfg(test)]
mod tests;

/// The longest text an entry or snippet keeps, and the lines it spans, the bounds `event_log` applies to a frame.
pub const MAX_TEXT_CHARS: usize = 16_000;
pub const MAX_TEXT_LINES: usize = 200;
/// Characters of context on each side of a match in a snippet.
pub const SNIPPET_CONTEXT: usize = 80;

/// A session an adapter is asked about: the agent, its own session id, and the directory it ran in. The caller only
/// builds one for a directory that belongs to a known workspace or tab (step 6).
#[derive(Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub agent: String,
    pub id: String,
    pub dir: PathBuf,
}

impl fmt::Debug for SessionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionRef({})", self.agent)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    User,
    Message,
    Thought,
    Tool,
    Plan,
    Notice,
}

/// One turn of a conversation. `turn` is the agent's own record or message id: the key a hit, a window and the live
/// ring are joined on (never a timestamp). A `tool` entry holds the tool's name only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub turn: String,
    pub kind: Kind,
    pub text: String,
    pub truncated: bool,
}

impl Entry {
    /// An entry with `text` scrubbed and capped.
    pub fn new(turn: impl Into<String>, kind: Kind, text: &str) -> Self {
        let (text, truncated) =
            crate::events::sanitize_block_cut(text, MAX_TEXT_CHARS, MAX_TEXT_LINES);
        Entry {
            turn: turn.into(),
            kind,
            text,
            truncated,
        }
    }

    pub fn notice(turn: impl Into<String>, text: &str) -> Self {
        Entry::new(turn, Kind::Notice, text)
    }
}

/// A match: where it is, a scrubbed snippet, and the part of the snippet to highlight (character offsets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub turn: String,
    pub kind: Kind,
    pub snippet: String,
    pub highlight: (usize, usize),
    pub at: Option<String>,
}

/// The snippet of `raw` around the first match of `needle`, found in the *scrubbed* text. A match inside text the
/// scrubber hides is not a match: otherwise a query could confirm a guess at a secret one prefix at a time.
pub fn snippet(raw: &str, needle: &str) -> Option<(String, (usize, usize))> {
    let needle: Vec<char> = needle.chars().map(fold).collect();
    if needle.is_empty() {
        return None;
    }
    let text = sanitize_block(raw, MAX_TEXT_CHARS, MAX_TEXT_LINES);
    let chars: Vec<char> = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let folded: Vec<char> = chars.iter().copied().map(fold).collect();
    let start = folded
        .windows(needle.len())
        .position(|w| w == needle.as_slice())?;
    let end = start + needle.len();
    let from = start.saturating_sub(SNIPPET_CONTEXT);
    let to = (end + SNIPPET_CONTEXT).min(chars.len());
    let mut out = String::new();
    let lead = usize::from(from > 0);
    if from > 0 {
        out.push('…');
    }
    out.extend(&chars[from..to]);
    if to < chars.len() {
        out.push('…');
    }
    Some((out, (start - from + lead, end - from + lead)))
}

/// One char of case folding; step 3 extends it with accent folding for the scan.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The identity of a file at the moment a cursor was taken. The files are append-only, so growth is expected; a
/// different inode, a smaller file, or the same size with another mtime means it was rewritten or compacted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: u128,
}

impl Fingerprint {
    pub fn of(path: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path)?;
        Ok(Fingerprint {
            inode: m.ino(),
            size: m.size(),
            mtime_ns: m.mtime() as u128 * 1_000_000_000 + m.mtime_nsec() as u128,
        })
    }

    /// Whether a cursor taken at `self` still points at the same content in the file as it is `now`.
    pub fn still_holds(&self, now: &Fingerprint) -> bool {
        self.inode == now.inode
            && now.size >= self.size
            && (now.size > self.size || now.mtime_ns == self.mtime_ns)
    }
}

/// A position in a session, opaque to the client, with the fingerprint of the file it was taken on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub fingerprint: Fingerprint,
    pub pos: u64,
}

impl Cursor {
    pub fn to_token(self) -> String {
        let f = self.fingerprint;
        format!("{:x}.{:x}.{:x}.{:x}", f.inode, f.size, f.mtime_ns, self.pos)
    }

    pub fn from_token(token: &str) -> Option<Self> {
        let mut p = token.split('.');
        let inode = u64::from_str_radix(p.next()?, 16).ok()?;
        let size = u64::from_str_radix(p.next()?, 16).ok()?;
        let mtime_ns = u128::from_str_radix(p.next()?, 16).ok()?;
        let pos = u64::from_str_radix(p.next()?, 16).ok()?;
        if p.next().is_some() {
            return None;
        }
        Some(Cursor {
            fingerprint: Fingerprint {
                inode,
                size,
                mtime_ns,
            },
            pos,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Older,
    Newer,
}

/// The entries around an anchor turn, on the active branch, in conversation order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub entries: Vec<Entry>,
    pub anchor: String,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Where to continue in the asked direction, or none at the end.
    pub next: Option<Cursor>,
}

/// Why a read failed. The texts are fixed: none holds a query, a snippet, an id or a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogError {
    /// This agent's search is off (unreadable file, truncated head, unknown version). Other agents are unaffected.
    Disabled(&'static str),
    Missing,
    StaleCursor,
    NoAnchor,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::Disabled(why) => write!(f, "session search is off for this agent: {why}"),
            LogError::Missing => f.write_str("no file for this session"),
            LogError::StaleCursor => f.write_str("the session file changed"),
            LogError::NoAnchor => f.write_str("the anchor turn is not in the session"),
        }
    }
}

impl std::error::Error for LogError {}

pub trait SessionLog: Send + Sync {
    /// The agent name this adapter serves, as the registry keys it.
    fn agent(&self) -> &str;
    /// Up to `max_hits` matches of `query` in one session, newest first, on the active branch.
    fn search(
        &self,
        session: &SessionRef,
        query: &str,
        max_hits: usize,
    ) -> Result<Vec<Hit>, LogError>;
    /// `before` entries, the anchor turn and `after` entries.
    fn window(
        &self,
        session: &SessionRef,
        anchor: &str,
        before: usize,
        after: usize,
    ) -> Result<Window, LogError>;
    /// `count` entries from `cursor` (or the end, for none) in `direction`.
    fn page(
        &self,
        session: &SessionRef,
        cursor: Option<&Cursor>,
        direction: Direction,
        count: usize,
    ) -> Result<Page, LogError>;
}

/// The adapters by agent name, and which of them are off.
#[derive(Default)]
pub struct SessionLogs {
    adapters: BTreeMap<String, Arc<dyn SessionLog>>,
    off: Mutex<BTreeMap<String, &'static str>>,
}

impl SessionLogs {
    pub fn register(&mut self, adapter: Arc<dyn SessionLog>) {
        self.adapters.insert(adapter.agent().to_string(), adapter);
    }

    /// The adapter for `agent`, unless it is off or unknown.
    pub fn get(&self, agent: &str) -> Result<Arc<dyn SessionLog>, LogError> {
        if let Some(reason) = self.off.lock().unwrap().get(agent) {
            return Err(LogError::Disabled(reason));
        }
        self.adapters
            .get(agent)
            .cloned()
            .ok_or(LogError::Disabled("no adapter for this agent"))
    }

    /// Records a `Disabled` result, so later searches skip the agent and the Search sheet can say why.
    pub fn note(&self, agent: &str, error: &LogError) {
        if let LogError::Disabled(reason) = error {
            self.off.lock().unwrap().insert(agent.to_string(), reason);
        }
    }

    pub fn search(
        &self,
        session: &SessionRef,
        query: &str,
        max_hits: usize,
    ) -> Result<Vec<Hit>, LogError> {
        let result = self.get(&session.agent)?.search(session, query, max_hits);
        if let Err(e) = &result {
            self.note(&session.agent, e);
        }
        result
    }

    /// Each registered agent with the reason its search is off, if it is.
    pub fn status(&self) -> Vec<(String, Option<&'static str>)> {
        let off = self.off.lock().unwrap();
        self.adapters
            .keys()
            .map(|a| (a.clone(), off.get(a).copied()))
            .collect()
    }
}
