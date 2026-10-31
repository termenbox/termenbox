//! The container updater (decision 0155). A container install has the same host timers and root helper as a machine
//! install; only the apply step differs: pull the digest the signed manifest names, recreate the container from the
//! arguments the installer recorded, check it, and put the previous container back if it is not healthy.
//!
//! The container never gets the Docker socket. Every Docker call here has fixed arguments and no shell; the image
//! reference comes only from the verified manifest and the container name only from `container.json`.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const FILE: &str = "container.json";
const SCHEMA: u32 = 1;
const MAX_FILE: u64 = 64 * 1024;

/// What the installer ran, replayed by the helper. Written by the installer (root, mode 0600); the helper only adds
/// the digest it last applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub v: u32,
    pub name: String,
    /// The repository without tag or digest, like `ghcr.io/termenbox/termenbox`.
    pub image: String,
    /// The arguments of `docker run -d --name <name>` up to the image: port, env, volume, restart policy.
    pub args: Vec<String>,
    /// The `sha256:` digest this container runs, when known.
    #[serde(default)]
    pub digest: Option<String>,
    /// `TERMENBOX_IMAGE` pinned a tag by hand: the timer never pulls (a pinned image is a choice, like `hold`).
    #[serde(default)]
    pub pinned: bool,
}

fn plain(s: &str, extra: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b"._-".contains(&b) || extra.as_bytes().contains(&b)
        })
}

impl Spec {
    pub fn parse(text: &str) -> Result<Spec> {
        let spec: Spec = serde_json::from_str(text).map_err(|e| anyhow!("bad {FILE}: {e}"))?;
        if spec.v != SCHEMA {
            bail!("{FILE} has schema {}, this helper reads {SCHEMA}", spec.v);
        }
        if !plain(&spec.name, "") || spec.name.ends_with("-previous") {
            bail!("{FILE}: the container name is not a plain name");
        }
        // A repository, no digest and no tag (a registry port is allowed, a tag on the last segment is not).
        if !plain(&spec.image, "/:")
            || spec
                .image
                .rsplit('/')
                .next()
                .is_some_and(|last| last.contains(':'))
        {
            bail!("{FILE}: the image is not a repository");
        }
        if spec.args.len() > 64
            || spec
                .args
                .iter()
                .any(|a| a.is_empty() || a.len() > 1024 || a.contains('\0'))
        {
            bail!("{FILE}: the run arguments are not usable");
        }
        if spec
            .digest
            .as_deref()
            .is_some_and(|d| !crate::release::is_image_digest(d))
        {
            bail!("{FILE}: the digest is not a sha256 digest");
        }
        Ok(spec)
    }

    /// Reads the file the installer wrote: a regular file, not linked, owned by root with mode 0600 (tests and the
    /// no-root override accept the current user), small. Anything else is refused: the helper replays only what the
    /// installer wrote.
    pub fn load(path: &Path) -> Result<Spec> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let meta = file.metadata()?;
        let owner_ok = meta.uid() == 0
            || std::env::var_os("TERMENBOX_HELPER_ALLOW_NONROOT")
                .is_some_and(|_| meta.uid() == unsafe { libc::geteuid() });
        if !meta.is_file() || !owner_ok || meta.mode() & 0o077 != 0 || meta.len() > MAX_FILE {
            bail!(
                "{} is not a root-owned 0600 file the installer wrote",
                path.display()
            );
        }
        let mut text = String::new();
        file.take(MAX_FILE).read_to_string(&mut text)?;
        Spec::parse(&text)
    }

    pub fn store(&self, path: &Path) -> Result<()> {
        Ok(crate::requests::write_json_private(path, self)?)
    }

    pub fn reference(&self, digest: &str) -> String {
        format!("{}@{digest}", self.image)
    }

    fn previous(&self) -> String {
        format!("{}-previous", self.name)
    }
}

