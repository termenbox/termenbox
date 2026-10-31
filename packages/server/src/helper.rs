//! `termenbox-server --root-helper <update|requests>`: what the root-owned systemd units run (decisions 0076, 0077).
//! `update` runs the updater (daily timer). `requests` handles what the unprivileged server asked for.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use tracing::info;

use crate::release::{ReleaseKeys, URL_PREFIX};
use crate::requests::{take_requests, Request};
use crate::update_state::{self, State};
use crate::updater::{HttpFetch, Outcome, Systemd, Trigger, UpdatePolicy, Updater};

pub const DEFAULT_RELEASE_URL: &str =
    "https://github.com/termenbox/termenbox/releases/latest/download";

pub struct Setup {
    pub root_dir: PathBuf,
    pub requests_dir: PathBuf,
    /// The no-root install: the helper runs as the user who owns the service and drives their `systemctl --user`.
    pub user: bool,
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Exit status: 0 fine (also "nothing to do"), 1 error, 2 an update was rolled back.
///
/// Besides the two units' actions (`update`, `requests`) the owner at a shell can run `status` (read-only, any user who
/// can read the state), and `check`, `apply`, `postpone` and `skip` (the helper's own account: root, or the user of a
/// no-root install). `check` never installs, even in `auto` mode.
pub async fn run(action: &str, setup: &Setup, json: bool) -> Result<i32> {
    if action == "status" {
        return status(setup, json);
    }
    // A system install's helper changes system files, so only root may run it (tests set the override). The no-root
    // install's helper changes only the user's own files, and refuses to run as root so it cannot be mixed up with it.
    if setup.user {
        if is_root() {
            bail!("the user helper must not run as root");
        }
    } else if !is_root() && std::env::var_os("TERMENBOX_HELPER_ALLOW_NONROOT").is_none() {
        bail!("the root helper must run as root");
    }
    // Older user timer units omitted --requests-dir and inherited the system default.
    let setup = Setup {
        root_dir: setup.root_dir.clone(),
        requests_dir: requests_dir(setup),
        user: setup.user,
    };
    match action {
        "update" => update(&setup, Trigger::Timer, None).await,
        "check" => update(&setup, Trigger::Check, None).await,
        "apply" => update(&setup, Trigger::UpdateNow, None).await,
        "postpone" => handle(Request::UpdatePostpone {}, &setup).await,
        "skip" => {
            let available = setup
                .requests_dir
                .parent()
                .and_then(|dir| State::load(dir).available);
            match available {
                Some(version) => handle(Request::UpdateSkip { version }, &setup).await,
                None => {
                    println!("no release is on offer");
                    Ok(0)
                }
            }
        }
        "requests" => {
            let mut status = 0;
            for request in take_requests(&setup.requests_dir) {
                // A failed check must not discard an owner update already taken from the queue.
                match handle(request, &setup).await {
                    Ok(code) => status = status.max(code),
                    Err(_) => {
                        info!(target: "audit", event = "helper_request_failed");
                        status = status.max(1);
                    }
                }
            }
            Ok(status)
        }
        other => bail!(
            "unknown helper action `{other}` (update, requests, status, check, apply, postpone or skip)"
        ),
    }
}

async fn handle(request: Request, setup: &Setup) -> Result<i32> {
    match request {
        Request::UpdateNow { id } => {
            if let Some(id) = &id {
                let current = crate::requests::pending_update(&setup.requests_dir);
                if !current.is_some_and(|(pending, _)| pending == *id) {
                    // Never let a stale queued request overwrite the newest request's result.
                    info!(target: "audit", event = "update_request_expired");
                    return Ok(0);
                }
            }
            update(setup, Trigger::UpdateNow, id.as_deref()).await
        }
        Request::CheckUpdate {} => update(setup, Trigger::Check, None).await,
        Request::UpdatePostpone {} => {
            let done = change_state(setup, |state| state.postpone(0));
            info!(target: "audit", event = "update_postpone", done = done.is_ok());
            report(done, "postponed")
        }
        Request::UpdateSkip { version } => {
            let done = change_state(setup, |state| state.skip(&version));
            info!(target: "audit", event = "update_skip", done = done.is_ok());
            report(done, "skipped")
        }
        Request::Uninstall { .. } => {
            info!(target: "audit", event = "helper_uninstall_not_available");
            println!("uninstall through the helper is not available in this version");
            Ok(0)
        }
    }
}

/// Applies an owner's choice to `update-state.json`. The helper is the only writer of the file.
fn change_state(
    setup: &Setup,
    change: impl FnOnce(&mut State) -> std::result::Result<(), &'static str>,
) -> std::result::Result<(), &'static str> {
    let dir = setup.requests_dir.parent().ok_or("no state directory")?;
    let _lock = update_lock(&setup.root_dir).map_err(|_| "an update is running; try again")?;
    let mut state = State::load(dir);
    change(&mut state)?;
    state
        .store(dir)
        .map_err(|_| "the state could not be written")
}

