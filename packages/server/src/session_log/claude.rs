//! The Claude Code adapter (decision 0157). Reads `<projects>/<cwd, every non-alphanumeric as "-">/<session>.jsonl`.
//!
//! The file is a tree: each record names its `parentUuid`, and a rewind appends a new child under an older record
//! instead of removing anything. The active branch is the chain from the last user or assistant record back to the
//! root (over a compaction through `logicalParentUuid`). `Chain` holds only that chain, as file offsets, so a window
//! or a page reads the records it shows and no more; a search scans the raw bytes (`scan`) and keeps the matches
//! that are on the chain. Sidechains (subagents) are not part of the conversation shown.
//!
//! The format is the vendor's and internal (fact 0065). The head of the file is checked on every open: a first line
//! that is not JSON (the head was lost) or a `version` of another major than 2 turns Claude search off, and a record
//! this code does not know becomes a `notice`. Tool input and output are never read into an entry: a tool shows as
//! its name.
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::ops::ControlFlow;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::Value;

use super::scan::{Needle, BLOCK, MAX_LINE};
use super::*;

pub const AGENT: &str = "claude";
/// The major version of the files this adapter was written against (2.1.308 measured).
const SUPPORTED_MAJOR: &str = "2";
const CACHED_CHAINS: usize = 4;
/// Claude keeps the first 200 characters of an encoded directory name and appends `-` and a short hash (measured on
/// 2.1.308: 207 characters in all). The hash is not computed here: the directory is found by its head.
const LONG_DIR: usize = 200;
const UNMAPPABLE: &str = "A record this version cannot show";
/// Record types that carry no conversation. A record of another type that sits in the tree is shown as a notice.
const NOISE: &[&str] = &[
    "queue-operation",
    "atis-latch",
    "api-request",
    "last-prompt",
    "cost-state",
    "summary",
    "ai-title",
    "custom-title",
    "permission-mode",
    "file-history-snapshot",
    "attachment",
    "progress",
    "pr-link",
    "mode",
];

pub struct ClaudeLog {
    /// The `projects` directory.
    root: PathBuf,
    chains: Mutex<Vec<(PathBuf, Arc<Chain>)>>,
}

impl ClaudeLog {
    pub fn new(root: PathBuf) -> Self {
        ClaudeLog {
            root,
            chains: Mutex::new(Vec::new()),
        }
    }

    /// `$CLAUDE_CONFIG_DIR` or `~/.claude`, then `projects`.
    pub fn from_env() -> Option<Self> {
        let base = match std::env::var_os("CLAUDE_CONFIG_DIR") {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".claude"),
        };
        Some(Self::new(base.join("projects")))
    }

    /// Where a session lives. Only the session's own directory is looked in (or, for a long name, directories that
    /// share its head and hold this exact id), so an id cannot reach another project's file; an id that is not a plain name reaches nothing.
    fn path(&self, s: &SessionRef) -> Option<PathBuf> {
        if s.id.is_empty()
            || !s
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        let dir: String = s
            .dir
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let file = format!("{}.jsonl", s.id);
        let direct = self.root.join(&dir).join(&file);
        if direct.exists() || dir.len() <= LONG_DIR {
            return Some(direct);
        }
        // A name over 200 characters is stored shortened (`LONG_DIR`), so look among the directories that start with
        // its first 200, for this exact session file.
        let head = format!("{}-", &dir[..LONG_DIR]);
        let found = std::fs::read_dir(&self.root)
            .ok()?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&head))
            .map(|e| e.path().join(&file))
            .find(|p| p.is_file());
        Some(found.unwrap_or(direct))
    }

    fn open(&self, s: &SessionRef) -> Result<(PathBuf, File, Fingerprint), LogError> {
        let path = self.path(s).ok_or(LogError::Missing)?;
        let file = File::open(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => LogError::Missing,
            // A directory name past the file system's limit: Claude shortened it and no such directory was found.
            _ if e.raw_os_error() == Some(36) => LogError::Missing,
            _ => LogError::Disabled(UNREADABLE),
        })?;
        let meta = file
            .metadata()
            .map_err(|_| LogError::Disabled(UNREADABLE))?;
        let fp = Fingerprint::of_meta(&meta);
        check_head(&file, fp.size)?;
        Ok((path, file, fp))
    }

    /// The active branch of the file as it is at `fp`, from the cache while the file has not changed.
    fn chain(&self, file: &File, path: &Path, fp: Fingerprint) -> Result<Arc<Chain>, LogError> {
        if let Some((_, c)) = self
            .chains
            .lock()
            .unwrap()
            .iter()
            .find(|(p, c)| p == path && c.fp == fp)
        {
            return Ok(c.clone());
        }
        let chain = Arc::new(Chain::build(file, fp).map_err(|_| LogError::Disabled(UNREADABLE))?);
        let mut cache = self.chains.lock().unwrap();
        cache.retain(|(p, _)| p != path);
        cache.push((path.to_path_buf(), chain.clone()));
        if cache.len() > CACHED_CHAINS {
            cache.remove(0);
        }
        Ok(chain)
    }
}

