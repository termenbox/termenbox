//! What the server leaves behind when it dies (decision 0142). A panic writes its message, place and backtrace to the
//! log and to `<config-dir>/logs/crash-<ms>.json`. A run marker, `<config-dir>/run.marker`, is written at start and
//! removed on a clean exit; finding it at the next start means the last run ended without one (a panic on the main
//! task, an OOM kill, a SIGKILL), which nothing else can report afterwards.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const MARKER: &str = "run.marker";
/// Crash files kept, and written by one process: a panic loop must not fill the disk.
const KEEP_CRASH_FILES: usize = 20;
const MAX_CRASH_FILES_PER_RUN: usize = 10;
const MAX_MESSAGE_CHARS: usize = 2000;
/// A run this old has proved itself: the unclean exits before it stop counting.
const STABLE_MS: u64 = 10 * 60 * 1000;
/// Unclean exits whose live workspaces are remembered; a workspace live at all of them is not resumed (decision 0143).
const SAFE_MODE_RUNS: usize = 2;

static MARKER_PATH: OnceLock<PathBuf> = OnceLock::new();
static WRITTEN: AtomicUsize = AtomicUsize::new(0);
static SAFE_MODE: OnceLock<HashSet<String>> = OnceLock::new();
/// Marker updates read and rewrite the file from several threads (the health beat, the workspace list).
static MARKER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct MarkerFile {
    pid: u32,
    started_ms: u64,
    /// Last time `update` ran, and the resident memory it was given (set by the health sampler).
    #[serde(default)]
    beat_ms: u64,
    #[serde(default)]
    rss_kb: Option<u64>,
    /// Runs in a row that ended unclean, this one not counted.
    #[serde(default)]
    unclean_streak: u32,
    /// Workspace ids live in this run, as of the last `set_live`.
    #[serde(default)]
    live: Vec<String>,
    /// The live ids of the last unclean runs before this one, oldest first.
    #[serde(default)]
    prior_live: Vec<Vec<String>>,
}

impl MarkerFile {
    /// Up for `STABLE_MS`, the run was not part of a crash loop: the unclean exits before it stop counting.
    fn forget_if_stable(&mut self) {
        if self.beat_ms.saturating_sub(self.started_ms) >= STABLE_MS {
            self.unclean_streak = 0;
            self.prior_live.clear();
        }
    }
}

/// How the run before this one ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Previous {
    /// No marker: the last run released it (or there was no last run).
    Clean,
    Unclean(Unclean),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unclean {
    pub started_ms: u64,
    pub beat_ms: u64,
    pub rss_kb: Option<u64>,
    /// Consecutive unclean runs, this report included.
    pub streak: u32,
    /// The newest crash file written since that run started, by name.
    pub crash_file: Option<String>,
    /// The workspace ids live at each of the last unclean exits, this one last (at most `SAFE_MODE_RUNS`).
    pub live_sets: Vec<Vec<String>>,
}

fn marker_path(config_dir: &Path) -> PathBuf {
    config_dir.join(MARKER)
}

fn write_marker(path: &Path, marker: &MarkerFile, sync: bool) -> std::io::Result<()> {
    let tmp = path.with_extension("marker.tmp");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(serde_json::to_string(marker)?.as_bytes())?;
    if sync {
        file.sync_all()?;
    }
    fs::rename(tmp, path)
}

fn crash_ms(name: &str) -> Option<u64> {
    name.strip_prefix("crash-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn crash_files(logs: &Path) -> Vec<(u64, String)> {
    let mut files: Vec<(u64, String)> = fs::read_dir(logs)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            crash_ms(&name).map(|ms| (ms, name))
        })
        .collect();
    files.sort();
    files
}