fn report(done: std::result::Result<(), &'static str>, what: &str) -> Result<i32> {
    match done {
        Ok(()) => println!("{what}"),
        Err(why) => println!("not {what}: {why}"),
    }
    Ok(0)
}

/// `status`: what the updater knows, for a person or (`--json`) a script. Reads only; a policy the user cannot read is
/// said so rather than guessed.
fn status(setup: &Setup, json: bool) -> Result<i32> {
    let setup = Setup {
        root_dir: setup.root_dir.clone(),
        requests_dir: requests_dir(setup),
        user: setup.user,
    };
    let state = setup
        .requests_dir
        .parent()
        .map(State::load)
        .unwrap_or_default();
    let version = crate::version();
    let policy = UpdatePolicy::load(&setup.root_dir.join("update.toml"), &version);
    let unreleased = is_unreleased(&version);
    let ago = |at: u64| seconds_ago(at);
    if json {
        println!("{}", status_json(&version, &policy, &state));
        return Ok(0);
    }
    let hours = |at: u64| match ago(at) {
        None => "never".to_string(),
        Some(s) if s < 90 => "just now".to_string(),
        Some(s) if s < 5400 => format!("{} min ago", s / 60),
        Some(s) => format!("{} h ago", s / 3600),
    };
    println!("running:      {version}");
    if unreleased {
        println!("              an unreleased build: it is newer than the releases of its channel, so none is offered");
    }
    match &policy {
        Ok(p) => {
            println!(
                "policy:       channel {}, mode {}{}",
                format!("{:?}", p.channel).to_ascii_lowercase(),
                format!("{:?}", p.mode).to_ascii_lowercase(),
                p.window.map_or(String::new(), |w| format!(", window {w}"))
            );
        }
        Err(e) => println!("policy:       unreadable ({e})"),
    }
    println!(
        "last check:   {} (good: {})",
        hours(state.checked_at),
        hours(state.checked_ok_at)
    );
    if let Some(error) = &state.error {
        println!("              failed: {error}");
    }
    match (&state.available, state.phase) {
        (Some(v), update_state::Phase::RolledBack) => {
            println!("available:    {v}, rolled back before; not retried unless you apply it")
        }
        (Some(v), _) if state.is_pinned(v) => println!("available:    {v}, skipped"),
        (Some(v), _) => println!("available:    {v}"),
        (None, _) => println!("available:    nothing newer"),
    }
    if let Some(last) = &state.last {
        println!(
            "last result:  {:?} {} -> {}",
            last.result, last.from, last.to
        );
    }
    println!(
        "next check:   systemctl{} list-timers termenbox-update.timer",
        if setup.user { " --user" } else { "" }
    );
    Ok(0)
}

/// A build from source reports `+dev`; it compares newer than the prereleases of its own version (`0.1.57+dev` against
/// `0.1.57-beta.7`), so no release is offered. Said, not worked around.
fn is_unreleased(version: &str) -> bool {
    version.ends_with("+dev")
}

fn seconds_ago(at: u64) -> Option<u64> {
    (at > 0).then(|| crate::requests::now_secs().saturating_sub(at))
}

