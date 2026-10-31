//! Change journal (decision 0067, guardrail host-changes-are-journaled). Everything the installer, profile or
//! updater changes on a machine is recorded here *before* it is applied, so that `--revert` can undo exactly
//! that and nothing else. The journal lives in a root-owned directory, out of reach of the unprivileged server.
//!
//! The file is append-only JSON lines: a change record, or a `reverted` marker that points at one. Reverting
//! twice therefore changes nothing, and a file that was modified after we wrote it is reported, never deleted.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::write_private;

pub const FILE_NAME: &str = "journal.jsonl";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A file we created; `sha256` is the hash of what we wrote.
    FileCreated { path: String, sha256: String },
    /// A file we replaced; the original was moved to `backup`.
    FileReplaced {
        path: String,
        backup: String,
        sha256: String,
    },
    /// A directory we created; reverted only when it is empty (its contents are the owner's or the state a
    /// dedicated uninstall step removes first).
    DirCreated { path: String },
    /// A user account we created.
    UserCreated { name: String, home: String },
    /// A package we installed with the host's package manager.
    PackageInstalled { manager: String, name: String },
    /// A systemd unit file we installed.
    UnitInstalled { name: String, path: String },
    /// A firewall rule we added (`rule` like `4443/udp`).
    FirewallRuleAdded { tool: String, rule: String },
    /// Lingering we switched on for a user, so their service starts at boot without a login (the no-root install).
    LingerEnabled { user: String },
    /// A container recreated from another image by the container updater (decision 0155). Recorded before the
    /// change, so it is the account of what ran; nothing here is reverted (the previous container is kept until the
    /// new one is healthy, and removing the install removes the container).
    ContainerReplaced {
        name: String,
        from: String,
        to: String,
    },
}

