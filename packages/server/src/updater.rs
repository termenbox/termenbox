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
use crate::release::{Manifest, ReleaseKeys};

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
}

pub struct Updater<'a> {
    pub fetch: &'a dyn Fetch,
    pub service: &'a dyn Service,
    pub keys: &'a ReleaseKeys,
    pub binary: PathBuf,
    /// The root-only directory with the journal (`/var/lib/termenbox-root`).
    pub root_dir: PathBuf,
    /// Where releases are published; assets are fetched from here, only the hash comes from the manifest.
    pub release_url: String,
    pub url_prefix: String,
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

/// A bundled tmux downloaded, verified and staged beside the installed one, to be swapped in with the server.
struct StagedTmux {
    path: PathBuf,
    staged: PathBuf,
    sha256: String,
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

    /// The new bundled tmux, downloaded, checked against the signed hash and run once, or `None` when this release
    /// has none for this architecture, or this install has none to update (it uses the machine's own tmux: an update
    /// does not add one under running sessions), or the installed one is already the signed one.
    async fn stage_tmux(&self, manifest: &Manifest, base: &str) -> Result<Option<StagedTmux>> {
        let path = self.tmux_path();
        let Some(entry) = manifest.tmux(&self.arch) else {
            return Ok(None);
        };
        if !path.is_file() {
            return Ok(None);
        }
        let expected = entry.sha256.to_ascii_lowercase();
        if crate::journal::hash_file(&path)? == expected {
            return Ok(None);
        }
        let bytes = self
            .fetch
            .get(&format!("{base}/tmux-{}-linux", self.arch))
            .await?;
        if hex(&Sha256::digest(&bytes)) != expected {
            bail!("the downloaded tmux does not match the signed hash; nothing was changed");
        }
        let staged = suffixed(&path, ".new");
        write_executable(&staged, &bytes)?;
        if !self.service.smoke_tmux(&staged).await {
            let _ = std::fs::remove_file(&staged);
            bail!("the new tmux does not run on this machine; nothing was changed");
        }
        Ok(Some(StagedTmux {
            path,
            staged,
            sha256: expected,
        }))
    }

