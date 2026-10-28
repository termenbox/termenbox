//! The server's logs: stderr as before, plus JSON Lines in `<config-dir>/logs/termenbox.jsonl` that outlive the
//! service manager's own retention (decision 0142). Rotated by size, pruned by age and total size. A field that can
//! hold a secret is replaced and one that can hold session text is left out (guardrail `audit-logging-no-secrets`).
//!
//! Logging never waits for the disk: a line is queued for a writer thread that owns the file, rotation and pruning. A
//! full queue drops the line and counts it; the writer then logs how many were lost. `flush` waits (briefly) for the
//! queue to reach the file, for the places a process is about to die.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

const CURRENT: &str = "termenbox.jsonl";
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
/// A longer string is cut: one runaway value must not fill the budget.
const MAX_VALUE_CHARS: usize = 2048;
/// Lines waiting for the writer. A line is a few hundred bytes, so a full queue is about a megabyte.
const QUEUE_LINES: usize = 2048;
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// Names that hold secret material: logged as present, never with their value.
pub const SECRET_NAMES: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "signature",
    "nonce",
    "private",
    "challenge",
    "authorization",
    "cookie",
    "session_key",
];
/// Names that hold session text: left out whole.
pub const PAYLOAD_NAMES: &[&str] = &[
    "text",
    "payload",
    "body",
    "content",
    "output",
    "prompt",
    "stdin",
    "keystrokes",
    "data",
];

fn is_secret(name: &str) -> bool {
    SECRET_NAMES.iter().any(|s| name.contains(s))
}

fn is_payload(name: &str) -> bool {
    PAYLOAD_NAMES.contains(&name)
}

/// The size-capped file the layer writes to.
pub struct Rotating {
    dir: PathBuf,
    file: File,
    size: u64,
    max_file: u64,
    max_total: u64,
    max_age: Duration,
    last_prune: Instant,
}