impl Change {
    pub fn kind(&self) -> &'static str {
        match self {
            Change::FileCreated { .. } => "file_created",
            Change::FileReplaced { .. } => "file_replaced",
            Change::DirCreated { .. } => "dir_created",
            Change::UserCreated { .. } => "user_created",
            Change::PackageInstalled { .. } => "package_installed",
            Change::UnitInstalled { .. } => "unit_installed",
            Change::FirewallRuleAdded { .. } => "firewall_rule_added",
            Change::LingerEnabled { .. } => "linger_enabled",
            Change::ContainerReplaced { .. } => "container_replaced",
        }
    }

    /// Builds a change from the `--journal-record KIND --journal-field K=V` command line. File hashes are
    /// computed here so the installer script does not have to.
    pub fn from_fields(kind: &str, fields: &[(String, String)]) -> Result<Change> {
        let get = |k: &str| -> Result<String> {
            fields
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| anyhow!("{kind} needs {k}=..."))
        };
        Ok(match kind {
            "file_created" => {
                let path = get("path")?;
                // `sha256=` lets the installer record a file before it is put in place (it knows the hash).
                Change::FileCreated {
                    sha256: file_hash_field(&get("sha256").ok(), &path)?,
                    path,
                }
            }
            "file_replaced" => {
                let path = get("path")?;
                Change::FileReplaced {
                    sha256: file_hash_field(&get("sha256").ok(), &path)?,
                    backup: get("backup")?,
                    path,
                }
            }
            "dir_created" => Change::DirCreated { path: get("path")? },
            "user_created" => Change::UserCreated {
                name: get("name")?,
                home: get("home")?,
            },
            "package_installed" => Change::PackageInstalled {
                manager: get("manager")?,
                name: get("name")?,
            },
            "unit_installed" => Change::UnitInstalled {
                name: get("name")?,
                path: get("path")?,
            },
            "firewall_rule_added" => Change::FirewallRuleAdded {
                tool: get("tool")?,
                rule: get("rule")?,
            },
            "linger_enabled" => Change::LingerEnabled { user: get("user")? },
            other => bail!("unknown change kind `{other}`"),
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
enum Record {
    Change { id: u64, ts: u64, change: Change },
    Reverted { reverted: u64, ts: u64 },
}

fn file_hash_field(given: &Option<String>, path: &str) -> Result<String> {
    match given {
        Some(h) if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) => {
            Ok(h.to_ascii_lowercase())
        }
        Some(_) => bail!("sha256 must be 64 hex digits"),
        None => hash_file(Path::new(path)),
    }
}

pub fn hash_file(path: &Path) -> Result<String> {
    Ok(hex(&Sha256::digest(std::fs::read(path)?)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a revert does to the machine; the real implementation runs commands, tests use a recording fake.
pub trait Ops {
    /// Hash of the file now, or `None` when it does not exist.
    fn file_hash(&self, path: &Path) -> Option<String>;
    fn remove_file(&mut self, path: &Path) -> Result<()>;
    fn restore_file(&mut self, backup: &Path, path: &Path) -> Result<()>;
    /// `delete_home` is the owner's explicit choice (decision 0072); a plain revert keeps the home directory.
    /// Removes an empty directory; `Ok(false)` when it still has contents.
    fn remove_empty_dir(&mut self, path: &Path) -> Result<bool>;
    fn delete_user(&mut self, name: &str, delete_home: bool) -> Result<()>;
    fn remove_package(&mut self, manager: &str, name: &str) -> Result<()>;
    fn remove_unit(&mut self, name: &str, path: &Path) -> Result<()>;
    fn remove_firewall_rule(&mut self, tool: &str, rule: &str) -> Result<()>;
    fn disable_linger(&mut self, user: &str) -> Result<()>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Ids that were (or, in a dry run, would be) reverted.
    pub reverted: Vec<u64>,
    /// Ids left alone on purpose, with the reason (for example "modified since it was installed").
    pub skipped: Vec<(u64, String)>,
    pub failed: Vec<(u64, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct RevertOptions {
    pub only: Option<String>,
    pub dry_run: bool,
    pub delete_home: bool,
}

pub struct Journal {
    file: PathBuf,
}

impl Journal {
    /// Opens (creating, root-only) the journal in `dir`.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let file = dir.join(FILE_NAME);
        if !file.exists() {
            write_private(&file, b"")?;
        }
        Ok(Self { file })
    }

    fn records(&self) -> Result<Vec<Record>> {
        let text = std::fs::read_to_string(&self.file)?;
        // A torn or foreign line is ignored rather than blocking a revert.
        Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    fn append(&self, record: &Record) -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&self.file)?;
        writeln!(f, "{}", serde_json::to_string(record)?)?;
        f.sync_all()?;
        Ok(())
    }

    /// Records a change; call it *before* applying the change.
    pub fn record(&self, change: Change) -> Result<u64> {
        let id = self
            .records()?
            .iter()
            .filter(|r| matches!(r, Record::Change { .. }))
            .count() as u64
            + 1;
        self.append(&Record::Change {
            id,
            ts: now(),
            change,
        })?;
        Ok(id)
    }

    /// Marks a change as no longer pending without undoing anything (for example an update that was rolled back).
    pub fn mark_done(&self, id: u64) -> Result<()> {
        self.append(&Record::Reverted {
            reverted: id,
            ts: now(),
        })
    }

    /// Changes not yet reverted, oldest first.
    pub fn pending(&self) -> Result<Vec<(u64, Change)>> {
        let records = self.records()?;
        let done: std::collections::HashSet<u64> = records
            .iter()
            .filter_map(|r| {
                if let Record::Reverted { reverted, .. } = r {
                    Some(*reverted)
                } else {
                    None
                }
            })
            .collect();
        Ok(records
            .into_iter()
            .filter_map(|r| match r {
                Record::Change { id, change, .. } if !done.contains(&id) => Some((id, change)),
                _ => None,
            })
            .collect())
    }

    /// Undoes the pending changes newest first. Anything that changed since we made it is skipped and reported.
    pub fn revert(&self, ops: &mut dyn Ops, opts: &RevertOptions) -> Result<Report> {
        let mut report = Report::default();
        let mut pending = self.pending()?;
        pending.reverse();
        for (id, change) in pending {
            if opts.only.as_deref().is_some_and(|k| k != change.kind()) {
                continue;
            }
            match apply_revert(ops, &change, opts) {
                Ok(Outcome::Done) => {
                    report.reverted.push(id);
                    if !opts.dry_run {
                        self.append(&Record::Reverted {
                            reverted: id,
                            ts: now(),
                        })?;
                    }
                }
                Ok(Outcome::Skip(why)) => report.skipped.push((id, why)),
                Err(e) => report.failed.push((id, e.to_string())),
            }
        }
        Ok(report)
    }
}

enum Outcome {
    Done,
    Skip(String),
}

fn apply_revert(ops: &mut dyn Ops, change: &Change, opts: &RevertOptions) -> Result<Outcome> {
    let dry = opts.dry_run;
    match change {
        Change::FileCreated { path, sha256 } => {
            check_path(path)?;
            match ops.file_hash(Path::new(path)) {
                None => Ok(Outcome::Done), // already gone
                Some(h) if &h != sha256 => Ok(Outcome::Skip(format!(
                    "{path} was modified since it was installed"
                ))),
                Some(_) => {
                    if !dry {
                        ops.remove_file(Path::new(path))?;
                    }
                    Ok(Outcome::Done)
                }
            }
        }
        Change::FileReplaced {
            path,
            backup,
            sha256,
        } => {
            check_path(path)?;
            check_path(backup)?;
            match ops.file_hash(Path::new(path)) {
                Some(h) if &h != sha256 => Ok(Outcome::Skip(format!(
                    "{path} was modified since it was installed"
                ))),
                _ => {
                    if !dry {
                        ops.restore_file(Path::new(backup), Path::new(path))?;
                    }
                    Ok(Outcome::Done)
                }
            }
        }
        Change::DirCreated { path } => {
            check_path(path)?;
            if dry || !Path::new(path).exists() {
                return Ok(Outcome::Done);
            }
            if ops.remove_empty_dir(Path::new(path))? {
                Ok(Outcome::Done)
            } else {
                Ok(Outcome::Skip(format!(
                    "{path} is not empty, so it was left in place"
                )))
            }
        }
        Change::UserCreated { name, .. } => {
            check_name(name)?;
            if !dry {
                ops.delete_user(name, opts.delete_home)?;
            }
            Ok(Outcome::Done)
        }
        Change::PackageInstalled { manager, name } => {
            check_name(name)?;
            if !dry {
                ops.remove_package(manager, name)?;
            }
            Ok(Outcome::Done)
        }
        Change::UnitInstalled { name, path } => {
            check_name(name)?;
            check_path(path)?;
            if !dry {
                ops.remove_unit(name, Path::new(path))?;
            }
            Ok(Outcome::Done)
        }
        Change::FirewallRuleAdded { tool, rule } => {
            if !dry {
                ops.remove_firewall_rule(tool, rule)?;
            }
            Ok(Outcome::Done)
        }
        Change::ContainerReplaced { .. } => Ok(Outcome::Skip(
            "a container update is not undone here; the previous container was kept until the new one was healthy".into(),
        )),
        Change::LingerEnabled { user } => {
            check_name(user)?;
            if !dry {
                ops.disable_linger(user)?;
            }
            Ok(Outcome::Done)
        }
    }
}

/// Where a unit file may be removed from: `Some(false)` for the system directory, `Some(true)` for the running user's
/// own `~/.config/systemd/user`, `None` for anywhere else. A corrupted entry must not reach outside these.
fn unit_scope(path: &Path, home: Option<&Path>) -> Option<bool> {
    if path.starts_with("/etc/systemd/system") {
        return Some(false);
    }
    let home = home.filter(|h| h.is_absolute())?;
    path.starts_with(home.join(".config/systemd/user"))
        .then_some(true)
}

/// Paths in a journal entry must be absolute and free of `..`; a corrupted entry must not reach outside.
fn check_path(path: &str) -> Result<()> {
    let p = Path::new(path);
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("refusing the path `{path}` in the journal");
    }
    Ok(())
}

/// User, package and unit names: letters, digits and `_ - . + @` only, never starting with `-`.
fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.+@".contains(c));
    if ok {
        Ok(())
    } else {
        bail!("refusing the name `{name}` in the journal")
    }
}

/// The real machine: runs the host's own tools, with fixed argument lists and no shell.
pub struct RealOps;

fn run(program: &str, args: &[&str]) -> Result<()> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| anyhow!("cannot run {program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        bail!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

impl Ops for RealOps {
    fn file_hash(&self, path: &Path) -> Option<String> {
        hash_file(path).ok()
    }

    fn remove_file(&mut self, path: &Path) -> Result<()> {
        Ok(std::fs::remove_file(path)?)
    }

    fn restore_file(&mut self, backup: &Path, path: &Path) -> Result<()> {
        Ok(std::fs::rename(backup, path)?)
    }

    fn remove_empty_dir(&mut self, path: &Path) -> Result<bool> {
        match std::fs::remove_dir(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    fn delete_user(&mut self, name: &str, delete_home: bool) -> Result<()> {
        // Recorded before the account was made: when creating it failed there is nothing to remove.
        if run("id", &["-u", "--", name]).is_err() {
            return Ok(());
        }
        let mut args = vec!["--", name];
        if delete_home {
            args.insert(0, "-r");
        }
        run("userdel", &args)
    }

    fn remove_package(&mut self, manager: &str, name: &str) -> Result<()> {
        match manager {
            "apt" => run("apt-get", &["remove", "-y", "--", name]),
            "dnf" => run("dnf", &["remove", "-y", "--", name]),
            "yum" => run("yum", &["remove", "-y", "--", name]),
            "apk" => run("apk", &["del", "--", name]),
            "pacman" => run("pacman", &["-R", "--noconfirm", "--", name]),
            other => bail!("unknown package manager `{other}`"),
        }
    }

    fn remove_unit(&mut self, name: &str, path: &Path) -> Result<()> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some(user_scope) = unit_scope(path, home.as_deref()) else {
            bail!("refusing to remove a unit outside /etc/systemd/system or this user's systemd directory");
        };
        // A user unit is stopped and forgotten by that user's own manager, never by the system one.
        let sctl = |rest: &[&str]| {
            let mut args: Vec<&str> = if user_scope {
                vec!["--user"]
            } else {
                Vec::new()
            };
            args.extend_from_slice(rest);
            run("systemctl", &args)
        };
        let _ = sctl(&["disable", "--now", "--", name]); // may already be stopped
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        sctl(&["daemon-reload"])
    }

    fn disable_linger(&mut self, user: &str) -> Result<()> {
        // The change is journaled before it is tried, and systemd may have refused it: only switch off what is on.
        let on = std::process::Command::new("loginctl")
            .args(["show-user", user, "-p", "Linger", "--value"])
            .output()
            .is_ok_and(|o| {
                o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "yes"
            });
        if !on {
            return Ok(());
        }
        run("loginctl", &["disable-linger", user])
    }

    fn remove_firewall_rule(&mut self, tool: &str, rule: &str) -> Result<()> {
        let valid = rule.split_once('/').is_some_and(|(port, proto)| {
            port.parse::<u16>().is_ok() && matches!(proto, "udp" | "tcp")
        });
        if !valid {
            bail!("refusing the firewall rule `{rule}` in the journal");
        }
        match tool {
            "ufw" => run("ufw", &["delete", "allow", rule]),
            "firewalld" => {
                run(
                    "firewall-cmd",
                    &["--permanent", &format!("--remove-port={rule}")],
                )?;
                run("firewall-cmd", &["--reload"])
            }
            other => bail!("unknown firewall tool `{other}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("termenbox-journal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Records calls; files are real so hashes work.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        fail_on: Option<&'static str>,
    }

    impl Ops for Fake {
        fn file_hash(&self, path: &Path) -> Option<String> {
            hash_file(path).ok()
        }
        fn remove_file(&mut self, path: &Path) -> Result<()> {
            self.calls.push(format!("rm {}", path.display()));
            Ok(std::fs::remove_file(path)?)
        }
        fn restore_file(&mut self, backup: &Path, path: &Path) -> Result<()> {
            self.calls
                .push(format!("restore {} {}", backup.display(), path.display()));
            Ok(std::fs::rename(backup, path)?)
        }
        fn remove_empty_dir(&mut self, path: &Path) -> Result<bool> {
            self.calls.push(format!("rmdir {}", path.display()));
            Ok(std::fs::remove_dir(path).is_ok())
        }
        fn delete_user(&mut self, name: &str, delete_home: bool) -> Result<()> {
            self.calls
                .push(format!("userdel {name} home={delete_home}"));
            if self.fail_on == Some("user") {
                bail!("userdel failed");
            }
            Ok(())
        }
        fn remove_package(&mut self, manager: &str, name: &str) -> Result<()> {
            self.calls.push(format!("pkg {manager} {name}"));
            Ok(())
        }
        fn remove_unit(&mut self, name: &str, _: &Path) -> Result<()> {
            self.calls.push(format!("unit {name}"));
            Ok(())
        }
        fn remove_firewall_rule(&mut self, tool: &str, rule: &str) -> Result<()> {
            self.calls.push(format!("fw {tool} {rule}"));
            Ok(())
        }
        fn disable_linger(&mut self, user: &str) -> Result<()> {
            self.calls.push(format!("unlinger {user}"));
            Ok(())
        }
    }

    fn journal() -> (Journal, PathBuf) {
        let dir = temp();
        (Journal::open(&dir.join("j")).unwrap(), dir)
    }

    #[test]
    fn every_kind_is_recorded_and_reverted_newest_first() {
        let (j, dir) = journal();
        let created = dir.join("created.txt");
        std::fs::write(&created, "ours").unwrap();
        let replaced = dir.join("conf");
        let backup = dir.join("conf.orig");
        std::fs::write(&backup, "original").unwrap();
        std::fs::write(&replaced, "new").unwrap();
        j.record(Change::PackageInstalled {
            manager: "apt".into(),
            name: "tmux".into(),
        })
        .unwrap();
        j.record(Change::UserCreated {
            name: "termenbox".into(),
            home: "/home/termenbox".into(),
        })
        .unwrap();
        j.record(Change::FileCreated {
            path: created.display().to_string(),
            sha256: hash_file(&created).unwrap(),
        })
        .unwrap();
        j.record(Change::FileReplaced {
            path: replaced.display().to_string(),
            backup: backup.display().to_string(),
            sha256: hash_file(&replaced).unwrap(),
        })
        .unwrap();
        j.record(Change::UnitInstalled {
            name: "termenbox.service".into(),
            path: "/etc/systemd/system/termenbox.service".into(),
        })
        .unwrap();
        j.record(Change::FirewallRuleAdded {
            tool: "ufw".into(),
            rule: "4443/udp".into(),
        })
        .unwrap();

        let mut ops = Fake::default();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert_eq!(report.reverted, vec![6, 5, 4, 3, 2, 1]);
        assert!(report.skipped.is_empty() && report.failed.is_empty());
        assert_eq!(ops.calls[0], "fw ufw 4443/udp");
        assert_eq!(ops.calls.last().unwrap(), "pkg apt tmux");
        assert!(!created.exists());
        assert_eq!(std::fs::read_to_string(&replaced).unwrap(), "original");
        assert!(
            ops.calls
                .contains(&"userdel termenbox home=false".to_string()),
            "the home directory is kept by default"
        );
    }

    #[test]
    fn reverting_twice_changes_nothing() {
        let (j, dir) = journal();
        let f = dir.join("a");
        std::fs::write(&f, "x").unwrap();
        j.record(Change::FileCreated {
            path: f.display().to_string(),
            sha256: hash_file(&f).unwrap(),
        })
        .unwrap();
        let mut ops = Fake::default();
        j.revert(&mut ops, &RevertOptions::default()).unwrap();
        let mut again = Fake::default();
        let report = j.revert(&mut again, &RevertOptions::default()).unwrap();
        assert_eq!(report, Report::default());
        assert!(again.calls.is_empty());
        assert!(j.pending().unwrap().is_empty());
    }

    #[test]
    fn a_modified_file_is_reported_not_deleted() {
        let (j, dir) = journal();
        let f = dir.join("conf");
        std::fs::write(&f, "ours").unwrap();
        j.record(Change::FileCreated {
            path: f.display().to_string(),
            sha256: hash_file(&f).unwrap(),
        })
        .unwrap();
        std::fs::write(&f, "the owner edited this").unwrap();
        let mut ops = Fake::default();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert!(report.reverted.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].1.contains("modified"));
        assert!(f.exists());
        assert_eq!(j.pending().unwrap().len(), 1, "it stays pending");
    }

    #[test]
    fn a_dry_run_lists_without_touching_anything() {
        let (j, dir) = journal();
        let f = dir.join("a");
        std::fs::write(&f, "x").unwrap();
        j.record(Change::FileCreated {
            path: f.display().to_string(),
            sha256: hash_file(&f).unwrap(),
        })
        .unwrap();
        j.record(Change::UserCreated {
            name: "termenbox".into(),
            home: "/h".into(),
        })
        .unwrap();
        let mut ops = Fake::default();
        let report = j
            .revert(
                &mut ops,
                &RevertOptions {
                    dry_run: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(report.reverted, vec![2, 1]);
        assert!(ops.calls.is_empty());
        assert!(f.exists());
        assert_eq!(j.pending().unwrap().len(), 2);
    }

    #[test]
    fn only_limits_a_revert_to_one_kind() {
        let (j, _dir) = journal();
        j.record(Change::PackageInstalled {
            manager: "apt".into(),
            name: "tmux".into(),
        })
        .unwrap();
        j.record(Change::UserCreated {
            name: "termenbox".into(),
            home: "/h".into(),
        })
        .unwrap();
        let mut ops = Fake::default();
        let report = j
            .revert(
                &mut ops,
                &RevertOptions {
                    only: Some("user_created".into()),
                    delete_home: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(report.reverted, vec![2]);
        assert_eq!(ops.calls, vec!["userdel termenbox home=true"]);
        assert_eq!(j.pending().unwrap().len(), 1);
    }

    #[test]
    fn a_failure_is_reported_and_the_rest_still_reverts() {
        let (j, _dir) = journal();
        j.record(Change::PackageInstalled {
            manager: "apt".into(),
            name: "tmux".into(),
        })
        .unwrap();
        j.record(Change::UserCreated {
            name: "termenbox".into(),
            home: "/h".into(),
        })
        .unwrap();
        let mut ops = Fake {
            fail_on: Some("user"),
            ..Default::default()
        };
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert_eq!(report.reverted, vec![1]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(
            j.pending().unwrap().len(),
            1,
            "the failed one is retried next time"
        );
    }

    #[test]
    fn corrupted_entries_cannot_reach_outside_or_inject_arguments() {
        let (j, _dir) = journal();
        j.record(Change::FileCreated {
            path: "/etc/../etc/passwd".into(),
            sha256: "0".into(),
        })
        .unwrap();
        j.record(Change::UserCreated {
            name: "--root".into(),
            home: "/h".into(),
        })
        .unwrap();
        j.record(Change::FileCreated {
            path: "relative/path".into(),
            sha256: "0".into(),
        })
        .unwrap();
        let mut ops = Fake::default();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert_eq!(report.failed.len(), 3);
        assert!(ops.calls.is_empty());
    }

    #[test]
    fn a_torn_line_does_not_block_the_journal() {
        let (j, _dir) = journal();
        j.record(Change::PackageInstalled {
            manager: "apt".into(),
            name: "tmux".into(),
        })
        .unwrap();
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&j.file)
            .unwrap();
        write!(f, "{{\"id\":2,\"ts\":1,\"change\":{{\"kind\":\"user_cr").unwrap();
        assert_eq!(j.pending().unwrap().len(), 1);
    }

    #[test]
    fn the_command_line_builds_changes_and_hashes_files() {
        let dir = temp();
        let f = dir.join("bin");
        std::fs::write(&f, "binary").unwrap();
        let fields = vec![("path".to_string(), f.display().to_string())];
        match Change::from_fields("file_created", &fields).unwrap() {
            Change::FileCreated { path, sha256 } => {
                assert_eq!(path, f.display().to_string());
                assert_eq!(sha256, hash_file(&f).unwrap());
            }
            other => panic!("{other:?}"),
        }
        assert!(Change::from_fields("user_created", &[]).is_err());
        assert!(Change::from_fields("rm_rf", &[]).is_err());
    }

    #[test]
    fn a_directory_is_removed_only_when_empty_and_a_given_hash_is_used() {
        let (j, dir) = journal();
        let state = dir.join("state");
        std::fs::create_dir(&state).unwrap();
        j.record(Change::DirCreated {
            path: state.display().to_string(),
        })
        .unwrap();
        std::fs::write(state.join("server_key"), "secret").unwrap();
        let mut ops = Fake::default();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert!(
            report.reverted.is_empty() && report.skipped.len() == 1,
            "{report:?}"
        );
        assert!(
            state.join("server_key").exists(),
            "the contents are never deleted by a journal revert"
        );
        std::fs::remove_file(state.join("server_key")).unwrap();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert_eq!(report.reverted, vec![1]);
        assert!(!state.exists());

        let given = "ab".repeat(32);
        let fields = vec![
            (
                "path".to_string(),
                "/usr/local/bin/not-there-yet".to_string(),
            ),
            ("sha256".to_string(), given.clone()),
        ];
        assert!(
            matches!(Change::from_fields("file_created", &fields).unwrap(), Change::FileCreated { sha256, .. } if sha256 == given)
        );
        let bad = vec![
            ("path".to_string(), "/x".to_string()),
            ("sha256".to_string(), "nope".to_string()),
        ];
        assert!(Change::from_fields("file_created", &bad).is_err());
    }

    #[test]
    fn the_journal_directory_is_private() {
        let dir = temp().join("j");
        let _ = Journal::open(&dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dir.join(FILE_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn units_are_removed_only_from_the_system_directory_or_the_users_own() {
        let home = Path::new("/home/me");
        assert_eq!(
            unit_scope(
                Path::new("/etc/systemd/system/termenbox.service"),
                Some(home)
            ),
            Some(false)
        );
        assert_eq!(
            unit_scope(
                Path::new("/home/me/.config/systemd/user/termenbox.service"),
                Some(home)
            ),
            Some(true)
        );
        for bad in [
            "/home/other/.config/systemd/user/x.service",
            "/home/me/.config/other/x.service",
            "/usr/lib/systemd/system/x.service",
            "/home/me/.config/systemd/userx/x.service",
        ] {
            assert_eq!(unit_scope(Path::new(bad), Some(home)), None, "{bad}");
        }
        assert_eq!(
            unit_scope(Path::new("/home/me/.config/systemd/user/x.service"), None),
            None
        );
        assert_eq!(
            unit_scope(
                Path::new("relative/.config/systemd/user/x.service"),
                Some(Path::new("relative"))
            ),
            None
        );
    }

    #[test]
    fn lingering_we_enabled_is_switched_off_again_by_a_revert() {
        let (j, _dir) = journal();
        j.record(Change::from_fields("linger_enabled", &[("user".into(), "me".into())]).unwrap())
            .unwrap();
        let mut ops = Fake::default();
        let report = j.revert(&mut ops, &RevertOptions::default()).unwrap();
        assert_eq!(ops.calls, vec!["unlinger me".to_string()]);
        assert_eq!(report.reverted.len(), 1);
    }
}
