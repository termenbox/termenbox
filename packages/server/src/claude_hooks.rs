//! "Connect Claude Code" (plan v6, P.3, decision 0079): adds Termenbox's hook entries to the user's Claude
//! Code settings, only when asked, only ours, with a backup and a journal; "Disconnect" removes only ours.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::agent_hooks::Reply;

/// Every event the turn-and-obligation reducer reads (`adapters::claude`). `PermissionRequest` is the one that
/// says a dialog is open (`Notification` follows it by seconds and covers only some of them); the rest end a wait
/// or a turn. Settings written by an earlier version lack the newer ones: `merge` adds them, nothing else.
pub const EVENTS: [&str; 17] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
    "Elicitation",
    "ElicitationResult",
    "Notification",
    "Stop",
    "StopFailure",
    // Subagent and compaction edges feed the status machine (fact 0073).
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
];
/// Marks an entry as ours; `remove` touches nothing else.
const MARKER: &str = "--hook claude";

pub fn hook_command(binary: &str) -> String {
    format!("{binary} {MARKER}")
}

fn is_ours(entry: &Value) -> bool {
    entry["hooks"]
        .as_array()
        .map(|h| {
            h.iter().any(|c| {
                c["command"]
                    .as_str()
                    .map(|s| s.contains(MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// The lines shown to the user before anything is written.
pub fn preview(binary: &str) -> String {
    let entry =
        json!({"hooks": [{"type": "command", "command": hook_command(binary), "timeout": 2}]});
    let mut hooks = serde_json::Map::new();
    for e in EVENTS {
        hooks.insert(e.into(), json!([entry]));
    }
    serde_json::to_string_pretty(&json!({"hooks": hooks})).unwrap()
}

/// Adds our entry to each event unless it is already there; everything else is kept as it is.
pub fn merge(existing: &str, binary: &str) -> Result<String> {
    let mut root: Value = if existing.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(existing)?
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("the settings file is not a JSON object"))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow!("`hooks` in the settings file is not an object"))?;
    let entry =
        json!({"hooks": [{"type": "command", "command": hook_command(binary), "timeout": 2}]});
    for e in EVENTS {
        let list = hooks.entry(e).or_insert_with(|| json!([]));
        let list = list
            .as_array_mut()
            .ok_or_else(|| anyhow!("`hooks.{e}` is not a list"))?;
        if !list.iter().any(is_ours) {
            list.push(entry.clone());
        }
    }
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

/// Removes our entries and whatever became empty because of that.
pub fn remove(existing: &str) -> Result<String> {
    let mut root: Value = serde_json::from_str(existing)?;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for e in EVENTS {
            if let Some(list) = hooks.get_mut(e).and_then(Value::as_array_mut) {
                list.retain(|x| !is_ours(x));
                if list.is_empty() {
                    hooks.remove(e);
                }
            }
        }
        if hooks.is_empty() {
            root.as_object_mut().unwrap().remove("hooks");
        }
    }
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Where Claude Code reads its user settings: `CLAUDE_CONFIG_DIR` when set (as Claude Code itself does, and as
/// the transcript reader does), else `~/.claude`.
pub fn default_settings_path() -> PathBuf {
    settings_in(config_dir(), std::env::var_os("HOME"))
}

/// Claude Code's config directory as the user's Claude Code has it: this process's `CLAUDE_CONFIG_DIR`, else the
/// one the running `claude` processes of this user agree on. A service started by systemd does not inherit the
/// variable a login shell sets, so without this the hooks land in `~/.claude` while Claude reads another dir.
pub fn config_dir() -> Option<std::ffi::OsString> {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(d);
    }
    detected_config_dir()
}

/// How long a look at the process table is kept: it is read once per request, never in a loop.
const DETECT_TTL: std::time::Duration = std::time::Duration::from_secs(10);

fn detected_config_dir() -> Option<std::ffi::OsString> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(std::time::Instant, Option<std::ffi::OsString>)>> =
        Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, v)) = cache.as_ref() {
        if at.elapsed() < DETECT_TTL {
            return v.clone();
        }
    }
    let v = scan_proc_for_config_dir();
    *cache = Some((std::time::Instant::now(), v.clone()));
    v
}

/// The values of `CLAUDE_CONFIG_DIR` in the environments of this user's `claude` processes (`None` for a process
/// without it).
#[cfg(target_os = "linux")]
fn scan_proc_for_config_dir() -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let me = std::fs::metadata("/proc/self").ok()?.uid();
    let mut seen = Vec::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        if !name.as_bytes().iter().all(u8::is_ascii_digit) {
            continue;
        }
        let dir = entry.path();
        if std::fs::metadata(&dir).map(|m| m.uid()).ok() != Some(me) {
            continue;
        }
        if std::fs::read(dir.join("comm"))
            .ok()
            .as_deref()
            .map(<[u8]>::trim_ascii)
            != Some(b"claude")
        {
            continue;
        }
        if let Ok(env) = std::fs::read(dir.join("environ")) {
            seen.push(config_dir_in_environ(&env));
            if seen.len() >= 64 {
                break;
            }
        }
    }
    agreed_dir(seen)
}

#[cfg(not(target_os = "linux"))]
fn scan_proc_for_config_dir() -> Option<std::ffi::OsString> {
    None
}

/// `CLAUDE_CONFIG_DIR` from a `/proc/<pid>/environ` blob, when it is a non-empty absolute path.
fn config_dir_in_environ(environ: &[u8]) -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStrExt;
    environ
        .split(|b| *b == 0)
        .find_map(|kv| kv.strip_prefix(b"CLAUDE_CONFIG_DIR="))
        .filter(|v| v.first() == Some(&b'/'))
        .map(|v| std::ffi::OsStr::from_bytes(v).to_os_string())
}