const UNREADABLE: &str = "unreadable file";

/// Whether the head of the file looks like a Claude session this code understands.
fn check_head(file: &File, size: u64) -> Result<(), LogError> {
    let len = (size as usize).min(BLOCK);
    if len == 0 {
        return Ok(());
    }
    let mut buf = vec![0u8; len];
    file.read_exact_at(&mut buf, 0)
        .map_err(|_| LogError::Disabled(UNREADABLE))?;
    let mut pieces: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
    pieces.pop(); // after the last newline: empty, or a line not finished
    if pieces.is_empty() {
        // One line and no newline: a session being born (its first record is half written), or a file that is not
        // one. Only the first can be told from the second once it has grown past a block.
        return if size as usize <= BLOCK {
            Ok(())
        } else {
            Err(LogError::Disabled("unreadable head"))
        };
    }
    if serde_json::from_slice::<Value>(pieces[0]).is_err() {
        return Err(LogError::Disabled("unreadable head"));
    }
    for piece in pieces {
        let version = serde_json::from_slice::<Value>(piece)
            .ok()
            .and_then(|v| v.get("version").and_then(Value::as_str).map(str::to_string));
        if let Some(v) = version {
            return if v.split('.').next() == Some(SUPPORTED_MAJOR) {
                Ok(())
            } else {
                Err(LogError::Disabled("unknown version"))
            };
        }
    }
    Ok(())
}

/// A positional reader, so a read of the file does not move any shared offset.
struct AtRead<'a> {
    file: &'a File,
    pos: u64,
}

impl Read for AtRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read_at(buf, self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

/// Reads the next line into `buf` and returns how many bytes it took in the file, or `None` at the end. A line over
/// `MAX_LINE` is stepped over and `buf` is left empty, so one huge record cannot take the memory of the process.
fn next_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> io::Result<Option<u64>> {
    buf.clear();
    let mut total = 0u64;
    let mut too_long = false;
    loop {
        let avail = r.fill_buf()?;
        if avail.is_empty() {
            break;
        }
        let (take, done) = match memchr::memchr(b'\n', avail) {
            Some(i) => (i + 1, true),
            None => (avail.len(), false),
        };
        if !too_long {
            if buf.len() + take > MAX_LINE {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&avail[..take]);
            }
        }
        r.consume(take);
        total += take as u64;
        if done {
            break;
        }
    }
    Ok((total > 0).then_some(total))
}

/// The few fields the tree is built from. The rest of the record is skipped over, not kept.
#[derive(Deserialize)]
struct Head<'a> {
    #[serde(rename = "type", borrow)]
    ty: Option<Cow<'a, str>>,
    #[serde(borrow)]
    uuid: Option<Cow<'a, str>>,
    #[serde(rename = "parentUuid", borrow)]
    parent: Option<Cow<'a, str>>,
    #[serde(rename = "logicalParentUuid", borrow)]
    logical: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
}

struct Node {
    uuid: String,
    offset: u64,
    len: usize,
}

/// The active branch, oldest record first, with where each record is in the file.
pub struct Chain {
    fp: Fingerprint,
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
}

