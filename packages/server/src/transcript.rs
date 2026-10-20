//! The Claude Code transcript as a timeline (decision 0103, tier 1; plan step U09).
//!
//! A hook names its transcript file (fact 0029). That name is untrusted: it is used only after it resolves, with
//! symlinks followed, to a `.jsonl` file under the invoking user's Claude config directory. The file is read
//! incrementally from a saved offset, never more than `PASS_BYTES` per pass, and only a few dozen short entries
//! are kept, so a 50 MB transcript costs the same memory as a small one (`resource-constraints`). Every text that
//! leaves this module went through `events::sanitize_text` (control characters out, secrets replaced, capped),
//! and nothing in here is ever logged (`audit-logging-no-secrets`).
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde_json::Value;
use termenbox_core::events::PlanItem;
use termenbox_core::protocol::{Timeline, TimelineEntry};

use crate::events::sanitize_text;
use crate::history::Tokens;

/// The most bytes one pass reads. Older bytes than this behind the end of the file are skipped.
pub const PASS_BYTES: usize = 256 * 1024;
/// A transcript line longer than this is skipped unparsed (a pasted file, an image).
pub const MAX_LINE: usize = 64 * 1024;
pub const MAX_ENTRIES: usize = 30;
pub const MAX_FILES: usize = 20;
pub const MAX_PLAN: usize = 20;
pub const TEXT_CHARS: usize = 200;
pub const SUMMARY_CHARS: usize = 120;
pub const MESSAGE_CHARS: usize = 400;
/// Text is cut to this many characters before it is scrubbed, so one huge field costs nothing.
const PRE_CUT: usize = 4_000;
/// Sessions with a reader at once.
const MAX_READERS: usize = 20;

/// The invoking user's Claude config directory: `CLAUDE_CONFIG_DIR` when set (as Claude Code reads it), else the one
/// their running `claude` processes use, else `~/.claude`.
pub fn claude_root() -> Option<PathBuf> {
    if let Some(d) = crate::claude_hooks::config_dir() {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".claude"))
}

/// The transcript file a hook named, if and only if it is a `.jsonl` file under `root`. The path must be
/// absolute and hold no `..`; it is canonicalised, so a symlink pointing out of `root` is refused too.
pub fn resolve(root: &Path, raw: &str) -> Result<PathBuf> {
    let refused = || anyhow!("transcript path refused");
    if raw.is_empty() || raw.len() > 1024 || raw.contains('\0') {
        return Err(refused());
    }
    let p = Path::new(raw);
    if !p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return Err(refused());
    }
    let root = root.canonicalize().map_err(|_| refused())?;
    let real = p.canonicalize().map_err(|_| refused())?;
    let ok = real.starts_with(&root)
        && real.extension().is_some_and(|e| e == "jsonl")
        && real.metadata().map(|m| m.is_file()).unwrap_or(false);
    if ok {
        Ok(real)
    } else {
        Err(refused())
    }
}

fn clean(s: &str, max: usize) -> String {
    let cut: String = s.chars().take(PRE_CUT).collect();
    sanitize_text(&cut, max)
}

/// Reads one transcript file in bounded passes and keeps the recent timeline.
pub struct Reader {
    path: PathBuf,
    offset: u64,
    /// In the middle of a line that was too long to keep: drop bytes up to the next newline.
    skipping: bool,
    entries: VecDeque<TimelineEntry>,
    plan: Vec<PlanItem>,
    files: Vec<String>,
    last_message: Option<String>,
    /// Bytes the last pass read from disk (for tests and budgets).
    pub last_pass: usize,
}