/// The directory every process agrees on; no answer when they differ or when some have none, since then
/// Claude Code is not using one place.
fn agreed_dir(seen: Vec<Option<std::ffi::OsString>>) -> Option<std::ffi::OsString> {
    let first = seen.first()?.clone()?;
    seen.iter()
        .all(|d| d.as_ref() == Some(&first))
        .then_some(first)
}

fn settings_in(
    config_dir: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    match config_dir.filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(home.unwrap_or_default()).join(".claude"),
    }
    .join("settings.json")
}

/// The user-scope journal of this one change: where the backup is and what the file hashed to after.
fn journal_path(dir: &Path) -> PathBuf {
    dir.join("claude-hooks.json")
}

/// The settings file the journal says `connect` changed; `None` without a journal. A later `--disconnect-claude`
/// follows it, so it undoes the change in the file it was made in whatever `CLAUDE_CONFIG_DIR` is then.
pub fn journal_settings(state_dir: &Path) -> Option<PathBuf> {
    let journal: Value =
        serde_json::from_str(&std::fs::read_to_string(journal_path(state_dir)).ok()?).ok()?;
    journal["settings"].as_str().map(PathBuf::from)
}

pub struct Outcome(pub String);

/// What the file looks like to the offer (U10): all of our entries are there, or they are not.
pub fn is_connected(settings: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    EVENTS.iter().all(|e| {
        root["hooks"][*e]
            .as_array()
            .is_some_and(|l| l.iter().any(is_ours))
    })
}

/// The whole settings file as `connect(apply = true)` will write it: this one text is both what the owner is
/// shown and what ends up on disk, so the preview cannot drift from the change.
pub fn planned_text(settings: &Path, binary: &str) -> Result<String> {
    let text = std::fs::read(settings)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    merge(&text, binary)
}

/// Where a control request from a client points: the settings file, the binary the entries call, and the
/// directory of the backup and the journal.
#[derive(Clone, Debug)]
pub struct Target {
    pub settings: PathBuf,
    /// No file was named by the owner: find Claude Code's settings at each request (`default_settings_path`).
    pub auto: bool,
    pub binary: String,
    pub state_dir: PathBuf,
}

/// Whether Claude Code has a verified hook surface (fact 0038).
pub fn supported(agent: &str) -> bool {
    agent == "claude"
}

