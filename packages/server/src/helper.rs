//! `termenbox-server --root-helper <update|requests>`: what the root-owned systemd units run (decisions 0076, 0077).
//! `update` runs the updater (daily timer). `requests` handles what the unprivileged server asked for.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use tracing::info;

use crate::release::{ReleaseKeys, URL_PREFIX};
use crate::requests::{take_requests, Request};
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
pub async fn run(action: &str, setup: &Setup) -> Result<i32> {
    // A system install's helper changes system files, so only root may run it (tests set the override). The no-root
    // install's helper changes only the user's own files, and refuses to run as root so it cannot be mixed up with it.
    if setup.user {
        if is_root() {
            bail!("the user helper must not run as root");
        }
    } else if !is_root() && std::env::var_os("TERMENBOX_HELPER_ALLOW_NONROOT").is_none() {
        bail!("the root helper must run as root");
    }
    match action {
        "update" => update(setup, Trigger::Timer).await,
        "requests" => {
            let mut status = 0;
            for request in take_requests(&setup.requests_dir) {
                status = status.max(handle(request, setup).await?);
            }
            Ok(status)
        }
        other => bail!("unknown helper action `{other}` (update or requests)"),
    }
}

async fn handle(request: Request, setup: &Setup) -> Result<i32> {
    match request {
        Request::UpdateNow {} => update(setup, Trigger::UpdateNow).await,
        Request::Uninstall { .. } => {
            info!(target: "audit", event = "helper_uninstall_not_available");
            println!("uninstall through the helper is not available in this version");
            Ok(0)
        }
    }
}

async fn update(setup: &Setup, trigger: Trigger) -> Result<i32> {
    let keys = ReleaseKeys::from_env()?;
    anyhow::ensure!(!keys.is_empty(), "no release keys configured");
    let port = std::fs::read_to_string(setup.root_dir.join("http_port")).map_err(|_| {
        anyhow::anyhow!(
            "no http_port in {}; was this installed by the installer?",
            setup.root_dir.display()
        )
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
    let policy = UpdatePolicy::load(&setup.root_dir.join("update.toml"), &current)?;
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
    let outcome = updater.run().await?;
    info!(target: "audit", event = "update", outcome = ?outcome);
    match outcome {
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
            println!("update to {to} was rolled back: {reason}");
            return Ok(2);
        }
    }
    Ok(0)
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
