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

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::events::sanitize_block;
use scan::{Budget, Cancel, Limits, PooledScanner};

pub mod claude;
pub mod codex;
#[cfg(test)]
mod contract;
pub mod scan;
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

/// A session file found by walking an adapter's root (decision 0167): the agent, its own session id, the directory the
/// file says it ran in, and when it was last written.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionFile {
    pub agent: String,
    pub id: String,
    pub dir: PathBuf,
    pub mtime: std::time::SystemTime,
}

impl fmt::Debug for SessionFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionFile({})", self.agent) // the id and the directory are not logged
    }
}

/// What walking a root found: the files to search, newest first, and how many the `since` bound ruled out unopened.
#[derive(Debug, Default)]
pub struct Listing {
    pub files: Vec<SessionFile>,
    pub older: usize,
}

/// The entries of `dir` that are real directories or real files, as the walk of a root sees them. A symlink is neither
/// (`DirEntry::file_type` does not follow it), so nothing a link points at, inside the root or out of it, is read.
pub(crate) fn real_entries(dir: &Path, want_dir: bool) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_type()
                .is_ok_and(|t| if want_dir { t.is_dir() } else { t.is_file() })
        })
        .collect()
}

/// A session id the adapters accept in a path: a plain name.
pub(crate) fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
/// scrubber hides is not a match: otherwise a query could confirm a guess at a secret one prefix at a time. The match
/// ignores case and accents, as the scan does (`scan::Needle`).
pub fn snippet(raw: &str, needle: &str) -> Option<(String, (usize, usize))> {
    let mut want: Vec<char> = Vec::new();
    for c in needle.chars() {
        fold_char(c, |f| want.push(f));
    }
    if want.is_empty() {
        return None;
    }
    let text = sanitize_block(raw, MAX_TEXT_CHARS, MAX_TEXT_LINES);
    let chars: Vec<char> = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    // A char can fold to several or to none, so each folded char remembers the char it came from.
    let mut folded: Vec<char> = Vec::with_capacity(chars.len());
    let mut origin: Vec<usize> = Vec::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        fold_char(*c, |f| {
            folded.push(f);
            origin.push(i);
        });
    }
    let found = folded
        .windows(want.len())
        .position(|w| w == want.as_slice())?;
    let start = origin[found];
    let end = origin[found + want.len() - 1] + 1;
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