#[async_trait]
pub trait Docker: Send + Sync {
    async fn pull(&self, reference: &str) -> Result<()>;
    /// The `sha256:` manifest digest of the image the container runs, when it can be told.
    async fn running_digest(&self, name: &str, repo: &str) -> Result<Option<String>>;
    async fn exists(&self, name: &str) -> bool;
    async fn stop(&self, name: &str) -> Result<()>;
    async fn rename(&self, from: &str, to: &str) -> Result<()>;
    async fn run(&self, name: &str, args: &[String], reference: &str) -> Result<()>;
    async fn start(&self, name: &str) -> Result<()>;
    /// The server answers inside the container.
    async fn healthy(&self, name: &str) -> bool;
    async fn remove(&self, name: &str) -> Result<()>;
    async fn remove_image(&self, reference: &str);
    /// Copies the server binary out of an image, for the host helper.
    async fn extract_binary(&self, reference: &str, dest: &Path) -> Result<()>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// The container already runs this digest.
    Current,
    Updated,
    RolledBack(String),
}

/// Recreates the container from `digest`. `record` is called once, before anything is changed, with the digest the
/// container runs now and the one it is about to run (the journal entry).
pub async fn apply(
    docker: &dyn Docker,
    spec: &Spec,
    digest: &str,
    health_wait: Duration,
    record: &mut dyn FnMut(&str, &str) -> Result<()>,
) -> Result<Applied> {
    if !crate::release::is_image_digest(digest) {
        bail!("the manifest names no usable image digest");
    }
    let (name, previous) = (spec.name.as_str(), spec.previous());
    // A run that died between the rename and the new container left the old one aside: put it back first.
    if !docker.exists(name).await && docker.exists(&previous).await {
        docker.rename(&previous, name).await?;
        docker.start(name).await?;
    }
    let before = docker
        .running_digest(name, &spec.image)
        .await?
        .or_else(|| spec.digest.clone());
    if before.as_deref() == Some(digest) {
        return Ok(Applied::Current);
    }
    let reference = spec.reference(digest);
    docker.pull(&reference).await?;
    record(before.as_deref().unwrap_or("unknown"), digest)?;
    // Same UDP port, so the old container stops before the new one starts.
    docker.stop(name).await?;
    docker.rename(name, &previous).await?;
    let started = docker.run(name, &spec.args, &reference).await;
    let healthy = started.is_ok() && wait_healthy(docker, name, health_wait).await;
    if healthy {
        let _ = docker.remove(&previous).await;
        if let Some(old) = before {
            docker.remove_image(&spec.reference(&old)).await;
        }
        return Ok(Applied::Updated);
    }
    let _ = docker.remove(name).await;
    docker.rename(&previous, name).await?;
    docker.start(name).await?;
    Ok(Applied::RolledBack(match started {
        Err(_) => "the new container did not start".into(),
        Ok(()) => "the new container did not become healthy within the time allowed".into(),
    }))
}