/// Handles one `status`, `preview`, `apply` or `remove` for `agent`. File contents never leave this function
/// except as the preview the owner asked for; callers log the event only.
pub fn respond(target: Option<&Target>, agent: &str, action: &str) -> Reply {
    if !supported(agent) {
        return Reply::unsupported();
    }
    let Some(t) = target else {
        return Reply::unavailable();
    };
    let settings = if t.auto {
        default_settings_path()
    } else {
        t.settings.clone()
    };
    let state = |settings: &Path| {
        if is_connected(settings) {
            "connected"
        } else {
            "not_connected"
        }
    };
    let (preview, error) = match action {
        "status" => (None, None),
        "preview" => match planned_text(&settings, &t.binary) {
            Ok(text) => (Some(text), None),
            Err(e) => (None, Some(e.to_string())),
        },
        "apply" => match connect(&settings, &t.binary, &t.state_dir, true) {
            Ok(_) => (None, None),
            Err(e) => (None, Some(e.to_string())),
        },
        // A removal follows the file the connect changed (the journal says which), wherever the settings are now.
        "remove" => {
            let file = journal_settings(&t.state_dir)
                .filter(|f| f.exists())
                .unwrap_or_else(|| settings.clone());
            if !file.exists() {
                (None, None)
            } else {
                match disconnect(&file, &t.state_dir) {
                    Ok(_) => (None, None),
                    Err(e) => (None, Some(e.to_string())),
                }
            }
        }
        _ => (None, Some("unknown action".to_string())),
    };
    Reply {
        state: state(&settings),
        preview,
        error,
    }
}

