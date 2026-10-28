//! Server health (U14, decision 0105): CPU, RAM and disk sampled every 10 s into an in-memory ring of 6 hours, and a
//! verdict (`healthy`) the server decides so no client has to.
//!
//! What is measured, against what:
//! - **RAM** is the working set of the container (cgroup v2 `memory.current` less inactive file cache, else cgroup v1)
//!   against its limit, so a 512 MB container reads 100% at 512 MB. Without a cgroup memory limit it is the machine's
//!   `MemTotal` against `MemAvailable`.
//! - **CPU** is the CPU time the cgroup used between two ticks over the wall time and the cores it may use
//!   (`cpu.max`; else every core). Without cgroup accounting it is the `/proc/stat` busy share between two ticks. The
//!   first sample has nothing to compare with and is unknown.
//! - **Disk** is `statvfs` of the data directory, the way `df` counts it, not the root of an overlay.
//! - **Round trip** is not here: it belongs to one client's path to the server, so each client measures its own from its
//!   heartbeat ping.
//!
//! A value that cannot be read is unknown (absent), never zero. Nothing is written to disk and nothing leaves the
//! process except through the messages of the authenticated owner: the ring is lost on restart by design.
//!
//! Verdict: a metric turns unhealthy when it stays at or above its `enter` level for [`CONFIRM`] samples in a row
//! (30 s) and healthy again when it stays at or below its `exit` level for [`CONFIRM`] samples, so one busy tick does
//! not flip a dot and a value hovering at a level does not flicker. The levels are [`LEVELS`].
//!
//! Cost: a tick reads six small files and one `statvfs` on a blocking thread, and wakes the process once per 10 s.

use std::collections::VecDeque;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use termenbox_core::protocol::{HealthLevel, HealthPoint, ServerMessage};
use tokio::sync::broadcast;
use tracing::warn;

/// Seconds between two samples.
pub const INTERVAL_S: u32 = 10;
/// Six hours of samples.
pub const RING_SIZE: usize = 6 * 60 * 60 / INTERVAL_S as usize;
/// The points one `health_history` answer carries, give or take the one short group at its start: one per minute over
/// the full ring.
pub const REPLY_MAX: usize = 360;
/// Samples in a row that must cross a level, either way, before the verdict changes.
pub const CONFIRM: u32 = 3;

/// A level pair in percent: unhealthy from `enter`, healthy again only at or below `exit`.
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub enter: u8,
    pub exit: u8,
}

/// The levels per metric. CPU 90% of the cores the container may use is the point where the guardrail's burst
/// allowance (100% for under 5 s) has become the steady state; RAM 90% of the limit is close to the OOM killer; disk
/// 90% is when a log or a checkpoint can fail to write.
#[derive(Clone, Copy, Debug)]
pub struct Levels {
    pub cpu: Level,
    pub ram: Level,
    pub disk: Level,
}

pub const LEVELS: Levels = Levels {
    cpu: Level {
        enter: 90,
        exit: 75,
    },
    ram: Level {
        enter: 90,
        exit: 80,
    },
    disk: Level {
        enter: 90,
        exit: 85,
    },
};

/// One metric's verdict with its hysteresis.
#[derive(Clone, Copy, Debug, Default)]
struct Latch {
    unhealthy: bool,
    run: u32,
}

impl Latch {
    /// An unknown value changes nothing: it is neither evidence of trouble nor of recovery.
    fn step(&mut self, value: Option<u8>, level: Level, confirm: u32) {
        let Some(v) = value else { return };
        let crossing = if self.unhealthy {
            v <= level.exit
        } else {
            v >= level.enter
        };
        if !crossing {
            self.run = 0;
            return;
        }
        self.run += 1;
        if self.run >= confirm {
            self.unhealthy = !self.unhealthy;
            self.run = 0;
        }
    }
}

/// The server's verdict at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    pub healthy: bool,
    /// Which metrics are over their level: `cpu`, `ram`, `disk`.
    pub unhealthy: Vec<String>,
    /// RAM is against a container limit rather than the machine.
    pub container: bool,
}

impl Verdict {
    fn scope(&self) -> String {
        if self.container { "container" } else { "host" }.into()
    }
}

/// A sample with the verdict it led to, as pushed to connections.
#[derive(Clone, Debug)]
pub struct Update {
    pub point: HealthPoint,
    pub verdict: Verdict,
}

impl Update {
    pub fn message(&self) -> ServerMessage {
        ServerMessage::ServerHealth {
            healthy: self.verdict.healthy,
            unhealthy: self.verdict.unhealthy.clone(),
            scope: self.verdict.scope(),
            point: self.point.clone(),
        }
    }
}