    pub async fn run(&self) -> Result<Outcome> {
        let base = self.release_url.trim_end_matches('/');
        let manifest_bytes = self.fetch.get(&format!("{base}/manifest.json")).await?;
        let signature =
            String::from_utf8(self.fetch.get(&format!("{base}/manifest.json.sig")).await?)?;
        // Nothing in the manifest is used before its signature has been checked.
        let (manifest, _kid) =
            Manifest::verify(&manifest_bytes, &signature, self.keys, &self.url_prefix)?;
        if manifest.freeze {
            return Ok(Outcome::Frozen);
        }
        if !manifest.is_newer_than(&self.current)? {
            return Ok(Outcome::UpToDate);
        }
        let expected = manifest.binary(&self.arch)?.sha256.to_ascii_lowercase();
        let bytes = self
            .fetch
            .get(&format!("{base}/termenbox-server-{}-linux", self.arch))
            .await?;
        if hex(&Sha256::digest(&bytes)) != expected {
            bail!("the downloaded server does not match the signed hash; nothing was changed");
        }

        let staged = self.sibling(".new");
        write_executable(&staged, &bytes)?;
        if !self.service.smoke(&staged).await {
            let _ = std::fs::remove_file(&staged);
            bail!("the new server does not run on this machine; nothing was changed");
        }
        // The bundled tmux goes with the server: both are swapped, both are put back together.
        let tmux = match self.stage_tmux(&manifest, base).await {
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
        let mut swapped_tmux = None;
        if let Some(t) = tmux {
            let backup = backups.join(format!(
                "{}.{}.{}",
                crate::backend::BUNDLED_TMUX,
                core_version(&self.current),
                &crate::journal::hash_file(&t.path)?[..12]
            ));
            std::fs::copy(&t.path, &backup)?;
            let tmux_entry = journal.record(Change::FileReplaced {
                path: t.path.display().to_string(),
                backup: backup.display().to_string(),
                sha256: t.sha256.clone(),
            })?;
            let tmux_previous = suffixed(&t.path, ".prev");
            std::fs::copy(&t.path, &tmux_previous)?;
            std::fs::rename(&t.staged, &t.path)?;
            swapped_tmux = Some((t.path, tmux_previous, tmux_entry));
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
        if let Some((path, tmux_previous, tmux_entry)) = swapped_tmux {
            std::fs::rename(&tmux_previous, &path)?;
            journal.mark_done(tmux_entry)?;
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
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
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

    /// Like `world`, and the release also carries `new_tmux` as the x86_64 bundled tmux. The installed one, beside
    /// the server, is `old tmux`.
    fn world_with_tmux(
        version: &str,
        freeze: bool,
        new_binary: &[u8],
        new_tmux: Option<&[u8]>,
    ) -> Option<World> {
        let dir = std::env::temp_dir().join(format!("termenbox-updater-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        let tmux_entry = new_tmux.map_or(String::new(), |t| {
            format!(
                r#","tmux":{{"x86_64":{{"url":"{BASE}/tmux-x86_64-linux","sha256":"{}"}}}}"#,
                hex(&Sha256::digest(t))
            )
        });
        let manifest = format!(
            r#"{{"version":"{version}","freeze":{freeze},"notes":"","binaries":{{"x86_64":{{"url":"{BASE}/termenbox-server-x86_64-linux","sha256":"{}"}}}}{tmux_entry}}}"#,
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
            "r1 {}",
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
        files.insert(
            "https://rel/termenbox-server-x86_64-linux".to_string(),
            new_binary.to_vec(),
        );
        if let Some(t) = new_tmux {
            files.insert("https://rel/tmux-x86_64-linux".to_string(), t.to_vec());
            std::fs::write(dir.join("termenbox-tmux"), b"old tmux").ok()?;
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
            "https://rel/termenbox-server-x86_64-linux".into(),
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
    async fn the_bundled_tmux_is_updated_with_the_server_and_both_are_journaled() {
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        let (f, s) = (fake(&w), svc(&w, 1, true));
        let out = updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(matches!(out, Outcome::Updated { .. }), "{out:?}");
        let tmux = w.dir.join("termenbox-tmux");
        assert_eq!(std::fs::read(&tmux).unwrap(), b"new tmux");
        assert_eq!(std::fs::read(&w.binary).unwrap(), b"new binary");
        assert!(!w.dir.join("termenbox-tmux.new").exists());
        // one journal entry per file, and reverting puts both back
        let journal = Journal::open(&w.dir.join("root")).unwrap();
        assert_eq!(journal.pending().unwrap().len(), 2);
        let report = journal
            .revert(&mut crate::journal::RealOps, &Default::default())
            .unwrap();
        assert_eq!(report.reverted.len(), 2, "{report:?}");
        assert_eq!(std::fs::read(&tmux).unwrap(), b"old tmux");
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
            .insert("https://rel/tmux-x86_64-linux".into(), b"evil".to_vec());
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
    }

    #[tokio::test]
    async fn an_install_without_a_bundled_tmux_is_not_given_one_and_an_old_manifest_still_works() {
        // the release has a tmux, this install has none beside the server
        let Some(w) = world_with_tmux("9.9.9", false, b"new binary", Some(b"new tmux")) else {
            return;
        };
        std::fs::remove_file(w.dir.join("termenbox-tmux")).unwrap();
        let (f, s) = (fake(&w), svc(&w, 1, true));
        updater(&w, &f, &s, "0.1.0").run().await.unwrap();
        assert!(!w.dir.join("termenbox-tmux").exists());
        assert!(!f.fetched.lock().unwrap().iter().any(|u| u.contains("tmux")));
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

    #[test]
    fn a_trusting_world_is_never_assumed() {
        // The only key the updater trusts is the one it was given: an empty set refuses everything.
        let keys = ReleaseKeys::default();
        assert!(keys.is_empty());
    }
}
