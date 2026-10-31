//! `update-state.json`: what the updater knows, written by the helper (root or user), read by the server, never
//! written by the server (decision 0153). It replaces the all-or-nothing `update-status.json`, which is still written
//! beside it for one release so a new helper with an old server (or the reverse after a rollback) shows the notice.
//!
//! The file is only ever changed through `State::apply`, one event per outcome, so the transitions are testable
//! without a clock, a network or a service.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::requests::{now_secs, read_status, write_json};

pub const FILE: &str = "update-state.json";
const SCHEMA: u32 = 1;
const MAX_ERROR_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    #[default]
    Idle,
    Available,
    /// Reserved for the grace period (U5): a deadline is set.
    Scheduled,
    Installing,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Updated,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Last {
    pub result: Verdict,
    pub from: String,
    pub to: String,
    pub at: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub v: u32,
    pub phase: Phase,
    /// The last attempt to check, good or not; 0 when none was made.
    pub checked_at: u64,
    /// The last check that reached a verified manifest.
    pub checked_ok_at: u64,
    /// Why the last attempt failed; cleared by the next good check. A failure never erases the notice.
    pub error: Option<String>,
    pub available: Option<String>,
    pub first_seen_at: u64,
    pub deadline: u64,
    pub deferrals: u32,
    /// A version that is not offered or retried: it was rolled back, or the owner skipped it.
    pub skipped: Option<String>,
    pub last: Option<Last>,
    pub channel: String,
    pub mode: String,
    pub current: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            v: SCHEMA,
            phase: Phase::Idle,
            checked_at: 0,
            checked_ok_at: 0,
            error: None,
            available: None,
            first_seen_at: 0,
            deadline: 0,
            deferrals: 0,
            skipped: None,
            last: None,
            channel: String::new(),
            mode: String::new(),
            current: String::new(),
        }
    }
}