impl Chain {
    fn build(file: &File, fp: Fingerprint) -> io::Result<Chain> {
        struct Rec {
            uuid: String,
            parent: Option<String>,
            message: bool,
            offset: u64,
            len: usize,
        }
        // Only what the file held when the fingerprint was taken: a record being appended is not chased.
        let mut r = BufReader::with_capacity(BLOCK, AtRead { file, pos: 0 }.take(fp.size));
        let mut line = Vec::new();
        let mut at = 0u64;
        let mut recs: Vec<Rec> = Vec::new();
        while let Some(n) = next_line(&mut r, &mut line)? {
            // A line cut short fails to parse and is dropped; one that is whole without its newline is kept.
            if let Ok(h) = serde_json::from_slice::<Head>(&line) {
                if let (Some(uuid), false) = (h.uuid, h.sidechain == Some(true)) {
                    recs.push(Rec {
                        uuid: uuid.into_owned(),
                        parent: h.parent.or(h.logical).map(Cow::into_owned),
                        message: matches!(h.ty.as_deref(), Some("user" | "assistant")),
                        offset: at,
                        len: n as usize,
                    });
                }
            }
            at += n;
        }
        let by_uuid: HashMap<&str, usize> = recs
            .iter()
            .enumerate()
            .map(|(i, r)| (r.uuid.as_str(), i))
            .collect();
        let mut path: Vec<usize> = Vec::new();
        let mut cur = recs.iter().rposition(|r| r.message);
        while let Some(i) = cur {
            if path.len() > recs.len() {
                break; // a loop in the parent links
            }
            path.push(i);
            cur = recs[i]
                .parent
                .as_deref()
                .and_then(|p| by_uuid.get(p).copied());
        }
        path.reverse();
        let nodes: Vec<Node> = path
            .into_iter()
            .map(|i| Node {
                uuid: recs[i].uuid.clone(),
                offset: recs[i].offset,
                len: recs[i].len,
            })
            .collect();
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.uuid.clone(), i))
            .collect();
        Ok(Chain { fp, nodes, index })
    }

    /// The node an entry's turn id belongs to: the record id, or `<id>:<n>` for a later block of a record.
    fn node_of(&self, turn: &str) -> Option<usize> {
        self.index
            .get(turn)
            .or_else(|| turn.rsplit_once(':').and_then(|(u, _)| self.index.get(u)))
            .copied()
    }
}

/// The `pos` of a cursor at the boundary before node `at`: the place, and a hash of the record just before it, so a
/// cursor taken before a rewind moved the branch is told apart from one on the same branch.
fn mark(chain: &Chain, at: usize) -> u64 {
    let h = at.checked_sub(1).map_or(0, |i| {
        chain.nodes[i].uuid.bytes().fold(0xcbf2_9ce4u32, |h, b| {
            (h ^ b as u32).wrapping_mul(0x0100_0193)
        })
    });
    (u64::from(h) << 32) | at as u64
}

/// The place a cursor's `pos` names, if the record before it is still the same one.
fn unmark(chain: &Chain, pos: u64) -> Option<usize> {
    let at = (pos & 0xffff_ffff) as usize;
    (at <= chain.nodes.len() && mark(chain, at) == pos).then_some(at)
}

/// One piece of a record that is shown: a text block, a thinking block or a tool's name.
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

/// What a record shows. Tool input and output are not read here at all.
fn parts(v: &Value) -> Vec<Part> {
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    let content = v.pointer("/message/content");
    match ty {
        "user" => {
            if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
                return Vec::new();
            }
            match content {
                Some(Value::String(t)) => part(Kind::User, t).into_iter().collect(),
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                        Some("text") => part(Kind::User, b.get("text")?.as_str()?),
                        Some("tool_result" | "image" | "document") => None,
                        _ => Some(notice()),
                    })
                    .collect(),
                _ => vec![notice()],
            }
        }
        "assistant" => match content {
            Some(Value::String(t)) => part(Kind::Message, t).into_iter().collect(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                    Some("text") => part(Kind::Message, b.get("text")?.as_str()?),
                    Some("thinking") => part(Kind::Thought, b.get("thinking")?.as_str()?),
                    Some("redacted_thinking") => None,
                    Some("tool_use" | "server_tool_use") => Some(Part {
                        kind: Kind::Tool,
                        text: b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string(),
                    }),
                    _ => Some(notice()),
                })
                .collect(),
            _ => vec![notice()],
        },
        "system" => {
            if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                vec![Part {
                    kind: Kind::Notice,
                    text: "Conversation compacted".to_string(),
                }]
            } else {
                Vec::new()
            }
        }
        t if NOISE.contains(&t) => Vec::new(),
        _ => vec![notice()],
    }
}