struct Inner {
    ring: VecDeque<HealthPoint>,
    /// Samples taken since the start: the ring's first sample is number `total - ring.len()`.
    total: u64,
    cpu: Latch,
    ram: Latch,
    disk: Latch,
    container: bool,
}

impl Inner {
    fn verdict(&self) -> Verdict {
        let unhealthy: Vec<String> = [("cpu", self.cpu), ("ram", self.ram), ("disk", self.disk)]
            .into_iter()
            .filter(|(_, l)| l.unhealthy)
            .map(|(m, _)| m.to_string())
            .collect();
        Verdict {
            healthy: unhealthy.is_empty(),
            unhealthy,
            container: self.container,
        }
    }
}

/// A loop that has not beaten for this long is stuck, and `/health` says so (decision 0143).
pub const STALL_MS: u64 = 30_000;

/// When the loops that must keep running last did: the health sampler and the ACP supervisor. A loop that has never
/// beaten is not running in this server (no ACP backend, say) and is not waited for. Plain atomics: a stuck lock
/// elsewhere cannot hold up a read.
pub struct Heartbeats {
    sampler_ms: AtomicU64,
    acp_ms: AtomicU64,
}

/// The server's own, read by `/health`.
pub static HEARTBEATS: Heartbeats = Heartbeats::new();

impl Heartbeats {
    pub const fn new() -> Self {
        Self {
            sampler_ms: AtomicU64::new(0),
            acp_ms: AtomicU64::new(0),
        }
    }

    pub fn beat_sampler(&self) {
        self.sampler_ms
            .store(crate::events::now_ms(), Ordering::Relaxed);
    }

    pub fn beat_acp(&self) {
        self.acp_ms
            .store(crate::events::now_ms(), Ordering::Relaxed);
    }

    /// Whether every loop that has started beat within [`STALL_MS`] of `now_ms`.
    pub fn alive_at(&self, now_ms: u64) -> bool {
        [&self.sampler_ms, &self.acp_ms].iter().all(|beat| {
            let last = beat.load(Ordering::Relaxed);
            last == 0 || now_ms.saturating_sub(last) <= STALL_MS
        })
    }

    /// The answer to `/health`: 200 `OK`, or 503 with no detail.
    pub async fn respond(&'static self) -> (axum::http::StatusCode, &'static str) {
        if self.alive_at(crate::events::now_ms()) {
            (axum::http::StatusCode::OK, "OK")
        } else {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, "stalled")
        }
    }
}

impl Default for Heartbeats {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps the ring and the verdict and tells the connections about each sample.
pub struct HealthCollector {
    inner: Mutex<Inner>,
    levels: Levels,
    confirm: u32,
    tx: broadcast::Sender<Update>,
}

impl Default for HealthCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthCollector {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            inner: Mutex::new(Inner {
                ring: VecDeque::with_capacity(RING_SIZE),
                total: 0,
                cpu: Latch::default(),
                ram: Latch::default(),
                disk: Latch::default(),
                container: false,
            }),
            levels: LEVELS,
            confirm: CONFIRM,
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Update> {
        self.tx.subscribe()
    }

    /// Takes one sample into the ring (dropping the oldest past six hours), updates the verdict and tells subscribers.
    pub fn record(&self, point: HealthPoint, container: bool) -> Update {
        let update = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.ring.len() == RING_SIZE {
                g.ring.pop_front();
            }
            g.ring.push_back(point.clone());
            g.total += 1;
            g.container = container;
            let (l, c) = (self.levels, self.confirm);
            g.cpu.step(point.cpu_percent, l.cpu, c);
            g.ram.step(point.ram_percent, l.ram, c);
            g.disk.step(point.disk_percent, l.disk, c);
            Update {
                point,
                verdict: g.verdict(),
            }
        };
        let _ = self.tx.send(update.clone()); // no subscriber is not an error
        update
    }

