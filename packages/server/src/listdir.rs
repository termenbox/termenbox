//! `list_dir`: the directories inside one folder, for the New workspace folder picker (decision 0147).
//!
//! The reach is deliberately small. The path is canonicalised and must be `$HOME` or below it, so `..` and a symlink
//! that leaves `$HOME` end in a refusal; the reply carries directory names only; the audit line holds a hash of the
//! path and never an entry. Creating a workspace does not depend on any of this.

use std::fs;
use std::path::{Path, PathBuf};

use termenbox_core::protocol::{DirEntry, ServerMessage, DIR_LISTING_MAX};

/// The home directory of the OS user the server runs as.
pub fn server_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// What the audit log records about a request: a short hash of the path, never the path.
pub fn path_hash(path: Option<&str>) -> String {
    crate::agentwatch::hex8(path.unwrap_or(""))
}

/// The reply to a `list_dir` request, `DirListing` or `DirError`. Blocking: call it from `spawn_blocking`.
pub fn list_dir(
    home: &Path,
    path: Option<&str>,
    prefix: Option<&str>,
    hidden: bool,
) -> ServerMessage {
    match listing(home, path, prefix, hidden) {
        Ok(m) => m,
        Err(reason) => ServerMessage::DirError {
            reason: reason.to_string(),
        },
    }
}

/// The reply for one request: a refusal for anyone but the owner (a session guest, decision 0094), else the listing
/// read off the async threads.
pub async fn answer(
    is_owner: bool,
    home: Option<PathBuf>,
    path: Option<String>,
    prefix: Option<String>,
    hidden: bool,
) -> ServerMessage {
    let refuse = |reason: &str| ServerMessage::DirError {
        reason: reason.to_string(),
    };
    if !is_owner {
        return refuse("Only the owner can browse folders");
    }
    let Some(home) = home else {
        return refuse("The home folder is unknown");
    };
    tokio::task::spawn_blocking(move || list_dir(&home, path.as_deref(), prefix.as_deref(), hidden))
        .await
        .unwrap_or_else(|_| refuse("The folder cannot be read"))
}

