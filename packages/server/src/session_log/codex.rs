//! The Codex adapter (decision 0157). Reads `<codex home>/sessions/YYYY/MM/DD/rollout-<time>-<session id>.jsonl`.
//!
//! A rollout is a line of records in order, with no tree: a fork is a new file that repeats the parent's records, so
//! it reads as one conversation. Each record has an `ordinal`, which is the turn id. The conversation is the
//! `response_item` records: messages (`user`, `assistant`; `developer` and tagged setup text are not shown),
//! reasoning summaries, the names of tool calls, and a `compacted` marker. The `event_msg` stream repeats them and is
//! skipped. Tool input and output are never read into an entry.
//!
//! The format is the vendor's and internal (fact 0065). The head is checked on every open: a first line that is not
//! a `session_meta` record, or a `cli_version` of another major than 0, turns Codex search off. The session's `cwd`
//! must be the directory asked for, so an id cannot reach another project's rollout.
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::ops::ControlFlow;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::Value;

use super::claude::{next_line, turn_id, AtRead, UNMAPPABLE, UNREADABLE};
use super::scan::{Needle, BLOCK};
use super::*;

pub const AGENT: &str = "codex";
/// The major version of the files this adapter was written against (0.166.0 measured).
const SUPPORTED_MAJOR: &str = "0";
const CACHED_LINES: usize = 4;
/// `YYYY-MM-DDThh-mm-ss`, the time in a rollout file name.
const STAMP_LEN: usize = 19;
/// Top-level record types that carry no conversation. `event_msg` repeats the `response_item` stream.
const NOISE: &[&str] = &[
    "session_meta",
    "event_msg",
    "turn_context",
    "world_state",
    "token_usage_record",
    "token_count",
    "thread_settings_applied",
    "additional_tools",
];

pub struct CodexLog {
    /// The `sessions` directory.
    root: PathBuf,
    lines: Mutex<Vec<(PathBuf, Arc<Lines>)>>,
    /// Where each session id was found, so the date folders are walked once.
    found: Mutex<HashMap<String, PathBuf>>,
}

impl CodexLog {
    pub fn new(root: PathBuf) -> Self {
        CodexLog {
            root,
            lines: Mutex::new(Vec::new()),
            found: Mutex::new(HashMap::new()),
        }
    }

    /// `$CODEX_HOME` or `~/.codex`, then `sessions`.
    pub fn from_env() -> Option<Self> {
        let base = match std::env::var_os("CODEX_HOME") {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".codex"),
        };
        Some(Self::new(base.join("sessions")))
    }

    /// The rollout file of a session: the newest date folder first, the file named for this exact id.
    fn locate(&self, id: &str) -> Option<PathBuf> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        if let Some(p) = self.found.lock().unwrap().get(id) {
            if p.is_file() {
                return Some(p.clone());
            }
        }
        let suffix = format!("-{id}.jsonl");
        let is_ours = |name: &str| {
            name.strip_prefix("rollout-")
                .and_then(|r| r.strip_suffix(&suffix))
                .is_some_and(|stamp| stamp.len() == STAMP_LEN)
        };
        let path = newest_first_dirs(&self.root)
            .into_iter()
            .flat_map(|y| newest_first_dirs(&y))
            .flat_map(|m| newest_first_dirs(&m))
            .find_map(|d| {
                std::fs::read_dir(d)
                    .ok()?
                    .flatten()
                    .find(|e| is_ours(&e.file_name().to_string_lossy()))
                    .map(|e| e.path())
            })?;
        self.found
            .lock()
            .unwrap()
            .insert(id.to_string(), path.clone());
        Some(path)
    }

    fn open(&self, s: &SessionRef) -> Result<(PathBuf, File, Fingerprint), LogError> {
        let path = self.locate(&s.id).ok_or(LogError::Missing)?;
        let file = File::open(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => LogError::Missing,
            _ => LogError::Disabled(UNREADABLE),
        })?;
        let meta = file
            .metadata()
            .map_err(|_| LogError::Disabled(UNREADABLE))?;
        let fp = Fingerprint::of_meta(&meta);
        if check_head(&file, fp.size)?.as_deref() != Some(s.dir.as_path()) {
            return Err(LogError::Missing);
        }
        Ok((path, file, fp))
    }

    /// The conversation records of the file as it is at `fp`, from the cache while the file has not changed.
    fn lines(&self, file: &File, path: &Path, fp: Fingerprint) -> Result<Arc<Lines>, LogError> {
        if let Some((_, l)) = self
            .lines
            .lock()
            .unwrap()
            .iter()
            .find(|(p, l)| p == path && l.fp == fp)
        {
            return Ok(l.clone());
        }
        let lines = Arc::new(Lines::build(file, fp).map_err(|_| LogError::Disabled(UNREADABLE))?);
        let mut cache = self.lines.lock().unwrap();
        cache.retain(|(p, _)| p != path);
        cache.push((path.to_path_buf(), lines.clone()));
        if cache.len() > CACHED_LINES {
            cache.remove(0);
        }
        Ok(lines)
    }
}