    /// The newest sample and the verdict, or nothing before the first tick.
    pub fn latest(&self) -> Option<Update> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.ring.back().map(|p| Update {
            point: p.clone(),
            verdict: g.verdict(),
        })
    }

    /// The answer to `health_history`: the ring cut to at most [`REPLY_MAX`] points.
    pub fn history(&self) -> ServerMessage {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let samples: Vec<HealthPoint> = g.ring.iter().cloned().collect();
        let first = g.total - samples.len() as u64;
        let (samples, group) = downsample_from(&samples, REPLY_MAX, first);
        let v = g.verdict();
        let l = self.levels;
        ServerMessage::HealthHistory {
            interval_s: INTERVAL_S,
            step_s: INTERVAL_S * group as u32,
            healthy: v.healthy,
            scope: v.scope(),
            unhealthy: v.unhealthy,
            confirm: self.confirm,
            levels: [("cpu", l.cpu), ("ram", l.ram), ("disk", l.disk)]
                .into_iter()
                .map(|(m, l)| HealthLevel {
                    metric: m.into(),
                    enter: l.enter,
                    exit: l.exit,
                })
                .collect(),
            samples,
        }
    }

    /// Samples every [`INTERVAL_S`] seconds for the life of the process. The reads run on a blocking thread so a slow
    /// filesystem cannot hold a runtime worker.
    pub fn spawn(self: &Arc<Self>, data_dir: PathBuf) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut sampler = Sampler::new(data_dir.clone());
            let mut tick = tokio::time::interval(Duration::from_secs(INTERVAL_S.into()));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                HEARTBEATS.beat_sampler();
                let (next, reading) = sample_blocking(sampler, &data_dir).await;
                sampler = next;
                // The run marker's heartbeat: when this run was last alive and how big it was (decision 0142).
                tokio::task::spawn_blocking(|| crate::crash::update(crate::crash::own_rss_kb()));
                if let Some(reading) = reading {
                    me.record(reading.point, reading.container);
                }
            }
        });
    }
}

/// One reading on a blocking thread. A sampler that panicked is lost with the thread: log it, start a new one (its first
/// CPU share is unknown, as after a start) and take no sample this tick. The gap shows in the ring's timestamps, and the
/// clients show the age of the newest sample.
async fn sample_blocking(sampler: Sampler, data_dir: &Path) -> (Sampler, Option<Reading>) {
    let joined = tokio::task::spawn_blocking(move || {
        let mut sampler = sampler;
        let reading = sampler.sample();
        (sampler, reading)
    })
    .await;
    match joined {
        Ok((sampler, reading)) => (sampler, Some(reading)),
        Err(e) => {
            warn!("health sampler failed, starting it over: {e}");
            (Sampler::new(data_dir.to_path_buf()), None)
        }
    }
}

/// Cuts `samples` to at most `max` points by taking the highest of each group of consecutive samples per metric, so a
/// spike survives; returns the points and the group size. Each point keeps the time of the newest sample in its group.
#[cfg(test)]
pub fn downsample(samples: &[HealthPoint], max: usize) -> (Vec<HealthPoint>, usize) {
    downsample_from(samples, max, 0)
}

/// As [`downsample`], with the groups cut at multiples of their size counted from the first sample ever taken
/// (`first` is the number of `samples[0]`), so as the ring slides one sample at a time a point keeps its group and a
/// spike does not wander between neighbours from one reply to the next. The first and last groups may be short, so
/// an answer can carry one point more than `max`.
pub fn downsample_from(
    samples: &[HealthPoint],
    max: usize,
    first: u64,
) -> (Vec<HealthPoint>, usize) {
    let max = max.max(1);
    let group = samples.len().div_ceil(max).max(1);
    let lead = ((group as u64 - first % group as u64) % group as u64) as usize;
    let (head, rest) = samples.split_at(lead.min(samples.len()));
    let hi = |a: Option<u8>, b: Option<u8>| match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let fold = |c: &[HealthPoint]| {
        c.iter().fold(
            HealthPoint {
                ts: c.last().map_or(0, |p| p.ts),
                ..HealthPoint::default()
            },
            |acc, p| HealthPoint {
                ts: acc.ts,
                cpu_percent: hi(acc.cpu_percent, p.cpu_percent),
                ram_percent: hi(acc.ram_percent, p.ram_percent),
                disk_percent: hi(acc.disk_percent, p.disk_percent),
                event_log_bytes: acc.event_log_bytes.max(p.event_log_bytes),
            },
        )
    };
    let out = std::iter::once(head)
        .filter(|h| !h.is_empty())
        .chain(rest.chunks(group))
        .map(fold)
        .collect();
    (out, group)
}

// ---- Reading the machine.

/// One reading of the CPU: time used, and what it is measured against.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CpuReading {
    /// CPU time used so far: cgroup microseconds, or `/proc/stat` busy jiffies.
    used: u64,
    /// Wall microseconds since the sampler started (cgroup), or total jiffies (`/proc/stat`).
    basis: u64,
    /// Cores the cgroup may use; absent for `/proc/stat`, whose basis already spans every core.
    cores: Option<f64>,
}

/// CPU use between two readings in whole percent of what was available; unknown when the readings are not comparable.
fn cpu_percent(prev: CpuReading, cur: CpuReading) -> Option<u8> {
    if prev.cores.is_some() != cur.cores.is_some() {
        return None;
    }
    let db = cur.basis.checked_sub(prev.basis).filter(|d| *d > 0)? as f64;
    let du = cur.used.checked_sub(prev.used)? as f64;
    Some(whole(du / (db * cur.cores.unwrap_or(1.0))))
}