fn listing(
    home: &Path,
    path: Option<&str>,
    prefix: Option<&str>,
    hidden: bool,
) -> Result<ServerMessage, &'static str> {
    let home = fs::canonicalize(home).map_err(|_| "The home folder cannot be read")?;
    let target = match path {
        None => home.clone(),
        Some(p) if Path::new(p).is_absolute() => {
            fs::canonicalize(p).map_err(|_| "That folder does not exist")?
        }
        Some(_) => return Err("Use an absolute path"),
    };
    if !target.starts_with(&home) {
        return Err("Only folders inside your home folder can be browsed");
    }
    if !target.is_dir() {
        return Err("That is not a folder");
    }
    let prefix = prefix.map(str::to_lowercase).filter(|p| !p.is_empty());
    let mut entries: Vec<DirEntry> = fs::read_dir(&target)
        .map_err(|_| "That folder cannot be read")?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            let symlink = e.file_type().ok()?.is_symlink();
            // A link is listed when it points at a directory; whether it may be entered is decided by the
            // canonicalisation above on the next request.
            let is_dir = if symlink {
                fs::metadata(e.path()).ok()?.is_dir()
            } else {
                e.file_type().ok()?.is_dir()
            };
            let hidden_name = name.starts_with('.');
            let wanted = is_dir
                && (hidden || !hidden_name)
                && prefix
                    .as_ref()
                    .is_none_or(|p| name.to_lowercase().starts_with(p.as_str()));
            wanted.then_some(DirEntry {
                name,
                symlink,
                hidden: hidden_name,
            })
        })
        .collect();
    entries.sort_by_cached_key(|e| (e.name.to_lowercase(), e.name.clone()));
    let truncated = entries.len() > DIR_LISTING_MAX;
    entries.truncate(DIR_LISTING_MAX);
    let parent = (target != home)
        .then(|| target.parent().map(|p| p.to_string_lossy().into_owned()))
        .flatten();
    Ok(ServerMessage::DirListing {
        path: target.to_string_lossy().into_owned(),
        parent,
        entries,
        truncated,
        home: home.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A fresh `<tmp>/tb-listdir-N/{home,outside}`; `home` holds `code/app`, `code/tool`, `.config`, `Zed`, a file.
    struct Fixture {
        root: PathBuf,
        home: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("tb-listdir-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let home = root.join("home");
            for d in [
                "home/code/app",
                "home/code/tool",
                "home/.config",
                "home/Zed",
                "outside/secret",
            ] {
                fs::create_dir_all(root.join(d)).unwrap();
            }
            fs::write(home.join("notes.txt"), "x").unwrap();
            let root = fs::canonicalize(root).unwrap();
            let home = root.join("home");
            Fixture { root, home }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn names(m: &ServerMessage) -> Vec<&str> {
        match m {
            ServerMessage::DirListing { entries, .. } => {
                entries.iter().map(|e| e.name.as_str()).collect()
            }
            other => panic!("not a listing: {other:?}"),
        }
    }

    fn refused(m: ServerMessage) -> String {
        match m {
            ServerMessage::DirError { reason } => reason,
            other => panic!("not an error: {other:?}"),
        }
    }

    #[test]
    fn list_dir_lists_home_directories_only_sorted_and_without_hidden() {
        let f = Fixture::new("home");
        let m = list_dir(&f.home, None, None, false);
        // The file `notes.txt` and the hidden `.config` are absent; the sort ignores case.
        assert_eq!(names(&m), ["code", "Zed"]);
        let ServerMessage::DirListing {
            path, parent, home, ..
        } = m
        else {
            unreachable!()
        };
        assert_eq!(path, f.home.to_string_lossy());
        assert_eq!(home, path);
        assert_eq!(parent, None);
    }

    #[test]
    fn list_dir_hidden_names_come_back_only_when_asked() {
        let f = Fixture::new("hidden");
        let m = list_dir(&f.home, None, None, true);
        assert_eq!(names(&m), [".config", "code", "Zed"]);
        let ServerMessage::DirListing { entries, .. } = m else {
            unreachable!()
        };
        assert!(entries[0].hidden && !entries[1].hidden);
    }

    #[test]
    fn list_dir_a_subfolder_reports_its_parent_and_the_prefix_filters_case_insensitively() {
        let f = Fixture::new("sub");
        let code = f.home.join("code");
        let m = list_dir(&f.home, Some(code.to_str().unwrap()), Some("AP"), false);
        assert_eq!(names(&m), ["app"]);
        let ServerMessage::DirListing { parent, .. } = m else {
            unreachable!()
        };
        assert_eq!(parent.as_deref(), f.home.to_str());
    }

    #[test]
    fn list_dir_dotdot_and_paths_outside_home_are_refused() {
        let f = Fixture::new("escape");
        let up = format!("{}/..", f.home.display());
        let beside = f.root.join("outside");
        for p in [up.as_str(), beside.to_str().unwrap(), "/etc", "/"] {
            let r = refused(list_dir(&f.home, Some(p), None, false));
            assert!(r.contains("home folder"), "{p}: {r}");
        }
        assert_eq!(
            refused(list_dir(&f.home, Some("code"), None, false)),
            "Use an absolute path"
        );
        let gone = f.home.join("nope");
        assert_eq!(
            refused(list_dir(&f.home, Some(gone.to_str().unwrap()), None, false)),
            "That folder does not exist"
        );
        let file = f.home.join("notes.txt");
        assert_eq!(
            refused(list_dir(&f.home, Some(file.to_str().unwrap()), None, false)),
            "That is not a folder"
        );
    }

    #[test]
    fn list_dir_a_link_out_of_home_is_listed_but_not_followed() {
        let f = Fixture::new("link");
        symlink(f.root.join("outside"), f.home.join("out")).unwrap();
        symlink(f.home.join("notes.txt"), f.home.join("filelink")).unwrap();
        let m = list_dir(&f.home, None, None, false);
        // The link to a directory is listed and marked; the link to a file is not.
        assert_eq!(names(&m), ["code", "out", "Zed"]);
        let ServerMessage::DirListing { entries, .. } = &m else {
            unreachable!()
        };
        assert!(entries.iter().find(|e| e.name == "out").unwrap().symlink);
        let through = f.home.join("out");
        let r = refused(list_dir(
            &f.home,
            Some(through.to_str().unwrap()),
            None,
            false,
        ));
        assert!(r.contains("home folder"), "{r}");
    }

    #[test]
    fn list_dir_five_hundred_and_one_children_give_five_hundred_and_truncated() {
        let f = Fixture::new("many");
        let big = f.home.join("big");
        fs::create_dir(&big).unwrap();
        for i in 0..501 {
            fs::create_dir(big.join(format!("d{i:03}"))).unwrap();
        }
        let ServerMessage::DirListing {
            entries, truncated, ..
        } = list_dir(&f.home, Some(big.to_str().unwrap()), None, false)
        else {
            panic!("not a listing")
        };
        assert_eq!(entries.len(), DIR_LISTING_MAX);
        assert!(truncated);
        assert_eq!(entries[0].name, "d000");
    }

    #[test]
    fn list_dir_exactly_five_hundred_is_not_truncated() {
        let f = Fixture::new("exact");
        let big = f.home.join("big");
        fs::create_dir(&big).unwrap();
        for i in 0..500 {
            fs::create_dir(big.join(format!("d{i:03}"))).unwrap();
        }
        let ServerMessage::DirListing {
            entries, truncated, ..
        } = list_dir(&f.home, Some(big.to_str().unwrap()), None, false)
        else {
            panic!("not a listing")
        };
        assert_eq!((entries.len(), truncated), (500, false));
    }

    #[tokio::test]
    async fn list_dir_a_guest_is_refused_and_the_owner_is_served() {
        let f = Fixture::new("guest");
        let ask = |owner| answer(owner, Some(f.home.clone()), None, None, false);
        assert_eq!(
            refused(ask(false).await),
            "Only the owner can browse folders"
        );
        assert_eq!(names(&ask(true).await), ["code", "Zed"]);
        let none = answer(true, None, None, None, false).await;
        assert_eq!(refused(none), "The home folder is unknown");
    }

    #[test]
    fn list_dir_the_audit_hash_hides_the_path() {
        let h = path_hash(Some("/home/me/secret-project"));
        assert_eq!(h.len(), 8);
        assert!(!h.contains("secret"));
        assert_eq!(h, path_hash(Some("/home/me/secret-project")));
    }
}
