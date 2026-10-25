//! Requests from the unprivileged server to the root helper (decision 0077, guardrail
//! unprivileged-server-never-holds-root). The server drops a small JSON file into a directory that only it and
//! root can use; a root path unit starts the helper, which calls `take_requests`.
//!
//! The directory belongs to the less trusted side, so everything in it is treated as hostile: files are opened
//! without following links and without blocking, must be small regular files, are deleted before they are acted
//! on, and can only name one of a fixed set of actions. Nothing in a request is ever run or used as a path.

use std::io::Read;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;

use serde::Deserialize;
use tracing::{info, warn};

const MAX_BYTES: u64 = 4096;
const MAX_FILES: usize = 16;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Check for a new release now instead of waiting for the timer.
    // A struct variant so that serde's deny_unknown_fields applies (it is ignored for unit variants).
    UpdateNow {},
    /// Remove this server (decision 0072); the flags are the owner's opt-ins for host cleanup.
    Uninstall {
        #[serde(default)]
        delete_user: bool,
        #[serde(default)]
        remove_tmux: bool,
        #[serde(default, rename = "kill_sessions")]
        kill_workspaces: bool,
    },
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Request::UpdateNow {} => "update-now",
            Request::Uninstall { .. } => "uninstall",
        }
    }
}

/// Asks the root helper to check for and install a release now. The file is created exclusively and readable by the
/// server's user only; the helper treats it as hostile anyway (see `read_one`).
pub fn leave_update_now(dir: &Path) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(br#"{"action":"update-now"}"#)
}

/// The release the updater recorded as installable but not installed (`update-status.json`, written next to the
/// requests directory), or None. A file that is large, unreadable or not what the updater writes reads as None.
pub fn available_update(requests_dir: &Path) -> Option<String> {
    let path = requests_dir.parent()?.join("update-status.json");
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = v.get("available")?.as_str()?;
    (!version.is_empty()
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)))
    .then(|| version.to_string())
}

/// Reads and removes the pending requests (at most 16 per run). Anything that is not a valid request is removed
/// and logged without its content, so the path unit cannot be kept busy by junk.
pub fn take_requests(dir: &Path) -> Vec<Request> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut found = vec![];
    for entry in entries.flatten().take(MAX_FILES) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        match read_one(&path) {
            Ok(request) => found.push(request),
            Err(why) => warn!(target: "audit", event = "helper_request_rejected", reason = why),
        }
        // Gone before it is acted on, whatever it was. A link is removed, never followed; a directory only if empty.
        if std::fs::remove_file(&path).is_err() {
            let _ = std::fs::remove_dir(&path);
        }
    }
    for r in &found {
        info!(target: "audit", event = "helper_request", action = r.name());
    }
    found
}

fn read_one(path: &Path) -> Result<Request, &'static str> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "could not be opened (a link?)")?;
    let meta = file.metadata().map_err(|_| "could not be inspected")?;
    let kind = meta.file_type();
    if !kind.is_file() || kind.is_fifo() || kind.is_socket() {
        return Err("not a regular file");
    }
    if meta.len() > MAX_BYTES {
        return Err("too large");
    }
    let mut text = String::new();
    file.by_ref()
        .take(MAX_BYTES)
        .read_to_string(&mut text)
        .map_err(|_| "not text")?;
    serde_json::from_str(&text).map_err(|_| "not a known request")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("termenbox-requests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn known_requests_are_read_and_removed() {
        let d = dir();
        std::fs::write(d.join("a.json"), r#"{"action":"update-now"}"#).unwrap();
        std::fs::write(
            d.join("b.json"),
            r#"{"action":"uninstall","delete_user":true}"#,
        )
        .unwrap();
        let mut got = take_requests(&d);
        got.sort_by_key(|r| r.name());
        assert_eq!(
            got,
            vec![
                Request::Uninstall {
                    delete_user: true,
                    remove_tmux: false,
                    kill_workspaces: false
                },
                Request::UpdateNow {}
            ]
        );
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            0,
            "nothing is left to trigger the path unit again"
        );
    }

    #[test]
    fn unknown_actions_fields_and_junk_are_dropped_not_run() {
        let d = dir();
        for (name, body) in [
            ("a.json", r#"{"action":"rm","path":"/"}"#),
            (
                "b.json",
                r#"{"action":"update-now","command":"id > /tmp/pwned"}"#,
            ),
            ("c.json", "not json at all"),
            ("d.json", r#"{"action":"uninstall","delete_user":"yes"}"#),
        ] {
            std::fs::write(d.join(name), body).unwrap();
        }
        assert!(take_requests(&d).is_empty());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn a_link_is_not_followed_and_what_it_points_to_is_untouched() {
        let d = dir();
        let secret = d.join("target.txt");
        std::fs::write(&secret, r#"{"action":"update-now"}"#).unwrap();
        std::os::unix::fs::symlink(&secret, d.join("evil.json")).unwrap();
        assert!(
            take_requests(&d).is_empty(),
            "a request behind a link is refused"
        );
        assert!(secret.exists(), "the link is removed, its target is not");
        assert!(!d.join("evil.json").exists());
    }

    #[test]
    fn oversized_files_and_pipes_do_not_hang_or_load_memory() {
        let d = dir();
        std::fs::write(
            d.join("big.json"),
            format!(r#"{{"action":"update-now","pad":"{}"}}"#, "x".repeat(5000)),
        )
        .unwrap();
        let fifo = std::ffi::CString::new(d.join("pipe.json").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        std::fs::create_dir(d.join("dir.json")).unwrap();
        assert!(take_requests(&d).is_empty());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn other_files_are_left_alone_and_a_missing_directory_is_fine() {
        let d = dir();
        std::fs::write(d.join("notes.txt"), "keep").unwrap();
        assert!(take_requests(&d).is_empty());
        assert!(d.join("notes.txt").exists());
        assert!(take_requests(&d.join("nope")).is_empty());
    }

    #[test]
    fn no_more_than_sixteen_files_are_handled_per_run() {
        let d = dir();
        for i in 0..40 {
            std::fs::write(d.join(format!("{i}.json")), r#"{"action":"update-now"}"#).unwrap();
        }
        assert_eq!(take_requests(&d).len(), 16);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 24);
    }

    #[test]
    fn an_update_request_the_server_leaves_is_one_the_helper_accepts() {
        let d = dir();
        leave_update_now(&d).unwrap();
        assert_eq!(take_requests(&d), vec![Request::UpdateNow {}]);
    }

    #[test]
    fn the_available_release_is_read_from_the_status_file_next_to_the_requests() {
        let d = dir().join("requests");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(available_update(&d), None);
        let status = d.parent().unwrap().join("update-status.json");
        std::fs::write(
            &status,
            r#"{"available":"0.1.54-beta.1","channel":"beta","mode":"notify"}"#,
        )
        .unwrap();
        assert_eq!(available_update(&d).as_deref(), Some("0.1.54-beta.1"));
        std::fs::write(&status, r#"{"available":"1.0 <b>"}"#).unwrap();
        assert_eq!(available_update(&d), None);
        std::fs::write(&status, "junk").unwrap();
        assert_eq!(available_update(&d), None);
        std::fs::remove_dir_all(d.parent().unwrap()).unwrap();
    }
}