fn status_json(version: &str, policy: &Result<UpdatePolicy>, state: &State) -> serde_json::Value {
    let (channel, mode, window) = match policy {
        Ok(p) => (
            Some(format!("{:?}", p.channel).to_ascii_lowercase()),
            Some(format!("{:?}", p.mode).to_ascii_lowercase()),
            p.window.map(|w| w.to_string()),
        ),
        Err(_) => (None, None, None),
    };
    serde_json::json!({
        "version": version,
        "unreleased_build": is_unreleased(version),
        "channel": channel,
        "mode": mode,
        "maintenance_window": window,
        "policy_error": policy.as_ref().err().map(|e| e.to_string()),
        "state": state,
        "checked_seconds_ago": seconds_ago(state.checked_at),
        "checked_ok_seconds_ago": seconds_ago(state.checked_ok_at),
    })
}

fn requests_dir(setup: &Setup) -> PathBuf {
    if setup.user && setup.requests_dir == Path::new("/var/lib/termenbox/requests") {
        // The installer deliberately separates runtime state (~/.config) from the journal (~/.local/state).
        if let Some(home) = setup
            .root_dir
            .ancestors()
            .nth(3)
            .filter(|home| setup.root_dir == home.join(".local/state/termenbox"))
        {
            return home.join(".config/termenbox/requests");
        }
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| setup.root_dir.clone())
            .join(".config/termenbox/requests")
    } else {
        setup.requests_dir.clone()
    }
}

fn outcome_error(outcome: &Outcome) -> Option<&'static str> {
    match outcome {
        Outcome::Updated { .. } => None,
        Outcome::UpToDate => Some("No newer release is available."),
        Outcome::Held => Some("Updates are on hold in update.toml."),
        Outcome::Frozen => Some("Updates are frozen by the signed release manifest."),
        Outcome::OtherChannel => Some("The release is outside this server's update channel."),
        Outcome::RolledBack { .. } => {
            Some("The update failed its health check and was rolled back.")
        }
        Outcome::Available { .. } => Some("The release was checked but not installed."),
    }
}

async fn update(setup: &Setup, trigger: Trigger, id: Option<&str>) -> Result<i32> {
    let result = update_outcome(setup, trigger).await;
    if let Some(id) = id {
        let error = match &result {
            Ok(outcome) => outcome_error(outcome),
            Err(_) => {
                Some("The update could not finish. Check the helper service logs on this server.")
            }
        };
        if let Some(dir) = setup.requests_dir.parent() {
            crate::requests::write_json(
                &dir.join("update-result.json"),
                &crate::requests::UpdateResult {
                    id: id.to_string(),
                    error: error.map(str::to_string),
                },
            )?;
        }
    }
    let outcome = result?;
    info!(target: "audit", event = "update", outcome = ?outcome);
    match &outcome {
        Outcome::UpToDate => println!("already up to date"),
        Outcome::Frozen => println!("updates are paused by the maintainers (a signed freeze)"),
        Outcome::Held => println!("updates are on hold (update.toml: mode = \"hold\")"),
        Outcome::OtherChannel => {
            println!("the latest release is of a channel this server does not follow")
        }
        Outcome::Available { version, reason } => {
            println!("{version} is available and was not installed ({reason})")
        }
        Outcome::Updated { from, to } => println!("updated {from} -> {to}"),
        Outcome::RolledBack { to, reason } => {
            println!("update to {to} was rolled back: {reason}")
        }
    }
    Ok(if matches!(outcome, Outcome::RolledBack { .. }) {
        2
    } else {
        0
    })
}

fn update_lock(root: &Path) -> Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    // Both systemd units and every device share one installation. flock is released even on a crash.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("update.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("another updater is already running");
    }
    Ok(lock)
}