impl Reader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            skipping: false,
            entries: VecDeque::new(),
            plan: Vec::new(),
            files: Vec::new(),
            last_message: None,
            last_pass: 0,
        }
    }

    fn reset(&mut self) {
        let path = std::mem::take(&mut self.path);
        *self = Reader::new(path);
    }

    /// Reads what was appended since the last pass, at most `PASS_BYTES`. When more than that was appended, the
    /// older part is skipped: the timeline is the recent steps.
    pub fn pass(&mut self) -> Result<()> {
        self.last_pass = 0;
        let mut f = std::fs::File::open(&self.path)?;
        let len = f.metadata()?.len();
        if len < self.offset {
            self.reset(); // truncated or replaced
        }
        let mut start = self.offset;
        let mut realign = false;
        if len - start > PASS_BYTES as u64 {
            start = len - PASS_BYTES as u64;
            realign = true;
        }
        if start >= len {
            return Ok(());
        }
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::with_capacity(PASS_BYTES.min((len - start) as usize));
        f.take(PASS_BYTES as u64).read_to_end(&mut buf)?;
        self.last_pass = buf.len();
        if realign {
            self.skipping = true;
        }
        let mut pos = 0usize;
        if self.skipping {
            match buf.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    pos = i + 1;
                    self.skipping = false;
                }
                None => {
                    self.offset = start + buf.len() as u64;
                    return Ok(());
                }
            }
        }
        while let Some(i) = buf[pos..].iter().position(|b| *b == b'\n') {
            let line = &buf[pos..pos + i];
            pos += i + 1;
            if line.len() <= MAX_LINE {
                self.line(line);
            }
        }
        if buf.len() - pos > MAX_LINE {
            // A line that is already too long and still not finished.
            pos = buf.len();
            self.skipping = true;
        }
        self.offset = start + pos as u64;
        Ok(())
    }

    fn push(&mut self, e: TimelineEntry) {
        if e.text.is_empty() && e.name.is_none() {
            return;
        }
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    fn line(&mut self, raw: &[u8]) {
        let Ok(v) = serde_json::from_slice::<Value>(raw) else {
            return;
        };
        if v["isSidechain"].as_bool() == Some(true) || v["isMeta"].as_bool() == Some(true) {
            return;
        }
        match v["type"].as_str() {
            Some("user") => self.user(&v["message"]["content"]),
            Some("assistant") => self.assistant(&v["message"]["content"]),
            _ => {}
        }
    }

    fn user(&mut self, content: &Value) {
        let mut texts: Vec<&str> = Vec::new();
        match content {
            Value::String(s) => texts.push(s),
            Value::Array(items) => {
                for i in items {
                    if i["type"] == "text" {
                        if let Some(t) = i["text"].as_str() {
                            texts.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
        for t in texts {
            let t = t.trim_start();
            // Wrappers Claude Code adds around slash commands and hook output are not something the user typed.
            if t.starts_with("<command-")
                || t.starts_with("<local-command")
                || t.starts_with("<system-reminder")
                || t.starts_with("Caveat:")
            {
                continue;
            }
            let text = clean(t, TEXT_CHARS);
            self.push(TimelineEntry {
                kind: "prompt".into(),
                name: None,
                text,
            });
        }
    }

    fn assistant(&mut self, content: &Value) {
        let Value::Array(items) = content else {
            return;
        };
        for i in items {
            match i["type"].as_str() {
                Some("text") => {
                    let m = clean(i["text"].as_str().unwrap_or(""), MESSAGE_CHARS);
                    if !m.is_empty() {
                        self.last_message = Some(m);
                    }
                }
                Some("tool_use") => self.tool(i["name"].as_str().unwrap_or(""), &i["input"]),
                _ => {}
            }
        }
    }

    fn tool(&mut self, name: &str, input: &Value) {
        if name.is_empty() {
            return;
        }
        let name_c = clean(name, 40);
        let summary = match name {
            "TodoWrite" => {
                let items: Vec<PlanItem> = input["todos"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .take(MAX_PLAN)
                            .filter_map(|t| {
                                let status = t["status"].as_str().unwrap_or("pending");
                                let text = if status == "in_progress" {
                                    t["activeForm"].as_str().or(t["content"].as_str())
                                } else {
                                    t["content"].as_str()
                                }?;
                                Some(PlanItem {
                                    text: clean(text, TEXT_CHARS),
                                    status: clean(status, 20),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let n = items.len();
                self.plan = items;
                format!("{n} tasks")
            }
            _ => {
                let mut s = crate::adapters::tool_summary(name, input);
                if s.is_empty() {
                    s = input["description"]
                        .as_str()
                        .and_then(|d| d.lines().next())
                        .unwrap_or("")
                        .to_string();
                }
                if matches!(name, "Edit" | "Write" | "MultiEdit" | "NotebookEdit") {
                    let path = input["file_path"]
                        .as_str()
                        .or(input["notebook_path"].as_str())
                        .unwrap_or("");
                    let file = clean(path.rsplit('/').next().unwrap_or(""), 80);
                    if !file.is_empty() {
                        self.files.retain(|f| *f != file);
                        self.files.push(file);
                        if self.files.len() > MAX_FILES {
                            self.files.remove(0);
                        }
                    }
                }
                clean(&s, SUMMARY_CHARS)
            }
        };
        self.push(TimelineEntry {
            kind: "tool".into(),
            name: Some(name_c),
            text: summary,
        });
    }

    /// True once the transcript has given anything to show.
    pub fn has_content(&self) -> bool {
        !self.entries.is_empty() || self.last_message.is_some() || !self.plan.is_empty()
    }

    pub fn timeline(&self) -> Timeline {
        Timeline {
            entries: self.entries.iter().cloned().collect(),
            plan: self.plan.clone(),
            files: self.files.clone(),
            last_message: self.last_message.clone(),
        }
    }
}

/// One reader per session, for the sessions whose hooks named a transcript.
#[derive(Default)]
pub struct Transcripts {
    readers: Mutex<HashMap<String, Reader>>,
}

impl Transcripts {
    /// The timeline for `session`, or None when the path is not allowed, cannot be read, or holds nothing yet.
    /// Blocking file work, at most `PASS_BYTES` per call.
    pub fn timeline(&self, root: &Path, session: &str, raw_path: &str) -> Option<Timeline> {
        let path = resolve(root, raw_path).ok()?;
        let mut readers = self.readers.lock().ok()?;
        if !readers.contains_key(session) && readers.len() >= MAX_READERS {
            let evict = readers.keys().next().cloned();
            if let Some(k) = evict {
                readers.remove(&k);
            }
        }
        let r = readers
            .entry(session.to_string())
            .or_insert_with(|| Reader::new(path.clone()));
        if r.path != path {
            *r = Reader::new(path);
        }
        r.pass().ok()?;
        r.has_content().then(|| r.timeline())
    }
}

// ---------------------------------------------------------------------------------------------------
// Token counts (U13)
// ---------------------------------------------------------------------------------------------------

/// The most bytes one collection reads (several passes of a turn's worth of transcript).
const USAGE_PASS_BYTES: u64 = 8 * 1024 * 1024;
/// Message ids remembered per session, to count a message once however many lines Claude Code wrote for it.
const SEEN_IDS: usize = 256;

/// The token counts Claude Code writes into its own transcript, one `message.usage` per assistant message (verified on a
/// real transcript, Claude Code 2.1.x). Only the four counts are read, as numbers; no text of any line is kept. A
/// message is written once per content block with the same id and the same counts, so each id counts once.
pub struct UsageReader {
    path: PathBuf,
    offset: u64,
    seen: VecDeque<String>,
}

impl UsageReader {
    /// Starts at the end of the file: what the transcript already holds happened before this server watched.
    fn armed(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self {
            path,
            offset,
            seen: VecDeque::new(),
        }
    }

    /// The counts of messages appended since the last collection. Whole lines only: a line still being written waits.
    fn collect(&mut self) -> Result<Tokens> {
        let mut f = std::fs::File::open(&self.path)?;
        let len = f.metadata()?.len();
        if len < self.offset {
            self.offset = 0; // truncated or replaced
            self.seen.clear();
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let mut reader = std::io::BufReader::new(f.take((len - self.offset).min(USAGE_PASS_BYTES)));
        let mut total = Tokens::default();
        let mut line = Vec::new();
        loop {
            line.clear();
            let n = (&mut reader)
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                if line.len() > MAX_LINE {
                    // Too long to be a message of ours: drop it up to its end.
                    let mut rest = Vec::new();
                    loop {
                        rest.clear();
                        let m = (&mut reader)
                            .take(MAX_LINE as u64)
                            .read_until(b'\n', &mut rest)?;
                        self.offset += m as u64;
                        if m == 0 || rest.last() == Some(&b'\n') {
                            break;
                        }
                    }
                    self.offset += n as u64;
                    continue;
                }
                break; // an unfinished last line
            }
            self.offset += n as u64;
            self.line(&line, &mut total);
        }
        Ok(total)
    }

    fn line(&mut self, raw: &[u8], total: &mut Tokens) {
        // Most lines carry no usage; do not parse those.
        if !raw.windows(7).any(|w| w == b"\"usage\"") {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(raw) else {
            return;
        };
        if v["type"].as_str() != Some("assistant") {
            return;
        }
        let m = &v["message"];
        let (Some(id), Some(u)) = (m["id"].as_str(), m["usage"].as_object()) else {
            return;
        };
        if self.seen.iter().any(|s| s == id) {
            return;
        }
        self.seen.push_back(id.chars().take(80).collect());
        while self.seen.len() > SEEN_IDS {
            self.seen.pop_front();
        }
        let n = |k: &str| u.get(k).and_then(Value::as_u64);
        crate::history::add_tokens(
            total,
            Tokens {
                input: n("input_tokens"),
                output: n("output_tokens"),
                cache_read: n("cache_read_input_tokens"),
                cache_write: n("cache_creation_input_tokens"),
            },
        );
    }
}

/// One usage reader per session, for the sessions whose hooks named a transcript.
#[derive(Default)]
pub struct UsageTracker {
    readers: Mutex<HashMap<String, UsageReader>>,
}

impl UsageTracker {
    /// Notes that a session's transcript is watched from here on (a no-op when it already is). Blocking: one `stat`.
    pub fn arm(&self, root: &Path, session: &str, raw_path: &str) {
        let Ok(path) = resolve(root, raw_path) else {
            return;
        };
        let Ok(mut readers) = self.readers.lock() else {
            return;
        };
        if readers.get(session).is_some_and(|r| r.path == path) {
            return;
        }
        if !readers.contains_key(session) && readers.len() >= MAX_READERS {
            let evict = readers.keys().next().cloned();
            if let Some(k) = evict {
                readers.remove(&k);
            }
        }
        readers.insert(session.to_string(), UsageReader::armed(path));
    }

    /// The counts appended since the last collection, or None when nothing is watched, nothing was appended, or the
    /// file cannot be read. Blocking file work, bounded by `USAGE_PASS_BYTES`.
    pub fn collect(&self, session: &str) -> Option<Tokens> {
        let mut readers = self.readers.lock().ok()?;
        let t = readers.get_mut(session)?.collect().ok()?;
        (!t.is_empty()).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = include_str!("../testdata/transcripts/claude-session.jsonl");

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tbx-transcript-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_path_outside_the_claude_directory_is_refused() {
        let root = dir("outside");
        let home = root.join(".claude");
        let inside = home.join("projects/p/s.jsonl");
        write(&inside, FIXTURE);
        let elsewhere = root.join("elsewhere.jsonl");
        write(&elsewhere, FIXTURE);
        assert!(resolve(&home, inside.to_str().unwrap()).is_ok());
        assert!(resolve(&home, elsewhere.to_str().unwrap()).is_err());
        // `..` is refused even when it lands back inside
        let dotted = format!("{}/projects/../projects/p/s.jsonl", home.display());
        assert!(resolve(&home, &dotted).is_err());
        let up = format!("{}/../elsewhere.jsonl", home.display());
        assert!(resolve(&home, &up).is_err());
        // a sibling that shares the prefix of the directory name is outside it
        let sibling = root.join(".claude-evil/s.jsonl");
        write(&sibling, FIXTURE);
        assert!(resolve(&home, sibling.to_str().unwrap()).is_err());
        // relative, empty, not a transcript, missing
        assert!(resolve(&home, "projects/p/s.jsonl").is_err());
        assert!(resolve(&home, "").is_err());
        let other = home.join("settings.json");
        write(&other, "{}");
        assert!(resolve(&home, other.to_str().unwrap()).is_err());
        assert!(resolve(&home, home.join("projects/none.jsonl").to_str().unwrap()).is_err());
        // /etc/passwd style targets
        assert!(resolve(&home, "/etc/passwd").is_err());
    }

    #[test]
    fn a_symlink_out_of_the_claude_directory_is_refused() {
        let root = dir("symlink");
        let home = root.join(".claude");
        std::fs::create_dir_all(home.join("projects")).unwrap();
        let secret = root.join("secret.jsonl");
        write(&secret, FIXTURE);
        let link = home.join("projects/link.jsonl");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert!(resolve(&home, link.to_str().unwrap()).is_err());
        // a directory link that leaves the tree
        let dlink = home.join("projects/out");
        std::os::unix::fs::symlink(&root, &dlink).unwrap();
        assert!(resolve(&home, dlink.join("secret.jsonl").to_str().unwrap()).is_err());
    }

    #[test]
    fn the_recorded_transcript_gives_a_timeline() {
        let root = dir("fixture");
        let home = root.join(".claude");
        let p = home.join("projects/p/s.jsonl");
        write(&p, FIXTURE);
        let t = Transcripts::default();
        let tl = t.timeline(&home, "s", p.to_str().unwrap()).unwrap();
        let kinds: Vec<_> = tl.entries.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds.first(), Some(&"prompt"));
        assert!(kinds.contains(&"tool"));
        assert!(tl
            .entries
            .iter()
            .any(|e| e.name.as_deref() == Some("Bash") && e.text.starts_with("cargo test")));
        assert_eq!(tl.files, vec!["lib.rs", "main.rs"]);
        assert_eq!(tl.plan.len(), 3);
        assert!(tl.plan.iter().any(|i| i.status == "in_progress"));
        assert!(tl.last_message.as_deref().unwrap().contains("tests pass"));
        // command wrappers and sidechains are not prompts
        assert!(!tl.entries.iter().any(|e| e.text.contains("command-name")));
        assert!(!tl.entries.iter().any(|e| e.text.contains("subagent")));
    }

    /// The client renders this same file (`packages/client/src/lib/sessionView.test.ts`), so the two cannot drift.
    /// `TBX_BLESS=1 cargo test transcript` rewrites it after an intended change.
    #[test]
    fn the_wire_timeline_of_the_fixture_is_the_one_the_client_renders() {
        let root = dir("wire");
        let home = root.join(".claude");
        let p = home.join("projects/p/s.jsonl");
        write(&p, FIXTURE);
        let tl = Transcripts::default()
            .timeline(&home, "s", p.to_str().unwrap())
            .unwrap();
        let msg = termenbox_core::protocol::ServerMessage::SessionView {
            session: "s".into(),
            tier: "t1".into(),
            agent: Some("claude".into()),
            tail: vec!["$".into()],
            hash: "h1".into(),
            unchanged: false,
            task: None,
            prompt: None,
            timeline: Some(tl),
        };
        let got = serde_json::to_string_pretty(&msg).unwrap() + "\n";
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/transcripts/claude-session.view.json"
        );
        if std::env::var_os("TBX_BLESS").is_some() {
            std::fs::write(file, &got).unwrap();
        }
        assert_eq!(got, std::fs::read_to_string(file).unwrap());
    }

    #[test]
    fn a_token_in_a_tool_line_is_scrubbed() {
        let root = dir("scrub");
        let home = root.join(".claude");
        let p = home.join("p/s.jsonl");
        let token = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let line = |content: &str| {
            format!("{{\"type\":\"assistant\",\"message\":{{\"content\":{content}}}}}\n")
        };
        let text = [
            line(&format!(
                "[{{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"curl -H 'Authorization: Bearer abcdef' https://x.test/?k={token} --token hunter2\"}}}}]"
            )),
            format!(
                "{{\"type\":\"user\",\"message\":{{\"content\":\"use key {token} please\"}}}}\n"
            ),
            line(&format!("[{{\"type\":\"text\",\"text\":\"the key is {token}\"}}]")),
        ]
        .concat();
        write(&p, &text);
        let tl = Transcripts::default()
            .timeline(&home, "s", p.to_str().unwrap())
            .unwrap();
        let wire = serde_json::to_string(&tl).unwrap();
        assert!(!wire.contains(&token), "{wire}");
        assert!(
            !wire.contains("hunter2") && !wire.contains("abcdef"),
            "{wire}"
        );
        assert!(
            wire.contains("[token]") || wire.contains("[secret]"),
            "{wire}"
        );
    }

    #[test]
    fn a_fifty_megabyte_transcript_is_read_with_bounded_memory() {
        let root = dir("big");
        let home = root.join(".claude");
        let p = home.join("p/s.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        {
            let mut f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
            let mut i = 0u64;
            let mut size = 0u64;
            while size < 50 * 1024 * 1024 {
                // some ordinary lines, and every so often one far longer than MAX_LINE
                let l = if i.is_multiple_of(500) {
                    format!(
                        "{{\"type\":\"user\",\"message\":{{\"content\":\"{}\"}}}}\n",
                        "x".repeat(MAX_LINE * 2)
                    )
                } else {
                    format!(
                        "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"name\":\"Edit\",\"input\":{{\"file_path\":\"/w/f{i}.rs\"}}}},{{\"type\":\"text\",\"text\":\"step {i} {}\"}}]}}}}\n",
                        "y".repeat(2_000)
                    )
                };
                size += l.len() as u64;
                f.write_all(l.as_bytes()).unwrap();
                i += 1;
            }
        }
        assert!(std::fs::metadata(&p).unwrap().len() >= 50 * 1024 * 1024);
        let mut r = Reader::new(p.clone());
        r.pass().unwrap();
        // one pass read at most the cap and kept at most the entry limits
        assert!(r.last_pass <= PASS_BYTES, "{}", r.last_pass);
        assert!(r.offset > 49 * 1024 * 1024);
        let tl = r.timeline();
        assert!(tl.entries.len() <= MAX_ENTRIES && tl.files.len() <= MAX_FILES);
        assert!(!tl.entries.is_empty());
        assert!(tl.last_message.as_deref().unwrap().chars().count() <= MESSAGE_CHARS);
        // more lines appended: the next pass is incremental and small
        let before = r.offset;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{\"type\":\"user\",\"message\":{\"content\":\"one more\"}}\n")
            .unwrap();
        r.pass().unwrap();
        assert!(r.last_pass < 200 && r.offset > before);
        assert_eq!(r.timeline().entries.last().unwrap().text, "one more");
        // bytes appended in a burst larger than the cap: still one capped pass
        let burst = "{\"type\":\"user\",\"message\":{\"content\":\"b\"}}\n".repeat(20_000);
        f.write_all(burst.as_bytes()).unwrap();
        r.pass().unwrap();
        assert!(r.last_pass <= PASS_BYTES);
    }

    #[test]
    fn a_half_written_line_waits_for_its_end() {
        let root = dir("partial");
        let home = root.join(".claude");
        let p = home.join("p/s.jsonl");
        write(
            &p,
            "{\"type\":\"user\",\"message\":{\"content\":\"first\"}}\n{\"type\":\"user\",\"mess",
        );
        let mut r = Reader::new(resolve(&home, p.to_str().unwrap()).unwrap());
        r.pass().unwrap();
        assert_eq!(r.timeline().entries.len(), 1);
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"age\":{\"content\":\"second\"}}\n").unwrap();
        r.pass().unwrap();
        let texts: Vec<_> = r.timeline().entries.into_iter().map(|e| e.text).collect();
        assert_eq!(texts, ["first", "second"]);
    }

    #[test]
    fn nothing_in_a_missing_or_empty_transcript_is_a_timeline() {
        let root = dir("empty");
        let home = root.join(".claude");
        let p = home.join("p/s.jsonl");
        write(&p, "");
        let t = Transcripts::default();
        assert!(t.timeline(&home, "s", p.to_str().unwrap()).is_none());
        assert!(t.timeline(&home, "s", "/nope/x.jsonl").is_none());
    }

    const USAGE: &str = include_str!("../testdata/transcripts/claude-usage.jsonl");

    fn watched(name: &str) -> (PathBuf, PathBuf, UsageTracker) {
        let root = dir(name);
        let file = root.join("projects/p/s.jsonl");
        write(&file, "");
        let t = UsageTracker::default();
        t.arm(&root, "claude:a", file.to_str().unwrap());
        (root, file, t)
    }

    #[test]
    fn a_message_written_on_several_lines_is_counted_once() {
        let (_root, file, t) = watched("usage-once");
        std::fs::write(&file, USAGE).unwrap();
        let got = t.collect("claude:a").unwrap();
        assert_eq!(got.input, Some(5));
        assert_eq!(got.output, Some(1311 + 264));
        assert_eq!(got.cache_read, Some(26221 + 41693));
        assert_eq!(got.cache_write, Some(15472 + 120));
        // Nothing new: nothing to report, and the same lines are not counted again.
        assert_eq!(t.collect("claude:a"), None);
    }

    #[test]
    fn only_what_was_appended_after_the_session_was_first_seen_counts() {
        let root = dir("usage-since");
        let file = root.join("projects/p/s.jsonl");
        let (old, new) = USAGE.split_at(USAGE.find("{\"parentUuid\":\"u2\"").unwrap());
        write(&file, old);
        let t = UsageTracker::default();
        t.arm(&root, "claude:a", file.to_str().unwrap());
        assert_eq!(t.collect("claude:a"), None);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        f.write_all(new.as_bytes()).unwrap();
        let got = t.collect("claude:a").unwrap();
        assert_eq!((got.input, got.output), (Some(3), Some(264)));
    }

    #[test]
    fn a_line_still_being_written_waits_for_its_end() {
        let (_root, file, t) = watched("usage-partial");
        let last = USAGE.lines().last().unwrap();
        let (a, b) = last.split_at(last.len() / 2);
        std::fs::write(&file, a).unwrap();
        assert_eq!(t.collect("claude:a"), None);
        std::fs::write(&file, format!("{a}{b}\n")).unwrap();
        assert_eq!(t.collect("claude:a").unwrap().output, Some(264));
    }

    #[test]
    fn an_overlong_line_and_garbage_are_skipped_not_fatal() {
        let (_root, file, t) = watched("usage-garbage");
        let huge = format!("{{\"usage\":\"{}\"}}\n", "x".repeat(MAX_LINE * 2));
        std::fs::write(&file, format!("not json \"usage\"\n{huge}{USAGE}")).unwrap();
        assert_eq!(t.collect("claude:a").unwrap().input, Some(5));
    }

    #[test]
    fn a_message_without_usage_reports_nothing_rather_than_zero() {
        let (_root, file, t) = watched("usage-none");
        std::fs::write(&file, FIXTURE).unwrap();
        assert_eq!(t.collect("claude:a"), None);
    }

    #[test]
    fn a_path_outside_the_claude_directory_is_not_watched() {
        let root = dir("usage-outside");
        let other = root.join("elsewhere.jsonl");
        write(&other, "");
        let t = UsageTracker::default();
        t.arm(&root.join(".claude"), "claude:a", other.to_str().unwrap());
        std::fs::write(&other, USAGE).unwrap();
        assert_eq!(t.collect("claude:a"), None);
    }
}
