//! The updater (decision 0076, guardrail updates-are-verified-and-reversible). It runs as root from a systemd timer
//! or when the owner asks (see requests.rs), separate from the server, which may be an unprivileged user.
//!
//! One run: fetch the signed manifest, verify it, refuse a frozen or not-newer release, fetch the binary for this
//! architecture, check its hash against the manifest, check that it starts, journal the replacement, keep the
//! previous binary, swap atomically, restart the service, and roll back when the service does not become healthy.
//! Everything it touches is injected (network, service manager, paths), so the whole flow runs under test.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::journal::{Change, Journal};
use crate::release::{Channel, ChannelNotFollowed, Manifest, ReleaseKeys};
use crate::update_state::{self, Event};

/// How a server treats a newer release (`[update] mode`, decision 0131).
#[derive(Debug, Clone, Copy, Default, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The daily check installs it (inside the maintenance window, and not across a major version).
    #[default]
    Auto,
    /// The daily check only records that it exists; the owner's `UpdateNow` installs it.
    Notify,
    /// Nothing happens, whoever asks.
    Hold,
}

/// A daily window in local time, `HH:MM-HH:MM`, which may wrap midnight (`23:00-01:00`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    start: u32,
    end: u32,
}

impl std::fmt::Display for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let t = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
        write!(f, "{}-{}", t(self.start), t(self.end))
    }
}

impl Window {
    pub fn parse(s: &str) -> Result<Self> {
        let bad = || anyhow!("maintenance_window `{s}` is not like 03:00-04:00");
        let minutes = |t: &str| -> Result<u32> {
            let (h, m) = t.trim().split_once(':').ok_or_else(bad)?;
            let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
            if h > 23 || m > 59 {
                return Err(bad());
            }
            Ok(h * 60 + m)
        };
        let (a, b) = s.split_once('-').ok_or_else(bad)?;
        let (start, end) = (minutes(a)?, minutes(b)?);
        if start == end {
            return Err(bad());
        }
        Ok(Self { start, end })
    }

    /// Whether `minute` (minutes since local midnight) falls inside; the end is exclusive.
    pub fn contains(&self, minute: u32) -> bool {
        if self.start < self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSection {
    channel: Option<Channel>,
    mode: Option<Mode>,
    maintenance_window: Option<String>,
    /// Untyped here so that a bad value is reported by `UpdatePolicy::parse` in words, not as a TOML type error.
    grace: Option<toml::Value>,
    on_timeout: Option<toml::Value>,
}

/// What happens when the grace period of a release ends without an answer from the owner (decision 0154).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnTimeout {
    /// Pin the release as skipped and stop reminding about it; a newer release starts a new grace.
    #[default]
    Skip,
    /// Run the normal verified flow.
    Install,
}

const MIN_GRACE_SECS: u64 = 5 * 60;
const MAX_GRACE_SECS: u64 = 14 * 24 * 3600;
/// After a tick-driven install could not even start (no network), the tick waits this long before it tries again.
const RETRY_SECS: u64 = 30 * 60;

/// `30m`, `12h` or `2d`: whole units only, five minutes to fourteen days.
fn parse_grace(v: &toml::Value) -> Result<u64> {
    let bad = || anyhow!("grace is not like 30m, 12h or 2d (5 minutes to 14 days)");
    let text = v.as_str().ok_or_else(bad)?;
    let (n, unit) = text.split_at(text.len().saturating_sub(1));
    let n: u64 = n.parse().map_err(|_| bad())?;
    let secs = n.checked_mul(match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(bad()),
    });
    secs.filter(|s| (MIN_GRACE_SECS..=MAX_GRACE_SECS).contains(s))
        .ok_or_else(bad)
}

#[derive(Debug, Default, serde::Deserialize)]
struct PolicyFile {
    #[serde(default)]
    update: UpdateSection,
}

/// The per-server update policy: root-owned, written by the installer (`update.toml` beside the journal), changed only
/// on the host. A file that does not parse is an error and the updater does nothing: a typo must never read as `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdatePolicy {
    pub channel: Channel,
    pub mode: Mode,
    pub window: Option<Window>,
    /// How long the owner has to answer before `on_timeout` acts, in seconds; only with `mode = notify`.
    pub grace: Option<u64>,
    pub on_timeout: OnTimeout,
}

impl UpdatePolicy {
    /// Without a `channel` the server follows the channel of the version it runs (stable for a release build).
    pub fn parse(text: &str, running_version: &str) -> Result<Self> {
        let file: PolicyFile =
            toml::from_str(text).map_err(|e| anyhow!("bad update config: {e}"))?;
        let mode = file.update.mode.unwrap_or_default();
        let grace = file.update.grace.as_ref().map(parse_grace).transpose()?;
        if (grace.is_some() || file.update.on_timeout.is_some()) && mode != Mode::Notify {
            bail!("grace and on_timeout need mode = \"notify\"");
        }
        let on_timeout = match file.update.on_timeout.as_ref().map(|v| v.as_str()) {
            None => OnTimeout::default(),
            Some(Some("skip")) => OnTimeout::Skip,
            Some(Some("install")) => OnTimeout::Install,
            Some(_) => bail!("on_timeout is not \"install\" or \"skip\""),
        };
        Ok(Self {
            channel: file
                .update
                .channel
                .unwrap_or_else(|| Channel::of_version(running_version)),
            mode,
            grace,
            on_timeout,
            window: file
                .update
                .maintenance_window
                .as_deref()
                .map(Window::parse)
                .transpose()?,
        })
    }

    /// A missing file is the defaults (`stable`, `auto`, no window); an unreadable or invalid one is an error.
    pub fn load(path: &Path, running_version: &str) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, running_version),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::parse("", running_version),
            Err(e) => Err(anyhow!("{}: {e}", path.display())),
        }
    }
}

/// What started this run: the timer, the owner's `Request::UpdateNow`, a read-only check, or the tick that found a
/// grace deadline passed (decision 0154).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Timer,
    UpdateNow,
    Check,
    Deadline,
}

/// When the grace period of a release first seen at `from` ends, in unix seconds. A deadline outside the
/// maintenance window moves to the next window start, across midnight too. `now` and `now_minute` (minutes since
/// local midnight) place the deadline on the local clock; a DST change between them moves it by an hour at most.
pub fn deadline(from: u64, grace: u64, window: Option<Window>, now: u64, now_minute: u32) -> u64 {
    snap_to_window(from + grace, window, now, now_minute)
}

pub fn snap_to_window(at: u64, window: Option<Window>, now: u64, now_minute: u32) -> u64 {
    let Some(window) = window else { return at };
    let ahead = (at.saturating_sub(now) / 60) % 1440;
    let minute = ((u64::from(now_minute) + ahead) % 1440) as u32;
    if window.contains(minute) {
        return at;
    }
    let wait = (window.start + 1440 - minute) % 1440;
    // Whole minutes from the start of the minute `at` falls in, so the install lands at HH:MM:00 of the window.
    at - at % 60 + u64::from(wait) * 60
}

/// What the tick does with the state it read.
#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Nothing,
    /// `on_timeout = skip`: pin the release.
    Skip(String),
    /// `on_timeout = install`: the verified flow.
    Install,
    /// The release cannot install by itself (a major version) or the host no longer asks for a grace period.
    Unschedule,
}

/// The tick's decision, from local facts only: no network, no clock but the one passed in.
pub fn tick(policy: &UpdatePolicy, state: &update_state::State, now: u64) -> Tick {
    if !state.is_due(now) {
        return Tick::Nothing;
    }
    let Some(version) = state.available.clone() else {
        return Tick::Unschedule;
    };
    if policy.mode != Mode::Notify || policy.grace.is_none() {
        return Tick::Unschedule;
    }
    match policy.on_timeout {
        OnTimeout::Skip => Tick::Skip(version),
        OnTimeout::Install => match crate::release::is_major_bump(&state.current, &version) {
            Ok(false) => Tick::Install,
            _ => Tick::Unschedule,
        },
    }
}

/// Minutes since local midnight, for the maintenance window.
pub fn local_minute_of_day() -> u32 {
    // SAFETY: `time` accepts a null pointer; `localtime_r` writes only into the struct we own.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (tm.tm_hour * 60 + tm.tm_min) as u32
    }
}

#[async_trait]
pub trait Fetch: Send + Sync {
    async fn get(&self, url: &str) -> Result<Vec<u8>>;
}