/// The sub-directories of `dir`, newest name first (the date folders sort as they read).
fn newest_first_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.reverse();
    dirs
}

/// Whether the head of the file looks like a Codex rollout this code understands. Returns the session's `cwd`, or
/// none when the first record is not whole yet (a session being born).
fn check_head(file: &File, size: u64) -> Result<Option<PathBuf>, LogError> {
    let len = (size as usize).min(BLOCK);
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    file.read_exact_at(&mut buf, 0)
        .map_err(|_| LogError::Disabled(UNREADABLE))?;
    let Some(end) = memchr::memchr(b'\n', &buf) else {
        return if size as usize <= BLOCK {
            Ok(None)
        } else {
            Err(LogError::Disabled("unreadable head"))
        };
    };
    let first: Value =
        serde_json::from_slice(&buf[..end]).map_err(|_| LogError::Disabled("unreadable head"))?;
    if first.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Err(LogError::Disabled("unreadable head"));
    }
    let major = first
        .pointer("/payload/cli_version")
        .and_then(Value::as_str)
        .and_then(|v| v.split('.').next());
    if major != Some(SUPPORTED_MAJOR) {
        return Err(LogError::Disabled("unknown version"));
    }
    Ok(first
        .pointer("/payload/cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from))
}

/// The few fields that say whether a record is part of the conversation.
#[derive(Deserialize)]
struct Head<'a> {
    ordinal: Option<u64>,
    #[serde(rename = "type", borrow)]
    ty: Option<Cow<'a, str>>,
    #[serde(borrow)]
    payload: Option<PayloadHead<'a>>,
}

#[derive(Deserialize)]
struct PayloadHead<'a> {
    #[serde(rename = "type", borrow)]
    ty: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
}

impl Head<'_> {
    /// Whether the record can show something: false for the noise, setup messages and tool output.
    fn is_shown(&self) -> bool {
        let Some(ty) = self.ty.as_deref() else {
            return false;
        };
        if NOISE.contains(&ty) {
            return false;
        }
        if ty != "response_item" {
            return true; // `compacted`, or a type this version does not know: a notice
        }
        let p = self.payload.as_ref();
        let pty = p.and_then(|p| p.ty.as_deref()).unwrap_or("");
        if pty == "message" {
            return matches!(
                p.and_then(|p| p.role.as_deref()),
                Some("user" | "assistant")
            );
        }
        !pty.ends_with("_output")
    }
}

struct Node {
    turn: String,
    offset: u64,
    len: usize,
}

/// The records that can show something, in file order, with where each is in the file.
pub struct Lines {
    fp: Fingerprint,
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
}

