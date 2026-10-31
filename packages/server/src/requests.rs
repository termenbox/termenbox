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

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::release::Channel;

const MAX_BYTES: u64 = 4096;
const MAX_FILES: usize = 16;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Check for a new release now instead of waiting for the timer.
    // A struct variant so that serde's deny_unknown_fields applies (it is ignored for unit variants).
    UpdateNow {
        #[serde(default)]
        id: Option<String>,
    },
    /// Refresh the signed release notice without installing anything.
    CheckUpdate {},
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
            Request::UpdateNow { .. } => "update-now",
            Request::CheckUpdate {} => "check-update",
            Request::Uninstall { .. } => "uninstall",
        }
    }
}

/// Asks the root helper to check for and install a release now. The file is created exclusively and readable by the
/// server's user only; the helper treats it as hostile anyway (see `read_one`).
pub fn leave_update_now(dir: &Path) -> std::io::Result<String> {
    std::fs::create_dir_all(dir)?;
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(state) = dir.parent() {
        write_json(
            &state.join("update-progress.json"),
            &serde_json::json!({"id":id, "started_at":now_secs()}),
        )?;
    }
    if let Err(error) = write_json_mode(
        &dir.join(format!("{id}.json")),
        &serde_json::json!({"action":"update-now", "id":id}),
        0o600,
    ) {
        if let Some(state) = dir.parent() {
            let _ = std::fs::remove_file(state.join("update-progress.json"));
        }
        return Err(error);
    }
    Ok(id)
}

pub fn leave_update_check(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // One fixed request per server, even when several devices authenticate at once.
    write_json_mode(
        &dir.join("check-update.json"),
        &serde_json::json!({"action":"check-update"}),
        0o600,
    )
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Publish complete files only: a systemd path unit must never consume a half-written request.
pub fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    write_json_mode(path, value, 0o644)
}