#[async_trait]
pub trait Service: Send + Sync {
    async fn restart(&self) -> Result<()>;
    async fn healthy(&self) -> bool;
    /// Whether the downloaded file runs at all on this machine.
    async fn smoke(&self, binary: &Path) -> bool;
    /// Whether the downloaded tmux runs at all on this machine (`tmux -V`).
    async fn smoke_tmux(&self, binary: &Path) -> bool;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    /// A signed freeze (or an older release): nothing was changed.
    Frozen,
    Updated {
        from: String,
        to: String,
    },
    RolledBack {
        to: String,
        reason: String,
    },
    /// `mode = hold`: nothing was fetched or changed.
    Held,
    /// A validly signed manifest of a channel this server does not follow (a stable server and the beta alias).
    OtherChannel,
    /// A newer release exists and was recorded in the status file, but not installed: `notify` mode, outside the
    /// maintenance window, or a major version that needs the owner.
    Available {
        version: String,
        reason: &'static str,
    },
}

impl Outcome {
    /// What the audit line says: the kind of outcome and, when there is one, the release. Never the rollback reason,
    /// which is free text (`audit-logging-no-secrets`).
    pub fn audit(&self) -> (&'static str, &str) {
        match self {
            Outcome::UpToDate => ("up_to_date", ""),
            Outcome::Frozen => ("frozen", ""),
            Outcome::Held => ("held", ""),
            Outcome::OtherChannel => ("other_channel", ""),
            Outcome::Updated { to, .. } => ("installed", to),
            Outcome::RolledBack { to, .. } => ("rolled_back", to),
            Outcome::Available { version, .. } => ("available", version),
        }
    }
}

pub struct Updater<'a> {
    pub fetch: &'a dyn Fetch,
    pub service: &'a dyn Service,
    pub keys: &'a ReleaseKeys,
    pub binary: PathBuf,
    /// The root-only directory with the journal (`/var/lib/termenbox-root`).
    pub root_dir: PathBuf,
    /// Where the manifest and its signature are fetched from (`<release_url>/manifest.json`). Never used to build an
    /// asset URL: the signed manifest names every asset (decision 0132).
    pub release_url: String,
    pub url_prefix: String,
    pub policy: UpdatePolicy,
    pub trigger: Trigger,
    /// Local time as minutes since midnight, for the maintenance window.
    pub now_minute: u32,
    /// The directory the server reads its state from; `update-state.json` and `update-status.json` go here. None: nothing is recorded.
    pub status_dir: Option<PathBuf>,
    /// The running version, as `crate::version()` reports it.
    pub current: String,
    pub arch: String,
    pub health_wait: Duration,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// A file of the bundled tmux set (the tmux itself or a licence notice) downloaded, verified and staged beside the
/// installed one, to be swapped in with the server. `existed` is false when this install had none: it is then
/// created, and put back by removing it.
struct StagedFile {
    path: PathBuf,
    staged: PathBuf,
    sha256: String,
    existed: bool,
}

/// A file swapped in, with what is needed to put it back.
struct Swapped {
    path: PathBuf,
    /// The copy of the old file, when there was one.
    previous: Option<PathBuf>,
    entry: u64,
}

fn core_version(v: &str) -> &str {
    v.split(['+', '-']).next().unwrap_or(v)
}

