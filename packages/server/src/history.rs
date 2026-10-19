//! History (U12, decision 0105): what agents did, kept on this server so the Activity view can show a day or a week.
//!
//! Metadata only. A record holds the session name, the agent, a short scrubbed task title, when the task started and
//! ended, how it ended and how long the agent waited on the user. No prompt and no terminal text ever reaches it.
//!
//! The store is one append-only file of JSON lines in the state directory. A task is written once, when it ends; a task
//! still running lives in memory only, so a crash loses the tasks in flight and nothing else. The file is bounded by age
//! and size: when it grows past its limit, or on the hourly check, it is rewritten without the oldest records (to a
//! temporary file, then renamed), so a crash leaves either the old file or the new one. A torn last line is dropped when
//! the store opens. Nothing is held in memory but the tasks in flight: a query reads the file line by line.
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use termenbox_core::events::{AgentState, Confidence};
use termenbox_core::protocol::{HistoryRun, RunOutcome};
use tracing::{info, warn};

use crate::events::sanitize_text;

pub const DEFAULT_DAYS: u32 = 90;
pub const DEFAULT_MAX_MB: u64 = 20;
/// The longest task title kept. The title is the first line of the user's prompt, so it stays short.
pub const TITLE_CHARS: usize = 80;
/// Tasks one answer carries. About 200 bytes each on the wire, so an answer stays well under 100 KB.
pub const MAX_RUNS: usize = 300;
/// Tasks in flight at once, one per session (the same bound as the event hub).
const MAX_OPEN: usize = crate::events::MAX_SESSIONS;
/// A rewrite leaves room for this share of the limit, so it does not run again on the next record.
const TRIM_TO_PERCENT: u64 = 80;
const DAY_MS: u64 = 86_400_000;
/// A line longer than any record we write is damage, not data.
const MAX_LINE: usize = 4096;
const FIELD_CHARS: usize = 100;
const FILE: &str = "history.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub keep_days: u32,
    pub max_bytes: u64,
}

impl Limits {
    pub fn new(keep_days: u32, max_mb: u64) -> Self {
        Self {
            keep_days,
            max_bytes: max_mb * 1_000_000,
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------------------------------

struct Store {
    path: PathBuf,
    limits: Limits,
    bytes: u64,
    /// The last append failed part-way: the next one starts on a fresh line so it cannot join the fragment.
    torn: bool,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Rewrite {
    before: u64,
    after: u64,
    aged: u64,
    oversize: u64,
    invalid: u64,
}

/// A record the store accepts: a finished task.
fn parse(line: &[u8]) -> Option<HistoryRun> {
    let run: HistoryRun = serde_json::from_slice(line).ok()?;
    (run.ended_at.is_some() && run.outcome.is_some()).then_some(run)
}

/// The lines of a file, each with its length on disk, without ever holding more than one in memory. A line past
/// `MAX_LINE` is skipped and reported as an empty one.
fn for_each_line(file: File, mut f: impl FnMut(&[u8], u64)) -> Result<()> {
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // Read at most MAX_LINE + 1 bytes of a line: a longer one is garbage and must not grow the buffer.
        let n = (&mut reader)
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let mut len = n as u64;
        if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') {
            // Drop the rest of the over-long line.
            let mut rest = Vec::new();
            loop {
                rest.clear();
                let m = (&mut reader)
                    .take(MAX_LINE as u64)
                    .read_until(b'\n', &mut rest)?;
                len += m as u64;
                if m == 0 || rest.last() == Some(&b'\n') {
                    break;
                }
            }
            buf.clear();
        }
        f(buf.strip_suffix(b"\n").unwrap_or(&buf), len);
    }
}

impl Store {
    fn open(dir: &Path, limits: Limits, now: u64) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| "creating the state directory")?;
        let mut store = Self {
            path: dir.join(FILE),
            limits,
            bytes: 0,
            torn: false,
        };
        let r = store.rewrite(now)?;
        info!(
            records_bytes = store.bytes,
            dropped_aged = r.aged,
            dropped_oversize = r.oversize,
            dropped_invalid = r.invalid,
            keep_days = limits.keep_days,
            max_bytes = limits.max_bytes,
            "history store opened"
        );
        Ok(store)
    }

    fn cutoff(&self, now: u64) -> u64 {
        now.saturating_sub(u64::from(self.limits.keep_days) * DAY_MS)
    }