/// With `apply == false` only the preview is returned (the dry run the guardrail asks for).
pub fn connect(settings: &Path, binary: &str, state_dir: &Path, apply: bool) -> Result<Outcome> {
    let before = std::fs::read(settings).ok();
    let merged = planned_text(settings, binary)?;
    if !apply {
        return Ok(Outcome(format!(
            "Would add these hooks to {} (nothing else changes; a backup is kept):\n{}\nRun again with --yes to apply.",
            settings.display(),
            preview(binary)
        )));
    }
    std::fs::create_dir_all(state_dir)?;
    let jp = journal_path(state_dir);
    // Keep the first backup: a second connect must not overwrite the original bytes.
    let backup = state_dir.join("claude-settings.backup");
    let prior: Option<Value> = std::fs::read_to_string(&jp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    if prior.is_none() {
        if let Some(b) = &before {
            std::fs::write(&backup, b)?;
        }
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = settings.with_extension("json.termenbox-tmp");
    std::fs::write(&tmp, &merged)?;
    std::fs::rename(&tmp, settings)?;
    let entry = prior.unwrap_or_else(|| {
        json!({
            "settings": settings.display().to_string(),
            "existed": before.is_some(),
            "backup": backup.display().to_string(),
            "before_sha256": before.as_deref().map(sha),
        })
    });
    let mut entry = entry;
    entry["after_sha256"] = json!(sha(merged.as_bytes()));
    std::fs::write(&jp, serde_json::to_string_pretty(&entry)?)?;
    Ok(Outcome(format!(
        "Connected: hooks added to {}. Undo with --disconnect-claude.",
        settings.display()
    )))
}

/// Restores the original bytes when only we have changed the file since; otherwise removes just our entries.
pub fn disconnect(settings: &Path, state_dir: &Path) -> Result<Outcome> {
    let jp = journal_path(state_dir);
    let journal: Option<Value> = std::fs::read_to_string(&jp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let current = match std::fs::read(settings) {
        Ok(c) => c,
        // Nothing was connected here, or the file is gone with whatever it held: there is nothing to undo, and
        // an uninstall must not fail on it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let _ = std::fs::remove_file(&jp);
            return Ok(Outcome(format!(
                "{} does not exist; nothing to disconnect.",
                settings.display()
            )));
        }
        Err(e) => return Err(anyhow!("cannot read {}: {e}", settings.display())),
    };
    // Without a journal (the state was removed first) only entries that are ours can be taken out, and a file that
    // has none is left byte for byte as it is.
    if journal.is_none() && !String::from_utf8_lossy(&current).contains(MARKER) {
        return Ok(Outcome("No Termenbox hooks in the settings file.".into()));
    }
    let untouched =
        journal.as_ref().and_then(|j| j["after_sha256"].as_str()) == Some(sha(&current).as_str());
    if let (true, Some(j)) = (untouched, &journal) {
        if j["existed"] == json!(true) {
            std::fs::copy(j["backup"].as_str().unwrap_or_default(), settings)?;
        } else {
            std::fs::remove_file(settings)?;
        }
        let _ = std::fs::remove_file(&jp);
        return Ok(Outcome(
            "Disconnected: the settings file is back to what it was.".into(),
        ));
    }
    let cleaned = remove(&String::from_utf8_lossy(&current))?;
    std::fs::write(settings, cleaned)?;
    let _ = std::fs::remove_file(&jp);
    Ok(Outcome(
        "Disconnected: removed only Termenbox's hooks; your other changes stay.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/usr/local/bin/termenbox-server";
    const MINE: &str = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "my-linter"}]}],
    "Custom": [{"hooks": []}]
  }
}
"#;

    fn tmpdir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tm-ch-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn merge_keeps_unrelated_settings_and_is_idempotent() {
        let m = merge(MINE, BIN).unwrap();
        let v: Value = serde_json::from_str(&m).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "my-linter"
        );
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["Custom"], json!([{"hooks": []}]));
        for e in EVENTS {
            assert!(v["hooks"][e].as_array().unwrap().iter().any(is_ours), "{e}");
        }
        assert_eq!(merge(&m, BIN).unwrap(), m);
        assert!(merge("[1]", BIN).is_err());
    }

    #[test]
    fn remove_takes_out_only_ours() {
        let back: Value =
            serde_json::from_str(&remove(&merge(MINE, BIN).unwrap()).unwrap()).unwrap();
        assert_eq!(back, serde_json::from_str::<Value>(MINE).unwrap());
        assert_eq!(
            serde_json::from_str::<Value>(&remove(&merge("", BIN).unwrap()).unwrap()).unwrap(),
            json!({})
        );
    }

    #[test]
    fn disconnect_restores_the_original_bytes_when_nothing_else_changed() {
        let d = tmpdir("a");
        let settings = d.join("settings.json");
        std::fs::write(&settings, MINE).unwrap();
        assert!(connect(&settings, BIN, &d.join("state"), false)
            .unwrap()
            .0
            .contains("--yes"));
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            MINE,
            "a dry run changes nothing"
        );
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        assert!(std::fs::read_to_string(&settings).unwrap().contains(MARKER));
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        disconnect(&settings, &d.join("state")).unwrap();
        assert_eq!(std::fs::read(&settings).unwrap(), MINE.as_bytes());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disconnect_after_the_user_edited_the_file_keeps_their_edit() {
        let d = tmpdir("b");
        let settings = d.join("settings.json");
        std::fs::write(&settings, MINE).unwrap();
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        let edited = std::fs::read_to_string(&settings)
            .unwrap()
            .replace("\"opus\"", "\"sonnet\"");
        std::fs::write(&settings, edited).unwrap();
        disconnect(&settings, &d.join("state")).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["model"], "sonnet");
        assert!(!serde_json::to_string(&v).unwrap().contains(MARKER));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disconnect_without_a_journal_takes_out_ours_and_leaves_a_clean_file_untouched() {
        let d = tmpdir("nj");
        let settings = d.join("settings.json");
        std::fs::write(&settings, MINE).unwrap();
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        // The state directory was removed before the hooks were: no journal, no backup.
        std::fs::remove_dir_all(d.join("state")).unwrap();
        disconnect(&settings, &d.join("state")).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v, serde_json::from_str::<Value>(MINE).unwrap());
        let odd = "{ \"model\":   \"opus\" }";
        std::fs::write(&settings, odd).unwrap();
        disconnect(&settings, &d.join("state")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            odd,
            "no hooks of ours: not rewritten"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disconnect_of_a_missing_settings_file_is_not_an_error_and_the_journal_names_the_file() {
        let d = tmpdir("ms");
        let settings = d.join("elsewhere").join("settings.json");
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        assert_eq!(journal_settings(&d.join("state")), Some(settings.clone()));
        std::fs::remove_file(&settings).unwrap();
        assert!(disconnect(&settings, &d.join("state")).is_ok());
        assert_eq!(journal_settings(&d.join("state")), None);
        assert!(disconnect(&d.join("nope.json"), &d.join("state")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn connect_preview_equals_what_apply_writes_and_remove_restores() {
        let d = tmpdir("p");
        let settings = d.join("settings.json");
        std::fs::write(&settings, MINE).unwrap();
        let t = Target {
            settings: settings.clone(),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        assert_eq!(respond(Some(&t), "claude", "status").state, "not_connected");
        let shown = respond(Some(&t), "claude", "preview").preview.unwrap();
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            MINE,
            "a preview changes nothing"
        );
        assert_eq!(respond(Some(&t), "claude", "apply").state, "connected");
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            shown,
            "the file holds exactly the preview"
        );
        assert!(shown.contains("my-linter") && shown.contains("\"model\": \"opus\""));
        assert_eq!(respond(Some(&t), "claude", "remove").state, "not_connected");
        assert_eq!(std::fs::read(&settings).unwrap(), MINE.as_bytes());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn connect_requests_for_other_agents_or_without_a_target_touch_nothing() {
        let d = tmpdir("q");
        let t = Target {
            settings: d.join("settings.json"),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        assert_eq!(respond(Some(&t), "codex", "apply").state, "unsupported");
        assert_eq!(respond(None, "claude", "apply").state, "unavailable");
        assert!(respond(Some(&t), "claude", "frobnicate").error.is_some());
        assert!(!t.settings.exists());
        assert_eq!(respond(Some(&t), "claude", "remove").state, "not_connected");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn settings_live_in_the_claude_config_dir_when_it_is_set_else_under_home() {
        let os = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(
            settings_in(os("/data/.claude"), os("/home/u")),
            PathBuf::from("/data/.claude/settings.json")
        );
        assert_eq!(
            settings_in(None, os("/home/u")),
            PathBuf::from("/home/u/.claude/settings.json")
        );
        assert_eq!(
            settings_in(os(""), os("/home/u")),
            PathBuf::from("/home/u/.claude/settings.json")
        );
    }

    #[test]
    fn connecting_when_no_settings_file_exists_creates_and_disconnect_removes_it() {
        let d = tmpdir("c");
        let settings = d.join("home/.claude/settings.json");
        connect(&settings, BIN, &d.join("state"), true).unwrap();
        assert!(settings.exists());
        disconnect(&settings, &d.join("state")).unwrap();
        assert!(!settings.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn os(p: &str) -> Option<std::ffi::OsString> {
        Some(std::ffi::OsString::from(p))
    }

    #[test]
    fn the_config_dir_is_read_from_a_process_environment_when_it_is_absolute() {
        let env = b"HOME=/home/u\0CLAUDE_CONFIG_DIR=/data/.claude\0TERM=xterm\0";
        assert_eq!(config_dir_in_environ(env), os("/data/.claude"));
        assert_eq!(config_dir_in_environ(b"HOME=/home/u\0"), None);
        assert_eq!(config_dir_in_environ(b"CLAUDE_CONFIG_DIR=\0"), None);
        assert_eq!(
            config_dir_in_environ(b"CLAUDE_CONFIG_DIR=relative/dir\0"),
            None
        );
    }

    #[test]
    fn processes_must_agree_on_the_config_dir() {
        let a = os("/data/.claude");
        assert_eq!(agreed_dir(vec![a.clone(), a.clone()]), a);
        assert_eq!(
            agreed_dir(vec![a.clone(), os("/other")]),
            None,
            "two places"
        );
        assert_eq!(
            agreed_dir(vec![a.clone(), None]),
            None,
            "one uses the default"
        );
        assert_eq!(agreed_dir(vec![]), None);
    }

    #[test]
    fn a_target_without_a_named_file_finds_the_settings_at_each_request() {
        let d = std::env::temp_dir().join(format!("tm-auto-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let named = d.join("named.json");
        let t = Target {
            settings: named.clone(),
            auto: false,
            binary: "/bin/tb".into(),
            state_dir: d.clone(),
        };
        assert_eq!(respond(Some(&t), "claude", "apply").error, None);
        assert!(named.exists(), "a named file is used as it is");
        assert_eq!(respond(Some(&t), "claude", "remove").error, None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