impl Lines {
    fn build(file: &File, fp: Fingerprint) -> io::Result<Lines> {
        // Only what the file held when the fingerprint was taken: a record being appended is not chased.
        let mut r = BufReader::with_capacity(BLOCK, AtRead { file, pos: 0 }.take(fp.size));
        let mut line = Vec::new();
        let mut at = 0u64;
        let mut nodes: Vec<Node> = Vec::new();
        while let Some(n) = next_line(&mut r, &mut line)? {
            // A line cut short fails to parse and is dropped; one that is whole without its newline is kept.
            if let Ok(h) = serde_json::from_slice::<Head>(&line) {
                if let (Some(o), true) = (h.ordinal, h.is_shown()) {
                    nodes.push(Node {
                        turn: o.to_string(),
                        offset: at,
                        len: n as usize,
                    });
                }
            }
            at += n;
        }
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.turn.clone(), i))
            .collect();
        Ok(Lines { fp, nodes, index })
    }

    /// The node an entry's turn id belongs to: the ordinal, or `<ordinal>:<n>` for a later block of a record.
    fn node_of(&self, turn: &str) -> Option<usize> {
        self.index
            .get(turn)
            .or_else(|| turn.rsplit_once(':').and_then(|(u, _)| self.index.get(u)))
            .copied()
    }
}

struct Part {
    kind: Kind,
    text: String,
}

fn part(kind: Kind, text: &str) -> Option<Part> {
    (!text.trim().is_empty()).then(|| Part {
        kind,
        text: text.to_string(),
    })
}

fn notice() -> Part {
    Part {
        kind: Kind::Notice,
        text: UNMAPPABLE.to_string(),
    }
}

/// Text that opens and closes with one tag is the agent's own setup (environment, instructions), not the user's.
fn is_setup(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('<') && t.ends_with('>')
}

/// Whether a line can show any text to a search, judged from its bytes alone, before any JSON is parsed. Only a
/// message or a reasoning summary has text to show; a tool call and its output are dropped here instead of parsed and
/// discarded. The markers are quoted JSON values, which text inside a string never matches (its quotes are escaped).
fn may_hold_text(line: &[u8]) -> bool {
    let has = |m: &[u8]| memchr::memmem::find(line, m).is_some();
    has(br#""message""#) || has(br#""reasoning""#)
}

/// What a record shows. Tool input and output are not read here at all.
fn parts(v: &Value) -> Vec<Part> {
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "response_item" => {}
        "compacted" => {
            return vec![Part {
                kind: Kind::Notice,
                text: "Conversation compacted".to_string(),
            }]
        }
        t if NOISE.contains(&t) => return Vec::new(),
        _ => return vec![notice()],
    }
    let p = &v["payload"];
    let blocks = p.get("content").and_then(Value::as_array);
    match p.get("type").and_then(Value::as_str).unwrap_or("") {
        "message" => {
            let (kind, want) = match p.get("role").and_then(Value::as_str) {
                Some("user") => (Kind::User, "input_text"),
                Some("assistant") => (Kind::Message, "output_text"),
                Some("developer") => return Vec::new(),
                _ => return vec![notice()],
            };
            let Some(blocks) = blocks else {
                return vec![notice()];
            };
            blocks
                .iter()
                .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                    Some(t) if t == want => {
                        let text = b.get("text")?.as_str()?;
                        if kind == Kind::User && is_setup(text) {
                            None
                        } else {
                            part(kind, text)
                        }
                    }
                    Some("input_image") => None,
                    _ => Some(notice()),
                })
                .collect()
        }
        "reasoning" => p
            .get("summary")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|b| part(Kind::Thought, b.get("text")?.as_str()?))
            .collect(),
        t if t.ends_with("_call") => vec![Part {
            kind: Kind::Tool,
            text: p
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_else(|| t.trim_end_matches("_call"))
                .to_string(),
        }],
        _ => vec![notice()],
    }
}