/// What happened, as the updater reports it.
pub enum Event<'a> {
    /// A verified manifest with nothing newer for this server.
    UpToDate,
    /// `mode = hold`: nothing was fetched.
    Held,
    /// A newer release this server could install.
    Available(&'a str),
    Installing,
    Updated {
        from: &'a str,
        to: &'a str,
    },
    RolledBack {
        to: &'a str,
        reason: &'a str,
    },
    /// The check or the install failed before anything was changed.
    Failed(&'a str),
}

impl State {
    /// Reads the file in the server's state directory; anything unreadable, oversize, linked or from another schema
    /// reads as a fresh state.
    pub fn load(dir: &Path) -> Self {
        read_status(&dir.join(FILE))
            .and_then(|text| serde_json::from_str::<State>(&text).ok())
            .filter(|s| s.v == SCHEMA)
            .unwrap_or_default()
    }

    pub fn store(&self, dir: &Path) -> std::io::Result<()> {
        write_json(&dir.join(FILE), self)
    }

    pub fn is_pinned(&self, version: &str) -> bool {
        self.skipped.as_deref() == Some(version)
    }

    pub fn apply(&mut self, now: u64, event: Event) {
        match event {
            Event::UpToDate => {
                self.checked_ok(now);
                self.clear_notice();
                self.skipped = None;
            }
            Event::Held => {
                self.error = None;
                self.clear_notice();
            }
            Event::Available(version) => {
                self.checked_ok(now);
                if self.available.as_deref() != Some(version) {
                    self.first_seen_at = now;
                    self.deadline = 0;
                    self.deferrals = 0;
                }
                self.available = Some(version.to_string());
                if !self.is_pinned(version) {
                    self.skipped = None;
                }
                self.phase = if self.is_pinned(version) {
                    Phase::RolledBack
                } else {
                    Phase::Available
                };
            }
            Event::Installing => self.phase = Phase::Installing,
            Event::Updated { from, to } => {
                self.checked_ok(now);
                self.clear_notice();
                self.skipped = None;
                self.last = Some(Last {
                    result: Verdict::Updated,
                    from: from.into(),
                    to: to.into(),
                    at: now,
                    reason: String::new(),
                });
            }
            Event::RolledBack { to, reason } => {
                self.checked_ok(now);
                self.skipped = Some(to.to_string());
                self.available = Some(to.to_string());
                self.phase = Phase::RolledBack;
                self.last = Some(Last {
                    result: Verdict::RolledBack,
                    from: self.current.clone(),
                    to: to.into(),
                    at: now,
                    reason: short(reason),
                });
            }
            Event::Failed(reason) => {
                self.checked_at = now;
                self.error = Some(short(reason));
                if self.phase == Phase::Installing {
                    self.phase = if self.available.is_some() {
                        Phase::Available
                    } else {
                        Phase::Idle
                    };
                }
            }
        }
    }

    fn checked_ok(&mut self, now: u64) {
        self.checked_at = now;
        self.checked_ok_at = now;
        self.error = None;
    }

    fn clear_notice(&mut self) {
        self.phase = Phase::Idle;
        self.available = None;
        self.first_seen_at = 0;
        self.deadline = 0;
        self.deferrals = 0;
    }
}

/// One line, bounded: the file is read by the server and shown in the app.
fn short(reason: &str) -> String {
    reason
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(MAX_ERROR_CHARS)
        .collect()
}

/// Records one event in the state directory, stamped with the policy and version it was made under. A directory the
/// helper cannot write to never stops an update.
pub fn record(dir: &Path, channel: &str, mode: &str, current: &str, event: Event) {
    let mut state = State::load(dir);
    state.channel = channel.into();
    state.mode = mode.into();
    state.current = current.into();
    state.apply(now_secs(), event);
    let _ = state.store(dir);
}

/// A failure that happened before there was a policy to stamp (a bad key, a missing port file): the state keeps what it
/// had and gains the reason.
pub fn record_failure(dir: &Path, reason: &str) {
    let mut state = State::load(dir);
    state.apply(now_secs(), Event::Failed(reason));
    let _ = state.store(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_check_keeps_the_notice_and_says_why() {
        let mut s = State::default();
        s.apply(100, Event::Available("0.2.0"));
        s.apply(200, Event::Failed("signature does not verify\nsecond line"));
        assert_eq!(s.available.as_deref(), Some("0.2.0"));
        assert_eq!(s.phase, Phase::Available);
        assert_eq!(s.error.as_deref(), Some("signature does not verify"));
        assert_eq!((s.checked_at, s.checked_ok_at), (200, 100));
        s.apply(300, Event::Available("0.2.0"));
        assert_eq!(s.error, None);
        assert_eq!(
            s.first_seen_at, 100,
            "the same release keeps its first sighting"
        );
        s.apply(400, Event::Available("0.3.0"));
        assert_eq!(s.first_seen_at, 400, "a newer release starts again");
    }

    #[test]
    fn a_rolled_back_version_is_pinned_until_a_newer_one_or_a_success() {
        let mut s = State {
            current: "0.1.0".into(),
            ..Default::default()
        };
        s.apply(1, Event::Available("0.2.0"));
        s.apply(2, Event::Installing);
        s.apply(
            3,
            Event::RolledBack {
                to: "0.2.0",
                reason: "unhealthy",
            },
        );
        assert!(s.is_pinned("0.2.0"));
        assert_eq!(s.phase, Phase::RolledBack);
        assert_eq!(s.last.as_ref().unwrap().result, Verdict::RolledBack);
        s.apply(4, Event::Available("0.2.0"));
        assert_eq!(
            s.phase,
            Phase::RolledBack,
            "seeing it again does not unpin it"
        );
        s.apply(5, Event::Available("0.2.1"));
        assert!(!s.is_pinned("0.2.0") && s.phase == Phase::Available);
        s.apply(
            6,
            Event::Updated {
                from: "0.1.0",
                to: "0.2.1",
            },
        );
        assert_eq!((s.phase, s.available.clone()), (Phase::Idle, None));
        assert_eq!(s.last.as_ref().unwrap().result, Verdict::Updated);
    }

    #[test]
    fn a_failed_install_goes_back_to_available_and_a_held_server_shows_nothing() {
        let mut s = State::default();
        s.apply(1, Event::Available("0.2.0"));
        s.apply(2, Event::Installing);
        s.apply(3, Event::Failed("does not run"));
        assert_eq!(s.phase, Phase::Available);
        s.apply(4, Event::Held);
        assert_eq!(
            (s.phase, s.available.clone(), s.error.clone()),
            (Phase::Idle, None, None)
        );
        assert_eq!(s.checked_ok_at, 1, "a held server checked nothing");
    }

    #[test]
    fn the_state_file_round_trips_and_junk_reads_as_fresh() {
        let dir = std::env::temp_dir().join(format!("tm-update-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(State::load(&dir), State::default());
        record(&dir, "beta", "notify", "0.1.0", Event::Available("0.2.0"));
        let s = State::load(&dir);
        assert_eq!(s.available.as_deref(), Some("0.2.0"));
        assert_eq!((s.channel.as_str(), s.mode.as_str()), ("beta", "notify"));
        // A key from a later release is ignored; another schema or junk is not trusted.
        std::fs::write(dir.join(FILE), r#"{"v":1,"phase":"idle","later":true}"#).unwrap();
        assert_eq!(State::load(&dir).phase, Phase::Idle);
        std::fs::write(dir.join(FILE), r#"{"v":2,"available":"9.9.9"}"#).unwrap();
        assert_eq!(State::load(&dir), State::default());
        std::fs::write(dir.join(FILE), "junk").unwrap();
        assert_eq!(State::load(&dir), State::default());
        std::fs::write(dir.join(FILE), " ".repeat(5000)).unwrap();
        assert_eq!(State::load(&dir), State::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