/// What one tick measured.
pub struct Reading {
    pub point: HealthPoint,
    /// RAM is against a container limit.
    pub container: bool,
}

pub struct Sampler {
    cgroup: PathBuf,
    proc_dir: PathBuf,
    data_dir: PathBuf,
    start: Instant,
    prev: Option<CpuReading>,
    /// Tests: the next sample panics.
    #[cfg(test)]
    panic_next: bool,
}

impl Sampler {
    pub fn new(data_dir: PathBuf) -> Self {
        let proc_dir = PathBuf::from("/proc");
        let cgroup = own_cgroup(Path::new("/sys/fs/cgroup"), &proc_dir);
        Self::at(cgroup, proc_dir, data_dir)
    }

    fn at(cgroup: PathBuf, proc_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            cgroup,
            proc_dir,
            data_dir,
            start: Instant::now(),
            prev: None,
            #[cfg(test)]
            panic_next: false,
        }
    }

    pub fn sample(&mut self) -> Reading {
        let now_us = self.start.elapsed().as_micros() as u64;
        self.sample_at(now_us)
    }

    fn sample_at(&mut self, now_us: u64) -> Reading {
        #[cfg(test)]
        if std::mem::take(&mut self.panic_next) {
            panic!("a sampler that fails");
        }
        let cur = self.read_cpu(now_us);
        let cpu = match (self.prev, cur) {
            (Some(p), Some(c)) => cpu_percent(p, c),
            _ => None,
        };
        self.prev = cur;
        let (ram, container) = self.read_ram();
        Reading {
            point: HealthPoint {
                ts: crate::events::now_ms(),
                cpu_percent: cpu,
                ram_percent: ram,
                disk_percent: disk_percent(&self.data_dir),
                event_log_bytes: Some(crate::event_log::total_bytes()),
            },
            container,
        }
    }

    fn read(&self, base: &Path, rel: &str) -> Option<String> {
        std::fs::read_to_string(base.join(rel)).ok()
    }

    fn read_cpu(&self, now_us: u64) -> Option<CpuReading> {
        // cgroup v2, then v1, then the machine.
        if let Some(used) = self
            .read(&self.cgroup, "cpu.stat")
            .and_then(|s| stat_value(&s, "usage_usec"))
        {
            let cores = self
                .read(&self.cgroup, "cpu.max")
                .and_then(|s| parse_cpu_max(&s));
            return Some(CpuReading {
                used,
                basis: now_us,
                cores: Some(cores.unwrap_or_else(all_cores)),
            });
        }
        for dir in ["cpuacct", "cpu,cpuacct"] {
            let d = self.cgroup.join(dir);
            if let Some(ns) = self
                .read(&d, "cpuacct.usage")
                .and_then(|s| s.trim().parse::<u64>().ok())
            {
                let quota = ["cpu", "cpu,cpuacct"].iter().find_map(|c| {
                    let d = self.cgroup.join(c);
                    parse_cfs(
                        &self.read(&d, "cpu.cfs_quota_us")?,
                        &self.read(&d, "cpu.cfs_period_us")?,
                    )
                });
                return Some(CpuReading {
                    used: ns / 1000,
                    basis: now_us,
                    cores: Some(quota.unwrap_or_else(all_cores)),
                });
            }
        }
        let stat = self.read(&self.proc_dir, "stat")?;
        let f: Vec<u64> = stat
            .lines()
            .next()?
            .split_whitespace()
            .skip(1)
            .filter_map(|x| x.parse().ok())
            .collect();
        let total: u64 = f.iter().take(8).sum(); // user..steal; guest time is already inside user
        let idle = f.get(3)? + f.get(4).copied().unwrap_or(0); // idle + iowait
        Some(CpuReading {
            used: total.saturating_sub(idle),
            basis: total,
            cores: None,
        })
    }

    /// RAM in whole percent and whether it is against a container limit.
    fn read_ram(&self) -> (Option<u8>, bool) {
        let num = |base: &Path, rel: &str| self.read(base, rel)?.trim().parse::<u64>().ok();
        let v2 = || {
            let max = num(&self.cgroup, "memory.max")?; // "max" does not parse: no limit
            let cur = num(&self.cgroup, "memory.current")?;
            let cache = self
                .read(&self.cgroup, "memory.stat")
                .and_then(|s| stat_value(&s, "inactive_file"))
                .unwrap_or(0);
            Some((cur.saturating_sub(cache), max))
        };
        let v1 = || {
            let d = self.cgroup.join("memory");
            let max = num(&d, "memory.limit_in_bytes")?;
            if max >= 1 << 60 {
                return None; // the kernel's "unlimited"
            }
            let cur = num(&d, "memory.usage_in_bytes")?;
            let cache = self
                .read(&d, "memory.stat")
                .and_then(|s| stat_value(&s, "total_inactive_file"))
                .unwrap_or(0);
            Some((cur.saturating_sub(cache), max))
        };
        if let Some((used, max)) = v2().or_else(v1).filter(|(_, m)| *m > 0) {
            return (Some(percent(used, max)), true);
        }
        let info = self.read(&self.proc_dir, "meminfo").unwrap_or_default();
        let kb = |k: &str| {
            info.lines().find_map(|l| {
                l.strip_prefix(k)?
                    .trim()
                    .strip_suffix("kB")?
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
        };
        match (kb("MemTotal:"), kb("MemAvailable:")) {
            (Some(total), Some(avail)) if total > 0 => {
                (Some(percent(total.saturating_sub(avail), total)), false)
            }
            _ => (None, false),
        }
    }
}

/// A share in whole percent, cut down and not rounded: 89.6% is 89 and so under a 90% level, whatever the dot says.
/// The small addend keeps an exact 90% (0.9 * 100.0 is not exactly 90.0) from falling to 89.
/// The cgroup this process runs in, under the cgroup v2 mount `base`: `/proc/self/cgroup` names it (`0::/path`). On a
/// host where the mount shows the root, the root's files describe the whole machine, which is not what a service with its
/// own limits is measured against. A path that is missing, escapes the mount or holds no accounting leaves `base`.
fn own_cgroup(base: &Path, proc_dir: &Path) -> PathBuf {
    let own = std::fs::read_to_string(proc_dir.join("self/cgroup"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("0::").map(str::to_string))
        });
    let Some(rel) = own.as_deref().map(|p| p.trim().trim_start_matches('/')) else {
        return base.to_path_buf();
    };
    if rel.is_empty() || rel.split('/').any(|c| c == ".." || c == ".") {
        return base.to_path_buf();
    }
    let dir = base.join(rel);
    if dir.join("cpu.stat").is_file() {
        dir
    } else {
        base.to_path_buf()
    }
}