fn entries_of(file: &File, node: &Node) -> Result<Vec<Entry>, LogError> {
    let mut buf = vec![0u8; node.len];
    file.read_exact_at(&mut buf, node.offset)
        .map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => LogError::StaleCursor,
            _ => LogError::Disabled(UNREADABLE),
        })?;
    let Ok(v) = serde_json::from_slice::<Value>(&buf) else {
        return Ok(vec![Entry::notice(&node.turn, UNMAPPABLE)]);
    };
    Ok(parts(&v)
        .into_iter()
        .enumerate()
        .map(|(n, p)| Entry::new(turn_id(&node.turn, n), p.kind, &p.text))
        .collect())
}

impl SessionLog for CodexLog {
    fn agent(&self) -> &str {
        AGENT
    }

    fn search(&self, s: &SessionRef, query: &str, max_hits: usize) -> Result<Vec<Hit>, LogError> {
        self.search_in(s, query, &mut SearchRun::single(max_hits))
    }

    fn search_in(
        &self,
        s: &SessionRef,
        query: &str,
        run: &mut SearchRun,
    ) -> Result<Vec<Hit>, LogError> {
        let Some(needle) = Needle::new(query) else {
            return Ok(Vec::new());
        };
        let (_, file, fp) = self.open(s)?;
        let size = run.start(fp);
        let SearchRun {
            scanner,
            budget,
            stopped,
            ..
        } = run;
        let limit = budget
            .limits()
            .per_session
            .min(budget.limits().max_hits.saturating_sub(budget.hits()));
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut hits: Vec<Hit> = Vec::new();
        scanner
            .scan_file(&file, size, &needle, budget, |c| {
                // The scan matched bytes; what counts is the scrubbed text of a conversation record.
                if !may_hold_text(c.bytes) {
                    return ControlFlow::Continue(());
                }
                let Ok(v) = serde_json::from_slice::<Value>(c.bytes) else {
                    return ControlFlow::Continue(()); // torn
                };
                let Some(ordinal) = v.get("ordinal").and_then(Value::as_u64) else {
                    return ControlFlow::Continue(());
                };
                for (n, p) in parts(&v).into_iter().enumerate() {
                    if matches!(p.kind, Kind::Tool | Kind::Notice) {
                        continue;
                    }
                    let Some((snippet, highlight)) = snippet(&p.text, query) else {
                        continue;
                    };
                    hits.push(Hit {
                        turn: turn_id(&ordinal.to_string(), n),
                        kind: p.kind,
                        snippet,
                        highlight,
                        at: v
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
                // The cap is checked between records, so a record's hits are never split across two pages.
                if hits.len() >= limit {
                    return ControlFlow::Break(());
                }
                ControlFlow::Continue(())
            })
            .map_err(|_| LogError::Disabled(UNREADABLE))?;
        *stopped = scanner.stopped_at();
        for _ in &hits {
            let _ = budget.record_hit();
        }
        Ok(hits)
    }

    fn list(&self, since: Option<std::time::SystemTime>) -> Listing {
        let mut out = Listing::default();
        // `<root>/YYYY/MM/DD/rollout-<stamp>-<id>.jsonl`, three real directories deep; a link is never entered.
        let days = real_entries(&self.root, true)
            .into_iter()
            .flat_map(|y| real_entries(&y.path(), true))
            .flat_map(|m| real_entries(&m.path(), true));
        for day in days {
            for e in real_entries(&day.path(), false) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(id) = name
                    .strip_prefix("rollout-")
                    .and_then(|r| r.strip_suffix(".jsonl"))
                    .and_then(|r| r.get(STAMP_LEN + 1..))
                    .filter(|i| plain_id(i))
                else {
                    continue;
                };
                let Ok(mtime) = e.metadata().and_then(|m| m.modified()) else {
                    continue;
                };
                if since.is_some_and(|s| mtime < s) {
                    out.older += 1;
                    continue;
                }
                let Ok(file) = File::open(e.path()) else {
                    continue;
                };
                let Ok(meta) = file.metadata() else {
                    continue;
                };
                let Ok(Some(dir)) = check_head(&file, meta.len()) else {
                    continue;
                };
                out.files.push(SessionFile {
                    agent: AGENT.into(),
                    id: id.to_string(),
                    dir,
                    mtime,
                });
            }
        }
        out.files.sort_by(|a, b| b.mtime.cmp(&a.mtime));
        out
    }

    fn last_event(&self, s: &SessionRef) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.locate(&s.id)?).ok()?.modified().ok()
    }