/// Reads how the last run ended and writes this run's marker. Call once, on the server run path.
pub fn start(config_dir: &Path) -> Previous {
    let path = marker_path(config_dir);
    let previous = match fs::read_to_string(&path) {
        Ok(text) => {
            // A marker that cannot be read is still a run that did not finish.
            let old = serde_json::from_str::<MarkerFile>(&text).ok();
            let started_ms = old.as_ref().map_or(0, |m| m.started_ms);
            let live_sets = old.as_ref().map_or_else(Vec::new, |m| {
                let mut sets = m.prior_live.clone();
                sets.push(m.live.clone());
                sets.split_off(sets.len().saturating_sub(SAFE_MODE_RUNS))
            });
            let crash_file = crash_files(&config_dir.join("logs"))
                .into_iter()
                .rfind(|(ms, _)| *ms >= started_ms)
                .map(|c| c.1);
            Some(Previous::Unclean(Unclean {
                started_ms,
                beat_ms: old.as_ref().map_or(0, |m| m.beat_ms),
                rss_kb: old.as_ref().and_then(|m| m.rss_kb),
                streak: old.as_ref().map_or(0, |m| m.unclean_streak) + 1,
                crash_file,
                live_sets,
            }))
        }
        Err(_) => None,
    };
    let streak = match &previous {
        Some(Previous::Unclean(u)) => u.streak,
        _ => 0,
    };
    let live_sets = match &previous {
        Some(Previous::Unclean(u)) => u.live_sets.clone(),
        _ => Vec::new(),
    };
    let now = crate::events::now_ms();
    let marker = MarkerFile {
        pid: std::process::id(),
        started_ms: now,
        beat_ms: now,
        rss_kb: None,
        unclean_streak: streak,
        live: Vec::new(),
        prior_live: live_sets,
    };
    let _guard = MARKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = write_marker(&path, &marker, true) {
        tracing::warn!("cannot write the run marker {}: {e}", path.display());
    }
    let _ = MARKER_PATH.set(path);
    let previous = previous.unwrap_or(Previous::Clean);
    let _ = SAFE_MODE.set(previous.safe_mode_ids());
    previous
}

/// Records which workspaces are live now, so that the next start knows who was running when this run died.
pub fn set_live(ids: Vec<String>) {
    rewrite_marker(false, |m| m.live = ids);
}

/// The workspace ids this start must not resume: live at each of the last two unclean exits.
pub fn safe_mode_ids() -> HashSet<String> {
    SAFE_MODE.get().cloned().unwrap_or_default()
}

fn rewrite_marker(sync: bool, f: impl FnOnce(&mut MarkerFile)) {
    let Some(path) = MARKER_PATH.get() else {
        return;
    };
    let _guard = MARKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let Ok(mut marker) = serde_json::from_str::<MarkerFile>(&text) else {
        return;
    };
    f(&mut marker);
    let _ = write_marker(path, &marker, sync);
}

/// Records that the server is alive and how much memory it holds. The health sampler calls it every tick, on a
/// blocking thread; it does not sync, since a lost beat only makes the report a few seconds old.
pub fn update(rss_kb: Option<u64>) {
    rewrite_marker(false, |marker| {
        marker.beat_ms = crate::events::now_ms();
        marker.rss_kb = rss_kb.or(marker.rss_kb);
        marker.forget_if_stable();
    });
}

/// Resident memory of this process in KiB, or nothing where `/proc` has no answer.
pub fn own_rss_kb() -> Option<u64> {
    let statm = fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (page > 0).then(|| pages * page as u64 / 1024)
}

impl Previous {
    /// Ids live at both of the last two unclean exits. One unclean exit resumes everything, as before.
    pub fn safe_mode_ids(&self) -> HashSet<String> {
        let Previous::Unclean(u) = self else {
            return HashSet::new();
        };
        if u.live_sets.len() < SAFE_MODE_RUNS {
            return HashSet::new();
        }
        let (first, rest) = u.live_sets.split_first().expect("two sets");
        first
            .iter()
            .filter(|id| rest.iter().all(|set| set.contains(*id)))
            .cloned()
            .collect()
    }

    /// What the owner's app is told: only the last time seen alive, the streak and whether a panic left a file.
    pub fn message(&self) -> Option<termenbox_core::protocol::ServerMessage> {
        let Previous::Unclean(u) = self else {
            return None;
        };
        Some(termenbox_core::protocol::ServerMessage::PreviousExit {
            at_ms: if u.beat_ms > 0 {
                u.beat_ms
            } else {
                u.started_ms
            },
            streak: u.streak,
            crashed: u.crash_file.is_some(),
        })
    }

    /// The line the log gets at start. Times and sizes only.
    pub fn log(&self) {
        if let Previous::Unclean(u) = self {
            tracing::warn!(
                target: "crash",
                started_ms = u.started_ms,
                last_seen_ms = u.beat_ms,
                rss_kb = u.rss_kb,
                streak = u.streak,
                crash_file = u.crash_file.as_deref(),
                "previous run ended uncleanly"
            );
        }
    }
}

/// The run ended on purpose: removes the marker.
pub fn mark_clean() {
    if let Some(path) = MARKER_PATH.get() {
        let _ = fs::remove_file(path);
    }
}