/// Hands `push` the folded form of `c`: lower case with accents removed (canonical decomposition, combining marks
/// dropped). The one definition of "the same letter" for the scan and for `snippet`.
pub fn fold_char(c: char, mut push: impl FnMut(char)) {
    if c.is_ascii() {
        push(c.to_ascii_lowercase());
        return;
    }
    for lower in c.to_lowercase() {
        unicode_normalization::char::decompose_canonical(lower, |d| {
            if !unicode_normalization::char::is_combining_mark(d) {
                push(d);
            }
        });
    }
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
        Ok(Self::of_meta(&std::fs::metadata(path)?))
    }

    pub fn of_meta(m: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Fingerprint {
            inode: m.ino(),
            size: m.size(),
            mtime_ns: m.mtime() as u128 * 1_000_000_000 + m.mtime_nsec() as u128,
        }
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

/// Most bytes of entries a window carries: a reply must fit the 64 KiB a direct-transport message allows.
pub const WINDOW_BYTES: usize = 48 * 1024;
/// Most entries a window or a page reads on each side, whatever the caller asks.
pub const MAX_SIDE: usize = 200;

/// What a search carries from one session to the next: the reusable scan buffers and the caps shared by all of them.
pub struct SearchRun {
    pub scanner: PooledScanner,
    pub budget: Budget,
    /// Input to the next `search_in`: continue the file where a page stopped. A file that was rewritten since starts
    /// over, and `restarted` says so.
    pub resume: Option<Cursor>,
    pub restarted: bool,
    /// Output of `search_in`: where it stopped before the start of the file, or none when it read to the start.
    pub stopped: Option<u64>,
    /// Output of `search_in`: the file as it was read, to tie a continuation to it.
    pub seen: Option<Fingerprint>,
}

impl SearchRun {
    pub fn new(limits: Limits, cancel: Cancel) -> Self {
        SearchRun {
            scanner: PooledScanner::take(),
            budget: Budget::new(limits, cancel),
            resume: None,
            restarted: false,
            stopped: None,
            seen: None,
        }
    }

    /// How many bytes of the file to read, from its start: all of it, or up to where a continuation left off. Records
    /// the file as it is, to tie the next continuation to it.
    pub fn start(&mut self, fp: Fingerprint) -> u64 {
        self.seen = Some(fp);
        match self.resume.take() {
            Some(c) if c.fingerprint.still_holds(&fp) => c.pos.min(fp.size),
            Some(_) => {
                self.restarted = true;
                fp.size
            }
            None => fp.size,
        }
    }

    /// A run for one session that ends after `max_hits`.
    pub fn single(max_hits: usize) -> Self {
        let limits = Limits {
            max_hits,
            per_session: max_hits,
            ..Limits::default()
        };
        Self::new(limits, Cancel::default())
    }
}

/// Drops the entries furthest from `entries[anchor]` until the rest fit `cap` bytes, and returns where the anchor is
/// now. The anchor itself is cut to fit when it alone is too big.
pub fn fit_window(entries: &mut Vec<Entry>, mut anchor: usize, cap: usize) -> usize {
    let size = |e: &Entry| e.text.len() + e.turn.len() + 48;
    let mut total: usize = entries.iter().map(size).sum();
    while total > cap && entries.len() > 1 {
        let gone = if entries.len() - 1 - anchor >= anchor {
            entries.pop().unwrap()
        } else {
            anchor -= 1;
            entries.remove(0)
        };
        total -= size(&gone);
    }
    if total > cap {
        let e = &mut entries[anchor];
        let room = cap.saturating_sub(e.turn.len() + 48);
        let mut end = room.min(e.text.len());
        while !e.text.is_char_boundary(end) {
            end -= 1;
        }
        e.text.truncate(end);
        e.truncated = true;
    }
    anchor
}

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
    /// `search` under caps shared with other sessions (`run.budget`): stops at `per_session` hits, counts the hits
    /// it keeps against the budget, and leaves the reason for any cut there. The default ignores the sharing.
    fn search_in(
        &self,
        session: &SessionRef,
        query: &str,
        run: &mut SearchRun,
    ) -> Result<Vec<Hit>, LogError> {
        self.search(session, query, run.budget.limits().per_session)
    }
    /// Every session file in the adapter's root, newest first, without following a symlink and without opening a file
    /// last written before `since`, which is counted in `older`. A file of a format the adapter does not recognise is
    /// left out. The default lists nothing.
    fn list(&self, _since: Option<std::time::SystemTime>) -> Listing {
        Listing::default()
    }
    /// When the session's file was last written, read without opening it, or none when it cannot be told. Files are
    /// append-only, so nothing in one is newer. The ledger's recorded time will win over this when there is one.
    fn last_event(&self, _session: &SessionRef) -> Option<std::time::SystemTime> {
        None
    }
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
    /// Every adapter this build has, each rooted at its agent's home (`$CLAUDE_CONFIG_DIR`, `$CODEX_HOME`).
    pub fn standard() -> Self {
        let mut logs = SessionLogs::default();
        if let Some(a) = claude::ClaudeLog::from_env() {
            logs.register(Arc::new(a));
        }
        if let Some(a) = codex::CodexLog::from_env() {
            logs.register(Arc::new(a));
        }
        logs
    }

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

    /// The files of every agent that is on, merged newest first, and the agents that are off with their reasons.
    pub fn list(
        &self,
        since: Option<std::time::SystemTime>,
    ) -> (Listing, Vec<(String, &'static str)>) {
        let mut all = Listing::default();
        let mut off = Vec::new();
        for agent in self.adapters.keys() {
            match self.get(agent) {
                Ok(a) => {
                    let l = a.list(since);
                    all.older += l.older;
                    all.files.extend(l.files);
                }
                Err(LogError::Disabled(why)) => off.push((agent.clone(), why)),
                Err(_) => {}
            }
        }
        all.files.sort_by(|a, b| b.mtime.cmp(&a.mtime));
        (all, off)
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