async fn update_outcome(setup: &Setup, trigger: Trigger) -> Result<Outcome> {
    let _lock = update_lock(&setup.root_dir)?;
    // A configuration or key error happens before Updater::run: it is recorded as the reason the check failed, and
    // an earlier notice stays.
    let failed = |e: anyhow::Error| {
        if let Some(dir) = setup.requests_dir.parent() {
            update_state::record_failure(dir, &format!("{e:#}"));
        }
        e
    };
    let keys = ReleaseKeys::from_env().map_err(failed)?;
    if keys.is_empty() {
        return Err(failed(anyhow::anyhow!("no release keys configured")));
    }
    let port = std::fs::read_to_string(setup.root_dir.join("http_port")).map_err(|_| {
        failed(anyhow::anyhow!(
            "no http_port in {}; was this installed by the installer?",
            setup.root_dir.display()
        ))
    })?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("termenbox-updater")
        .build()?;
    let fetch = HttpFetch(http.clone());
    let service = Systemd {
        unit: "termenbox.service".into(),
        user: setup.user,
        health_url: format!("http://127.0.0.1:{}/health", port.trim()),
        http,
    };
    let binary: PathBuf = std::env::var_os("TERMENBOX_BINARY")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)?;
    // Where the manifest lives: the channel's address, set in the unit by the installer (stable: `latest`, beta and
    // nightly: their rolling alias releases). It names no asset; the signed manifest does (decision 0132).
    let release_url = std::env::var("TERMENBOX_RELEASE_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASE_URL.into());
    let current = crate::version();
    // Root-owned, beside the journal; a file that does not parse stops the run (a typo must not read as `auto`).
    let policy =
        UpdatePolicy::load(&setup.root_dir.join("update.toml"), &current).map_err(failed)?;
    let updater = Updater {
        fetch: &fetch,
        service: &service,
        keys: &keys,
        binary: strip_deleted(&binary),
        root_dir: setup.root_dir.clone(),
        release_url: release_url.clone(),
        url_prefix: asset_prefix(&release_url),
        policy,
        trigger,
        now_minute: crate::updater::local_minute_of_day(),
        // The server's state directory is the parent of its requests directory.
        status_dir: setup.requests_dir.parent().map(Path::to_path_buf),
        current,
        arch: std::env::consts::ARCH.into(),
        health_wait: Duration::from_secs(30),
    };
    updater.run().await
}

/// Where assets named by the manifest must live: this project's GitHub releases. Only the installer's own tests, with a
/// `file://` release address (the helper's environment is trusted, see `HttpFetch`), keep assets under that address.
fn asset_prefix(release_url: &str) -> String {
    if release_url.starts_with("file://") {
        format!("{}/", release_url.trim_end_matches('/'))
    } else {
        URL_PREFIX.into()
    }
}

