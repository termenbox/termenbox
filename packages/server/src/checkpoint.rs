//! Checkpoints (decision 0176 lever 1, plan step 8.1): a verified restorable copy of an agent's workspace.
//!
//! A checkpoint is taken when a contract-bound run starts and before a risky class runs. It counts for the ladder
//! only once a test restore into a scratch directory reproduced every file (`create_verified`), and only while it
//! still exists on disk (`status` looks every time). A failed verify, or a checkpoint that is gone, leaves the class at
//! propose (guardrail `irreversible-classes-never-leave-propose-without-a-checkpoint`). Snapshots are bounded by
//! bytes and age. `.git` is never copied, restored or removed; symlinks are not followed and not restored.
#![allow(dead_code)] // not called until the permission path (R85 4.6) and non-blocking asks (8.2) use it
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest";
const VERIFIED: &str = "verified";
const TREE: &str = "tree";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Present and a scratch restore matched: the ladder may count it.
    Verified,
    /// Present but never verified, or the last verify failed.
    Unverified,
    /// Not on disk (expired, deleted or never made).
    Missing,
}

impl Status {
    /// What the ladder reads: only a verified checkpoint lets an irreversible class rise above propose.
    pub fn permits_undoable_answer(self) -> bool {
        self == Status::Verified
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    max_bytes: u64,
    max_age_secs: u64,
}

fn skipped(rel: &Path) -> bool {
    rel.components()
        .next()
        .is_some_and(|c| c.as_os_str() == ".git")
}

/// Relative path (`/`-separated) to the content hash of every regular file below `dir`.
fn scan(dir: &Path) -> Result<(BTreeMap<String, String>, u64)> {
    fn walk(
        base: &Path,
        dir: &Path,
        out: &mut BTreeMap<String, String>,
        bytes: &mut u64,
    ) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base)?;
            if skipped(rel) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(base, &path, out, bytes)?;
            } else if kind.is_file() {
                let data = fs::read(&path)?;
                *bytes += data.len() as u64;
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, format!("{:x}", Sha256::digest(&data)));
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    let mut bytes = 0;
    walk(dir, dir, &mut out, &mut bytes)?;
    Ok((out, bytes))
}