    /// Rewrites the file without records that are too old, damaged, or past the size limit (oldest first), when there
    /// is any such record. Otherwise leaves the file alone.
    fn rewrite(&mut self, now: u64) -> Result<Rewrite> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.bytes = 0;
                return Ok(Rewrite::default());
            }
            Err(e) => return Err(e.into()),
        };
        let cutoff = self.cutoff(now);
        let mut r = Rewrite::default();
        let mut kept = 0u64;
        let mut unterminated = false;
        let keep = |line: &[u8]| parse(line).is_some_and(|run| run.ended_at.unwrap_or(0) >= cutoff);
        for_each_line(file, |line, len| {
            r.before += len;
            if parse(line).is_none() {
                r.invalid += 1;
            } else if !keep(line) {
                r.aged += 1;
            } else {
                // A record is rewritten with its newline, so a missing one is not a reason to rewrite on its own.
                kept += line.len() as u64 + 1;
            }
            unterminated = len == line.len() as u64;
        })?;
        let target = if kept > self.limits.max_bytes {
            self.limits.max_bytes * TRIM_TO_PERCENT / 100
        } else {
            kept
        };
        let mut to_drop = kept - target;
        if r.aged == 0 && r.invalid == 0 && to_drop == 0 && !unterminated {
            self.bytes = r.before;
            r.after = r.before;
            return Ok(r);
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        let out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        let mut out = BufWriter::new(out);
        let mut written = 0u64;
        let mut failed = None;
        for_each_line(File::open(&self.path)?, |line, _| {
            if failed.is_some() || !keep(line) {
                return;
            }
            let size = line.len() as u64 + 1;
            if to_drop > 0 {
                to_drop = to_drop.saturating_sub(size);
                r.oversize += 1;
                return;
            }
            if let Err(e) = out.write_all(line).and_then(|_| out.write_all(b"\n")) {
                failed = Some(e);
            } else {
                written += size;
            }
        })?;
        if let Some(e) = failed {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        let out = out.into_inner().map_err(|e| e.into_error())?;
        out.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        self.bytes = written;
        r.after = written;
        self.torn = false;
        Ok(r)
    }

    fn compact(&mut self, now: u64, why: &'static str) {
        match self.rewrite(now) {
            Ok(r) if r.after != r.before => info!(
                why,
                before_bytes = r.before,
                after_bytes = r.after,
                dropped_aged = r.aged,
                dropped_oversize = r.oversize,
                dropped_invalid = r.invalid,
                "history store compacted"
            ),
            Ok(_) => {}
            Err(e) => warn!(why, "history store compaction failed: {e}"),
        }
    }

    fn append(&mut self, run: &HistoryRun, now: u64) {
        let Ok(mut line) = serde_json::to_vec(run) else {
            return;
        };
        line.push(b'\n');
        if self.torn {
            line.insert(0, b'\n');
        }
        let written = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| f.write_all(&line));
        match written {
            Ok(()) => {
                self.torn = false;
                self.bytes += line.len() as u64;
                if self.bytes > self.limits.max_bytes {
                    self.compact(now, "size");
                }
            }
            Err(e) => {
                self.torn = true;
                warn!("history record not written: {e}");
            }
        }
    }

    /// Tasks that ended in `[from, to)`, oldest first, at most `MAX_RUNS` (the newest when there are more).
    fn query(&self, from: u64, to: u64) -> (Vec<HistoryRun>, bool) {
        let mut runs = VecDeque::new();
        let mut truncated = false;
        if let Ok(file) = File::open(&self.path) {
            let _ = for_each_line(file, |line, _| {
                let Some(run) = parse(line) else { return };
                if !run.ended_at.is_some_and(|e| e >= from && e < to) {
                    return;
                }
                if runs.len() == MAX_RUNS {
                    runs.pop_front();
                    truncated = true;
                }
                runs.push_back(run);
            });
        }
        let mut runs: Vec<_> = runs.into();
        runs.sort_by_key(|r| (r.ended_at, r.started_at));
        (runs, truncated)
    }

    fn delete(&mut self) -> u64 {
        let mut removed = 0;
        if let Ok(file) = File::open(&self.path) {
            let _ = for_each_line(file, |line, _| removed += u64::from(parse(line).is_some()));
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!("history file not removed: {e}"),
        }
        self.bytes = 0;
        self.torn = false;
        removed
    }
}

// ---------------------------------------------------------------------------------------------------
// Tasks in flight
// ---------------------------------------------------------------------------------------------------

struct Open {
    started_at: u64,
    task: Option<String>,
    agent: Option<String>,
    blocked_ms: u64,
    blocked_since: Option<u64>,
}