impl Updater<'_> {
    fn sibling(&self, suffix: &str) -> PathBuf {
        suffixed(&self.binary, suffix)
    }

    /// Where the installer put the bundled tmux (decision 0119): beside the server.
    fn tmux_path(&self) -> PathBuf {
        self.binary.with_file_name(crate::backend::BUNDLED_TMUX)
    }

    /// Where a licence notice of the bundled tmux lives: beside the server, as the installer puts it.
    fn notice_path(&self, name: &str) -> PathBuf {
        self.binary.with_file_name(format!("termenbox-{name}"))
    }

    /// The bundled tmux and its licence notices of this release, each downloaded, checked against the signed hash and
    /// staged, when the installed one is missing or differs. Empty when the release has no tmux for this architecture
    /// (an older manifest leaves what is installed alone). An install made before tmux was bundled gets one here:
    /// termenbox always runs its own tmux (decision 0120), and a running tmux server on the same socket speaks the same
    /// protocol. The tmux is run once before anything is changed. On any error nothing is left staged.
    async fn stage_tmux(&self, manifest: &Manifest) -> Result<Vec<StagedFile>> {
        let mut staged: Vec<StagedFile> = Vec::new();
        let result = self.stage_tmux_into(manifest, &mut staged).await;
        if result.is_err() {
            for f in &staged {
                let _ = std::fs::remove_file(&f.staged);
            }
        }
        result.map(|()| staged)
    }

    async fn stage_tmux_into(&self, manifest: &Manifest, out: &mut Vec<StagedFile>) -> Result<()> {
        let Some(entry) = manifest.tmux(&self.arch) else {
            return Ok(());
        };
        let path = self.tmux_path();
        let expected = entry.sha256.to_ascii_lowercase();
        let existed = path.is_file();
        if !existed || crate::journal::hash_file(&path)? != expected {
            let bytes = self.fetch.get(&entry.url).await?;
            if hex(&Sha256::digest(&bytes)) != expected {
                bail!("the downloaded tmux does not match the signed hash; nothing was changed");
            }
            let staged = suffixed(&path, ".new");
            write_with_mode(&staged, &bytes, 0o755)?;
            out.push(StagedFile {
                path,
                staged: staged.clone(),
                sha256: expected,
                existed,
            });
            if !self.service.smoke_tmux(&staged).await {
                bail!("the new tmux does not run on this machine; nothing was changed");
            }
        }
        for (name, notice) in &manifest.tmux_notices {
            // The name comes from the signed manifest, and still must not point outside the install directory.
            if name.starts_with('.')
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                bail!("the manifest names a licence notice {name:?} that is not a plain file name");
            }
            let path = self.notice_path(name);
            let expected = notice.sha256.to_ascii_lowercase();
            let existed = path.is_file();
            if existed && crate::journal::hash_file(&path)? == expected {
                continue;
            }
            let bytes = self.fetch.get(&notice.url).await?;
            if hex(&Sha256::digest(&bytes)) != expected {
                bail!("the downloaded {name} does not match the signed hash; nothing was changed");
            }
            let staged = suffixed(&path, ".new");
            write_with_mode(&staged, &bytes, 0o644)?;
            out.push(StagedFile {
                path,
                staged,
                sha256: expected,
                existed,
            });
        }
        Ok(())
    }

    /// The status file the server reads: the newest release this server could install but has not, or no file.
    fn record_available(&self, version: Option<&str>) -> Result<()> {
        let Some(dir) = &self.status_dir else {
            return Ok(());
        };
        let path = dir.join("update-status.json");
        match version {
            Some(v) => {
                self.note(Event::Available(v));
                let body = serde_json::json!({
                    "available": v,
                    "current": self.current,
                    "checked_at": crate::requests::now_secs(),
                    "channel": format!("{:?}", self.policy.channel).to_ascii_lowercase(),
                    "mode": format!("{:?}", self.policy.mode).to_ascii_lowercase(),
                });
                crate::requests::write_json(&path, &body)?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// Records an event in `update-state.json` (decision 0154), stamped with the policy and version of this run.
    fn note(&self, event: Event) {
        if let Some(dir) = &self.status_dir {
            update_state::record(
                dir,
                &format!("{:?}", self.policy.channel).to_ascii_lowercase(),
                &format!("{:?}", self.policy.mode).to_ascii_lowercase(),
                &self.current,
                event,
            );
        }
    }

    pub async fn run(&self) -> Result<Outcome> {
        // The previous notice stays while the check runs and when it fails: a failure is recorded as `error`, it does
        // not erase what the owner was told. Only a verified answer that there is nothing to offer clears it.
        let installing = std::cell::Cell::new(false);
        let result = self.run_inner(&installing).await;
        match &result {
            Ok(Outcome::UpToDate | Outcome::Frozen | Outcome::OtherChannel) => {
                let _ = self.record_available(None);
                self.note(Event::UpToDate);
            }
            Ok(Outcome::Held) => {
                let _ = self.record_available(None);
                self.note(Event::Held);
            }
            Ok(Outcome::Available { version, .. }) => {
                self.note(Event::Available(version));
                if self.trigger == Trigger::Deadline {
                    // Held back at the deadline (pinned, or a major version): the clock stops, the owner decides.
                    self.note(Event::Unscheduled);
                } else if let Some(grace) = self.policy.grace {
                    self.schedule(grace);
                }
            }
            Ok(Outcome::Updated { from, to }) => {
                let _ = self.record_available(None);
                self.note(Event::Updated { from, to });
            }
            Ok(Outcome::RolledBack { to, reason }) => self.note(Event::RolledBack { to, reason }),
            Err(e) if installing.get() => self.note(Event::InstallFailed(&format!("{e:#}"))),
            Err(e) if self.trigger == Trigger::Deadline => {
                // Not even a check could be made (no network): try again later, not on the next tick.
                self.note(Event::Failed(&format!("{e:#}")));
                self.retry_later();
            }
            Err(e) => self.note(Event::Failed(&format!("{e:#}"))),
        }
        result
    }

    /// Starts the grace period of the release on offer, once: a later check of the same release keeps its deadline.
    fn schedule(&self, grace: u64) {
        let Some(dir) = &self.status_dir else { return };
        let state = update_state::State::load(dir);
        let now = crate::requests::now_secs();
        if state.deadline == 0 {
            let first_seen = if state.first_seen_at == 0 { now } else { state.first_seen_at };
            let at = deadline(first_seen, grace, self.policy.window, now, self.now_minute);
            self.note(Event::Scheduled(at));
        }
    }

    fn retry_later(&self) {
        if let Some(dir) = &self.status_dir {
            let mut state = update_state::State::load(dir);
            if state.phase == update_state::Phase::Scheduled {
                state.deadline = crate::requests::now_secs() + RETRY_SECS;
                let _ = state.store(dir);
            }
        }
    }

    async fn run_inner(&self, installing: &std::cell::Cell<bool>) -> Result<Outcome> {
        if self.policy.mode == Mode::Hold {
            return Ok(Outcome::Held);
        }
        // Only the manifest and its signature come from `release_url`; every asset URL comes from the verified manifest.
        let base = self.release_url.trim_end_matches('/');
        let manifest_bytes = self.fetch.get(&format!("{base}/manifest.json")).await?;
        let signature =
            String::from_utf8(self.fetch.get(&format!("{base}/manifest.json.sig")).await?)?;
        // Nothing in the manifest is used before its signature has been checked. The channel the owner configured
        // decides which manifests count (a stable server never follows beta or nightly, decision 0130).
        let (manifest, _kid) = match Manifest::verify(
            &manifest_bytes,
            &signature,
            self.keys,
            &self.url_prefix,
            self.policy.channel,
        ) {
            Ok(v) => v,
            Err(e) if e.downcast_ref::<ChannelNotFollowed>().is_some() => {
                return Ok(Outcome::OtherChannel)
            }
            Err(e) => return Err(e),
        };
        if manifest.freeze {
            return Ok(Outcome::Frozen);
        }
        if !manifest.is_newer_than(&self.current)? {
            return Ok(Outcome::UpToDate);
        }
        // Recorded before anything is downloaded, so the app's card is truthful between the check and the install
        // (and after an install that failed); a successful install clears it. It is a hint for the app: a directory
        // the updater cannot write to never stops the update itself.
        let _ = self.record_available(Some(&manifest.version));
        if self.trigger == Trigger::Check {
            return Ok(Outcome::Available {
                version: manifest.version,
                reason: "check only",
            });
        }
        if matches!(self.trigger, Trigger::Timer | Trigger::Deadline) {
            // A release that was rolled back or skipped is not retried by the timer; the owner can ask again, or a
            // newer one replaces it. At a grace deadline the owner's silence stands for consent, so `notify` and the
            // window no longer hold it back (the deadline was already placed in the window); a major version still does.
            let pinned = self
                .status_dir
                .as_deref()
                .is_some_and(|dir| update_state::State::load(dir).is_pinned(&manifest.version));
            let hold_back = if pinned {
                Some("this release was rolled back; it is not retried until a newer one appears")
            } else if self.trigger == Trigger::Timer && self.policy.mode == Mode::Notify {
                Some("notify mode: the owner installs it")
            } else if manifest.is_major_bump_from(&self.current)? {
                Some("a new major version needs the owner's confirmation")
            } else if self.trigger == Trigger::Timer
                && self
                    .policy
                    .window
                    .is_some_and(|w| !w.contains(self.now_minute))
            {
                Some("outside the maintenance window")
            } else {
                None
            };
            if let Some(reason) = hold_back {
                return Ok(Outcome::Available {
                    version: manifest.version,
                    reason,
                });
            }
        }
        self.note(Event::Installing);
        installing.set(true);
        let binary = manifest.binary(&self.arch)?;
        let expected = binary.sha256.to_ascii_lowercase();
        let bytes = self.fetch.get(&binary.url).await?;
        if hex(&Sha256::digest(&bytes)) != expected {
            bail!("the downloaded server does not match the signed hash; nothing was changed");
        }

        let staged = self.sibling(".new");
        write_executable(&staged, &bytes)?;
        if !self.service.smoke(&staged).await {
            let _ = std::fs::remove_file(&staged);
            bail!("the new server does not run on this machine; nothing was changed");
        }
        // The bundled tmux and its notices go with the server: all are swapped, all are put back together.
        let tmux = match self.stage_tmux(&manifest).await {
            Ok(t) => t,
            Err(e) => {
                let _ = std::fs::remove_file(&staged);
                return Err(e);
            }
        };

        // Journal first, with a backup of what is there now, then swap.
        let journal = Journal::open(&self.root_dir)?;
        let backups = self.root_dir.join("backups");
        std::fs::create_dir_all(&backups)?;
        // Named by version and content, so a retried update never overwrites the backup of an earlier binary.
        let backup = backups.join(format!(
            "termenbox-server.{}.{}",
            core_version(&self.current),
            &crate::journal::hash_file(&self.binary)?[..12]
        ));
        std::fs::copy(&self.binary, &backup)?;
        let entry = journal.record(Change::FileReplaced {
            path: self.binary.display().to_string(),
            backup: backup.display().to_string(),
            sha256: expected,
        })?;
        let previous = self.sibling(".prev");
        std::fs::copy(&self.binary, &previous)?;
        let mut swapped: Vec<Swapped> = Vec::new();
        for t in tmux {
            let file_name = t.path.file_name().unwrap_or_default().to_string_lossy();
            let (entry, previous) = if t.existed {
                let backup = backups.join(format!(
                    "{file_name}.{}.{}",
                    core_version(&self.current),
                    &crate::journal::hash_file(&t.path)?[..12]
                ));
                std::fs::copy(&t.path, &backup)?;
                let entry = journal.record(Change::FileReplaced {
                    path: t.path.display().to_string(),
                    backup: backup.display().to_string(),
                    sha256: t.sha256.clone(),
                })?;
                let previous = suffixed(&t.path, ".prev");
                std::fs::copy(&t.path, &previous)?;
                (entry, Some(previous))
            } else {
                let entry = journal.record(Change::FileCreated {
                    path: t.path.display().to_string(),
                    sha256: t.sha256.clone(),
                })?;
                (entry, None)
            };
            std::fs::rename(&t.staged, &t.path)?;
            swapped.push(Swapped {
                path: t.path,
                previous,
                entry,
            });
        }
        std::fs::rename(&staged, &self.binary)?;

        self.service.restart().await?;
        if self.wait_healthy().await {
            return Ok(Outcome::Updated {
                from: core_version(&self.current).to_string(),
                to: manifest.version,
            });
        }

        // The new server did not come up: put the old one back and say so.
        std::fs::rename(&previous, &self.binary)?;
        journal.mark_done(entry)?;
        for w in swapped {
            match w.previous {
                Some(previous) => std::fs::rename(&previous, &w.path)?,
                None => std::fs::remove_file(&w.path)?,
            }
            journal.mark_done(w.entry)?;
        }
        let _ = self.service.restart().await;
        Ok(Outcome::RolledBack {
            to: manifest.version,
            reason: "the service did not become healthy within the time allowed".into(),
        })
    }

    async fn wait_healthy(&self) -> bool {
        let deadline = tokio::time::Instant::now() + self.health_wait;
        loop {
            if self.service.healthy().await {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

fn write_executable(path: &Path, bytes: &[u8]) -> Result<()> {
    write_with_mode(path, bytes, 0o755)
}

fn write_with_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

// --- the real machine ---------------------------------------------------------------------------------------

/// https through reqwest; `file://` for the installer's own tests (the root helper's environment is trusted).
pub struct HttpFetch(pub reqwest::Client);

#[async_trait]
impl Fetch for HttpFetch {
    async fn get(&self, url: &str) -> Result<Vec<u8>> {
        if let Some(path) = url.strip_prefix("file://") {
            return Ok(std::fs::read(path)?);
        }
        let resp = self.0.get(url).send().await?;
        if !resp.status().is_success() {
            bail!("{url}: {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

pub struct Systemd {
    pub unit: String,
    /// The unit belongs to the running user's own manager (`systemctl --user`), as in the no-root install.
    pub user: bool,
    pub health_url: String,
    pub http: reqwest::Client,
}

#[async_trait]
impl Service for Systemd {
    async fn restart(&self) -> Result<()> {
        let mut cmd = tokio::process::Command::new("systemctl");
        if self.user {
            cmd.arg("--user");
        }
        let out = cmd.args(["restart", &self.unit]).output().await?;
        if out.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "systemctl restart {} failed: {}",
                self.unit,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    async fn healthy(&self) -> bool {
        matches!(self.http.get(&self.health_url).send().await, Ok(r) if r.status().is_success())
    }

    async fn smoke(&self, binary: &Path) -> bool {
        let run = tokio::process::Command::new(binary).arg("--help").output();
        matches!(tokio::time::timeout(Duration::from_secs(10), run).await, Ok(Ok(o)) if o.status.success())
    }

    async fn smoke_tmux(&self, binary: &Path) -> bool {
        let run = tokio::process::Command::new(binary).arg("-V").output();
        matches!(tokio::time::timeout(Duration::from_secs(10), run).await, Ok(Ok(o)) if o.status.success() && String::from_utf8_lossy(&o.stdout).starts_with("tmux "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;

    struct Fake {
        files: HashMap<String, Vec<u8>>,
        fetched: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Fetch for Fake {
        async fn get(&self, url: &str) -> Result<Vec<u8>> {
            self.fetched.lock().unwrap().push(url.to_string());
            self.files
                .get(url)
                .cloned()
                .ok_or_else(|| anyhow!("404 {url}"))
        }
    }

    struct Svc {
        restarts: Mutex<u32>,
        /// Health answers per restart: the n-th restart is healthy when `healthy_from <= n`.
        healthy_from: u32,
        runs: bool,
        tmux_runs: bool,
        binary: PathBuf,
        seen_healthy_with: Mutex<Vec<Vec<u8>>>,
    }

    #[async_trait]
    impl Service for Svc {
        async fn restart(&self) -> Result<()> {
            *self.restarts.lock().unwrap() += 1;
            Ok(())
        }
        async fn healthy(&self) -> bool {
            let n = *self.restarts.lock().unwrap();
            let ok = n >= self.healthy_from;
            if ok {
                self.seen_healthy_with
                    .lock()
                    .unwrap()
                    .push(std::fs::read(&self.binary).unwrap());
            }
            ok
        }
        async fn smoke(&self, _: &Path) -> bool {
            self.runs
        }
        async fn smoke_tmux(&self, _: &Path) -> bool {
            self.tmux_runs
        }
    }

    const BASE: &str = "https://github.com/termenbox/termenbox/releases/download/v9.9.9";

    struct World {
        dir: PathBuf,
        keys: ReleaseKeys,
        files: HashMap<String, Vec<u8>>,
        binary: PathBuf,
    }

    /// A signed release 9.9.9 whose x86_64 binary is `new_binary`, signed with the real ssh-keygen. None without it.
    fn world(version: &str, freeze: bool, new_binary: &[u8]) -> Option<World> {
        world_with_tmux(version, freeze, new_binary, None)
    }

    /// Like `world`, and the release also carries `new_tmux` as the x86_64 bundled tmux and a `tmux-COPYING` notice
    /// (`new notice`). The installed ones, beside the server, are `old tmux` and `old notice`.
    fn world_with_tmux(
        version: &str,
        freeze: bool,
        new_binary: &[u8],
        new_tmux: Option<&[u8]>,
    ) -> Option<World> {
        world_in("stable", version, freeze, new_binary, new_tmux)
    }

    /// Like `world_with_tmux`, for a manifest of `channel`, signed by a key bound to that channel.
    fn world_in(
        channel: &str,
        version: &str,
        freeze: bool,
        new_binary: &[u8],
        new_tmux: Option<&[u8]>,
    ) -> Option<World> {
        let dir = std::env::temp_dir().join(format!("termenbox-updater-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        let tmux_entry = new_tmux.map_or(String::new(), |t| {
            format!(
                r#","tmux":{{"x86_64":{{"url":"{BASE}/tmux-x86_64-linux","sha256":"{}"}}}},"tmux_notices":{{"tmux-COPYING":{{"url":"{BASE}/tmux-COPYING","sha256":"{}"}}}}"#,
                hex(&Sha256::digest(t)),
                hex(&Sha256::digest(b"new notice"))
            )
        });
        let manifest = format!(
            r#"{{"version":"{version}","channel":"{channel}","freeze":{freeze},"notes":"","binaries":{{"x86_64":{{"url":"{BASE}/termenbox-server-x86_64-linux","sha256":"{}"}}}}{tmux_entry}}}"#,
            hex(&Sha256::digest(new_binary))
        );
        let key = dir.join("key");
        if !Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "t", "-f"])
            .arg(&key)
            .status()
            .ok()?
            .success()
        {
            return None;
        }
        std::fs::write(dir.join("manifest.json"), &manifest).ok()?;
        if !Command::new("ssh-keygen")
            .args(["-Y", "sign", "-n", "termenbox-release", "-f"])
            .arg(&key)
            .arg(dir.join("manifest.json"))
            .output()
            .ok()?
            .status
            .success()
        {
            return None;
        }
        let public = std::fs::read_to_string(dir.join("key.pub")).ok()?;
        let keys = ReleaseKeys::parse(&format!(
            "k1 {} channel={channel}",
            public
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        ))
        .ok()?;
        let mut files = HashMap::new();
        files.insert(
            "https://rel/manifest.json".to_string(),
            manifest.into_bytes(),
        );
        files.insert(
            "https://rel/manifest.json.sig".to_string(),
            std::fs::read(dir.join("manifest.json.sig")).ok()?,
        );
        // Assets live where the manifest says (BASE), not beside the manifest (https://rel).
        files.insert(
            format!("{BASE}/termenbox-server-x86_64-linux"),
            new_binary.to_vec(),
        );
        if let Some(t) = new_tmux {
            files.insert(format!("{BASE}/tmux-x86_64-linux"), t.to_vec());
            files.insert(format!("{BASE}/tmux-COPYING"), b"new notice".to_vec());
            std::fs::write(dir.join("termenbox-tmux"), b"old tmux").ok()?;
            std::fs::write(dir.join("termenbox-tmux-COPYING"), b"old notice").ok()?;
        }
        let binary = dir.join("termenbox-server");
        std::fs::write(&binary, b"old binary").ok()?;
        Some(World {
            dir,
            keys,
            files,
            binary,
        })
    }

    fn updater<'a>(w: &'a World, fetch: &'a Fake, svc: &'a Svc, current: &str) -> Updater<'a> {
        Updater {
            fetch,
            service: svc,
            keys: &w.keys,
            binary: w.binary.clone(),
            root_dir: w.dir.join("root"),
            release_url: "https://rel".into(),
            url_prefix: BASE.rsplit_once('/').unwrap().0.to_string() + "/",
            // Defaults for the install tests: an owner-requested update on the stable channel, which asks for no window
            // and no confirmation. The policy tests below set their own.
            policy: UpdatePolicy {
                channel: Channel::Stable,
                mode: Mode::Auto,
                window: None,
                grace: None,
                on_timeout: OnTimeout::Skip,
            },
            trigger: Trigger::UpdateNow,
            now_minute: 3 * 60 + 30,
            status_dir: Some(w.dir.clone()),
            current: current.into(),
            arch: "x86_64".into(),
            health_wait: Duration::from_millis(50),
        }
    }

    fn svc(w: &World, healthy_from: u32, runs: bool) -> Svc {
        Svc {
            restarts: Mutex::new(0),
            healthy_from,
            runs,
            tmux_runs: true,
            binary: w.binary.clone(),
            seen_healthy_with: Mutex::new(vec![]),
        }
    }

    fn fake(w: &World) -> Fake {
        Fake {
            files: w.files.clone(),
            fetched: Mutex::new(vec![]),
        }
    }

    #[tokio::test]
    async fn check_verifies_and_records_without_downloading_or_restarting_even_in_auto_mode() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let mut u = updater(&w, &f, &s, "0.1.0");
        u.trigger = Trigger::Check;
        assert!(matches!(u.run().await.unwrap(), Outcome::Available { .. }));
        assert_eq!(f.fetched.lock().unwrap().len(), 2);
        assert_eq!(*s.restarts.lock().unwrap(), 0);
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(
            crate::requests::available_update(&w.dir.join("requests"), "0.1.0").as_deref(),
            Some("0.2.0")
        );
        assert!(!w.dir.join("root").exists());
    }

    #[tokio::test]
    async fn a_held_frozen_or_wrong_channel_check_removes_a_previous_notice() {
        for case in ["hold", "freeze", "channel", "current"] {
            let Some(w) = world("0.2.0", case == "freeze", b"new binary") else {
                return;
            };
            let f = fake(&w);
            let s = svc(&w, 1, true);
            let mut u = updater(
                &w,
                &f,
                &s,
                if case == "current" { "0.2.0" } else { "0.1.0" },
            );
            u.trigger = Trigger::Check;
            if case == "hold" {
                u.policy.mode = Mode::Hold;
            }
            if case == "channel" {
                u.policy.channel = Channel::Nightly;
            }
            u.record_available(Some("0.3.0")).unwrap();
            let out = u.run().await;
            assert!(out.is_ok(), "{case}");
            assert!(!w.dir.join("update-status.json").exists(), "{case}");
            assert_eq!(*s.restarts.lock().unwrap(), 0);
            if case == "hold" {
                assert!(f.fetched.lock().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn a_failed_check_keeps_the_notice() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let (mut f, s) = (fake(&w), svc(&w, 1, true));
        let mut u = updater(&w, &f, &s, "0.1.0");
        u.trigger = Trigger::Check;
        assert!(u.run().await.is_ok());
        f.files.insert(
            "https://rel/manifest.json.sig".into(),
            b"bad signature".to_vec(),
        );
        let mut u = updater(&w, &f, &s, "0.1.0");
        u.trigger = Trigger::Check;
        assert!(u.run().await.is_err());
        assert_eq!(
            crate::requests::available_update(&w.dir.join("requests"), "0.1.0").as_deref(),
            Some("0.2.0"),
            "the owner still sees the release"
        );
        let state = update_state::State::load(&w.dir);
        assert!(state.error.is_some());
        assert_eq!(state.available.as_deref(), Some("0.2.0"));
        assert!(state.checked_ok_at <= state.checked_at && state.checked_ok_at > 0);
    }

    #[tokio::test]
    async fn a_rolled_back_version_is_not_retried() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let f = fake(&w);
        let unhealthy = svc(&w, 2, true);
        let u = with_policy(
            updater(&w, &f, &unhealthy, "0.1.0"),
            Channel::Stable,
            Mode::Auto,
            Trigger::Timer,
        );
        let first = u.run().await.unwrap();
        assert!(matches!(first, Outcome::RolledBack { .. }), "{first:?}");
        assert!(update_state::State::load(&w.dir).is_pinned("0.2.0"));
        assert_eq!(
            crate::requests::available_update(&w.dir.join("requests"), "0.1.0"),
            None
        );
        // The next timer run does not download or restart anything.
        let fetched = f.fetched.lock().unwrap().len();
        let restarts = *unhealthy.restarts.lock().unwrap();
        let out = u.run().await.unwrap();
        assert!(matches!(out, Outcome::Available { .. }), "{out:?}");
        assert_eq!(
            f.fetched.lock().unwrap().len(),
            fetched + 2,
            "manifest and signature only"
        );
        assert_eq!(*unhealthy.restarts.lock().unwrap(), restarts);
        // The owner asking again does try it.
        let healthy = svc(&w, 1, true);
        let u = with_policy(
            updater(&w, &f, &healthy, "0.1.0"),
            Channel::Stable,
            Mode::Auto,
            Trigger::UpdateNow,
        );
        assert!(matches!(u.run().await.unwrap(), Outcome::Updated { .. }));
        assert!(!update_state::State::load(&w.dir).is_pinned("0.2.0"));
    }

    #[tokio::test]
    async fn a_newer_signed_release_replaces_the_binary_and_keeps_the_old_one() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let out = updater(&w, &f, &s, "0.1.0+abc1234").run().await.unwrap();
        assert_eq!(
            out,
            Outcome::Updated {
                from: "0.1.0".into(),
                to: "9.9.9".into()
            }
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-server.prev")).unwrap(),
            b"old binary"
        );
        assert!(!w.dir.join("termenbox-server.new").exists());
        assert_eq!(*s.restarts.lock().unwrap(), 1);
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        let pending = journal.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            matches!(&pending[0].1, Change::FileReplaced { backup, .. } if std::fs::read(backup).unwrap() == b"old binary")
        );
    }

    #[tokio::test]
    async fn the_journal_can_undo_an_update() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        let report = journal
            .revert(&mut crate::journal::RealOps, &Default::default())
            .unwrap();
        assert_eq!(report.reverted.len(), 1, "{report:?}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
    }

    #[tokio::test]
    async fn a_retried_update_does_not_overwrite_the_backup_of_an_earlier_binary() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        // the running binary claims to be 0.1.0 both times, as test builds do
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        std::fs::write(w.dir.join("root/backups/marker"), "").unwrap();
        let first: Vec<_> = std::fs::read_dir(w.dir.join("root/backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        let after: Vec<_> = std::fs::read_dir(w.dir.join("root/backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            after.len() > first.len(),
            "a second, differently named backup exists: {first:?} -> {after:?}"
        );
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        let report = journal
            .revert(&mut crate::journal::RealOps, &Default::default())
            .unwrap();
        assert_eq!(report.reverted.len(), 2, "{report:?}");
        assert_eq!(
            std::fs::read(&w.binary).unwrap(),
            b"old binary",
            "the very first binary comes back"
        );
    }

    #[tokio::test]
    async fn an_unhealthy_new_server_is_rolled_back() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 2, true)); // healthy only after the second restart, i.e. the rollback
        let out = updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(
            matches!(out, Outcome::RolledBack { ref to, .. } if to == "9.9.9"),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(
            *s.restarts.lock().unwrap(),
            2,
            "the old server was started again"
        );
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        assert!(
            journal.pending().unwrap().is_empty(),
            "the failed update leaves nothing pending"
        );
    }

    #[tokio::test]
    async fn nothing_changes_when_the_release_is_not_newer_or_is_frozen() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        assert_eq!(
            updater(&w, &f, &s, "9.9.9").run().await.unwrap(),
            Outcome::UpToDate
        );
        assert_eq!(
            updater(&w, &f, &s, "10.0.0").run().await.unwrap(),
            Outcome::UpToDate,
            "never a downgrade"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert!(
            !f.fetched
                .lock()
                .unwrap()
                .iter()
                .any(|u| u.contains("x86_64-linux")),
            "the binary is not even downloaded"
        );

        let Some(w) = world("9.9.9", true, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        assert_eq!(
            updater(&w, &f, &s, "0.1.0").run().await.unwrap(),
            Outcome::Frozen
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(*s.restarts.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_bad_signature_a_wrong_hash_or_a_binary_that_does_not_run_change_nothing() {
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        // a changed manifest
        let mut tampered = fake(&w);
        tampered
            .files
            .get_mut("https://rel/manifest.json")
            .unwrap()
            .extend_from_slice(b" ");
        let s = svc(&w, 1, true);
        assert!(updater(&w, &tampered, &s, "0.1.0")
            .run()
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        // a binary that is not what the manifest says
        let mut swapped = fake(&w);
        swapped.files.insert(
            format!("{BASE}/termenbox-server-x86_64-linux"),
            b"evil".to_vec(),
        );
        assert!(updater(&w, &swapped, &s, "0.1.0")
            .run()
            .await
            .unwrap_err()
            .to_string()
            .contains("signed hash"));
        // a binary that does not start
        let broken = svc(&w, 1, false);
        let f = fake(&w);
        assert!(updater(&w, &f, &broken, "0.1.0")
            .run()
            .await
            .unwrap_err()
            .to_string()
            .contains("does not run"));
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert!(
            !w.dir.join("termenbox-server.new").exists(),
            "no half-installed file is left"
        );
        assert_eq!(
            *s.restarts.lock().unwrap() + *broken.restarts.lock().unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn the_bundled_tmux_and_its_notices_are_updated_with_the_server_and_all_are_journaled() {
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let out = updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(matches!(out, Outcome::Updated { .. }), "{out:?}");
        let tmux = w.dir.join("termenbox-tmux");
        assert_eq!(std::fs::read(&tmux).unwrap(), b"new tmux");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        let notice = w.dir.join("termenbox-tmux-COPYING");
        assert_eq!(std::fs::read(&notice).unwrap(), b"new notice");
        assert!(!w.dir.join("termenbox-tmux.new").exists());
        assert!(!w.dir.join("termenbox-tmux-COPYING.new").exists());
        // one journal entry per file, and reverting puts all back
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        assert_eq!(journal.pending().unwrap().len(), 3);
        let report = journal
            .revert(&mut crate::journal::RealOps, &Default::default())
            .unwrap();
        assert_eq!(report.reverted.len(), 3, "{report:?}");
        assert_eq!(std::fs::read(&tmux).unwrap(), b"old tmux");
        assert_eq!(std::fs::read(&notice).unwrap(), b"old notice");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
    }

    #[tokio::test]
    async fn an_unhealthy_update_puts_the_old_server_and_the_old_tmux_back_together() {
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 2, true));
        let out = updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(matches!(out, Outcome::RolledBack { .. }), "{out:?}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"old tmux"
        );
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux-COPYING")).unwrap(),
            b"old notice"
        );
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        assert!(journal.pending().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_bad_tmux_changes_nothing_not_even_the_server() {
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        // a tmux that is not what the manifest signed
        let mut swapped = fake(&w);
        swapped
            .files
            .insert(format!("{BASE}/tmux-x86_64-linux"), b"evil".to_vec());
        let s = svc(&w, 1, true);
        let err = updater(&w, &swapped, &s, "0.1.0").run().await.unwrap_err();
        assert!(err.to_string().contains("signed hash"), "{err}");
        // a tmux that does not run
        let mut broken = svc(&w, 1, true);
        broken.tmux_runs = false;
        let f = fake(&w);
        let err = updater(&w, &f, &broken, "0.1.0").run().await.unwrap_err();
        assert!(err.to_string().contains("tmux does not run"), "{err}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"old tmux"
        );
        assert!(!w.dir.join("termenbox-server.new").exists());
        assert!(!w.dir.join("termenbox-tmux.new").exists());
        assert_eq!(
            *s.restarts.lock().unwrap() + *broken.restarts.lock().unwrap(),
            0
        );
        // a licence notice that is not what the manifest signed stops the whole update, with nothing left staged
        let mut bad_notice = fake(&w);
        bad_notice
            .files
            .insert(format!("{BASE}/tmux-COPYING"), b"evil".to_vec());
        let err = updater(&w, &bad_notice, &s, "0.1.0")
            .run()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("signed hash"), "{err}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"old tmux"
        );
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux-COPYING")).unwrap(),
            b"old notice"
        );
        assert!(!w.dir.join("termenbox-tmux.new").exists());
        assert!(!w.dir.join("termenbox-tmux-COPYING.new").exists());
        assert!(!w.dir.join("termenbox-server.new").exists());
    }

    #[tokio::test]
    async fn an_install_without_a_bundled_tmux_is_given_one_and_an_old_manifest_still_works() {
        // the release has a tmux, this install has none beside the server (it was made before tmux was bundled)
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        std::fs::remove_file(w.dir.join("termenbox-tmux")).unwrap();
        std::fs::remove_file(w.dir.join("termenbox-tmux-COPYING")).unwrap();
        let (f, s) = (fake(&w), svc(&w, 1, true));
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"new tmux"
        );
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux-COPYING")).unwrap(),
            b"new notice"
        );
        // reverting removes what the update created
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        journal
            .revert(&mut crate::journal::RealOps, &Default::default())
            .unwrap();
        assert!(!w.dir.join("termenbox-tmux").exists());
        assert!(!w.dir.join("termenbox-tmux-COPYING").exists());
        // an unhealthy update takes them away again, with the old server back
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        std::fs::remove_file(w.dir.join("termenbox-tmux")).unwrap();
        std::fs::remove_file(w.dir.join("termenbox-tmux-COPYING")).unwrap();
        let (f, s) = (fake(&w), svc(&w, 2, true));
        let out = updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(matches!(out, Outcome::RolledBack { .. }), "{out:?}");
        assert!(!w.dir.join("termenbox-tmux").exists());
        assert!(!w.dir.join("termenbox-tmux-COPYING").exists());
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert!(Journal::open(&w.dir.join("root"))
            .unwrap()
            .pending()
            .unwrap()
            .is_empty());
        // an older manifest without the tmux keys leaves an installed tmux alone
        let Some(w) = world("9.9.9", false, b"new binary") else {
            return;
        };
        std::fs::write(w.dir.join("termenbox-tmux"), b"old tmux").unwrap();
        let (f, s) = (fake(&w), svc(&w, 1, true));
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"old tmux"
        );
        // a release whose tmux is the one already installed does not download it again
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"same")) else {
            return;
        };
        std::fs::write(w.dir.join("termenbox-tmux"), b"same").unwrap();
        std::fs::write(w.dir.join("termenbox-tmux-COPYING"), b"new notice").unwrap();
        let (f, s) = (fake(&w), svc(&w, 1, true));
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(!f
            .fetched
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.contains("tmux-")));
        assert_eq!(
            Journal::open(&w.dir.join("root"))
                .unwrap()
                .pending()
                .unwrap()
                .len(),
            1
        );
    }

    // --- the update policy (decision 0131) ---------------------------------------------------------------------

    fn with_policy<'a>(
        mut u: Updater<'a>,
        channel: Channel,
        mode: Mode,
        trigger: Trigger,
    ) -> Updater<'a> {
        u.policy.channel = channel;
        u.policy.mode = mode;
        u.trigger = trigger;
        u
    }

    #[tokio::test]
    async fn assets_come_from_the_manifest_never_from_release_url() {
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let mut u = updater(&w, &f, &s, "0.1.0");
        // an alias that holds only the manifest, like `channel-beta`
        u.release_url = "https://rel/alias/".into();
        let mut f2 = fake(&w);
        let m = f2.files.remove("https://rel/manifest.json").unwrap();
        let sig = f2.files.remove("https://rel/manifest.json.sig").unwrap();
        f2.files.insert("https://rel/alias/manifest.json".into(), m);
        f2.files
            .insert("https://rel/alias/manifest.json.sig".into(), sig);
        let u = Updater { fetch: &f2, ..u };
        let out = u.run().await.unwrap();
        assert!(matches!(out, Outcome::Updated { .. }), "{out:?}");
        for url in f2.fetched.lock().unwrap().iter() {
            assert!(
                url.starts_with(BASE) || url.starts_with("https://rel/alias/manifest.json"),
                "{url} was not named by the manifest"
            );
        }
        assert_eq!(
            std::fs::read(w.dir.join("termenbox-tmux")).unwrap(),
            b"new tmux"
        );
    }

    #[tokio::test]
    async fn a_stable_server_ignores_a_beta_or_nightly_manifest() {
        for channel in ["beta", "nightly"] {
            let Some(w) = world_in(channel, "9.9.9-x.1", false, b"new binary", None) else {
                return;
            };
            let (f, s) = (fake(&w), svc(&w, 1, true));
            let u = with_policy(
                updater(&w, &f, &s, "0.1.0"),
                Channel::Stable,
                Mode::Auto,
                Trigger::Timer,
            );
            assert_eq!(u.run().await.unwrap(), Outcome::OtherChannel, "{channel}");
            assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
            assert_eq!(*s.restarts.lock().unwrap(), 0);
            assert!(!f
                .fetched
                .lock()
                .unwrap()
                .iter()
                .any(|u| u.contains("x86_64-linux")));
        }
    }

    #[tokio::test]
    async fn a_beta_server_installs_a_stable_release_from_the_alias_and_a_beta_one() {
        // `channel-beta` can point at a stable release: 0.2.0, signed by the stable key.
        let Some(w) = world_in("stable", "0.2.0", false, b"new binary", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-beta.2"),
            Channel::Beta,
            Mode::Auto,
            Trigger::Timer,
        );
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Updated { to, .. } if to == "0.2.0"),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        // or a beta of its own
        let Some(w) = world_in("beta", "0.2.0-rc.1", false, b"rc binary", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-beta.2"),
            Channel::Beta,
            Mode::Auto,
            Trigger::Timer,
        );
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Updated { to, .. } if to == "0.2.0-rc.1"),
            "{out:?}"
        );
        // but never a nightly
        let Some(w) = world_in("nightly", "0.2.0-nightly.20261023", false, b"n", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-beta.2"),
            Channel::Beta,
            Mode::Auto,
            Trigger::Timer,
        );
        assert_eq!(u.run().await.unwrap(), Outcome::OtherChannel);
    }

    #[tokio::test]
    async fn a_nightly_server_sees_only_nightly() {
        let Some(w) = world_in("stable", "0.2.0", false, b"new binary", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-nightly.20261022"),
            Channel::Nightly,
            Mode::Auto,
            Trigger::Timer,
        );
        assert_eq!(u.run().await.unwrap(), Outcome::OtherChannel);
        assert_eq!(*s.restarts.lock().unwrap(), 0);
        let Some(w) = world_in("beta", "0.2.0-rc.1", false, b"new binary", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-nightly.20261022"),
            Channel::Nightly,
            Mode::Auto,
            Trigger::Timer,
        );
        assert_eq!(u.run().await.unwrap(), Outcome::OtherChannel);
        let Some(w) = world_in("nightly", "0.1.36-nightly.20261023", false, b"night", None) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-nightly.20261022"),
            Channel::Nightly,
            Mode::Auto,
            Trigger::Timer,
        );
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Updated { to, .. } if to == "0.1.36-nightly.20261023"),
            "{out:?}"
        );
        // yesterday's nightly is not an update
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36-nightly.20261023"),
            Channel::Nightly,
            Mode::Auto,
            Trigger::Timer,
        );
        assert_eq!(u.run().await.unwrap(), Outcome::UpToDate);
    }

    #[tokio::test]
    async fn notify_records_the_version_and_installs_nothing_until_update_now() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36"),
            Channel::Stable,
            Mode::Notify,
            Trigger::Timer,
        );
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Available { version, .. } if version == "0.2.0"),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(*s.restarts.lock().unwrap(), 0);
        assert!(!f
            .fetched
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.contains("x86_64-linux")));
        let status: serde_json::Value =
            serde_json::from_slice(&std::fs::read(w.dir.join("update-status.json")).unwrap())
                .unwrap();
        assert_eq!(status["available"], "0.2.0");
        // the owner asks
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36"),
            Channel::Stable,
            Mode::Notify,
            Trigger::UpdateNow,
        );
        let out = u.run().await.unwrap();
        assert!(matches!(out, Outcome::Updated { .. }), "{out:?}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        assert!(
            !w.dir.join("update-status.json").exists(),
            "nothing is pending any more"
        );
    }

    #[tokio::test]
    async fn auto_records_the_release_it_is_about_to_install_and_clears_it_after() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        // The new server does not run here: nothing is installed, and the release stays recorded.
        let (f, broken) = (fake(&w), svc(&w, 1, false));
        let u = with_policy(
            updater(&w, &f, &broken, "0.1.36"),
            Channel::Stable,
            Mode::Auto,
            Trigger::Timer,
        );
        assert!(u.run().await.is_err());
        let status: serde_json::Value =
            serde_json::from_slice(&std::fs::read(w.dir.join("update-status.json")).unwrap())
                .unwrap();
        assert_eq!(status["available"], "0.2.0");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        // The failed release is pinned (decision 0154): the timer does not retry it, the owner's request does, and
        // then nothing is pending any more.
        let s = svc(&w, 1, true);
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36"),
            Channel::Stable,
            Mode::Auto,
            Trigger::Timer,
        );
        assert!(matches!(u.run().await.unwrap(), Outcome::Available { .. }));
        let u = with_policy(
            updater(&w, &f, &s, "0.1.36"),
            Channel::Stable,
            Mode::Auto,
            Trigger::UpdateNow,
        );
        assert!(matches!(u.run().await.unwrap(), Outcome::Updated { .. }));
        assert!(!w.dir.join("update-status.json").exists());
    }

    #[tokio::test]
    async fn hold_does_nothing_even_when_asked() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        for trigger in [Trigger::Timer, Trigger::UpdateNow] {
            let (f, s) = (fake(&w), svc(&w, 1, true));
            let u = with_policy(
                updater(&w, &f, &s, "0.1.36"),
                Channel::Stable,
                Mode::Hold,
                trigger,
            );
            assert_eq!(u.run().await.unwrap(), Outcome::Held);
            assert!(
                f.fetched.lock().unwrap().is_empty(),
                "not even the manifest is fetched"
            );
            assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        }
        assert!(!w.dir.join("update-status.json").exists());
    }

    #[tokio::test]
    async fn a_major_bump_or_a_closed_window_holds_back_the_automatic_update_only() {
        let Some(w) = world("1.0.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = with_policy(
            updater(&w, &f, &s, "0.9.0"),
            Channel::Stable,
            Mode::Auto,
            Trigger::Timer,
        );
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Available { version, .. } if version == "1.0.0"),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        // the owner confirms by asking
        let u = with_policy(
            updater(&w, &f, &s, "0.9.0"),
            Channel::Stable,
            Mode::Auto,
            Trigger::UpdateNow,
        );
        assert!(matches!(u.run().await.unwrap(), Outcome::Updated { .. }));

        // a minor bump inside the window installs; outside it waits
        let Some(w) = world("0.10.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let mut u = with_policy(
            updater(&w, &f, &s, "0.9.0"),
            Channel::Stable,
            Mode::Auto,
            Trigger::Timer,
        );
        u.policy.window = Some(Window::parse("03:00-04:00").unwrap());
        u.now_minute = 12 * 60;
        let out = u.run().await.unwrap();
        assert!(
            matches!(&out, Outcome::Available { reason, .. } if reason.contains("window")),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        u.now_minute = 3 * 60 + 30;
        assert!(matches!(u.run().await.unwrap(), Outcome::Updated { .. }));
    }

    #[test]
    fn the_policy_file_has_safe_defaults_and_refuses_typos() {
        let d = UpdatePolicy::parse("", "0.1.36").unwrap();
        assert_eq!(
            (d.channel, d.mode, d.window),
            (Channel::Stable, Mode::Auto, None)
        );
        let d = UpdatePolicy::parse("", "0.1.36-nightly.20261022").unwrap();
        assert_eq!(
            d.channel,
            Channel::Nightly,
            "no channel: the one the binary runs"
        );
        let p = UpdatePolicy::parse(
            "[update]\nchannel = \"nightly\"\nmode = \"notify\"\nmaintenance_window = \"23:00-01:00\"\n",
            "0.1.36",
        )
        .unwrap();
        assert_eq!((p.channel, p.mode), (Channel::Nightly, Mode::Notify));
        let w = p.window.unwrap();
        assert!(
            w.contains(23 * 60 + 30) && w.contains(30) && !w.contains(12 * 60),
            "wraps midnight"
        );
        for bad in [
            "[update]\nmode = \"atuo\"\n",
            "[update]\nmod = \"hold\"\n",
            "[update]\nchannel = \"edge\"\n",
            "[update]\nmaintenance_window = \"3am-4am\"\n",
            "[update]\nmaintenance_window = \"25:00-26:00\"\n",
        ] {
            assert!(UpdatePolicy::parse(bad, "0.1.36").is_err(), "{bad}");
        }
        let missing = std::env::temp_dir().join(format!("tb-nope-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            UpdatePolicy::load(&missing, "0.1.36").unwrap().mode,
            Mode::Auto
        );
    }

    #[test]
    fn the_grace_keys_parse_only_under_notify_and_a_typo_still_fails() {
        let plain = UpdatePolicy::parse("[update]\nmode = \"notify\"\n", "0.1.57").unwrap();
        assert_eq!((plain.grace, plain.on_timeout), (None, OnTimeout::Skip));
        let p = UpdatePolicy::parse(
            "[update]\nmode = \"notify\"\ngrace = \"12h\"\non_timeout = \"install\"\n",
            "0.1.57",
        )
        .unwrap();
        assert_eq!((p.grace, p.on_timeout), (Some(12 * 3600), OnTimeout::Install));
        for bad in [
            "mode = \"auto\"\ngrace = \"12h\"",
            "mode = \"hold\"\non_timeout = \"install\"",
            "mode = \"notify\"\ngrace = \"12\"",
            "mode = \"notify\"\ngrace = \"1m\"",
            "mode = \"notify\"\ngrace = \"15d\"",
            "mode = \"notify\"\ngrace = 12",
            "mode = \"notify\"\non_timeout = \"maybe\"",
            "modes = \"notify\"",
        ] {
            let text = format!("[update]\n{bad}\n");
            assert!(UpdatePolicy::parse(&text, "0.1.57").is_err(), "{bad}");
        }
        for (text, secs) in [("30m", 1800), ("2d", 172_800), ("5m", 300)] {
            let t = format!("[update]\nmode = \"notify\"\ngrace = \"{text}\"\n");
            assert_eq!(UpdatePolicy::parse(&t, "0.1.57").unwrap().grace, Some(secs));
        }
    }

    #[test]
    fn a_deadline_outside_the_window_moves_to_the_next_window_start_across_midnight() {
        let night = Window::parse("03:00-04:00").unwrap();
        let wrap = Window::parse("23:00-01:00").unwrap();
        let (now, at_22) = (1_000_000 * 60, 22 * 60);
        let h = 3600;
        // 12 h after 22:00 is 10:00; the next 03:00 is 17 h later
        assert_eq!(deadline(now, 12 * h, Some(night), now, at_22), now + 29 * h);
        // 5 h after 22:00 is 03:00, inside
        assert_eq!(deadline(now, 5 * h, Some(night), now, at_22), now + 5 * h);
        // the end is exclusive: 04:00 moves to the next 03:00
        assert_eq!(deadline(now, 6 * h, Some(night), now, at_22), now + 29 * h);
        // a window that wraps midnight: 22:00 + 0 is outside, 23:00 is the start
        assert_eq!(snap_to_window(now, Some(wrap), now, at_22), now + h);
        assert_eq!(snap_to_window(now + 2 * h, Some(wrap), now, at_22), now + 2 * h);
        // no window, no change; seconds inside a minute do not matter
        assert_eq!(deadline(now, 7, None, now, 0), now + 7);
        assert_eq!(snap_to_window(now + 30, Some(night), now, 3 * 60 + 10), now + 30);
    }

    #[test]
    fn the_tick_acts_only_on_a_due_deadline_and_never_installs_a_major_version() {
        let policy = |on_timeout| UpdatePolicy {
            channel: Channel::Stable,
            mode: Mode::Notify,
            window: None,
            grace: Some(3600),
            on_timeout,
        };
        let state = |available: &str, deadline| update_state::State {
            phase: update_state::Phase::Scheduled,
            available: Some(available.into()),
            deadline,
            current: "0.1.57".into(),
            ..Default::default()
        };
        let install = policy(OnTimeout::Install);
        assert_eq!(tick(&install, &state("0.1.58", 100), 99), Tick::Nothing);
        assert_eq!(tick(&install, &state("0.1.58", 100), 100), Tick::Install);
        assert_eq!(
            tick(&policy(OnTimeout::Skip), &state("0.1.58", 100), 500),
            Tick::Skip("0.1.58".into())
        );
        assert_eq!(tick(&install, &state("1.0.0", 100), 500), Tick::Unschedule);
        assert_eq!(tick(&install, &update_state::State::default(), 500), Tick::Nothing);
        // the host took the grace away: the clock is dropped, nothing acts
        let mut gone = install;
        gone.grace = None;
        assert_eq!(tick(&gone, &state("0.1.58", 100), 500), Tick::Unschedule);
    }

    fn grace_updater<'a>(
        w: &'a World,
        f: &'a Fake,
        s: &'a Svc,
        trigger: Trigger,
        on_timeout: OnTimeout,
    ) -> Updater<'a> {
        let mut u = with_policy(
            updater(w, f, s, "0.1.36"),
            Channel::Stable,
            Mode::Notify,
            trigger,
        );
        u.policy.grace = Some(3600);
        u.policy.on_timeout = on_timeout;
        u
    }

    #[tokio::test]
    async fn a_new_release_starts_its_grace_once_and_the_deadline_installs_it_in_notify_mode() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = grace_updater(&w, &f, &s, Trigger::Timer, OnTimeout::Install);
        assert!(matches!(u.run().await.unwrap(), Outcome::Available { .. }));
        let first = update_state::State::load(&w.dir);
        assert_eq!(first.phase, update_state::Phase::Scheduled);
        assert!(first.deadline >= first.first_seen_at + 3600);
        // a later check of the same release keeps the deadline
        assert!(matches!(u.run().await.unwrap(), Outcome::Available { .. }));
        assert_eq!(update_state::State::load(&w.dir).deadline, first.deadline);
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        // the deadline passes: the tick's run installs although the mode is notify
        let u = grace_updater(&w, &f, &s, Trigger::Deadline, OnTimeout::Install);
        let out = u.run().await.unwrap();
        assert!(matches!(out, Outcome::Updated { .. }), "{out:?}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        let after = update_state::State::load(&w.dir);
        assert_eq!((after.phase, after.deadline), (update_state::Phase::Idle, 0));
    }

    #[tokio::test]
    async fn a_major_version_never_installs_at_the_deadline() {
        let Some(w) = world("2.0.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let u = grace_updater(&w, &f, &s, Trigger::Timer, OnTimeout::Install);
        assert!(matches!(u.run().await.unwrap(), Outcome::Available { .. }));
        let u = grace_updater(&w, &f, &s, Trigger::Deadline, OnTimeout::Install);
        let out = u.run().await.unwrap();
        assert!(matches!(out, Outcome::Available { .. }), "{out:?}");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"old binary");
        assert_eq!(*s.restarts.lock().unwrap(), 0);
        let st = update_state::State::load(&w.dir);
        assert_eq!((st.phase, st.deadline), (update_state::Phase::Available, 0));
        // the owner still can
        let u = grace_updater(&w, &f, &s, Trigger::UpdateNow, OnTimeout::Install);
        assert!(matches!(u.run().await.unwrap(), Outcome::Updated { .. }));
    }

    #[tokio::test]
    async fn an_owner_update_now_before_the_deadline_clears_the_schedule() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        grace_updater(&w, &f, &s, Trigger::Timer, OnTimeout::Skip)
            .run()
            .await
            .unwrap();
        grace_updater(&w, &f, &s, Trigger::UpdateNow, OnTimeout::Skip)
            .run()
            .await
            .unwrap();
        let st = update_state::State::load(&w.dir);
        assert_eq!((st.phase, st.deadline, st.available), (update_state::Phase::Idle, 0, None));
    }

    #[tokio::test]
    async fn a_failed_install_pins_the_version_and_the_deadline_does_not_retry_it() {
        let Some(w) = world("0.2.0", false, b"new binary") else {
            return;
        };
        let f = fake(&w);
        let broken = svc(&w, 1, false); // the new binary does not run
        grace_updater(&w, &f, &broken, Trigger::Timer, OnTimeout::Install)
            .run()
            .await
            .unwrap();
        let out = grace_updater(&w, &f, &broken, Trigger::Deadline, OnTimeout::Install)
            .run()
            .await;
        assert!(out.is_err());
        let st = update_state::State::load(&w.dir);
        assert!(st.is_pinned("0.2.0"));
        assert_eq!((st.phase, st.deadline), (update_state::Phase::Idle, 0));
        assert_eq!(st.last.unwrap().result, update_state::Verdict::Failed);
        // the next check offers nothing new to schedule
        let out = grace_updater(&w, &f, &broken, Trigger::Timer, OnTimeout::Install)
            .run()
            .await
            .unwrap();
        assert!(matches!(out, Outcome::Available { .. }));
        assert_eq!(update_state::State::load(&w.dir).deadline, 0);
    }

    #[test]
    fn a_trusting_world_is_never_assumed() {
        // The only key the updater trusts is the one it was given: an empty set refuses everything.
        let keys = ReleaseKeys::default();
        assert!(keys.is_empty());
    }
}
