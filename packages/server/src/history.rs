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
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use termenbox_core::events::AgentState;
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
/// A task with no event for this long has lost its end (a crashed agent, a missed hook): it is closed as stopped at its
/// last sign of life instead of swallowing the next prompt.
const OPEN_MAX_AGE_MS: u64 = DAY_MS;
/// After a failed compaction, wait this long before trying on the next task.
const RETRY_AFTER_MS: u64 = 600_000;
/// Records waiting for the store while a query or a compaction holds it. Far more than tasks end in a minute.
const MAX_BACKLOG: usize = 1000;
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
    /// A failed compaction is not tried again before this time (ms), so a full disk is not rewritten on every task.
    retry_after: u64,
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
            retry_after: 0,
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
            match parse(line) {
                None => r.invalid += 1,
                Some(run) if run.ended_at.unwrap_or(0) < cutoff => r.aged += 1,
                Some(_) => {
                    // A record is rewritten with its newline, so a missing one is not a reason to rewrite on its own.
                    kept += line.len() as u64 + 1;
                }
            }
            unterminated = len == line.len() as u64;
        })?;
        let target = if kept > self.limits.max_bytes {
            self.limits.max_bytes * TRIM_TO_PERCENT / 100
        } else {
            kept
        };
        let to_drop = kept - target;
        if r.aged == 0 && r.invalid == 0 && to_drop == 0 && !unterminated {
            self.bytes = r.before;
            r.after = r.before;
            return Ok(r);
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        // Any failure below leaves the old file in place and no temporary file behind.
        match self.write_trimmed(&tmp, &keep, to_drop, &mut r) {
            Ok(written) => {
                self.bytes = written;
                r.after = written;
                self.torn = false;
                Ok(r)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// Writes the kept records to `tmp`, makes them durable and renames `tmp` over the file; then makes the rename itself
    /// durable. Returns the bytes written.
    fn write_trimmed(
        &self,
        tmp: &Path,
        keep: &dyn Fn(&[u8]) -> bool,
        mut to_drop: u64,
        r: &mut Rewrite,
    ) -> Result<u64> {
        let out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(tmp)?;
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
            return Err(e.into());
        }
        let out = out.into_inner().map_err(|e| e.into_error())?;
        out.sync_all()?;
        std::fs::rename(tmp, &self.path)?;
        // The new file is already in place; a directory that cannot be synced only risks the rename after a power cut.
        if let Some(Err(e)) = self
            .path
            .parent()
            .map(|d| File::open(d).and_then(|d| d.sync_all()))
        {
            warn!("history directory not synced: {e}");
        }
        Ok(written)
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
            Err(e) => {
                self.retry_after = now + RETRY_AFTER_MS;
                warn!(why, "history store compaction failed: {e}");
            }
        }
    }

    fn over_limit(&self, now: u64) -> bool {
        self.bytes > self.limits.max_bytes && now >= self.retry_after
    }

    fn append(&mut self, run: &HistoryRun) {
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
    /// The generation of the history when the task began: a deletion starts a new one, and a task of an older one is
    /// never written.
    generation: u64,
    started_at: u64,
    /// The last event of this task: how long it has been silent.
    touched: u64,
    task: Option<String>,
    agent: Option<String>,
    blocked_ms: u64,
    blocked_since: Option<u64>,
}

impl Open {
    fn new(ts: u64, generation: u64) -> Self {
        Self {
            generation,
            started_at: ts,
            touched: ts,
            task: None,
            agent: None,
            blocked_ms: 0,
            blocked_since: None,
        }
    }
}

pub struct Answer {
    pub runs: Vec<HistoryRun>,
    pub truncated: bool,
}

pub struct History {
    store: Arc<Mutex<Store>>,
    open: Mutex<HashMap<String, Open>>,
    /// Finished tasks the store has not taken yet: the store is busy with a query or a compaction, and the hub that
    /// reports a task end must never wait for a disk. Whoever holds the store flushes this when it lets go.
    backlog: Arc<Mutex<VecDeque<HistoryRun>>>,
    /// Wakes the writer thread, which does the disk work so that the hub's push path never does.
    wake: SyncSender<u64>,
    /// Bumped by `delete`: a task that began before it is not written after it.
    generation: AtomicU64,
    limits: Limits,
}

fn clip(s: &str) -> String {
    sanitize_text(s, FIELD_CHARS)
}

/// A task title as it is kept: what `sanitize_text` replaces (tokens, secrets after a keyword, IP addresses, host:port),
/// plus email addresses, URLs and absolute paths, which that does not touch; capped at `TITLE_CHARS`.
fn scrub_title(title: &str) -> String {
    let text = sanitize_text(title, 4 * TITLE_CHARS);
    let word = |w: &str| {
        let core = w.trim_start_matches(['(', '[', '"', '\'', '`', '<', '=']);
        // `--dir=/srv/x`: the part after the last `=` is the value.
        let value = core.rsplit('=').next().unwrap_or(core);
        // An absolute path has a second slash (`/var/www`); `/review` and `/commit` are commands, not paths.
        let path = |p: &str| p.starts_with("~/") || (p.starts_with('/') && p[1..].contains('/'));
        if w.contains("://") {
            "[url]"
        } else if w
            .split_once('@')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
        {
            "[email]"
        } else if path(core) || path(value) {
            "[path]"
        } else {
            ""
        }
    };
    let scrubbed: Vec<&str> = text
        .split(' ')
        .map(|w| Some(word(w)).filter(|r| !r.is_empty()).unwrap_or(w))
        .collect();
    scrubbed.join(" ").chars().take(TITLE_CHARS).collect()
}

/// The writer thread's work: everything waiting goes to the file, and the file is compacted while it is over its limit.
/// It waits for the store, which is fine here: nothing else waits for this thread.
fn drain(store: &Mutex<Store>, backlog: &Mutex<VecDeque<HistoryRun>>, now: u64) {
    let mut store = store.lock().unwrap();
    loop {
        loop {
            let Some(run) = backlog.lock().unwrap().pop_front() else {
                break;
            };
            store.append(&run);
        }
        if !store.over_limit(now) {
            break;
        }
        store.compact(now, "size");
    }
}

impl History {
    pub fn open(dir: &Path, limits: Limits, now: u64) -> Result<Self> {
        let store = Arc::new(Mutex::new(Store::open(dir, limits, now)?));
        let backlog: Arc<Mutex<VecDeque<HistoryRun>>> = Arc::default();
        let (wake, woken) = std::sync::mpsc::sync_channel::<u64>(1);
        std::thread::Builder::new()
            .name("history-writer".into())
            .spawn({
                let (store, backlog) = (store.clone(), backlog.clone());
                move || {
                    // Ends when the history is dropped and the sender with it.
                    while let Ok(now) = woken.recv() {
                        drain(&store, &backlog, now);
                    }
                }
            })
            .context("starting the history writer")?;
        Ok(Self {
            store,
            open: Mutex::default(),
            backlog,
            wake,
            generation: AtomicU64::new(0),
            limits,
        })
    }

    /// Waits until the writer thread has taken everything that waited and finished any compaction (tests, which look
    /// at the file straight after a task).
    #[cfg(test)]
    pub fn wait_idle(&self) {
        while !self.backlog.lock().unwrap().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // The writer holds the store while it appends and compacts.
        drop(self.store.lock().unwrap());
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// A session's state changed (a fact from a hook, Codex or ACP): a task starts when the agent starts working or
    /// waits, and ends when it is done, failed or idle. The time spent waiting on the user is summed as it happens.
    pub fn state(&self, session: &str, agent: Option<&str>, state: AgentState, ts: u64) {
        let mut ended = Vec::new();
        {
            let mut open = self.open.lock().unwrap();
            match state {
                AgentState::Working | AgentState::NeedsInput => {
                    // An end that never came: close that task where it went quiet, and start this one fresh.
                    if open
                        .get(session)
                        .is_some_and(|r| ts.saturating_sub(r.touched) >= OPEN_MAX_AGE_MS)
                    {
                        if let Some(run) = open.remove(session) {
                            let at = run.touched;
                            ended.push((run, RunOutcome::Stopped, at));
                        }
                    }
                    if !open.contains_key(session) && open.len() >= MAX_OPEN {
                        drop(open);
                        self.finish_all(session, agent, ended);
                        return;
                    }
                    let run = open
                        .entry(session.to_string())
                        .or_insert_with(|| Open::new(ts, self.generation.load(Ordering::Acquire)));
                    run.touched = ts;
                    if let Some(a) = agent {
                        run.agent = Some(clip(a));
                    }
                    if let Some(since) = run.blocked_since.take() {
                        run.blocked_ms += ts.saturating_sub(since);
                    }
                    if state == AgentState::NeedsInput {
                        run.blocked_since = Some(ts);
                    }
                }
                AgentState::Done | AgentState::Idle | AgentState::Error => {
                    // Only Done is a finished turn and only Error a failure. Idle is the agent going quiet (a session
                    // that ended or started, a cancelled or cut-off turn): the task stopped, it did not finish.
                    let outcome = match state {
                        AgentState::Done => RunOutcome::Finished,
                        AgentState::Error => RunOutcome::Failed,
                        _ => RunOutcome::Stopped,
                    };
                    if let Some(run) = open.remove(session) {
                        ended.push((run, outcome, ts));
                    }
                }
                AgentState::Unknown => {}
            }
        }
        self.finish_all(session, agent, ended);
    }

    fn finish_all(&self, session: &str, agent: Option<&str>, ended: Vec<(Open, RunOutcome, u64)>) {
        for (run, outcome, ts) in ended {
            self.finish(session, run, outcome, agent, ts);
        }
    }

    /// The agent named what it is working on; only a task in flight takes a title.
    pub fn task(&self, session: &str, title: &str) {
        let title = scrub_title(title);
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

    /// Hands a finished task to the store without waiting for it, and starts a compaction (off this thread) when the
    /// file is over its limit.
    fn finish(&self, session: &str, run: Open, outcome: RunOutcome, agent: Option<&str>, ts: u64) {
        let generation = run.generation;
        let ended_at = ts.max(run.started_at);
        let blocked = run.blocked_ms
            + run
                .blocked_since
                .map_or(0, |since| ended_at.saturating_sub(since));
        let record = HistoryRun {
            session: clip(session),
            // The agent the task ran under; the one named at its end only when the task never learned one.
            agent: run.agent.or_else(|| agent.map(clip)),
            task: run.task,
            started_at: run.started_at,
            ended_at: Some(ended_at),
            outcome: Some(outcome),
            blocked_ms: blocked.min(ended_at - run.started_at),
        };
        {
            let mut backlog = self.backlog.lock().unwrap();
            // Checked under the backlog lock that `delete` also takes after bumping the generation: a task that began
            // before a deletion cannot land after it.
            if generation != self.generation.load(Ordering::Acquire) {
                return;
            }
            if backlog.len() >= MAX_BACKLOG {
                backlog.pop_front();
                warn!("history backlog full: the oldest waiting record was dropped");
            }
            backlog.push_back(record);
        }
        // A full slot means the writer has been woken and has not looked yet: it will see this record too.
        let _ = self.wake.try_send(ts);
    }

    /// The tasks that ended in `[from, to)`, and the running ones that belong to that window: all of them when it
    /// reaches the present, otherwise those that started inside it.
    pub fn query(&self, from: u64, to: u64, now: u64) -> Answer {
        // The backlog is read before the file: a record the writer moves to the file meanwhile is then in the file and
        // in this snapshot (and counted once below), never in neither.
        let waiting: Vec<HistoryRun> = self
            .backlog
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.ended_at.is_some_and(|e| e >= from && e < to))
            .cloned()
            .collect();
        let (mut runs, truncated) = self.store.lock().unwrap().query(from, to);
        let key = |r: &HistoryRun| (r.session.clone(), r.started_at, r.ended_at);
        let known: HashSet<_> = runs.iter().map(key).collect();
        runs.extend(waiting.into_iter().filter(|r| !known.contains(&key(r))));
        let open = self.open.lock().unwrap();
        for (session, run) in open
            .iter()
            .filter(|(_, r)| r.started_at < to && (to > now || r.started_at >= from))
        {
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
        // First, so that a task finishing from here on is dropped, not written after the deletion.
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.open.lock().unwrap().clear();
        let mut store = self.store.lock().unwrap();
        let waiting = std::mem::take(&mut *self.backlog.lock().unwrap()).len() as u64;
        let removed = store.delete() + waiting;
        info!(removed, "history deleted");
        removed
    }

    /// The hourly check: closes tasks that lost their end, and drops what has aged out.
    pub fn maintain(&self, now: u64) {
        let lost: Vec<(String, Open)> = {
            let mut open = self.open.lock().unwrap();
            let names: Vec<String> = open
                .iter()
                .filter(|(_, r)| now.saturating_sub(r.touched) >= OPEN_MAX_AGE_MS)
                .map(|(k, _)| k.clone())
                .collect();
            names
                .into_iter()
                .filter_map(|k| open.remove(&k).map(|r| (k, r)))
                .collect()
        };
        for (session, run) in lost {
            let at = run.touched;
            self.finish(&session, run, RunOutcome::Stopped, None, at);
        }
        self.store.lock().unwrap().compact(now, "age");
        let _ = self.wake.try_send(now);
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

    fn idle(h: &History) {
        h.wait_idle();
    }

    fn file_len(d: &Path) -> u64 {
        std::fs::metadata(d.join(FILE))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// One whole task: working, then done, `mins` later.
    fn task(h: &History, session: &str, start: u64, mins: u64, end: AgentState) {
        h.state(session, Some("claude"), AgentState::Working, start);
        h.task(session, "Fix the flaky login test");
        h.state(session, Some("claude"), end, start + mins * MIN);
        idle(h);
    }

    #[test]
    fn a_task_is_recorded_when_it_ends_with_its_waiting_time() {
        let d = dir("basic");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("api", Some("claude"), AgentState::Working, T0);
        h.task("api", "Fix the flaky login test");
        h.state("api", None, AgentState::NeedsInput, T0 + MIN);
        h.state("api", None, AgentState::Working, T0 + 4 * MIN);
        h.state("api", None, AgentState::NeedsInput, T0 + 5 * MIN);
        h.state("api", None, AgentState::Done, T0 + 6 * MIN);
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
        h.state("c", None, AgentState::Working, T0);
        h.state("c", None, AgentState::Idle, T0 + MIN);
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
        // Idle is a session that ended or a turn that was cut off, never a finished task.
        assert_eq!(by("c"), Some(RunOutcome::Stopped));
    }

    #[test]
    fn a_task_in_flight_is_listed_without_an_end_and_a_gone_session_stops_it() {
        let d = dir("flight");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("api", None, AgentState::NeedsInput, T0);
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
        h.state("api", Some("claude"), AgentState::Working, T0);
        let title = format!(
            "deploy with ghp_{} to 10.1.2.3 {}",
            "a".repeat(30),
            "and a very long tail ".repeat(10)
        );
        h.task("api", &title);
        h.state("api", None, AgentState::Done, T0 + MIN);
        idle(&h);
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
        s.append(&run);
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
        }
        idle(&h);
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
        h.state("c", None, AgentState::Working, T0);
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

    #[test]
    fn titles_lose_emails_urls_and_home_paths_too() {
        let d = dir("scrub");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("a", None, AgentState::Working, T0);
        h.task(
            "a",
            "Mail bob@example.com the log from https://ci.example.com/run/1 at /home/bob/app/x.log and ~/notes",
        );
        h.state("a", None, AgentState::Done, T0 + MIN);
        let t = h.query(0, u64::MAX, T0).runs.remove(0).task.unwrap();
        assert_eq!(t, "Mail [email] the log from [url] at [path] and [path]");
    }

    #[test]
    fn a_task_end_never_waits_for_the_store() {
        let d = dir("backlog");
        let h = History::open(&d, limits(), T0).unwrap();
        let busy = h.store.lock().unwrap(); // a query or a compaction is running
        h.state("a", Some("claude"), AgentState::Working, T0);
        h.state("a", None, AgentState::Done, T0 + MIN); // must return, not block
        assert_eq!(h.backlog.lock().unwrap().len(), 1);
        drop(busy);
        assert_eq!(h.query(0, u64::MAX, T0).runs.len(), 1, "nothing is lost");
        idle(&h);
        assert!(h.backlog.lock().unwrap().is_empty());
        assert_eq!(h.delete(), 1);
    }

    #[test]
    fn a_failed_compaction_is_not_retried_on_every_task() {
        let d = dir("retry");
        let limits = Limits {
            keep_days: 90,
            max_bytes: 1_000,
        };
        let mut s = Store::open(&d, limits, T0).unwrap();
        for i in 0..20u64 {
            s.append(&HistoryRun {
                session: format!("s{i}"),
                agent: None,
                task: Some("Fix the flaky login test".into()),
                started_at: T0,
                ended_at: Some(T0 + MIN),
                outcome: Some(RunOutcome::Finished),
                blocked_ms: 0,
            });
        }
        assert!(s.over_limit(T0));
        // The temporary file cannot be created: a directory is in its place.
        std::fs::create_dir(d.join("history.jsonl.tmp")).unwrap();
        s.compact(T0, "size");
        assert!(!s.over_limit(T0 + MIN), "backs off after a failure");
        assert!(s.over_limit(T0 + RETRY_AFTER_MS), "and tries again later");
        assert!(file_len(&d) > limits.max_bytes, "the old file is untouched");
    }

    #[test]
    fn a_compaction_leaves_no_temporary_file() {
        let d = dir("tmp");
        let limits = Limits {
            keep_days: 90,
            max_bytes: 2_000,
        };
        let h = History::open(&d, limits, T0).unwrap();
        for i in 0..50u64 {
            task(&h, &format!("s{i}"), T0 + i * 2 * MIN, 1, AgentState::Done);
        }
        idle(&h);
        assert!(!d.join("history.jsonl.tmp").exists());
        assert!(file_len(&d) <= limits.max_bytes);
    }

    #[test]
    fn a_task_that_never_ended_is_closed_instead_of_swallowing_the_next() {
        let d = dir("lost");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("api", Some("claude"), AgentState::Working, T0);
        // The end never came; a day later a new prompt arrives.
        h.state("api", None, AgentState::Working, T0 + OPEN_MAX_AGE_MS + MIN);
        h.state(
            "api",
            None,
            AgentState::Done,
            T0 + OPEN_MAX_AGE_MS + 2 * MIN,
        );
        let runs = h.query(0, u64::MAX, T0 + OPEN_MAX_AGE_MS + 3 * MIN).runs;
        assert_eq!(runs.len(), 2);
        assert_eq!(
            (runs[0].started_at, runs[0].ended_at, runs[0].outcome),
            (T0, Some(T0), Some(RunOutcome::Stopped))
        );
        assert_eq!(runs[1].started_at, T0 + OPEN_MAX_AGE_MS + MIN);
        assert_eq!(runs[1].outcome, Some(RunOutcome::Finished));
        // The hourly check closes one nobody came back to.
        h.state("b", None, AgentState::Working, T0);
        h.maintain(T0 + OPEN_MAX_AGE_MS + 5 * MIN);
        let runs = h.query(0, u64::MAX, T0 + OPEN_MAX_AGE_MS + 6 * MIN).runs;
        assert!(runs.iter().all(|r| r.ended_at.is_some()));
        assert_eq!(runs.iter().filter(|r| r.session == "b").count(), 1);
    }

    #[test]
    fn a_task_that_ends_after_a_deletion_is_not_written() {
        let d = dir("delete-race");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("a", Some("claude"), AgentState::Working, T0);
        // The end has taken the task out of the map but not yet handed it over when the user deletes.
        let run = h.open.lock().unwrap().remove("a").unwrap();
        assert_eq!(h.delete(), 0);
        h.finish("a", run, RunOutcome::Finished, None, T0 + MIN);
        idle(&h);
        assert_eq!(h.query(0, u64::MAX, T0 + MIN).runs.len(), 0);
        assert_eq!(file_len(&d), 0);
        // A task that begins afterwards is recorded as usual.
        task(&h, "b", T0 + 2 * MIN, 1, AgentState::Done);
        assert_eq!(h.query(0, u64::MAX, T0 + 9 * MIN).runs.len(), 1);
    }

    #[test]
    fn a_record_in_the_file_and_the_backlog_is_answered_once() {
        let d = dir("both");
        let h = History::open(&d, limits(), T0).unwrap();
        task(&h, "a", T0, 1, AgentState::Done);
        let run = h.query(0, u64::MAX, T0 + 9 * MIN).runs.remove(0);
        // The moment between the writer's append and its pop, as a reader sees it.
        h.backlog.lock().unwrap().push_back(run);
        assert_eq!(h.query(0, u64::MAX, T0 + 9 * MIN).runs.len(), 1);
        h.backlog.lock().unwrap().clear();
    }

    #[test]
    fn any_absolute_path_is_scrubbed_but_a_slash_command_is_not() {
        for t in [
            "Fix /var/www/acme-corp/billing/secret.rb",
            "cd /srv/clients/acme",
            "run with --dir=/home/bob/work",
            "open ~/notes/x.md",
        ] {
            let got = scrub_title(t);
            assert!(got.contains("[path]"), "{t} -> {got}");
            assert!(
                !got.contains("acme") && !got.contains("bob") && !got.contains("notes"),
                "{got}"
            );
        }
        assert_eq!(scrub_title("/review the change"), "/review the change");
    }

    #[test]
    fn a_task_keeps_the_agent_it_ran_under() {
        let d = dir("agent");
        let h = History::open(&d, limits(), T0).unwrap();
        h.state("s", Some("claude"), AgentState::Working, T0);
        // A day later another agent's event closes the stale task and opens a new one.
        h.state("s", Some("codex"), AgentState::Working, T0 + 2 * DAY_MS);
        h.state("s", Some("codex"), AgentState::Done, T0 + 2 * DAY_MS + MIN);
        idle(&h);
        let runs = h.query(0, u64::MAX, T0 + 3 * DAY_MS).runs;
        let agents: Vec<_> = runs.iter().map(|r| r.agent.as_deref()).collect();
        assert_eq!(agents, [Some("claude"), Some("codex")]);
    }

    #[test]
    fn a_past_window_does_not_list_what_is_running_now() {
        let d = dir("window");
        let h = History::open(&d, limits(), T0).unwrap();
        let hour = 60 * MIN;
        h.state("old", Some("claude"), AgentState::Working, T0);
        h.state(
            "new",
            Some("claude"),
            AgentState::Working,
            T0 + 5 * DAY_MS - hour,
        );
        let now = T0 + 6 * DAY_MS;
        // Yesterday's window [now - 1 day, now) holds neither start.
        assert!(h.query(now - DAY_MS, now, now).runs.is_empty());
        // A window that reaches the present lists both.
        assert_eq!(h.query(now - DAY_MS, now + DAY_MS, now).runs.len(), 2);
        // A past window lists the one that began inside it.
        let w = h.query(T0 + 5 * DAY_MS - 2 * hour, T0 + 5 * DAY_MS + 12 * hour, now);
        assert_eq!(w.runs.len(), 1);
    }
}