/// `current_exe` can end in " (deleted)" after the file was replaced while the process runs.
fn strip_deleted(p: &Path) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_helper_failure_is_correlated_and_an_old_queued_request_cannot_overwrite_it() {
        let root = std::env::temp_dir().join(format!("tm-update-result-{}", uuid::Uuid::new_v4()));
        let requests_dir = root.join("requests");
        let id = crate::requests::leave_update_now(&requests_dir).unwrap();
        let setup = Setup {
            root_dir: root.join("missing-journal"),
            requests_dir: requests_dir.clone(),
            user: true,
        };
        assert!(handle(
            Request::UpdateNow {
                id: Some(id.clone())
            },
            &setup
        )
        .await
        .is_err());
        assert!(crate::requests::update_result(&requests_dir, &id)
            .unwrap()
            .error
            .is_some());
        let old = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            handle(Request::UpdateNow { id: Some(old) }, &setup)
                .await
                .unwrap(),
            0
        );
        assert!(crate::requests::update_result(&requests_dir, &id).is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skip_and_postpone_change_only_the_state_file_and_status_json_has_a_fixed_shape() {
        let root = std::env::temp_dir().join(format!("tm-update-skip-{}", uuid::Uuid::new_v4()));
        let requests = root.join("state/requests");
        std::fs::create_dir_all(&requests).unwrap();
        let journal = root.join("journal");
        std::fs::create_dir_all(&journal).unwrap();
        let setup = Setup {
            root_dir: journal,
            requests_dir: requests,
            user: true,
        };
        let state_dir = root.join("state");
        update_state::record(
            &state_dir,
            "stable",
            "notify",
            "0.1.0",
            update_state::Event::Available("0.2.0"),
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // postpone refuses while nothing is scheduled; a skip of another release does nothing
        rt.block_on(handle(Request::UpdatePostpone {}, &setup))
            .unwrap();
        rt.block_on(handle(
            Request::UpdateSkip {
                version: "0.1.9".into(),
            },
            &setup,
        ))
        .unwrap();
        assert!(!State::load(&state_dir).is_pinned("0.1.9"));
        rt.block_on(handle(
            Request::UpdateSkip {
                version: "0.2.0".into(),
            },
            &setup,
        ))
        .unwrap();
        let state = State::load(&state_dir);
        assert!(state.is_pinned("0.2.0"));
        assert_eq!(
            crate::requests::available_update(&setup.requests_dir, "0.1.0"),
            None
        );

        let policy = UpdatePolicy::parse(
            "[update]\nmode = \"notify\"\nmaintenance_window = \"03:00-04:00\"\n",
            "0.1.0+dev",
        );
        let v = status_json("0.1.0+dev", &policy, &state);
        assert_eq!(v["unreleased_build"], true);
        assert_eq!(v["mode"], "notify");
        assert_eq!(v["maintenance_window"], "03:00-04:00");
        assert_eq!(v["state"]["skipped"], "0.2.0");
        assert_eq!(v["policy_error"], serde_json::Value::Null);
        let broken = status_json(
            "0.1.0+abc1234",
            &UpdatePolicy::parse("[update]\nmode = 3", "0.1.0"),
            &state,
        );
        assert_eq!(broken["unreleased_build"], false);
        assert!(broken["policy_error"].is_string() && broken["mode"].is_null());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_timer_and_request_helper_cannot_replace_the_binary_concurrently() {
        let root = std::env::temp_dir().join(format!("tm-update-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let lock = update_lock(&root).unwrap();
        assert!(update_lock(&root).is_err());
        drop(lock);
        assert!(update_lock(&root).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_user_timer_paths_are_repaired_without_overriding_explicit_paths() {
        let mut setup = Setup {
            root_dir: "/home/u/.local/state/termenbox".into(),
            requests_dir: "/var/lib/termenbox/requests".into(),
            user: true,
        };
        assert_eq!(
            requests_dir(&setup),
            PathBuf::from("/home/u/.config/termenbox/requests")
        );
        setup.requests_dir = "/custom/requests".into();
        assert_eq!(requests_dir(&setup), setup.requests_dir);
        setup.user = false;
        setup.requests_dir = "/var/lib/termenbox/requests".into();
        assert_eq!(requests_dir(&setup), setup.requests_dir);
    }

    #[test]
    fn every_non_install_outcome_finishes_the_owner_request_with_an_explanation() {
        for outcome in [
            Outcome::Held,
            Outcome::Frozen,
            Outcome::UpToDate,
            Outcome::OtherChannel,
            Outcome::RolledBack {
                to: "1.0.0".into(),
                reason: "unhealthy".into(),
            },
        ] {
            assert!(outcome_error(&outcome).is_some());
        }
        assert_eq!(
            outcome_error(&Outcome::Updated {
                from: "0.1.0".into(),
                to: "0.2.0".into()
            }),
            None
        );
    }

    #[test]
    fn assets_must_live_on_github_unless_a_test_release_is_a_file_url() {
        assert_eq!(asset_prefix(DEFAULT_RELEASE_URL), URL_PREFIX);
        assert_eq!(
            asset_prefix("https://github.com/termenbox/termenbox/releases/download/channel-beta"),
            URL_PREFIX,
            "an alias holds only a manifest; its assets are under the release tags"
        );
        assert_eq!(asset_prefix("https://evil.example/x"), URL_PREFIX);
        assert_eq!(asset_prefix("file:///release2/"), "file:///release2/");
    }

    #[test]
    fn a_replaced_binary_path_is_cleaned() {
        assert_eq!(
            strip_deleted(Path::new("/usr/local/bin/termenbox-server (deleted)")),
            PathBuf::from("/usr/local/bin/termenbox-server")
        );
        assert_eq!(
            strip_deleted(Path::new("/usr/local/bin/termenbox-server")),
            PathBuf::from("/usr/local/bin/termenbox-server")
        );
    }
}