fn whole(share: f64) -> u8 {
    (share * 100.0 + 1e-6).floor().clamp(0.0, 100.0) as u8
}

fn percent(used: u64, total: u64) -> u8 {
    whole(used as f64 / total as f64)
}

fn all_cores() -> f64 {
    std::thread::available_parallelism().map_or(1.0, |n| n.get() as f64)
}

/// The value of `key` in a `key value` per line file such as `cpu.stat` or `memory.stat`.
fn stat_value(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let mut p = l.split_whitespace();
        (p.next()? == key).then(|| p.next()?.parse().ok())?
    })
}

/// Cores allowed by cgroup v2 `cpu.max` (`max 100000` or `50000 100000`); none when unlimited.
fn parse_cpu_max(text: &str) -> Option<f64> {
    let mut p = text.split_whitespace();
    let quota: f64 = p.next()?.parse().ok()?; // "max" does not parse: no limit
    let period: f64 = p.next()?.parse().ok()?;
    (quota > 0.0 && period > 0.0).then(|| quota / period)
}

/// Cores allowed by cgroup v1 `cpu.cfs_quota_us` / `cpu.cfs_period_us`; a quota of -1 is no limit.
fn parse_cfs(quota: &str, period: &str) -> Option<f64> {
    parse_cpu_max(&format!("{} {}", quota.trim(), period.trim()))
}