async fn wait_healthy(docker: &dyn Docker, name: &str, wait: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if docker.healthy(name).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The real thing: the `docker` CLI on `PATH`, fixed arguments, no shell.
pub struct Cli;

async fn docker(args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::new("docker")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| anyhow!("docker: {e}"))?;
    if !out.status.success() {
        bail!("docker {} failed", args.first().copied().unwrap_or(""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[async_trait]
impl Docker for Cli {
    async fn pull(&self, reference: &str) -> Result<()> {
        docker(&["pull", "-q", reference]).await.map(drop)
    }
    async fn running_digest(&self, name: &str, repo: &str) -> Result<Option<String>> {
        let Ok(id) = docker(&["inspect", "-f", "{{.Image}}", name]).await else {
            return Ok(None);
        };
        let Ok(list) = docker(&["image", "inspect", "-f", "{{json .RepoDigests}}", &id]).await
        else {
            return Ok(None);
        };
        let digests: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
        Ok(digests
            .iter()
            .filter_map(|d| d.strip_prefix(&format!("{repo}@")))
            .find(|d| crate::release::is_image_digest(d))
            .map(String::from))
    }
    async fn exists(&self, name: &str) -> bool {
        docker(&["inspect", "-f", "{{.Id}}", name]).await.is_ok()
    }
    async fn stop(&self, name: &str) -> Result<()> {
        docker(&["stop", name]).await.map(drop)
    }
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        docker(&["rename", from, to]).await.map(drop)
    }
    async fn run(&self, name: &str, args: &[String], reference: &str) -> Result<()> {
        let mut all: Vec<&str> = vec!["run", "-d", "--name", name];
        all.extend(args.iter().map(String::as_str));
        all.push(reference);
        docker(&all).await.map(drop)
    }
    async fn start(&self, name: &str) -> Result<()> {
        docker(&["start", name]).await.map(drop)
    }
    async fn healthy(&self, name: &str) -> bool {
        docker(&["exec", name, "termenbox-server", "--help"])
            .await
            .is_ok()
            && docker(&["exec", name, "curl", "-fs", "http://127.0.0.1:8080/health"])
                .await
                .is_ok()
    }
    async fn remove(&self, name: &str) -> Result<()> {
        docker(&["rm", "-f", name]).await.map(drop)
    }
    async fn remove_image(&self, reference: &str) {
        let _ = docker(&["rmi", reference]).await;
    }
    async fn extract_binary(&self, reference: &str, dest: &Path) -> Result<()> {
        let id = docker(&["create", reference]).await?;
        let copied = docker(&[
            "cp",
            &format!("{id}:/usr/local/bin/termenbox-server"),
            &dest.display().to_string(),
        ])
        .await;
        let _ = docker(&["rm", "-f", &id]).await;
        copied.map(drop)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    pub(crate) const OLD: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    pub(crate) const NEW: &str =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    pub(crate) fn spec() -> Spec {
        Spec {
            v: 1,
            name: "termenbox".into(),
            image: "ghcr.io/termenbox/termenbox".into(),
            args: vec![
                "--restart".into(),
                "unless-stopped".into(),
                "-p".into(),
                "4443:4443/udp".into(),
            ],
            digest: Some(OLD.into()),
            pinned: false,
        }
    }

    /// A Docker that keeps a list of containers (name, digest, running) and a log of what was asked.
    pub(crate) struct Fake {
        pub(crate) containers: Mutex<Vec<(String, String, bool)>>,
        pub(crate) log: Mutex<Vec<String>>,
        new_is_healthy: bool,
        pub(crate) pull_fails: bool,
    }
    pub(crate) fn fake(new_is_healthy: bool) -> Fake {
        Fake {
            containers: Mutex::new(vec![("termenbox".into(), OLD.into(), true)]),
            log: Mutex::default(),
            new_is_healthy,
            pull_fails: false,
        }
    }
    impl Fake {
        fn note(&self, s: String) {
            self.log.lock().unwrap().push(s);
        }
        pub(crate) fn state(&self) -> Vec<(String, String, bool)> {
            self.containers.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl Docker for Fake {
        async fn pull(&self, r: &str) -> Result<()> {
            self.note(format!("pull {r}"));
            if self.pull_fails {
                bail!("no")
            } else {
                Ok(())
            }
        }
        async fn running_digest(&self, name: &str, _: &str) -> Result<Option<String>> {
            Ok(self
                .state()
                .iter()
                .find(|c| c.0 == name)
                .map(|c| c.1.clone()))
        }
        async fn exists(&self, name: &str) -> bool {
            self.state().iter().any(|c| c.0 == name)
        }
        async fn stop(&self, name: &str) -> Result<()> {
            self.note(format!("stop {name}"));
            self.containers
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|c| c.0 == name)
                .for_each(|c| c.2 = false);
            Ok(())
        }
        async fn rename(&self, from: &str, to: &str) -> Result<()> {
            self.note(format!("rename {from} {to}"));
            self.containers
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|c| c.0 == from)
                .for_each(|c| c.0 = to.into());
            Ok(())
        }
        async fn run(&self, name: &str, args: &[String], r: &str) -> Result<()> {
            self.note(format!("run {name} {} {r}", args.join(" ")));
            let digest = r.split_once('@').unwrap().1.to_string();
            self.containers
                .lock()
                .unwrap()
                .push((name.into(), digest, true));
            Ok(())
        }
        async fn start(&self, name: &str) -> Result<()> {
            self.note(format!("start {name}"));
            self.containers
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|c| c.0 == name)
                .for_each(|c| c.2 = true);
            Ok(())
        }
        async fn healthy(&self, name: &str) -> bool {
            self.state()
                .iter()
                .any(|c| c.0 == name && c.2 && (c.1 == OLD || self.new_is_healthy))
        }
        async fn remove(&self, name: &str) -> Result<()> {
            self.note(format!("rm {name}"));
            self.containers.lock().unwrap().retain(|c| c.0 != name);
            Ok(())
        }
        async fn remove_image(&self, r: &str) {
            self.note(format!("rmi {r}"));
        }
        async fn extract_binary(&self, _: &str, _: &Path) -> Result<()> {
            Ok(())
        }
    }

    async fn run_apply(
        f: &Fake,
        spec: &Spec,
        digest: &str,
    ) -> (Result<Applied>, Vec<(String, String)>) {
        let mut recorded = Vec::new();
        let r = apply(f, spec, digest, Duration::from_millis(20), &mut |a, b| {
            recorded.push((a.to_string(), b.to_string()));
            Ok(())
        })
        .await;
        (r, recorded)
    }

    #[tokio::test]
    async fn a_new_digest_replaces_the_container_and_removes_the_old_one_after_it_is_healthy() {
        let f = fake(true);
        let (r, recorded) = run_apply(&f, &spec(), NEW).await;
        assert_eq!(r.unwrap(), Applied::Updated);
        assert_eq!(recorded, vec![(OLD.to_string(), NEW.to_string())]);
        assert_eq!(f.state(), vec![("termenbox".into(), NEW.into(), true)]);
        let log = f.log.lock().unwrap().join("\n");
        assert!(log.contains(&format!("run termenbox --restart unless-stopped -p 4443:4443/udp ghcr.io/termenbox/termenbox@{NEW}")), "{log}");
        assert!(log.find("stop termenbox").unwrap() < log.find("run termenbox").unwrap());
        assert!(
            log.contains("rm termenbox-previous")
                && log.contains(&format!("rmi ghcr.io/termenbox/termenbox@{OLD}"))
        );
    }

    #[tokio::test]
    async fn an_unhealthy_new_container_is_removed_and_the_old_one_is_started_again() {
        let f = fake(false);
        let (r, _) = run_apply(&f, &spec(), NEW).await;
        assert!(matches!(r.unwrap(), Applied::RolledBack(_)));
        assert_eq!(f.state(), vec![("termenbox".into(), OLD.into(), true)]);
        assert!(!f.log.lock().unwrap().iter().any(|l| l.starts_with("rmi")));
    }

    #[tokio::test]
    async fn the_digest_already_running_changes_nothing_and_a_failed_pull_changes_nothing() {
        let f = fake(true);
        let (r, recorded) = run_apply(&f, &spec(), OLD).await;
        assert_eq!(r.unwrap(), Applied::Current);
        assert!(recorded.is_empty() && f.log.lock().unwrap().is_empty());
        let mut f = fake(true);
        f.pull_fails = true;
        let (r, recorded) = run_apply(&f, &spec(), NEW).await;
        assert!(r.is_err() && recorded.is_empty());
        assert_eq!(f.state(), vec![("termenbox".into(), OLD.into(), true)]);
        // a manifest digest that is not a digest is refused before anything is asked
        let (r, _) = run_apply(&fake(true), &spec(), "latest").await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn a_run_that_died_with_the_old_container_set_aside_puts_it_back_first() {
        let f = fake(true);
        *f.containers.lock().unwrap() = vec![("termenbox-previous".into(), OLD.into(), false)];
        let (r, _) = run_apply(&f, &spec(), OLD).await;
        assert_eq!(r.unwrap(), Applied::Current);
        assert_eq!(f.state(), vec![("termenbox".into(), OLD.into(), true)]);
    }

    #[test]
    fn the_run_file_is_replayed_only_when_the_installer_wrote_it() {
        let s = spec();
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(Spec::parse(&text).unwrap(), s);
        for bad in [
            text.replace("\"v\":1", "\"v\":2"),
            text.replace("\"termenbox\",\"image\"", "\"a b\",\"image\""),
            text.replace("termenbox-previous", "x")
                .replace("\"name\":\"termenbox\"", "\"name\":\"x-previous\""),
            text.replace(
                "ghcr.io/termenbox/termenbox",
                "ghcr.io/termenbox/termenbox@sha256:1",
            ),
            text.replace(OLD, "latest"),
            text.replace("\"pinned\"", "\"extra\":1,\"pinned\""),
        ] {
            assert!(Spec::parse(&bad).is_err(), "{bad}");
        }
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("termenbox-container-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, &text).unwrap();
        // not root-owned (this test runs as a user) and not 0600 either: refused
        assert!(Spec::load(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Spec::load(&path).is_err(), "owned by a user, not root");
        std::env::set_var("TERMENBOX_HELPER_ALLOW_NONROOT", "1");
        assert_eq!(Spec::load(&path).unwrap(), s);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Spec::load(&path).is_err(), "readable by others");
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Spec::load(&link).is_err());
    }

    /// The real `Cli` against a real Docker and a local registry. Runs only with `TERMENBOX_E2E_DOCKER=1` (CI sets it;
    /// `scripts/dev.sh` does not reach a Docker daemon). The images are tiny stand-ins: a `termenbox-server` that
    /// answers `--help` and a `curl` that answers the health probe unless the image says otherwise.
    mod e2e {
        use super::*;
        use std::process::{Command, Stdio};

        fn sh(args: &[&str]) -> String {
            let out = Command::new("docker")
                .args(args)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "docker {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        /// Builds, pushes and returns the manifest digest of an image whose health probe `ok` answers.
        fn push(repo: &str, tag: &str, ok: bool) -> String {
            let dir = std::env::temp_dir().join(format!("termenbox-e2e-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let healthy = if ok { "exit 0" } else { "exit 1" };
            std::fs::write(
                dir.join("Dockerfile"),
                format!(
                    "FROM busybox:1.36\nRUN mkdir -p /usr/local/bin && printf '#!/bin/sh\\nexit 0\\n' > /usr/local/bin/termenbox-server && printf '#!/bin/sh\\n{healthy}\\n' > /usr/local/bin/curl && chmod 755 /usr/local/bin/*\nLABEL e2e={tag}\nENV PATH=/usr/local/bin:$PATH\nCMD [\"sleep\", \"3600\"]\n"
                ),
            )
            .unwrap();
            let image = format!("{repo}:{tag}");
            sh(&["build", "-q", "-t", &image, dir.to_str().unwrap()]);
            sh(&["push", "-q", &image]);
            let digests = sh(&["image", "inspect", "-f", "{{json .RepoDigests}}", &image]);
            let list: Vec<String> = serde_json::from_str(&digests).unwrap();
            let digest = list
                .iter()
                .find_map(|d| d.strip_prefix(&format!("{repo}@")))
                .unwrap()
                .to_string();
            // Forget the local copy so the update has to pull the digest from the registry.
            sh(&["rmi", "-f", &image]);
            digest
        }

        #[tokio::test]
        async fn the_cli_updates_a_container_from_a_registry_and_rolls_a_bad_image_back() {
            if std::env::var_os("TERMENBOX_E2E_DOCKER").is_none() {
                return;
            }
            let id = uuid::Uuid::new_v4().simple().to_string();
            let (registry, name) = (
                format!("tm-e2e-registry-{}", &id[..8]),
                format!("tm-e2e-{}", &id[..8]),
            );
            let port = "5127";
            sh(&[
                "run",
                "-d",
                "--name",
                &registry,
                "-p",
                &format!("127.0.0.1:{port}:5000"),
                "registry:2",
            ]);
            let repo = format!("localhost:{port}/termenbox-e2e");
            let cleanup = || {
                for n in [name.clone(), format!("{name}-previous"), registry.clone()] {
                    let _ = Command::new("docker").args(["rm", "-f", &n]).output();
                }
            };
            let result = std::panic::AssertUnwindSafe(async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let (v1, v2) = (push(&repo, "one", true), push(&repo, "two", true));
                let bad = push(&repo, "bad", false);
                let spec = Spec {
                    v: 1,
                    name: name.clone(),
                    image: repo.clone(),
                    args: vec![
                        "--restart".into(),
                        "no".into(),
                        "-e".into(),
                        "TERMENBOX_E2E=1".into(),
                    ],
                    digest: None,
                    pinned: false,
                };
                let wait = Duration::from_secs(10);
                let mut recorded = Vec::new();
                let mut record = |a: &str, b: &str| {
                    recorded.push((a.to_string(), b.to_string()));
                    Ok(())
                };
                // The installer's container: created from the first digest.
                Cli.pull(&spec.reference(&v1)).await.unwrap();
                Cli.run(&name, &spec.args, &spec.reference(&v1))
                    .await
                    .unwrap();
                assert!(wait_healthy(&Cli, &name, wait).await);
                assert_eq!(
                    Cli.running_digest(&name, &repo).await.unwrap(),
                    Some(v1.clone())
                );
                // An update to the second digest.
                assert_eq!(
                    apply(&Cli, &spec, &v2, wait, &mut record).await.unwrap(),
                    Applied::Updated
                );
                assert_eq!(
                    Cli.running_digest(&name, &repo).await.unwrap(),
                    Some(v2.clone())
                );
                assert!(
                    !Cli.exists(&format!("{name}-previous")).await,
                    "the old container is removed"
                );
                // A bad image is rolled back and the working container runs again.
                let short = Duration::from_secs(2);
                let r = apply(&Cli, &spec, &bad, short, &mut record).await.unwrap();
                assert!(matches!(r, Applied::RolledBack(_)), "{r:?}");
                assert_eq!(
                    Cli.running_digest(&name, &repo).await.unwrap(),
                    Some(v2.clone())
                );
                assert!(wait_healthy(&Cli, &name, wait).await);
                assert!(!Cli.exists(&format!("{name}-previous")).await);
                // The digest that already runs changes nothing.
                assert_eq!(
                    apply(&Cli, &spec, &v2, wait, &mut record).await.unwrap(),
                    Applied::Current
                );
                assert_eq!(recorded.len(), 2, "{recorded:?}");
                assert_eq!(recorded[0], (v1, v2.clone()));
                assert_eq!(recorded[1], (v2, bad));
                // The server binary the host helper is copied from is in the image.
                let dest = std::env::temp_dir().join(format!("tm-e2e-bin-{id}"));
                Cli.extract_binary(&spec.reference(&recorded[1].0), &dest)
                    .await
                    .unwrap();
                assert!(dest.exists());
                let _ = std::fs::remove_file(dest);
            });
            let outcome = futures_catch(result).await;
            cleanup();
            if let Err(panic) = outcome {
                std::panic::resume_unwind(panic);
            }
        }

        async fn futures_catch<F: std::future::Future<Output = ()>>(
            f: std::panic::AssertUnwindSafe<F>,
        ) -> std::thread::Result<()> {
            use futures_util::FutureExt;
            f.catch_unwind().await
        }
    }
}