impl Rotating {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        Self::open_with(dir, MAX_FILE_BYTES, MAX_TOTAL_BYTES, MAX_AGE)
    }

    fn open_with(
        dir: &Path,
        max_file: u64,
        max_total: u64,
        max_age: Duration,
    ) -> std::io::Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let (file, size) = Self::open_current(dir)?;
        let mut rotating = Rotating {
            dir: dir.to_path_buf(),
            file,
            size,
            max_file,
            max_total,
            max_age,
            last_prune: Instant::now(),
        };
        rotating.prune();
        Ok(rotating)
    }

    fn open_current(dir: &Path) -> std::io::Result<(File, u64)> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join(CURRENT))?;
        let size = file.metadata()?.len();
        Ok((file, size))
    }

    pub fn write_line(&mut self, line: &[u8]) {
        if self.size > 0 && self.size + line.len() as u64 > self.max_file {
            self.rotate();
        } else if self.last_prune.elapsed() > PRUNE_EVERY {
            self.prune();
        }
        // A full disk or a removed directory costs the file copy of a line, never the server.
        if self.file.write_all(line).is_ok() {
            self.size += line.len() as u64;
        }
    }

    fn rotate(&mut self) {
        let _ = self.file.flush();
        let mut to = self
            .dir
            .join(format!("termenbox-{}.jsonl", crate::events::now_ms()));
        let mut n = 1;
        while to.exists() {
            to = self
                .dir
                .join(format!("termenbox-{}-{n}.jsonl", crate::events::now_ms()));
            n += 1;
        }
        if fs::rename(self.dir.join(CURRENT), &to).is_ok() {
            if let Ok((file, size)) = Self::open_current(&self.dir) {
                self.file = file;
                self.size = size;
            }
        }
        self.prune();
    }

    /// Removes rotated files older than the age cap, then the oldest ones until the total fits.
    fn prune(&mut self) {
        self.last_prune = Instant::now();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        let mut rotated: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == CURRENT || !name.starts_with("termenbox-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            rotated.push((meta.modified().unwrap_or(now), meta.len(), entry.path()));
        }
        rotated.sort();
        let mut total = self.size + rotated.iter().map(|r| r.1).sum::<u64>();
        for (modified, len, path) in rotated {
            let old = now
                .duration_since(modified)
                .map(|age| age > self.max_age)
                .unwrap_or(false);
            if (old || total > self.max_total) && fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

enum Msg {
    Line(Vec<u8>),
    /// Answered once every line queued before it is in the file.
    Flush(SyncSender<()>),
}

struct Sink {
    tx: SyncSender<Msg>,
    dropped: Arc<AtomicU64>,
}

/// Where the layer's lines go once the server run path has chosen a config directory. Empty before that, so a
/// one-shot command (a hook, `--print-pairing`) writes no file.
#[derive(Clone, Default)]
pub struct Handle(Arc<OnceLock<Sink>>);

impl Handle {
    pub fn enable(&self, rotating: Rotating) -> bool {
        self.enable_with(rotating, QUEUE_LINES)
    }

    fn enable_with(&self, rotating: Rotating, capacity: usize) -> bool {
        let (tx, rx) = sync_channel(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let lost = dropped.clone();
        if std::thread::Builder::new()
            .name("log-writer".into())
            .spawn(move || write_loop(rotating, rx, lost))
            .is_err()
        {
            return false;
        }
        self.0.set(Sink { tx, dropped }).is_ok()
    }

    /// Waits up to a second for what is queued to reach the file.
    pub fn flush(&self) {
        let Some(sink) = self.0.get() else { return };
        let (ack, done) = sync_channel(1);
        let mut msg = Msg::Flush(ack);
        let start = Instant::now();
        loop {
            match sink.tx.try_send(msg) {
                Ok(()) => break,
                Err(TrySendError::Full(m)) if start.elapsed() < FLUSH_WAIT => {
                    msg = m;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return,
            }
        }
        let _ = done.recv_timeout(FLUSH_WAIT.saturating_sub(start.elapsed()));
    }
}

fn write_loop(mut rotating: Rotating, rx: Receiver<Msg>, dropped: Arc<AtomicU64>) {
    let report = |rotating: &mut Rotating| {
        let lost = dropped.swap(0, Ordering::Relaxed);
        if lost > 0 {
            let line = serde_json::json!({
                "ts": rfc3339(crate::events::now_ms()),
                "level": "warn",
                "target": "logging",
                "fields": {"event": "log_lines_dropped", "count": lost},
            });
            rotating.write_line(format!("{line}\n").as_bytes());
        }
    };
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Line(bytes) => {
                rotating.write_line(&bytes);
                report(&mut rotating);
            }
            Msg::Flush(ack) => {
                report(&mut rotating);
                let _ = ack.send(());
            }
        }
    }
}

/// Waits for the queued lines to reach the file: for a panic hook and a clean exit, before the process goes.
pub fn flush() {
    if let Some(handle) = HANDLE.get() {
        handle.flush();
    }
}

pub struct JsonlLayer(Handle);

#[derive(Default)]
struct Fields(Map<String, Value>);

impl Fields {
    fn put(&mut self, field: &Field, value: Value) {
        let name = field.name();
        let value = if is_secret(name) {
            Value::from("[redacted]")
        } else if is_payload(name) {
            Value::from("[omitted]")
        } else {
            match value {
                Value::String(s) if s.chars().count() > MAX_VALUE_CHARS => Value::from(format!(
                    "{}…",
                    s.chars().take(MAX_VALUE_CHARS).collect::<String>()
                )),
                other => other,
            }
        };
        self.0.insert(name.to_string(), value);
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::from(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::from(format!("{value:?}")));
    }
}

impl<S: Subscriber> Layer<S> for JsonlLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let Some(sink) = self.0 .0.get() else { return };
        let mut fields = Fields::default();
        event.record(&mut fields);
        let meta = event.metadata();
        let line = serde_json::json!({
            "ts": rfc3339(crate::events::now_ms()),
            "level": meta.level().as_str().to_lowercase(),
            "target": meta.target(),
            "fields": fields.0,
        });
        let mut bytes = line.to_string().into_bytes();
        bytes.push(b'\n');
        // Never waits: a full queue costs this line, counted and reported by the writer.
        if sink.tx.try_send(Msg::Line(bytes)).is_err() {
            sink.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `2026-10-28T11:51:09.638Z` for a Unix time in milliseconds.
fn rfc3339(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms % 1000
    )
}

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Starts logging to stderr and, once `enable_file` runs, to the file. `TERMENBOX_LOG` (an `EnvFilter` string) wins
/// over `RUST_LOG`; the default is `info`.
pub fn init() {
    let filter = || {
        EnvFilter::try_from_env("TERMENBOX_LOG")
            .or_else(|_| EnvFilter::try_from_default_env())
            .unwrap_or_else(|_| EnvFilter::new("info"))
    };
    let handle = Handle::default();
    let _ = HANDLE.set(handle.clone());
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(filter()))
        .with(JsonlLayer(handle).with_filter(filter()))
        .init();
}

/// Turns the file on for the server run path. A directory that cannot be opened is a warning, not a stop.
pub fn enable_file(config_dir: &Path) {
    let dir = config_dir.join("logs");
    match Rotating::open(&dir) {
        Ok(rotating) => {
            if let Some(handle) = HANDLE.get() {
                handle.enable(rotating);
            }
        }
        Err(e) => eprintln!(
            "logging: cannot write {}: {e}; logging to stderr only",
            dir.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termenbox-logging-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn a_secret_and_session_text_never_reach_the_file() {
        let dir = temp_dir();
        let handle = Handle::default();
        handle.enable(Rotating::open(&dir).unwrap());
        let subscriber = tracing_subscriber::registry().with(JsonlLayer(handle.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                claim_secret = "hunter2-secret-value",
                auth_token = "tok-123",
                text = "private agent reply",
                output = "ls -la output",
                key_fingerprint = "SHA256:abc",
                "an event"
            );
        });
        handle.flush();
        let written = fs::read_to_string(dir.join(CURRENT)).unwrap();
        for leaked in [
            "hunter2-secret-value",
            "tok-123",
            "private agent reply",
            "ls -la output",
        ] {
            assert!(
                !written.contains(leaked),
                "{leaked} reached the log: {written}"
            );
        }
        let line: Value = serde_json::from_str(written.lines().next().unwrap()).unwrap();
        assert_eq!(line["fields"]["claim_secret"], "[redacted]");
        assert_eq!(line["fields"]["key_fingerprint"], "SHA256:abc");
        assert_eq!(line["fields"]["message"], "an event");
        assert_eq!(line["level"], "info");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_is_written_before_the_file_is_enabled() {
        let handle = Handle::default();
        let subscriber = tracing_subscriber::registry().with(JsonlLayer(handle));
        tracing::subscriber::with_default(subscriber, || tracing::info!("early"));
    }

    #[test]
    fn a_long_value_is_cut() {
        let dir = temp_dir();
        let handle = Handle::default();
        handle.enable(Rotating::open(&dir).unwrap());
        let subscriber = tracing_subscriber::registry().with(JsonlLayer(handle.clone()));
        let long = "x".repeat(MAX_VALUE_CHARS * 3);
        tracing::subscriber::with_default(subscriber, || tracing::info!(detail = %long));
        handle.flush();
        let written = fs::read_to_string(dir.join(CURRENT)).unwrap();
        assert!(written.len() < MAX_VALUE_CHARS * 2, "{}", written.len());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_full_queue_drops_and_counts_lines_without_waiting_and_flush_reaches_the_file() {
        let dir = temp_dir();
        let handle = Handle::default();
        assert!(handle.enable_with(Rotating::open(&dir).unwrap(), 1));
        let subscriber = tracing_subscriber::registry().with(JsonlLayer(handle.clone()));
        let start = Instant::now();
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..500 {
                tracing::info!(n = i, "burst");
            }
        });
        assert!(start.elapsed() < Duration::from_secs(1), "logging waited");
        handle.flush();
        let written = fs::read_to_string(dir.join(CURRENT)).unwrap();
        let kept = written.lines().filter(|l| l.contains("\"burst\"")).count() as u64;
        let lost: u64 = written
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v["fields"]["event"] == "log_lines_dropped")
            .map(|v| v["fields"]["count"].as_u64().unwrap())
            .sum();
        assert!(kept >= 1);
        assert_eq!(kept + lost, 500, "{kept} kept, {lost} reported lost");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_pruner_removes_the_oldest_files_past_the_cap() {
        let dir = temp_dir();
        let mut rotating = Rotating::open_with(&dir, 200, 1000, MAX_AGE).unwrap();
        let line = vec![b'a'; 99];
        for _ in 0..200 {
            rotating.write_line(&line);
        }
        let total: u64 = files(&dir)
            .iter()
            .map(|n| fs::metadata(dir.join(n)).unwrap().len())
            .sum();
        assert!(total <= 1000 + 200, "{total} bytes kept: {:?}", files(&dir));
        assert!(
            files(&dir).len() > 2,
            "rotation never happened: {:?}",
            files(&dir)
        );
        assert!(files(&dir).contains(&CURRENT.to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pruner_removes_files_older_than_the_age_cap() {
        let dir = temp_dir();
        let old = dir.join("termenbox-1.jsonl");
        fs::write(&old, "old\n").unwrap();
        let aged = SystemTime::now() - Duration::from_secs(31 * 24 * 3600);
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(aged)
            .unwrap();
        let fresh = dir.join("termenbox-2.jsonl");
        fs::write(&fresh, "fresh\n").unwrap();
        let _rotating = Rotating::open(&dir).unwrap();
        assert!(!old.exists());
        assert!(fresh.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