pub struct Answer {
    pub runs: Vec<HistoryRun>,
    pub truncated: bool,
}

pub struct History {
    store: Mutex<Store>,
    open: Mutex<HashMap<String, Open>>,
    limits: Limits,
}

fn clip(s: &str) -> String {
    sanitize_text(s, FIELD_CHARS)
}

impl History {
    pub fn open(dir: &Path, limits: Limits, now: u64) -> Result<Self> {
        Ok(Self {
            store: Mutex::new(Store::open(dir, limits, now)?),
            open: Mutex::default(),
            limits,
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// A session's state changed (a fact from a hook, Codex or ACP): a task starts when the agent starts working or
    /// waits, and ends when it is done, failed or idle. The time spent waiting on the user is summed as it happens.
    pub fn state(
        &self,
        session: &str,
        agent: Option<&str>,
        state: AgentState,
        confidence: Confidence,
        ts: u64,
    ) {
        let ended = {
            let mut open = self.open.lock().unwrap();
            match state {
                AgentState::Working | AgentState::NeedsInput => {
                    if !open.contains_key(session) && open.len() >= MAX_OPEN {
                        return;
                    }
                    let run = open.entry(session.to_string()).or_insert_with(|| Open {
                        started_at: ts,
                        task: None,
                        agent: None,
                        blocked_ms: 0,
                        blocked_since: None,
                    });
                    if let Some(a) = agent {
                        run.agent = Some(clip(a));
                    }
                    if let Some(since) = run.blocked_since.take() {
                        run.blocked_ms += ts.saturating_sub(since);
                    }
                    if state == AgentState::NeedsInput {
                        run.blocked_since = Some(ts);
                    }
                    None
                }
                AgentState::Done | AgentState::Idle | AgentState::Error => {
                    let outcome = match (state, confidence) {
                        (AgentState::Done, _) | (AgentState::Idle, Confidence::High) => {
                            RunOutcome::Finished
                        }
                        (AgentState::Error, _) => RunOutcome::Failed,
                        // Idle on a hunch (the screen reconciled): the task ended, nobody said how.
                        _ => RunOutcome::Stopped,
                    };
                    open.remove(session).map(|run| (run, outcome))
                }
                AgentState::Unknown => None,
            }
        };
        if let Some((run, outcome)) = ended {
            self.finish(session, run, outcome, agent, ts);
        }
    }

    /// The agent named what it is working on; only a task in flight takes a title.
    pub fn task(&self, session: &str, title: &str) {
        let title = sanitize_text(title, TITLE_CHARS);
        if title.is_empty() {
            return;
        }
        if let Some(run) = self.open.lock().unwrap().get_mut(session) {
            run.task = Some(title);
        }
    }

    /// Sessions under `prefix` that are no longer live end their task, as stopped.
    pub fn retain_live(&self, prefix: &str, live: &[String], now: u64) {
        let gone: Vec<(String, Open)> = {
            let mut open = self.open.lock().unwrap();
            let names: Vec<String> = open
                .keys()
                .filter(|k| k.starts_with(prefix) && !live.contains(k))
                .cloned()
                .collect();
            names
                .into_iter()
                .filter_map(|k| open.remove(&k).map(|r| (k, r)))
                .collect()
        };
        for (session, run) in gone {
            self.finish(&session, run, RunOutcome::Stopped, None, now);
        }
    }

    fn finish(&self, session: &str, run: Open, outcome: RunOutcome, agent: Option<&str>, ts: u64) {
        let ended_at = ts.max(run.started_at);
        let blocked = run.blocked_ms
            + run
                .blocked_since
                .map_or(0, |since| ended_at.saturating_sub(since));
        let record = HistoryRun {
            session: clip(session),
            agent: agent.map(clip).or(run.agent),
            task: run.task,
            started_at: run.started_at,
            ended_at: Some(ended_at),
            outcome: Some(outcome),
            blocked_ms: blocked.min(ended_at - run.started_at),
        };
        self.store.lock().unwrap().append(&record, ts);
    }

    /// The tasks that ended in `[from, to)`, and those still running that started before `to`.
    pub fn query(&self, from: u64, to: u64, now: u64) -> Answer {
        let (mut runs, truncated) = self.store.lock().unwrap().query(from, to);
        let open = self.open.lock().unwrap();
        for (session, run) in open.iter().filter(|(_, r)| r.started_at < to) {
            let blocked = run.blocked_ms
                + run
                    .blocked_since
                    .map_or(0, |since| now.saturating_sub(since));
            runs.push(HistoryRun {
                session: clip(session),
                agent: run.agent.clone(),
                task: run.task.clone(),
                started_at: run.started_at,
                ended_at: None,
                outcome: None,
                blocked_ms: blocked,
            });
        }
        Answer { runs, truncated }
    }

    /// Forgets everything: the file and the tasks in flight. Returns how many finished tasks it held.
    pub fn delete(&self) -> u64 {
        self.open.lock().unwrap().clear();
        let removed = self.store.lock().unwrap().delete();
        info!(removed, "history deleted");
        removed
    }

    /// The hourly check: drops what has aged out.
    pub fn maintain(&self, now: u64) {
        self.store.lock().unwrap().compact(now, "age");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000_000;
    const MIN: u64 = 60_000;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("termenbox-history-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn limits() -> Limits {
        Limits::new(90, 20)
    }

    fn file_len(d: &Path) -> u64 {
        std::fs::metadata(d.join(FILE))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// One whole task: working, then done, `mins` later.
    fn task(h: &History, session: &str, start: u64, mins: u64, end: AgentState) {
        h.state(
            session,
            Some("claude"),
            AgentState::Working,
            Confidence::High,
            start,
        );
        h.task(session, "Fix the flaky login test");
        h.state(
            session,
            Some("claude"),
            end,
            Confidence::High,
            start + mins * MIN,
        );
    }

    #[test]
    fn a_task_is_recorded_when_it_ends_with_its_waiting_time() {
        let d = dir("basic");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state(
            "api",
            Some("claude"),
            AgentState::Working,
            Confidence::High,
            T0,
        );
        h.task("api", "Fix the flaky login test");
        h.state(
            "api",
            None,
            AgentState::NeedsInput,
            Confidence::High,
            T0 + MIN,
        );
        h.state(
            "api",
            None,
            AgentState::Working,
            Confidence::High,
            T0 + 4 * MIN,
        );
        h.state(
            "api",
            None,
            AgentState::NeedsInput,
            Confidence::High,
            T0 + 5 * MIN,
        );
        h.state(
            "api",
            None,
            AgentState::Done,
            Confidence::High,
            T0 + 6 * MIN,
        );
        let a = h.query(T0, T0 + 10 * MIN, T0 + 10 * MIN);
        assert_eq!(a.runs.len(), 1);
        let r = &a.runs[0];
        assert_eq!(r.session, "api");
        assert_eq!(r.agent.as_deref(), Some("claude"));
        assert_eq!(r.task.as_deref(), Some("Fix the flaky login test"));
        assert_eq!((r.started_at, r.ended_at), (T0, Some(T0 + 6 * MIN)));
        assert_eq!(r.outcome, Some(RunOutcome::Finished));
        assert_eq!(
            r.blocked_ms,
            4 * MIN,
            "3 minutes, then 1 more, waiting on the user"
        );
    }

    #[test]
    fn outcomes_follow_how_the_task_ended() {
        let d = dir("outcomes");
        let h = History::open(&d, limits(), T0).unwrap();
        task(&h, "a", T0, 1, AgentState::Done);
        task(&h, "b", T0, 1, AgentState::Error);
        h.state("c", None, AgentState::Working, Confidence::High, T0);
        h.state("c", None, AgentState::Idle, Confidence::Medium, T0 + MIN);
        h.state("d", None, AgentState::Working, Confidence::High, T0);
        h.state("d", None, AgentState::Idle, Confidence::High, T0 + MIN);
        let by = |s: &str| {
            h.query(T0, T0 + 9 * MIN, T0)
                .runs
                .into_iter()
                .find(|r| r.session == s)
                .unwrap()
                .outcome
        };
        assert_eq!(by("a"), Some(RunOutcome::Finished));
        assert_eq!(by("b"), Some(RunOutcome::Failed));
        assert_eq!(by("c"), Some(RunOutcome::Stopped));
        assert_eq!(by("d"), Some(RunOutcome::Finished));
    }

    #[test]
    fn a_task_in_flight_is_listed_without_an_end_and_a_gone_session_stops_it() {
        let d = dir("flight");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("api", None, AgentState::NeedsInput, Confidence::High, T0);
        let live = h.query(T0 - MIN, T0 + MIN, T0 + 2 * MIN);
        assert_eq!(live.runs.len(), 1);
        assert_eq!((live.runs[0].ended_at, live.runs[0].outcome), (None, None));
        assert_eq!(live.runs[0].blocked_ms, 2 * MIN);
        assert_eq!(file_len(&d), 0, "nothing is written until a task ends");
        h.retain_live("", &[], T0 + 3 * MIN);
        let a = h.query(T0, T0 + 9 * MIN, T0 + 9 * MIN);
        assert_eq!(a.runs.len(), 1);
        assert_eq!(a.runs[0].outcome, Some(RunOutcome::Stopped));
        assert_eq!(a.runs[0].blocked_ms, 3 * MIN);
    }

    #[test]
    fn titles_are_capped_and_scrubbed_and_nothing_else_is_stored() {
        let d = dir("scrub");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state(
            "api",
            Some("claude"),
            AgentState::Working,
            Confidence::High,
            T0,
        );
        let title = format!(
            "deploy with ghp_{} to 10.1.2.3 {}",
            "a".repeat(30),
            "and a very long tail ".repeat(10)
        );
        h.task("api", &title);
        h.state("api", None, AgentState::Done, Confidence::High, T0 + MIN);
        let text = std::fs::read_to_string(d.join(FILE)).unwrap();
        assert!(
            !text.contains("ghp_") && !text.contains("10.1.2.3"),
            "{text}"
        );
        let run = &h.query(T0, T0 + 9 * MIN, T0).runs[0];
        assert!(run.task.as_ref().unwrap().chars().count() <= TITLE_CHARS);
        assert!(run.task.as_ref().unwrap().contains("[token]"));
        // The record has exactly the metadata fields: no place for a prompt, an answer or screen text.
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "agent",
                "blocked_ms",
                "ended_at",
                "outcome",
                "session",
                "started_at",
                "task"
            ]
        );
    }

    #[test]
    fn it_survives_a_restart_and_answers_a_week_query() {
        let d = dir("week");
        let day = DAY_MS;
        {
            let h = History::open(&d, limits(), T0 + 7 * day).unwrap();
            for i in 0..7 {
                task(&h, "api", T0 + i * day, 5, AgentState::Done);
            }
            task(&h, "api", T0 + 8 * day, 5, AgentState::Error);
        }
        let h = History::open(&d, limits(), T0 + 9 * day).unwrap();
        let week = h.query(T0, T0 + 7 * day, T0 + 9 * day);
        assert_eq!(week.runs.len(), 7);
        assert!(!week.truncated);
        assert!(week.runs.windows(2).all(|w| w[0].ended_at < w[1].ended_at));
        let one_day = h.query(T0 + 8 * day, T0 + 9 * day, T0 + 9 * day);
        assert_eq!(one_day.runs.len(), 1);
        assert_eq!(one_day.runs[0].outcome, Some(RunOutcome::Failed));
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_the_rest_survives() {
        let d = dir("torn");
        {
            let h = History::open(&d, limits(), T0).unwrap();
            task(&h, "a", T0, 1, AgentState::Done);
            task(&h, "b", T0 + 2 * MIN, 1, AgentState::Done);
        }
        let mut f = OpenOptions::new().append(true).open(d.join(FILE)).unwrap();
        f.write_all(br#"{"session":"torn","started_at":1,"ended_at":"#)
            .unwrap();
        drop(f);
        let h = History::open(&d, limits(), T0 + 9 * MIN).unwrap();
        assert_eq!(h.query(0, u64::MAX, T0).runs.len(), 2);
        // New records land on their own line, not glued to the fragment.
        task(&h, "c", T0 + 5 * MIN, 1, AgentState::Done);
        assert_eq!(h.query(0, u64::MAX, T0).runs.len(), 3);
        let h = History::open(&d, limits(), T0 + 9 * MIN).unwrap();
        assert_eq!(h.query(0, u64::MAX, T0).runs.len(), 3);
        assert!(std::fs::read_to_string(d.join(FILE))
            .unwrap()
            .ends_with('\n'));
    }

    #[test]
    fn garbage_and_overlong_lines_are_ignored() {
        let d = dir("garbage");
        {
            let h = History::open(&d, limits(), T0).unwrap();
            task(&h, "a", T0, 1, AgentState::Done);
        }
        let mut f = OpenOptions::new().append(true).open(d.join(FILE)).unwrap();
        f.write_all(b"not json\n").unwrap();
        f.write_all(&vec![b'x'; 3 * MAX_LINE]).unwrap();
        f.write_all(b"\n").unwrap();
        f.write_all(br#"{"session":"running","started_at":1,"blocked_ms":0}"#)
            .unwrap();
        f.write_all(b"\n").unwrap();
        drop(f);
        let h = History::open(&d, limits(), T0 + 9 * MIN).unwrap();
        assert_eq!(
            h.query(0, u64::MAX, T0).runs.len(),
            1,
            "a record without an end is not a finished task"
        );
        assert!(file_len(&d) < 400, "the damage is removed from the file");
    }

    #[test]
    fn a_failed_append_does_not_corrupt_the_next_one() {
        let d = dir("failed");
        let mut s = Store::open(&d, limits(), T0).unwrap();
        s.torn = true; // as after a write that failed half-way
        let run = HistoryRun {
            session: "a".into(),
            agent: None,
            task: None,
            started_at: T0,
            ended_at: Some(T0),
            outcome: Some(RunOutcome::Finished),
            blocked_ms: 0,
        };
        std::fs::write(d.join(FILE), b"{\"sess").unwrap();
        s.append(&run, T0);
        assert_eq!(s.query(0, u64::MAX).0.len(), 1);
    }

    #[test]
    fn the_file_stays_within_its_size_limit_and_keeps_the_newest() {
        let d = dir("size");
        let limits = Limits {
            keep_days: 90,
            max_bytes: 10_000,
        };
        let h = History::open(&d, limits, T0).unwrap();
        for i in 0..500u64 {
            task(&h, &format!("s{i}"), T0 + i * 2 * MIN, 1, AgentState::Done);
            assert!(file_len(&d) <= limits.max_bytes, "after record {i}");
        }
        let kept = h.query(0, u64::MAX, T0).runs;
        assert!(!kept.is_empty() && kept.len() < 500);
        assert_eq!(kept.last().unwrap().session, "s499", "the newest survives");
        assert!(file_len(&d) <= limits.max_bytes);
        // The same holds after a restart with a smaller limit.
        drop(h);
        let smaller = Limits {
            keep_days: 90,
            max_bytes: 3_000,
        };
        History::open(&d, smaller, T0 + 2_000 * MIN).unwrap();
        assert!(file_len(&d) <= smaller.max_bytes);
    }

    #[test]
    fn records_older_than_the_age_limit_go_when_it_opens_and_on_the_hourly_check() {
        let d = dir("age");
        let limits = Limits {
            keep_days: 2,
            max_bytes: 20_000_000,
        };
        {
            let h = History::open(&d, limits, T0).unwrap();
            task(&h, "old", T0, 1, AgentState::Done);
            task(&h, "newer", T0 + DAY_MS, 1, AgentState::Done);
        }
        let h = History::open(&d, limits, T0 + 2 * DAY_MS + 5 * MIN).unwrap();
        let names = |h: &History| -> Vec<String> {
            h.query(0, u64::MAX, T0)
                .runs
                .into_iter()
                .map(|r| r.session)
                .collect()
        };
        assert_eq!(names(&h), ["newer"]);
        h.maintain(T0 + 3 * DAY_MS + 5 * MIN);
        assert!(names(&h).is_empty());
        assert_eq!(file_len(&d), 0);
    }

    #[test]
    fn one_answer_is_capped_and_says_so() {
        let d = dir("cap");
        let h = History::open(&d, limits(), T0).unwrap();
        for i in 0..(MAX_RUNS as u64 + 5) {
            task(&h, &format!("s{i}"), T0 + i * 2 * MIN, 1, AgentState::Done);
        }
        let a = h.query(0, u64::MAX, T0);
        assert_eq!(a.runs.len(), MAX_RUNS);
        assert!(a.truncated);
        assert_eq!(a.runs.last().unwrap().session, format!("s{}", MAX_RUNS + 4));
    }

    #[test]
    fn delete_forgets_the_file_and_the_tasks_in_flight() {
        let d = dir("delete");
        let h = History::open(&d, limits(), T0).unwrap();
        task(&h, "a", T0, 1, AgentState::Done);
        task(&h, "b", T0, 1, AgentState::Error);
        h.state("c", None, AgentState::Working, Confidence::High, T0);
        assert_eq!(h.delete(), 2);
        assert!(!d.join(FILE).exists());
        assert!(h.query(0, u64::MAX, T0).runs.is_empty());
        // It records again afterwards.
        task(&h, "d", T0, 1, AgentState::Done);
        assert_eq!(h.query(0, u64::MAX, T0).runs.len(), 1);
    }

    #[test]
    fn the_file_is_private_to_the_server_user() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        let h = History::open(&d, limits(), T0).unwrap();
        task(&h, "a", T0, 1, AgentState::Done);
        let mode = std::fs::metadata(d.join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
}