fn dir_bytes(dir: &Path) -> u64 {
    scan(dir).map(|(_, b)| b).unwrap_or(0)
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, max_bytes: u64, max_age_secs: u64) -> Self {
        Store {
            root: root.into(),
            max_bytes,
            max_age_secs,
        }
    }

    fn dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("bad checkpoint id");
        }
        Ok(self.root.join(id))
    }

    /// Copy `workspace` into the store under `id`. Fails, leaving nothing behind, if it is over the byte cap.
    pub fn create(&self, id: &str, workspace: &Path, now: u64) -> Result<()> {
        let dir = self.dir(id)?;
        if dir.exists() {
            bail!("checkpoint {id} exists");
        }
        let (files, bytes) = scan(workspace)?;
        if bytes > self.max_bytes {
            bail!(
                "workspace is {bytes} bytes, over the {} byte cap",
                self.max_bytes
            );
        }
        let result = (|| -> Result<()> {
            for rel in files.keys() {
                let to = dir.join(TREE).join(rel);
                fs::create_dir_all(to.parent().context("no parent")?)?;
                fs::copy(workspace.join(rel), &to)?;
            }
            fs::create_dir_all(dir.join(TREE))?;
            let manifest: String = files.iter().map(|(p, h)| format!("{h} {p}\n")).collect();
            fs::write(dir.join(MANIFEST), format!("{now}\n{manifest}"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&dir);
        }
        result
    }

    /// Create, then prove it restores. A verify that fails removes the checkpoint and returns the error.
    pub fn create_verified(&self, id: &str, workspace: &Path, now: u64) -> Result<()> {
        self.create(id, workspace, now)?;
        if let Err(e) = self.verify(id) {
            let _ = fs::remove_dir_all(self.dir(id)?);
            return Err(e);
        }
        Ok(())
    }

    fn manifest(&self, id: &str) -> Result<(u64, BTreeMap<String, String>)> {
        let text =
            fs::read_to_string(self.dir(id)?.join(MANIFEST)).context("no such checkpoint")?;
        let mut lines = text.lines();
        let at = lines
            .next()
            .and_then(|l| l.parse().ok())
            .context("bad manifest")?;
        let files = lines
            .filter_map(|l| l.split_once(' '))
            .map(|(h, p)| (p.to_string(), h.to_string()))
            .collect();
        Ok((at, files))
    }

    /// Restore into a scratch directory and compare every file with the manifest. Marks the checkpoint verified.
    pub fn verify(&self, id: &str) -> Result<()> {
        let dir = self.dir(id)?;
        let _ = fs::remove_file(dir.join(VERIFIED));
        let (_, expected) = self.manifest(id)?;
        let scratch = self.root.join(format!(".scratch-{id}"));
        let _ = fs::remove_dir_all(&scratch);
        let outcome = (|| -> Result<()> {
            fs::create_dir_all(&scratch)?;
            self.restore_into(id, &scratch)?;
            let (got, _) = scan(&scratch)?;
            if got != expected {
                bail!("restore does not match the checkpoint");
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&scratch);
        outcome?;
        fs::write(dir.join(VERIFIED), "")?;
        Ok(())
    }

    fn restore_into(&self, id: &str, target: &Path) -> Result<()> {
        let tree = self.dir(id)?.join(TREE);
        let (files, _) = scan(&tree)?;
        for rel in files.keys() {
            let to = target.join(rel);
            fs::create_dir_all(to.parent().context("no parent")?)?;
            fs::copy(tree.join(rel), to)?;
        }
        Ok(())
    }

    pub fn status(&self, id: &str) -> Status {
        match self.dir(id) {
            Ok(d) if d.join(MANIFEST).is_file() && d.join(TREE).is_dir() => {
                if d.join(VERIFIED).is_file() {
                    Status::Verified
                } else {
                    Status::Unverified
                }
            }
            _ => Status::Missing,
        }
    }

    /// Undo: make `workspace` match the checkpoint again. Files added since are removed; `.git` is left alone.
    pub fn undo(&self, id: &str, workspace: &Path) -> Result<()> {
        let (_, expected) = self.manifest(id)?;
        let (now, _) = scan(workspace)?;
        for rel in now.keys().filter(|p| !expected.contains_key(*p)) {
            fs::remove_file(workspace.join(rel))?;
        }
        self.restore_into(id, workspace)
    }

    /// Drop checkpoints older than the age cap, then the oldest until the total fits the byte cap.
    pub fn expire(&self, now: u64) -> Result<Vec<String>> {
        let mut live: Vec<(u64, String, u64)> = Vec::new();
        let mut gone = Vec::new();
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Ok(gone);
        };
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') {
                let _ = fs::remove_dir_all(e.path()); // an interrupted verify
                continue;
            }
            match self.manifest(&id) {
                Ok((at, _)) => live.push((at, id, dir_bytes(&e.path().join(TREE)))),
                Err(_) => {
                    let _ = fs::remove_dir_all(e.path()); // a half-written checkpoint
                }
            }
        }
        live.sort();
        let mut total: u64 = live.iter().map(|l| l.2).sum();
        for (at, id, bytes) in live {
            if now.saturating_sub(at) > self.max_age_secs || total > self.max_bytes {
                fs::remove_dir_all(self.root.join(&id))?;
                total -= bytes;
                gone.push(id);
            }
        }
        Ok(gone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("tb-checkpoint-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn workspace(t: &Tmp) -> PathBuf {
        let w = t.0.join("ws");
        fs::create_dir_all(w.join("src")).unwrap();
        fs::create_dir_all(w.join(".git")).unwrap();
        fs::write(w.join("a.txt"), "one").unwrap();
        fs::write(w.join("src/b.rs"), "fn b() {}").unwrap();
        fs::write(w.join(".git/HEAD"), "ref").unwrap();
        w
    }

    fn store(t: &Tmp, bytes: u64, age: u64) -> Store {
        Store::new(t.0.join("store"), bytes, age)
    }

    #[test]
    fn checkpoint_restores_changed_deleted_and_added_files() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 3600));
        s.create_verified("c1", &w, 100).unwrap();
        assert_eq!(s.status("c1"), Status::Verified);
        fs::write(w.join("a.txt"), "changed").unwrap();
        fs::remove_file(w.join("src/b.rs")).unwrap();
        fs::write(w.join("new.txt"), "x").unwrap();
        fs::write(w.join(".git/HEAD"), "moved").unwrap();
        s.undo("c1", &w).unwrap();
        assert_eq!(fs::read_to_string(w.join("a.txt")).unwrap(), "one");
        assert_eq!(fs::read_to_string(w.join("src/b.rs")).unwrap(), "fn b() {}");
        assert!(!w.join("new.txt").exists());
        assert_eq!(
            fs::read_to_string(w.join(".git/HEAD")).unwrap(),
            "moved",
            ".git is never touched"
        );
    }

    #[test]
    fn checkpoint_never_copies_git() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 3600));
        s.create("c1", &w, 1).unwrap();
        assert!(!t.0.join("store/c1/tree/.git").exists());
    }

    #[test]
    fn checkpoint_failed_verify_is_not_verified_and_leaves_the_class_at_propose() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 3600));
        s.create("c1", &w, 1).unwrap();
        assert_eq!(s.status("c1"), Status::Unverified);
        assert!(!s.status("c1").permits_undoable_answer());
        // A snapshot file corrupted after the copy: the scratch restore no longer matches.
        fs::write(t.0.join("store/c1/tree/a.txt"), "rot").unwrap();
        assert!(s.verify("c1").is_err());
        assert_eq!(s.status("c1"), Status::Unverified);
        assert!(!s.status("c1").permits_undoable_answer());
        assert!(s.create_verified("c2", &w, 1).is_ok());
        assert!(s.status("c2").permits_undoable_answer());
    }

    #[test]
    fn checkpoint_that_is_gone_drops_to_missing() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 3600));
        s.create_verified("c1", &w, 1).unwrap();
        fs::remove_dir_all(t.0.join("store/c1")).unwrap();
        assert_eq!(s.status("c1"), Status::Missing);
        assert!(!s.status("c1").permits_undoable_answer());
        assert_eq!(s.status("never"), Status::Missing);
    }

    #[test]
    fn checkpoint_expiry_by_age_then_by_bytes() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 100));
        s.create_verified("old", &w, 0).unwrap();
        s.create_verified("new", &w, 150).unwrap();
        assert_eq!(s.expire(160).unwrap(), vec!["old".to_string()]);
        assert_eq!(s.status("new"), Status::Verified);
        // Twelve bytes per snapshot: a 15 byte cap keeps only the newest.
        let s = store(&t, 15, 1000);
        s.create_verified("n1", &w, 200).unwrap();
        s.create_verified("n2", &w, 210).unwrap();
        assert_eq!(
            s.expire(220).unwrap(),
            vec!["new".to_string(), "n1".to_string()]
        );
        assert_eq!(s.status("n2"), Status::Verified);
    }

    #[test]
    fn checkpoint_over_the_byte_cap_leaves_nothing_behind() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 5, 3600));
        assert!(s.create("big", &w, 1).is_err());
        assert_eq!(s.status("big"), Status::Missing);
    }

    #[test]
    fn checkpoint_ids_cannot_escape_the_store() {
        let t = Tmp::new();
        let (w, s) = (workspace(&t), store(&t, 1 << 20, 3600));
        assert!(s.create("../x", &w, 1).is_err());
        assert_eq!(s.status("a/b"), Status::Missing);
    }
}