/// Marks the run clean when `main` returns or fails, but not while a panic unwinds through it.
pub struct CleanExit;

impl Drop for CleanExit {
    fn drop(&mut self) {
        crate::logging::flush();
        if !std::thread::panicking() {
            mark_clean();
        }
    }
}

#[derive(Serialize)]
struct CrashReport {
    ts_ms: u64,
    version: &'static str,
    thread: String,
    location: String,
    message: String,
    backtrace: String,
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-text panic payload)".into());
    text.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// After the hook, a panic is logged, written to a crash file, and then handled as before (a task dies, or the
/// process does).
pub fn install_panic_hook(config_dir: &Path) {
    let logs = config_dir.join("logs");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = CrashReport {
            ts_ms: crate::events::now_ms(),
            version: env!("CARGO_PKG_VERSION"),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
            location: info.location().map_or_else(
                || "unknown".into(),
                |l| format!("{}:{}", l.file(), l.line()),
            ),
            message: panic_message(info),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        tracing::error!(target: "panic", thread = %report.thread, location = %report.location, "panic: {}", report.message);
        if WRITTEN.fetch_add(1, Ordering::SeqCst) < MAX_CRASH_FILES_PER_RUN {
            let _ = write_crash_file(&logs, &report);
        }
        // The process may be about to end: what is queued must reach the file first.
        crate::logging::flush();
        previous(info);
    }));
}