fn write_json_mode(path: &Path, value: &impl Serialize, mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        file.write_all(&serde_json::to_vec(value).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateResult {
    pub id: String,
    pub error: Option<String>,
}

pub fn update_result(dir: &Path, id: &str) -> Option<UpdateResult> {
    let result: UpdateResult =
        serde_json::from_str(&read_status(&dir.parent()?.join("update-result.json"))?).ok()?;
    (result.id == id).then_some(result)
}

/// Reconnect and rollback restart the server too; a pending owner request must survive them.
pub fn pending_update(dir: &Path) -> Option<(String, std::time::Duration)> {
    let v: serde_json::Value =
        serde_json::from_str(&read_status(&dir.parent()?.join("update-progress.json"))?).ok()?;
    let id = v.get("id")?.as_str()?;
    uuid::Uuid::parse_str(id).ok()?;
    let age = now_secs().checked_sub(v.get("started_at")?.as_u64()?)?;
    (age < 600).then(|| (id.to_string(), std::time::Duration::from_secs(age)))
}

pub(crate) fn read_status(path: &Path) -> Option<String> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut text = String::new();
    file.by_ref()
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_BYTES {
        return None;
    }
    Some(text)
}

/// The newest of the two files the updater writes next to the requests directory: `update-state.json` and, for one
/// release, the older `update-status.json` (an old helper writes only that one). On a tie the state file wins.
fn newest_notice(dir: &Path) -> Option<serde_json::Value> {
    let read = |name: &str| -> Option<serde_json::Value> {
        serde_json::from_str(&read_status(&dir.join(name))?).ok()
    };
    let checked = |v: &serde_json::Value| v.get("checked_at").and_then(|c| c.as_u64());
    let state =
        read(crate::update_state::FILE).filter(|v| v.get("v").and_then(|v| v.as_u64()) == Some(1));
    let status = read("update-status.json");
    match (state, status) {
        (Some(a), Some(b)) => Some(if checked(&a) >= checked(&b) { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// The release the updater recorded as installable but not installed, or None. A notice stays valid until it is
/// superseded or installed; how old it is, is for the app to show (`checked_at`). A file that is large, unreadable or
/// not what the updater writes reads as None, and so does a version that was rolled back or skipped.
pub fn available_update(requests_dir: &Path, current: &str) -> Option<String> {
    let v = newest_notice(requests_dir.parent()?)?;
    let checked = v.get("checked_at")?.as_u64()?;
    if checked > now_secs() || !matches!(v.get("mode")?.as_str()?, "auto" | "notify") {
        return None;
    }
    // A notice left by another update channel is not an update this binary may install. Beta follows
    // stable (the beta alias can hold a stable release); stable and nightly follow only themselves.
    let recorded_channel: Channel = serde_json::from_value(v.get("channel")?.clone()).ok()?;
    if !Channel::of_version(current).trusts(recorded_channel) {
        return None;
    }
    let mut recorded = semver::Version::parse(v.get("current")?.as_str()?).ok()?;
    let mut running = semver::Version::parse(current).ok()?;
    recorded.build = semver::BuildMetadata::EMPTY;
    running.build = semver::BuildMetadata::EMPTY;
    if recorded != running {
        return None;
    }
    let version = v.get("available")?.as_str()?;
    if v.get("skipped").and_then(|s| s.as_str()) == Some(version) {
        return None;
    }
    let mut available = semver::Version::parse(version).ok()?;
    available.build = semver::BuildMetadata::EMPTY;
    if available <= running {
        return None;
    }
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
    let request: Request = serde_json::from_str(&text).map_err(|_| "not a known request")?;
    if let Request::UpdateNow { id: Some(id) } = &request {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err("invalid update id");
        }
    }
    Ok(request)
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
                Request::UpdateNow { id: None }
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
        let root = dir();
        let d = root.join("requests");
        let id = leave_update_now(&d).unwrap();
        assert_eq!(take_requests(&d), vec![Request::UpdateNow { id: Some(id) }]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_available_release_is_read_from_the_status_file_next_to_the_requests() {
        let d = dir().join("requests");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(available_update(&d, "0.1.53+build"), None);
        let status = d.parent().unwrap().join("update-status.json");
        write_json(
            &status,
            &serde_json::json!({"available":"0.1.54-beta.1", "current":"0.1.53-beta.1+helper", "checked_at":now_secs(), "channel":"beta", "mode":"notify"}),
        )
        .unwrap();
        assert_eq!(
            available_update(&d, "0.1.53-beta.1+build").as_deref(),
            Some("0.1.54-beta.1")
        );
        // A stable server must not offer a beta notice, while a beta server follows stable.
        write_json(
            &status,
            &serde_json::json!({"available":"0.1.54-beta.1", "current":"0.1.53+helper", "checked_at":now_secs(), "channel":"beta", "mode":"notify"}),
        )
        .unwrap();
        assert_eq!(available_update(&d, "0.1.53+build"), None);
        write_json(
            &status,
            &serde_json::json!({"available":"0.1.54", "current":"0.1.53-beta.1+helper", "checked_at":now_secs(), "channel":"stable", "mode":"notify"}),
        )
        .unwrap();
        assert_eq!(
            available_update(&d, "0.1.53-beta.1+build").as_deref(),
            Some("0.1.54")
        );
        write_json(
            &status,
            &serde_json::json!({"available":"0.1.54-beta.1", "current":"0.1.53-nightly.20261029+helper", "checked_at":now_secs(), "channel":"beta", "mode":"notify"}),
        )
        .unwrap();
        assert_eq!(available_update(&d, "0.1.53-nightly.20261029+build"), None);
        std::fs::write(&status, r#"{"available":"1.0 <b>"}"#).unwrap();
        assert_eq!(available_update(&d, "0.1.53"), None);
        std::fs::write(&status, "junk").unwrap();
        assert_eq!(available_update(&d, "0.1.53"), None);
        std::fs::remove_dir_all(d.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_state_file_is_read_beside_the_old_status_file_and_a_pinned_version_is_not_offered() {
        let d = dir();
        let requests = d.join("requests");
        std::fs::create_dir(&requests).unwrap();
        let status = d.join("update-status.json");
        let state = d.join(crate::update_state::FILE);
        let base = serde_json::json!({"v":1, "available":"0.2.0", "current":"0.1.0", "mode":"notify", "channel":"stable", "checked_at":now_secs()});
        // only the new file, only the old file
        write_json(&state, &base).unwrap();
        assert_eq!(
            available_update(&requests, "0.1.0").as_deref(),
            Some("0.2.0")
        );
        std::fs::remove_file(&state).unwrap();
        write_json(&status, &base).unwrap();
        assert_eq!(
            available_update(&requests, "0.1.0").as_deref(),
            Some("0.2.0")
        );
        // the newer of the two decides: a state that says up to date beats an older notice, and the reverse
        let mut none = base.clone();
        none["available"] = serde_json::Value::Null;
        none["checked_at"] = serde_json::json!(now_secs());
        write_json(&state, &none).unwrap();
        assert_eq!(available_update(&requests, "0.1.0"), None);
        none["checked_at"] = serde_json::json!(now_secs().saturating_sub(100));
        write_json(&state, &none).unwrap();
        assert_eq!(
            available_update(&requests, "0.1.0").as_deref(),
            Some("0.2.0")
        );
        // a pinned version is not offered
        let mut pinned = base.clone();
        pinned["skipped"] = serde_json::json!("0.2.0");
        write_json(&state, &pinned).unwrap();
        assert_eq!(available_update(&requests, "0.1.0"), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_check_is_coalesced_and_invalid_request_ids_are_rejected() {
        let d = dir();
        leave_update_check(&d).unwrap();
        leave_update_check(&d).unwrap();
        std::fs::write(
            d.join("bad.json"),
            r#"{"action":"update-now","id":"../bad"}"#,
        )
        .unwrap();
        assert_eq!(take_requests(&d), vec![Request::CheckUpdate {}]);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn old_equal_held_mismatched_and_future_notices_never_offer_an_update() {
        let d = dir();
        let requests = d.join("requests");
        std::fs::create_dir(&requests).unwrap();
        let status = d.join("update-status.json");
        let base = serde_json::json!({"available":"0.2.0", "current":"0.1.0", "checked_at":now_secs(), "mode":"notify"});
        for (key, value) in [
            ("checked_at", serde_json::json!(now_secs() + 60)),
            ("available", serde_json::json!("0.1.0+other")),
            ("available", serde_json::json!("0.0.9")),
            ("current", serde_json::json!("0.0.8")),
            ("mode", serde_json::json!("hold")),
        ] {
            let mut v = base.clone();
            v[key] = value;
            write_json(&status, &v).unwrap();
            assert_eq!(available_update(&requests, "0.1.0"), None);
        }
        // An old notice is still a notice: the timer no longer runs every hour.
        let mut old = base.clone();
        old["channel"] = serde_json::json!("stable");
        old["checked_at"] = serde_json::json!(now_secs().saturating_sub(7 * 86400));
        write_json(&status, &old).unwrap();
        assert_eq!(
            available_update(&requests, "0.1.0").as_deref(),
            Some("0.2.0")
        );
        std::fs::write(&status, " ".repeat(5000)).unwrap();
        assert_eq!(available_update(&requests, "0.1.0"), None);
        std::fs::remove_file(&status).unwrap();
        let target = d.join("secret");
        write_json(&target, &base).unwrap();
        std::os::unix::fs::symlink(&target, &status).unwrap();
        assert_eq!(available_update(&requests, "0.1.0"), None);
        assert!(target.exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_result_only_completes_the_request_it_names() {
        let d = dir();
        let requests = d.join("requests");
        std::fs::create_dir(&requests).unwrap();
        write_json(
            &d.join("update-result.json"),
            &UpdateResult {
                id: "first".into(),
                error: Some("held".into()),
            },
        )
        .unwrap();
        assert!(update_result(&requests, "second").is_none());
        assert_eq!(
            update_result(&requests, "first").unwrap().error.as_deref(),
            Some("held")
        );
        std::fs::remove_dir_all(d).unwrap();
    }
}