/// Used share of the filesystem holding `path`, counted as `df` does (used over used plus what a user may still take).
fn disk_percent(path: &Path) -> Option<u8> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` is a valid NUL-terminated path and `s` is a zeroed statvfs the call fills.
    let s = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        s
    };
    #[allow(clippy::unnecessary_cast)] // u32 on some targets
    let (blocks, free, avail) = (s.f_blocks as u64, s.f_bfree as u64, s.f_bavail as u64);
    let used = blocks.saturating_sub(free);
    let denom = used + avail;
    (denom > 0).then(|| percent(used, denom))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(ts: u64, cpu: Option<u8>, ram: Option<u8>, disk: Option<u8>) -> HealthPoint {
        HealthPoint {
            ts,
            cpu_percent: cpu,
            ram_percent: ram,
            disk_percent: disk,
            event_log_bytes: None,
        }
    }

    #[test]
    fn ring_holds_six_hours_and_drops_the_oldest() {
        let c = HealthCollector::new();
        for i in 0..(RING_SIZE as u64 + 500) {
            c.record(pt(i, Some(1), Some(1), Some(1)), false);
        }
        {
            let g = c.inner.lock().unwrap();
            assert_eq!(g.ring.len(), RING_SIZE);
            assert_eq!(g.ring.front().unwrap().ts, 500);
        }
        let ServerMessage::HealthHistory {
            samples, step_s, ..
        } = c.history()
        else {
            panic!("not a history")
        };
        // The reply is bounded and says what a point covers.
        assert!(samples.len() <= REPLY_MAX + 1);
        assert_eq!(step_s, INTERVAL_S * 6);
        assert_eq!(samples.last().unwrap().ts, RING_SIZE as u64 + 499);
    }

    #[test]
    fn a_short_ring_is_returned_whole() {
        let c = HealthCollector::new();
        for i in 0..5 {
            c.record(pt(i, Some(10), None, Some(30)), true);
        }
        let ServerMessage::HealthHistory {
            samples,
            step_s,
            scope,
            ..
        } = c.history()
        else {
            panic!()
        };
        assert_eq!((samples.len(), step_s), (5, INTERVAL_S));
        assert_eq!(scope, "container");
    }

    #[test]
    fn downsampling_keeps_a_spike() {
        let pts: Vec<_> = (0..10)
            .map(|i| pt(i, Some(if i == 3 { 99 } else { 5 }), None, Some(40)))
            .collect();
        let (out, group) = downsample(&pts, 5);
        assert_eq!((out.len(), group), (5, 2));
        assert_eq!(
            out[1].cpu_percent,
            Some(99),
            "the group with the spike keeps it"
        );
        assert_eq!(out[1].ts, 3, "and the time of its newest sample");
        assert_eq!(out[0].ram_percent, None, "unknown stays unknown");
    }

    #[test]
    fn a_point_keeps_its_group_as_the_ring_slides() {
        let c = HealthCollector::new();
        for i in 0..(RING_SIZE as u64 + 7) {
            c.record(pt(i, Some((i % 100) as u8), None, None), false);
        }
        let points = |c: &HealthCollector| match c.history() {
            ServerMessage::HealthHistory { samples, .. } => samples,
            _ => panic!("not a history"),
        };
        let before = points(&c);
        c.record(pt(RING_SIZE as u64 + 7, Some(1), None, None), false);
        let after = points(&c);
        // All but the first (it may have lost its oldest sample) and the last (it may have gained one) are the same.
        for p in &before[1..before.len() - 1] {
            assert!(after.contains(p), "{p:?} moved");
        }
    }

    fn feed(c: &HealthCollector, cpu: u8, n: usize) -> Update {
        let mut u = None;
        for _ in 0..n {
            u = Some(c.record(pt(1, Some(cpu), Some(10), Some(10)), false));
        }
        u.unwrap()
    }

    #[test]
    fn needs_consecutive_samples_to_turn_unhealthy() {
        let c = HealthCollector::new();
        assert!(
            feed(&c, 95, 2).verdict.healthy,
            "two busy ticks are not enough"
        );
        assert!(feed(&c, 10, 1).verdict.healthy);
        assert!(
            feed(&c, 95, 2).verdict.healthy,
            "the run was broken by a calm tick"
        );
        let u = feed(&c, 95, 1);
        assert!(!u.verdict.healthy);
        assert_eq!(u.verdict.unhealthy, vec!["cpu"]);
    }

    #[test]
    fn recovers_only_below_the_exit_level_and_not_on_one_tick() {
        let c = HealthCollector::new();
        assert!(!feed(&c, 95, 3).verdict.healthy);
        // Between exit (75) and enter (90): still unhealthy, no flicker.
        assert!(!feed(&c, 80, 10).verdict.healthy);
        assert!(
            !feed(&c, 70, 2).verdict.healthy,
            "two calm ticks are not enough"
        );
        assert!(
            !feed(&c, 80, 1).verdict.healthy,
            "a bounce above the exit level restarts the count"
        );
        assert!(!feed(&c, 70, 2).verdict.healthy);
        assert!(feed(&c, 70, 1).verdict.healthy);
        // A reading just under the enter level does not trip it again.
        assert!(feed(&c, 89, 10).verdict.healthy);
    }

    #[test]
    fn unknown_values_change_nothing_and_each_metric_is_named() {
        let c = HealthCollector::new();
        c.record(pt(1, Some(95), Some(95), None), false);
        c.record(pt(2, None, Some(95), None), false); // an unreadable CPU: neither evidence nor a break
        let u = c.record(pt(3, Some(95), Some(95), None), false);
        assert_eq!(
            u.verdict.unhealthy,
            vec!["ram"],
            "cpu has two readings so far, not three"
        );
        let u = c.record(pt(4, Some(95), Some(95), None), false);
        assert_eq!(u.verdict.unhealthy, vec!["cpu", "ram"]);
        assert_eq!(c.latest().unwrap().verdict, u.verdict);
    }

    #[test]
    fn every_record_reaches_subscribers() {
        let c = HealthCollector::new();
        let mut rx = c.subscribe();
        c.record(pt(7, Some(1), Some(2), Some(3)), false);
        let got = rx.try_recv().unwrap();
        assert_eq!(got.point.ts, 7);
        let ServerMessage::ServerHealth {
            healthy,
            scope,
            point,
            ..
        } = got.message()
        else {
            panic!()
        };
        assert!(healthy);
        assert_eq!(scope, "host");
        assert_eq!(point.ram_percent, Some(2));
    }

    #[test]
    fn cpu_share_is_a_difference_over_the_cores_allowed() {
        let a = CpuReading {
            used: 1_000_000,
            basis: 10_000_000,
            cores: Some(1.0),
        };
        let b = CpuReading {
            used: 1_500_000,
            basis: 20_000_000,
            cores: Some(1.0),
        };
        assert_eq!(cpu_percent(a, b), Some(5)); // 0.5 s of 10 s on one core
        let two = |r: CpuReading| CpuReading {
            cores: Some(2.0),
            ..r
        };
        assert_eq!(cpu_percent(two(a), two(b)), Some(2)); // 2.5% is cut down, not rounded
        assert_eq!(cpu_percent(b, b), None, "no time passed");
        let host = CpuReading { cores: None, ..b };
        assert_eq!(
            cpu_percent(a, host),
            None,
            "a source change is not comparable"
        );
    }

    #[test]
    fn parses_cgroup_limits() {
        assert_eq!(parse_cpu_max("max 100000\n"), None);
        assert_eq!(parse_cpu_max("50000 100000\n"), Some(0.5));
        assert_eq!(parse_cfs("-1\n", "100000\n"), None);
        assert_eq!(parse_cfs("200000\n", "100000\n"), Some(2.0));
        assert_eq!(
            stat_value("a 1\nusage_usec 42\nb 3\n", "usage_usec"),
            Some(42)
        );
        assert_eq!(stat_value("a 1\n", "usage_usec"), None);
    }

    fn fixture(name: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("tb-health-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (p, body) in files {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, body).unwrap();
        }
        (root.join("cg"), root.join("proc"))
    }

    #[test]
    fn a_share_is_cut_down_so_89_6_is_under_a_90_level() {
        assert_eq!(whole(0.896), 89);
        assert_eq!(whole(0.9), 90, "an exact 90% is not lost to float error");
        assert_eq!(whole(1.4), 100);
        assert_eq!(percent(896, 1000), 89);
        let mut l = Latch::default();
        for _ in 0..CONFIRM {
            l.step(Some(percent(896, 1000)), LEVELS.disk, CONFIRM);
        }
        assert!(!l.unhealthy);
    }

    #[test]
    fn the_cgroup_of_the_process_is_found_under_the_mount() {
        let (cg, pr) = fixture(
            "own",
            &[
                ("cg/cpu.stat", "usage_usec 9\n"),
                ("cg/system.slice/tb.service/cpu.stat", "usage_usec 1\n"),
                ("proc/self/cgroup", "0::/system.slice/tb.service\n"),
            ],
        );
        assert_eq!(own_cgroup(&cg, &pr), cg.join("system.slice/tb.service"));
        // The root, a path that is not there and a path that climbs out all leave the mount itself.
        for line in [
            "0::/\n",
            "0::/gone.service\n",
            "0::/../../etc\n",
            "1:cpu:/x\n",
            "",
        ] {
            std::fs::write(pr.join("self/cgroup"), line).unwrap();
            assert_eq!(own_cgroup(&cg, &pr), cg, "{line:?}");
        }
    }

    #[tokio::test]
    async fn a_sampler_that_panics_is_replaced_and_sampling_goes_on() {
        let data = std::env::temp_dir();
        let mut bad = Sampler::new(data.clone());
        bad.panic_next = true;
        let (next, reading) = sample_blocking(bad, &data).await;
        assert!(reading.is_none(), "no sample from the tick that failed");
        let (_, reading) = sample_blocking(next, &data).await;
        assert!(reading.is_some(), "the next tick samples again");
    }

    const MEMINFO: &str = "MemTotal: 8000000 kB\nMemFree: 100 kB\nMemAvailable: 2000000 kB\n";

    #[test]
    fn container_v2_reads_the_limit_not_the_host() {
        let (cg, pr) = fixture(
            "v2",
            &[
                ("cg/cpu.stat", "usage_usec 1000000\nuser_usec 1\n"),
                ("cg/cpu.max", "100000 100000\n"),
                ("cg/memory.max", "536870912\n"),
                ("cg/memory.current", "314572800\n"),
                ("cg/memory.stat", "anon 1\ninactive_file 52428800\n"),
                ("proc/meminfo", MEMINFO),
            ],
        );
        let mut s = Sampler::at(cg.clone(), pr, std::env::temp_dir());
        let first = s.sample_at(0);
        assert_eq!(
            first.point.cpu_percent, None,
            "the first CPU sample is unknown"
        );
        // (300 MB - 50 MB cache) of 512 MB (48.8%, cut down), not 75% of the 8 GB host.
        assert_eq!(first.point.ram_percent, Some(48));
        assert!(first.container);
        std::fs::write(cg.join("cpu.stat"), "usage_usec 6000000\n").unwrap();
        let second = s.sample_at(10_000_000);
        assert_eq!(
            second.point.cpu_percent,
            Some(50),
            "5 s of one allowed core in 10 s"
        );
        assert!(
            second.point.disk_percent.is_some(),
            "statvfs of the data directory"
        );
    }

    #[test]
    fn container_v1_reads_cpuacct_and_memory() {
        let (cg, pr) = fixture(
            "v1",
            &[
                ("cg/cpuacct/cpuacct.usage", "1000000000\n"),
                ("cg/cpu/cpu.cfs_quota_us", "50000\n"),
                ("cg/cpu/cpu.cfs_period_us", "100000\n"),
                ("cg/memory/memory.limit_in_bytes", "1000000\n"),
                ("cg/memory/memory.usage_in_bytes", "900000\n"),
                ("cg/memory/memory.stat", "total_inactive_file 100000\n"),
                ("proc/meminfo", MEMINFO),
            ],
        );
        let mut s = Sampler::at(cg.clone(), pr, std::env::temp_dir());
        let first = s.sample_at(0);
        assert_eq!(first.point.ram_percent, Some(80));
        std::fs::write(cg.join("cpuacct/cpuacct.usage"), "2000000000\n").unwrap(); // +1 s
        let second = s.sample_at(10_000_000);
        assert_eq!(
            second.point.cpu_percent,
            Some(20),
            "1 s of half a core in 10 s"
        );
    }

    #[test]
    fn without_cgroups_it_uses_the_machine() {
        let (cg, pr) = fixture(
            "host",
            &[
                (
                    "proc/stat",
                    "cpu 100 0 100 700 100 0 0 0 0 0\ncpu0 1 1 1 1\n",
                ),
                ("proc/meminfo", MEMINFO),
            ],
        );
        let mut s = Sampler::at(cg, pr.clone(), std::env::temp_dir());
        let first = s.sample_at(0);
        assert_eq!(
            (first.point.cpu_percent, first.point.ram_percent),
            (None, Some(75))
        );
        assert!(!first.container);
        std::fs::write(pr.join("stat"), "cpu 200 0 200 800 100 0 0 0 0 0\n").unwrap();
        // 200 busy of 300 total jiffies since the last tick: 66.7%, cut down.
        assert_eq!(s.sample_at(10_000_000).point.cpu_percent, Some(66));
    }

    #[test]
    fn an_unreadable_machine_gives_unknown_not_zero() {
        let mut s = Sampler::at(
            "/nonexistent/cg".into(),
            "/nonexistent/proc".into(),
            "/nonexistent/data".into(),
        );
        let r = s.sample_at(0);
        assert_eq!(
            (
                r.point.cpu_percent,
                r.point.ram_percent,
                r.point.disk_percent
            ),
            (None, None, None)
        );
    }

    async fn health_status(hb: &'static Heartbeats) -> axum::http::StatusCode {
        use tower::ServiceExt;
        let app = axum::Router::new().route("/health", axum::routing::get(move || hb.respond()));
        let req = axum::http::Request::get("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        app.oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn health_is_503_for_a_stalled_heartbeat_and_200_for_a_fresh_one() {
        let hb: &'static Heartbeats = Box::leak(Box::default());
        // Nothing has started: nothing to wait for.
        assert_eq!(health_status(hb).await, 200);
        hb.beat_sampler();
        hb.beat_acp();
        assert_eq!(health_status(hb).await, 200);
        // The ACP supervisor stopped beating 31 s ago while the sampler goes on.
        hb.acp_ms.store(
            crate::events::now_ms() - STALL_MS - 1_000,
            Ordering::Relaxed,
        );
        assert_eq!(health_status(hb).await, 503);
        hb.beat_acp();
        assert_eq!(health_status(hb).await, 200);
        // Either loop alone is enough to fail it.
        hb.sampler_ms.store(1, Ordering::Relaxed);
        assert_eq!(health_status(hb).await, 503);
    }

    #[test]
    fn a_beat_is_stale_only_after_thirty_seconds() {
        let hb = Heartbeats::new();
        hb.sampler_ms.store(100_000, Ordering::Relaxed);
        assert!(hb.alive_at(100_000 + STALL_MS));
        assert!(!hb.alive_at(100_000 + STALL_MS + 1));
    }
}