fn write_crash_file(logs: &Path, report: &CrashReport) -> std::io::Result<()> {
    fs::create_dir_all(logs)?;
    let path = logs.join(format!("crash-{}.json", report.ts_ms));
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    file.sync_all()?;
    let files = crash_files(logs);
    for (_, name) in files
        .iter()
        .take(files.len().saturating_sub(KEEP_CRASH_FILES))
    {
        let _ = fs::remove_file(logs.join(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termenbox-crash-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // `start` and `mark_clean` share one process-wide path, so the marker tests drive the pieces they can in a
    // fresh directory and mark clean by removing the file the way `mark_clean` does.
    fn release(dir: &Path) {
        let _ = fs::remove_file(marker_path(dir));
    }

    #[test]
    fn a_first_start_and_a_start_after_a_clean_exit_report_clean() {
        let dir = temp_dir();
        assert_eq!(start(&dir), Previous::Clean);
        release(&dir);
        assert_eq!(start(&dir), Previous::Clean);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_start_that_finds_the_marker_reports_unclean_and_counts_the_streak() {
        let dir = temp_dir();
        assert_eq!(start(&dir), Previous::Clean);
        let Previous::Unclean(first) = start(&dir) else {
            panic!("expected unclean")
        };
        assert_eq!(first.streak, 1);
        let Previous::Unclean(second) = start(&dir) else {
            panic!("expected unclean")
        };
        assert_eq!(second.streak, 2);
        release(&dir);
        assert_eq!(start(&dir), Previous::Clean);
        let text = fs::read_to_string(marker_path(&dir)).unwrap();
        assert!(text.contains("\"unclean_streak\":0"), "{text}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_message_names_the_last_beat_and_whether_a_panic_left_a_file() {
        let unclean = |beat_ms, crash_file: Option<&str>| {
            Previous::Unclean(Unclean {
                started_ms: 10,
                beat_ms,
                rss_kb: None,
                streak: 3,
                crash_file: crash_file.map(Into::into),
                live_sets: Vec::new(),
            })
        };
        assert_eq!(Previous::Clean.message(), None);
        let termenbox_core::protocol::ServerMessage::PreviousExit {
            at_ms,
            streak,
            crashed,
        } = unclean(50, Some("crash-60.json")).message().unwrap()
        else {
            panic!("wrong message")
        };
        assert_eq!((at_ms, streak, crashed), (50, 3, true));
        let termenbox_core::protocol::ServerMessage::PreviousExit { at_ms, crashed, .. } =
            unclean(0, None).message().unwrap()
        else {
            panic!("wrong message")
        };
        assert_eq!((at_ms, crashed), (10, false));
    }

    #[test]
    fn the_process_reports_its_own_memory() {
        assert!(own_rss_kb().is_some_and(|kb| kb > 0));
    }

    #[test]
    fn an_unreadable_marker_still_counts_as_unclean() {
        let dir = temp_dir();
        fs::write(marker_path(&dir), "not json").unwrap();
        assert!(matches!(start(&dir), Previous::Unclean(_)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_unclean_report_names_a_crash_file_from_after_the_run_started() {
        let dir = temp_dir();
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let marker = MarkerFile {
            pid: 1,
            started_ms: 5000,
            beat_ms: 6000,
            rss_kb: Some(9000),
            unclean_streak: 0,
            live: Vec::new(),
            prior_live: Vec::new(),
        };
        write_marker(&marker_path(&dir), &marker, true).unwrap();
        fs::write(logs.join("crash-4000.json"), "{}").unwrap();
        fs::write(logs.join("crash-7000.json"), "{}").unwrap();
        let Previous::Unclean(u) = start(&dir) else {
            panic!("expected unclean")
        };
        assert_eq!(u.crash_file.as_deref(), Some("crash-7000.json"));
        assert_eq!(
            (u.started_ms, u.beat_ms, u.rss_kb),
            (5000, 6000, Some(9000))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A run that died with these workspaces live: the marker `start` wrote, with `live` filled in as `set_live` would.
    fn die_with(dir: &Path, live: &[&str]) {
        let mut marker: MarkerFile =
            serde_json::from_str(&fs::read_to_string(marker_path(dir)).unwrap()).unwrap();
        marker.live = live.iter().map(|s| s.to_string()).collect();
        write_marker(&marker_path(dir), &marker, true).unwrap();
    }

    fn ids(set: &[&str]) -> HashSet<String> {
        set.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn safe_mode_names_the_workspaces_live_at_both_of_the_last_two_unclean_exits() {
        let dir = temp_dir();
        assert_eq!(start(&dir), Previous::Clean);
        die_with(&dir, &["a", "b"]);
        // One unclean exit: everything resumes.
        let one = start(&dir);
        assert_eq!(one.safe_mode_ids(), ids(&[]));
        die_with(&dir, &["a", "c"]);
        // Two: only what was live at both.
        let two = start(&dir);
        assert_eq!(two.safe_mode_ids(), ids(&["a"]));
        die_with(&dir, &["c"]);
        // The window moves on: the last two are [a, c] and [c].
        assert_eq!(start(&dir).safe_mode_ids(), ids(&["c"]));
        release(&dir);
        // A clean exit forgets them.
        assert_eq!(start(&dir).safe_mode_ids(), ids(&[]));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn safe_mode_forgets_the_exits_after_a_stable_run() {
        let dir = temp_dir();
        assert_eq!(start(&dir), Previous::Clean);
        die_with(&dir, &["a"]);
        start(&dir);
        // The marker as `update` finds it after ten minutes up.
        let mut marker: MarkerFile =
            serde_json::from_str(&fs::read_to_string(marker_path(&dir)).unwrap()).unwrap();
        marker.beat_ms = marker.started_ms + STABLE_MS;
        marker.forget_if_stable();
        marker.live = vec!["a".into()];
        write_marker(&marker_path(&dir), &marker, true).unwrap();
        assert_eq!(start(&dir).safe_mode_ids(), ids(&[]));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_panic_in_a_thread_writes_a_crash_file() {
        let dir = temp_dir();
        let logs = dir.join("logs");
        install_panic_hook(&dir);
        let handle = std::thread::Builder::new()
            .name("doomed".into())
            .spawn(|| panic!("boom {}", 7))
            .unwrap();
        assert!(handle.join().is_err());
        let _ = std::panic::take_hook();
        // Another test may panic on purpose while the hook is installed, so look for ours among the files.
        let reports: Vec<serde_json::Value> = crash_files(&logs)
            .iter()
            .map(|(_, name)| {
                serde_json::from_str(&fs::read_to_string(logs.join(name)).unwrap()).unwrap()
            })
            .collect();
        let report = reports
            .iter()
            .find(|r| r["message"] == "boom 7")
            .expect("no crash file for the panic");
        assert_eq!(report["thread"], "doomed");
        assert!(report["location"].as_str().unwrap().contains("crash.rs"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_crash_files_are_pruned() {
        let dir = temp_dir();
        for ms in 1..=(KEEP_CRASH_FILES as u64 + 5) {
            fs::write(dir.join(format!("crash-{ms}.json")), "{}").unwrap();
        }
        let report = CrashReport {
            ts_ms: 1000,
            version: "t",
            thread: "t".into(),
            location: "l".into(),
            message: "m".into(),
            backtrace: String::new(),
        };
        write_crash_file(&dir, &report).unwrap();
        assert_eq!(crash_files(&dir).len(), KEEP_CRASH_FILES);
        assert!(!dir.join("crash-1.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