    fn window(
        &self,
        s: &SessionRef,
        anchor: &str,
        before: usize,
        after: usize,
    ) -> Result<Window, LogError> {
        let (path, file, fp) = self.open(s)?;
        let lines = self.lines(&file, &path, fp)?;
        let at = lines.node_of(anchor).ok_or(LogError::NoAnchor)?;
        let own = entries_of(&file, &lines.nodes[at])?;
        let pos = own
            .iter()
            .position(|e| e.turn == anchor)
            .ok_or(LogError::NoAnchor)?;
        let (before, after) = (before.min(MAX_SIDE), after.min(MAX_SIDE));

        // Nearest first.
        let mut back: Vec<Entry> = own[..pos].iter().rev().cloned().collect();
        let mut i = at;
        while back.len() < before && i > 0 {
            i -= 1;
            back.extend(entries_of(&file, &lines.nodes[i])?.into_iter().rev());
        }
        back.truncate(before);
        let mut fwd: Vec<Entry> = own[pos + 1..].to_vec();
        let mut j = at + 1;
        while fwd.len() < after && j < lines.nodes.len() {
            fwd.extend(entries_of(&file, &lines.nodes[j])?);
            j += 1;
        }
        fwd.truncate(after);

        let mut entries: Vec<Entry> = back.into_iter().rev().collect();
        let anchor_at = entries.len();
        entries.push(own[pos].clone());
        entries.extend(fwd);
        fit_window(&mut entries, anchor_at, WINDOW_BYTES);
        Ok(Window {
            entries,
            anchor: anchor.to_string(),
            fingerprint: fp,
        })
    }

    /// Pages by record: a cursor's `pos` is a place in the record list, and a record's entries come together, so a
    /// page can hold more than `count`. The file only grows, so the fingerprint alone says a cursor still holds.
    fn page(
        &self,
        s: &SessionRef,
        cursor: Option<&Cursor>,
        direction: Direction,
        count: usize,
    ) -> Result<Page, LogError> {
        let (path, file, fp) = self.open(s)?;
        if let Some(c) = cursor {
            if !c.fingerprint.still_holds(&fp) {
                return Err(LogError::StaleCursor);
            }
        }
        let lines = self.lines(&file, &path, fp)?;
        let n = lines.nodes.len();
        let count = count.min(MAX_SIDE);
        let pos = match cursor {
            Some(c) if c.pos as usize <= n => c.pos as usize,
            Some(_) => return Err(LogError::StaleCursor),
            None => match direction {
                Direction::Older => n,
                Direction::Newer => 0,
            },
        };
        let mut got: Vec<Entry> = Vec::new();
        let next = match direction {
            Direction::Older => {
                let mut i = pos;
                while i > 0 && got.len() < count {
                    i -= 1;
                    got.splice(0..0, entries_of(&file, &lines.nodes[i])?);
                }
                (i > 0).then_some(i)
            }
            Direction::Newer => {
                let mut i = pos;
                while i < n && got.len() < count {
                    got.extend(entries_of(&file, &lines.nodes[i])?);
                    i += 1;
                }
                (i < n).then_some(i)
            }
        };
        Ok(Page {
            entries: got,
            next: next.map(|p| Cursor {
                fingerprint: fp,
                pos: p as u64,
            }),
        })
    }
}

#[cfg(test)]
mod tests;