fn turn_id(uuid: &str, n: usize) -> String {
    if n == 0 {
        uuid.to_string()
    } else {
        format!("{uuid}:{n}")
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
        return Ok(vec![Entry::notice(&node.uuid, UNMAPPABLE)]);
    };
    Ok(parts(&v)
        .into_iter()
        .enumerate()
        .map(|(n, p)| Entry::new(turn_id(&node.uuid, n), p.kind, &p.text))
        .collect())
}

impl SessionLog for ClaudeLog {
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
        let (path, file, fp) = self.open(s)?;
        let SearchRun { scanner, budget } = run;
        let limit = budget
            .limits()
            .per_session
            .min(budget.limits().max_hits.saturating_sub(budget.hits()));
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut chain: Option<Arc<Chain>> = None;
        let mut failed: Option<LogError> = None;
        scanner
            .scan_file(&file, fp.size, &needle, budget, |c| {
                // The scan matched bytes; what counts is the scrubbed text of a record on the active branch.
                let Ok(v) = serde_json::from_slice::<Value>(c.bytes) else {
                    return ControlFlow::Continue(()); // torn
                };
                let Some(uuid) = v.get("uuid").and_then(Value::as_str) else {
                    return ControlFlow::Continue(());
                };
                if chain.is_none() {
                    match self.chain(&file, &path, fp) {
                        Ok(c) => chain = Some(c),
                        Err(e) => {
                            failed = Some(e);
                            return ControlFlow::Break(());
                        }
                    }
                }
                if !chain.as_ref().is_some_and(|c| c.index.contains_key(uuid)) {
                    return ControlFlow::Continue(());
                }
                for (n, p) in parts(&v).into_iter().enumerate() {
                    if matches!(p.kind, Kind::Tool | Kind::Notice) {
                        continue;
                    }
                    let Some((snippet, highlight)) = snippet(&p.text, query) else {
                        continue;
                    };
                    hits.push(Hit {
                        turn: turn_id(uuid, n),
                        kind: p.kind,
                        snippet,
                        highlight,
                        at: v
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                    if hits.len() >= limit {
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            })
            .map_err(|_| LogError::Disabled(UNREADABLE))?;
        if let Some(e) = failed {
            return Err(e);
        }
        for _ in &hits {
            let _ = budget.record_hit();
        }
        Ok(hits)
    }

    fn window(
        &self,
        s: &SessionRef,
        anchor: &str,
        before: usize,
        after: usize,
    ) -> Result<Window, LogError> {
        let (path, file, fp) = self.open(s)?;
        let chain = self.chain(&file, &path, fp)?;
        let at = chain.node_of(anchor).ok_or(LogError::NoAnchor)?;
        let own = entries_of(&file, &chain.nodes[at])?;
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
            back.extend(entries_of(&file, &chain.nodes[i])?.into_iter().rev());
        }
        back.truncate(before);
        let mut fwd: Vec<Entry> = own[pos + 1..].to_vec();
        let mut j = at + 1;
        while fwd.len() < after && j < chain.nodes.len() {
            fwd.extend(entries_of(&file, &chain.nodes[j])?);
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

    /// Pages by record: a cursor's `pos` is a place in the active branch, and a record's entries come together, so a
    /// page can hold more than `count`.
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
        let chain = self.chain(&file, &path, fp)?;
        let n = chain.nodes.len();
        let count = count.min(MAX_SIDE);
        let pos = match cursor {
            Some(c) => unmark(&chain, c.pos).ok_or(LogError::StaleCursor)?,
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
                    got.splice(0..0, entries_of(&file, &chain.nodes[i])?);
                }
                (i > 0).then_some(i)
            }
            Direction::Newer => {
                let mut i = pos;
                while i < n && got.len() < count {
                    got.extend(entries_of(&file, &chain.nodes[i])?);
                    i += 1;
                }
                (i < n).then_some(i)
            }
        };
        Ok(Page {
            entries: got,
            next: next.map(|p| Cursor {
                fingerprint: fp,
                pos: mark(&chain, p),
            }),
        })
    }
}

#[cfg(test)]
mod tests;
