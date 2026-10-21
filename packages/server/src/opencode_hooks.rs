//! "Connect OpenCode" (decisions 0079 and 0106): writes one plugin into the user's OpenCode config directory,
//! only when asked, only ours, with a backup and a journal; "Disconnect" removes it again.
//!
//! OpenCode has no hooks in a settings file. It loads JavaScript/TypeScript plugins from
//! `<config>/plugins/`; the plugin subscribes to the same session and permission events the TUI sees and feeds
//! them to `termenbox-server --hook opencode` through the environment Termenbox puts in each session.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::agent_hooks::Reply;

/// Marks the file as ours. `disconnect` removes a file with this marker and nothing else.
pub const MARKER: &str = "termenbox-opencode-plugin";

/// The plugin shipped to the owner's config directory. It forwards only the events the adapter understands;
/// OpenCode can be restarted at any time, so the plugin keeps no state on disk.
///
/// The binary is embedded when the owner connects: a later server upgrade rewrites the plugin with the new path
/// the same way `--connect-claude` rewrites its hook command.
const TEMPLATE: &str = r#"// termenbox-opencode-plugin: written by `termenbox-server --connect-opencode`; remove with --disconnect-opencode.
// Reports OpenCode's session, tool, permission and question events to the Termenbox session it was started in.
// It does nothing when OpenCode runs outside Termenbox (the TERMENBOX_* variables are absent).
const BINARY = __TERMENBOX_BINARY__;
// Tool names are remembered between the input event and the call it starts; a long session must not grow it.
const MAX_TOOLS = 64;

const FORWARD = new Set([
  "session.inbox.enqueued",
  "session.execution.started",
  "session.execution.succeeded",
  "session.execution.failed",
  "session.execution.interrupted",
  "permission.asked",
  "permission.replied",
  "form.created",
  "form.replied",
  "form.cancelled",
  "session.tool.called",
  "session.tool.success",
  "session.tool.failed",
]);

export default {
  id: "termenbox",
  async setup(ctx) {
    if (!process.env.TERMENBOX_SESSION || !process.env.TERMENBOX_EVENT_TOKEN || !process.env.TERMENBOX_EVENT_SOCKET) return;
    const tools = new Map();
    const toolKey = (data) => `${data.sessionID}:${data.id}`;
    const cap = (value, max) => (typeof value === "string" ? value.slice(0, max) : value);
    // Keep only the fields the adapter reads. Tool output and provider metadata can be megabytes, and the hook
    // accepts at most 64 KiB of one event; the call's name is added here because OpenCode omits it from the
    // events that start and end a tool. Every string is capped so one long message cannot push the frame past
    // the line limit, and the update is dropped as invalid JSON.
    const compact = (type, data, toolName) => {
      switch (type) {
        case "session.inbox.enqueued":
          return { item: { type: data.item?.type, payload: { text: cap(data.item?.payload?.text, 2000) } } };
        case "session.execution.failed":
          return { error: { message: cap(data.error?.message, 2000) } };
        case "permission.asked":
          return {
            id: data.id,
            action: data.action,
            resources: Array.isArray(data.resources)
              ? data.resources.slice(0, 8).map((r) => cap(String(r), 500))
              : data.resources,
            message: cap(data.message, 2000),
          };
        case "permission.replied":
          return { requestID: data.requestID };
        case "form.created":
          return {
            form: {
              id: data.form?.id,
              title: cap(data.form?.title, 500),
              fields: (Array.isArray(data.form?.fields) ? data.form.fields : [])
                .slice(0, 8)
                .map((f) => ({ title: cap(f?.title, 200), description: cap(f?.description, 1000) })),
            },
          };
        case "form.replied":
        case "form.cancelled":
          return { id: data.id };
        case "session.tool.called": {
          const input = {};
          for (const k of ["command", "filePath", "path", "pattern", "url", "query"]) {
            if (typeof data.input?.[k] === "string") input[k] = data.input[k].slice(0, 4000);
          }
          return { id: data.id, sessionID: data.sessionID, toolName, input };
        }
        case "session.tool.success":
        case "session.tool.failed":
          return { id: data.id, sessionID: data.sessionID, toolName };
        default:
          return { sessionID: data.sessionID };
      }
    };
    const send = (frame) => {
      try {
        const child = Bun.spawn([BINARY, "--hook", "opencode"], {
          stdin: "pipe",
          stdout: "ignore",
          stderr: "ignore",
          env: process.env,
        });
        if (!child.stdin) {
          child.kill();
          return;
        }
        child.stdin.write(JSON.stringify(frame));
        child.stdin.end();
        child.exited.catch(() => {});
      } catch {
        // Termenbox must never hold up the agent.
      }
    };
    const controller = new AbortController();
    (async () => {
      try {
        for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
          const data = event && event.data;
          if (!data) continue;
          // Tool events carry an id but not always a name; remember it from the input event and add it to the
          // call that starts and ends the tool. The adapter then has the name for its block key and summary.
          if (event.type === "session.tool.input.started" && data.id) {
            tools.set(toolKey(data), cap(data.name, 100));
            if (tools.size > MAX_TOOLS) tools.delete(tools.keys().next().value);
          }
          let toolName;
          if (data.id && (event.type === "session.tool.called" || event.type === "session.tool.success" || event.type === "session.tool.failed")) {
            toolName = tools.get(toolKey(data));
            if (event.type !== "session.tool.called") tools.delete(toolKey(data));
          }
          if (FORWARD.has(event.type)) send({ type: event.type, data: compact(event.type, data, toolName) });
        }
      } catch {
        // A closed subscription is an OpenCode shutdown, not an error to report.
      }
    })();
    return () => controller.abort();
  },
};
"#;

pub fn plugin_source(binary: &str) -> String {
    let literal = serde_json::to_string(binary).unwrap_or_else(|_| "\"termenbox-server\"".into());
    TEMPLATE.replace("__TERMENBOX_BINARY__", &literal)
}

/// Where OpenCode reads global plugins: `OPENCODE_CONFIG_DIR` when the user set one (the custom directory is
/// searched for plugins just like `~/.config/opencode`), else the directory a running `opencode` process uses,
/// else `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`.
pub fn default_plugin_path() -> PathBuf {
    let home = std::env::var_os("HOME");
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty());
    let custom = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|d| !d.is_empty());
    // The service may not have the variable; a running OpenCode process does.
    plugin_in(custom.or_else(detected_config_dir), xdg, home)
}

fn plugin_in(
    custom: Option<std::ffi::OsString>,
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    let dir = match custom.filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => match xdg.filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d).join("opencode"),
            None => PathBuf::from(home.unwrap_or_default()).join(".config/opencode"),
        },
    };
    dir.join("plugins").join("termenbox.js")
}

/// How long a look at the process table is kept: it is read once per request, never in a loop.
const DETECT_TTL: Duration = Duration::from_secs(10);

/// The config directory the owner's running OpenCode uses. A service started by systemd does not inherit the
/// login shell's `OPENCODE_CONFIG_DIR`/`XDG_CONFIG_HOME`, so looking only in this process can write the plugin
/// where the user's OpenCode never reads it.
fn detected_config_dir() -> Option<std::ffi::OsString> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(Instant, Option<std::ffi::OsString>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, v)) = cache.as_ref() {
        if at.elapsed() < DETECT_TTL {
            return v.clone();
        }
    }
    let v = scan_proc_for_config_dir();
    *cache = Some((Instant::now(), v.clone()));
    v
}

/// The config directory from the environments of this user's `opencode` processes (`None` for a process whose
/// environment names none).
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
            != Some(b"opencode")
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

/// The config directory OpenCode would use from one `/proc/<pid>/environ` blob, with OpenCode's precedence:
/// `OPENCODE_CONFIG_DIR`, then `$XDG_CONFIG_HOME/opencode`, then `$HOME/.config/opencode`.
#[cfg(target_os = "linux")]
fn config_dir_in_environ(environ: &[u8]) -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStrExt;
    let value = |key: &[u8]| {
        environ
            .split(|b| *b == 0)
            .find_map(|kv| kv.strip_prefix(key))
            .filter(|v| v.first() == Some(&b'/'))
            .map(|v| std::ffi::OsStr::from_bytes(v).to_os_string())
    };
    if let Some(dir) = value(b"OPENCODE_CONFIG_DIR=") {
        return Some(dir);
    }
    if let Some(xdg) = value(b"XDG_CONFIG_HOME=") {
        return Some(PathBuf::from(xdg).join("opencode").into_os_string());
    }
    value(b"HOME=").map(|home| {
        PathBuf::from(home)
            .join(".config/opencode")
            .into_os_string()
    })
}

/// The directory every process agrees on; no answer when they differ or when some have none.
#[cfg(target_os = "linux")]
fn agreed_dir(seen: Vec<Option<std::ffi::OsString>>) -> Option<std::ffi::OsString> {
    let first = seen.first()?.clone()?;
    seen.iter()
        .all(|d| d.as_ref() == Some(&first))
        .then_some(first)
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The user-scope journal of this one change: the file it wrote and the backup it kept.
fn journal_path(state_dir: &Path) -> PathBuf {
    state_dir.join("opencode-plugin.json")
}

/// The plugin file the journal says `connect` wrote; `None` without a journal. A later `--disconnect-opencode`
/// follows it, so it undoes the change in the file it was made in whatever the environment is then.
pub fn journal_plugin(state_dir: &Path) -> Option<PathBuf> {
    let journal: Value =
        serde_json::from_str(&std::fs::read_to_string(journal_path(state_dir)).ok()?).ok()?;
    journal["plugin"].as_str().map(PathBuf::from)
}

pub struct Outcome(pub String);

/// What the file looks like to the offer: our plugin is there.
pub fn is_connected(plugin: &Path) -> bool {
    std::fs::read_to_string(plugin)
        .map(|text| text.contains(MARKER))
        .unwrap_or(false)
}

/// The whole plugin file as `connect(apply = true)` will write it. Refuses to replace a file someone else
/// wrote at our path.
pub fn planned_text(plugin: &Path, binary: &str) -> Result<String> {
    match std::fs::read(plugin) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) if text.contains(MARKER) => {}
            Ok(_) => {
                return Err(anyhow!(
                    "{} already exists and was not written by Termenbox; move it first",
                    plugin.display()
                ))
            }
            // A file we cannot read as text is not a plugin of ours; replacing it would destroy it silently.
            Err(_) => {
                return Err(anyhow!(
                    "{} already exists but is not text; move it first",
                    plugin.display()
                ))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(anyhow!("cannot read {}: {e}", plugin.display())),
    }
    Ok(plugin_source(binary))
}

/// Where a control request from a client points: the plugin file, the binary it calls, and the directory of the
/// backup and the journal.
#[derive(Clone, Debug)]
pub struct Target {
    pub plugin: PathBuf,
    /// No file was named by the owner: find OpenCode's plugin path at each request (`default_plugin_path`).
    pub auto: bool,
    pub binary: String,
    pub state_dir: PathBuf,
}

/// Only OpenCode's plugin surface is installed here.
pub fn supported(agent: &str) -> bool {
    agent == "opencode"
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
    let plugin = if t.auto {
        default_plugin_path()
    } else {
        t.plugin.clone()
    };
    let state = |plugin: &Path| {
        if is_connected(plugin) {
            "connected"
        } else {
            "not_connected"
        }
    };
    let (preview, error) = match action {
        "status" => (None, None),
        "preview" => match planned_text(&plugin, &t.binary) {
            Ok(text) => (Some(text), None),
            Err(e) => (None, Some(e.to_string())),
        },
        "apply" => match connect(&plugin, &t.binary, &t.state_dir, true) {
            Ok(_) => (None, None),
            Err(e) => (None, Some(e.to_string())),
        },
        // A removal follows the file the connect changed (the journal says which), wherever the config is now.
        // Without a journal it uses the path this request resolved.
        "remove" => {
            let file = journal_plugin(&t.state_dir).unwrap_or_else(|| plugin.clone());
            match disconnect(&file, &t.state_dir) {
                Ok(_) => (None, None),
                Err(e) => (None, Some(e.to_string())),
            }
        }
        _ => (None, Some("unknown action".to_string())),
    };
    Reply {
        state: state(&plugin),
        preview,
        error,
    }
}

/// With `apply == false` only the preview is returned (the dry run the guardrail asks for).
pub fn connect(plugin: &Path, binary: &str, state_dir: &Path, apply: bool) -> Result<Outcome> {
    let before = std::fs::read(plugin).ok();
    let text = planned_text(plugin, binary)?;
    if !apply {
        return Ok(Outcome(format!(
            "Would write the Termenbox OpenCode plugin to {} (nothing else changes; a backup is kept):\n{}\nRun again with --yes to apply.",
            plugin.display(),
            text
        )));
    }
    std::fs::create_dir_all(state_dir)?;
    let jp = journal_path(state_dir);
    // Keep the first backup: a second connect must not overwrite the original bytes.
    let backup = state_dir.join("opencode-termenbox.backup");
    let prior: Option<Value> = std::fs::read_to_string(&jp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    // The journal names one file: the one the last connect wrote and a disconnect will undo. When the owner's
    // config directory changed, carrying the old entry over would make disconnect follow the old path and report
    // success while leaving the new plugin installed. Start a new entry instead, with that file's own backup.
    let prior = prior.filter(|j| j["plugin"].as_str().is_some_and(|p| Path::new(p) == plugin));
    if prior.is_none() {
        if let Some(b) = &before {
            std::fs::write(&backup, b)?;
        } else {
            let _ = std::fs::remove_file(&backup);
        }
    }
    if let Some(parent) = plugin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = plugin.with_extension("js.termenbox-tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, plugin)?;
    let entry = prior.unwrap_or_else(|| {
        json!({
            "plugin": plugin.display().to_string(),
            "existed": before.is_some(),
            "backup": backup.display().to_string(),
            "before_sha256": before.as_deref().map(sha),
        })
    });
    let mut entry = entry;
    entry["after_sha256"] = json!(sha(text.as_bytes()));
    std::fs::write(&jp, serde_json::to_string_pretty(&entry)?)?;
    Ok(Outcome(format!(
        "Connected: OpenCode plugin written to {}. Restart OpenCode; undo with --disconnect-opencode.",
        plugin.display()
    )))
}

/// Restores the original file when only we have changed it since; otherwise removes our plugin file. A file the
/// owner edited after we wrote it is left in place (with the journal), so their edit is not destroyed.
pub fn disconnect(plugin: &Path, state_dir: &Path) -> Result<Outcome> {
    let jp = journal_path(state_dir);
    let journal: Option<Value> = std::fs::read_to_string(&jp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let current = match std::fs::read(plugin) {
        Ok(c) => c,
        // Nothing was connected here, or the file is gone with whatever it held: there is nothing to undo, and
        // an uninstall must not fail on it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let _ = std::fs::remove_file(&jp);
            return Ok(Outcome(format!(
                "{} does not exist; nothing to disconnect.",
                plugin.display()
            )));
        }
        Err(e) => return Err(anyhow!("cannot read {}: {e}", plugin.display())),
    };
    // A file that someone else replaced is left alone: it is no longer ours to remove.
    if !String::from_utf8_lossy(&current).contains(MARKER) {
        let _ = std::fs::remove_file(&jp);
        return Ok(Outcome(format!(
            "No Termenbox OpenCode plugin in {}.",
            plugin.display()
        )));
    }
    let untouched =
        journal.as_ref().and_then(|j| j["after_sha256"].as_str()) == Some(sha(&current).as_str());
    if let (true, Some(j)) = (untouched, &journal) {
        if j["existed"] == json!(true) {
            std::fs::copy(j["backup"].as_str().unwrap_or_default(), plugin)?;
        } else {
            std::fs::remove_file(plugin)?;
        }
        let _ = std::fs::remove_file(&jp);
        return Ok(Outcome(
            "Disconnected: the OpenCode plugin file is back to what it was.".into(),
        ));
    }
    // The owner edited the file after Termenbox wrote it. Deleting it would throw their work away, so leave both
    // the file and the journal: removing the file by hand still lets a later disconnect clear the journal.
    Err(anyhow!(
        "{} changed since Termenbox wrote it; leaving it in place. Remove it by hand, then run --disconnect-opencode again.",
        plugin.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/usr/local/bin/termenbox-server";

    fn tmpdir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tm-oc-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_plugin_is_ours_forwards_the_events_and_embeds_the_binary() {
        let text = plugin_source(BIN);
        assert!(text.contains(MARKER));
        assert!(text.contains("\"/usr/local/bin/termenbox-server\""));
        assert!(text.contains("export default"));
        assert!(text.contains("ctx.event.subscribe"));
        assert!(text.contains("TERMENBOX_SESSION"));
        // Only the adapter's fields are forwarded: a tool result can be far larger than the hook's 64 KiB line.
        assert!(text.contains("const compact"));
        assert!(!text.contains("JSON.stringify(event)"));
        for e in [
            "session.execution.started",
            "session.execution.succeeded",
            "permission.asked",
            "form.created",
            "session.tool.called",
        ] {
            assert!(text.contains(e), "{e}");
        }
        // `session.idle` repeats the turn end that execution.* already carries; forwarding it would let a late
        // event downgrade Done to Idle after the adapter closed the turn.
        assert!(!text.contains("session.idle"));
        // Bounded frames and a bounded remembered-tool map: neither can grow with a long session.
        assert!(text.contains("const cap ="));
        assert!(text.contains("const MAX_TOOLS ="));
        assert!(text.contains("tools.delete(tools.keys().next().value)"));
        // A path with a quote cannot break out of the JavaScript string.
        let odd = plugin_source("/tmp/a\"b/termenbox-server");
        assert!(odd.contains("/tmp/a\\\"b/termenbox-server"));
    }

    #[test]
    fn the_generated_plugin_is_valid_javascript() {
        let d = tmpdir("js");
        let file = d.join("plugin.mjs");
        std::fs::write(&file, plugin_source(BIN)).unwrap();
        match std::process::Command::new("node")
            .arg("--check")
            .arg(&file)
            .output()
        {
            Ok(out) => assert!(
                out.status.success(),
                "node --check: {}",
                String::from_utf8_lossy(&out.stderr)
            ),
            // The Rust CI image has no Node; local and --full builds do, and there the check is real.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("node is not installed; skipping the plugin syntax check")
            }
            Err(e) => panic!("{e}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_opencode_process_names_its_config_directory() {
        let parse = |s: &str| config_dir_in_environ(s.as_bytes());
        let some = |p: &str| Some(PathBuf::from(p).into_os_string());
        assert_eq!(
            parse("OPENCODE_CONFIG_DIR=/data/oc\0XDG_CONFIG_HOME=/xdg\0HOME=/home/u\0"),
            some("/data/oc")
        );
        assert_eq!(
            parse("XDG_CONFIG_HOME=/xdg\0HOME=/home/u\0"),
            some("/xdg/opencode")
        );
        assert_eq!(parse("HOME=/home/u\0"), some("/home/u/.config/opencode"));
        // A relative value is ignored, exactly as OpenCode would not find it.
        assert_eq!(
            parse("OPENCODE_CONFIG_DIR=relative\0HOME=/home/u\0"),
            some("/home/u/.config/opencode")
        );
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_plugin_path_follows_the_opencode_config_environment() {
        let os = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(
            plugin_in(os("/data/oc"), os("/xdg"), os("/home/u")),
            PathBuf::from("/data/oc/plugins/termenbox.js")
        );
        assert_eq!(
            plugin_in(None, os("/xdg"), os("/home/u")),
            PathBuf::from("/xdg/opencode/plugins/termenbox.js")
        );
        assert_eq!(
            plugin_in(None, None, os("/home/u")),
            PathBuf::from("/home/u/.config/opencode/plugins/termenbox.js")
        );
        assert_eq!(
            plugin_in(os(""), os(""), os("/home/u")),
            PathBuf::from("/home/u/.config/opencode/plugins/termenbox.js")
        );
    }

    #[test]
    fn connect_preview_equals_what_apply_writes_and_remove_restores() {
        let d = tmpdir("p");
        let plugin = d.join("opencode/plugins/termenbox.js");
        let t = Target {
            plugin: plugin.clone(),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        assert_eq!(
            respond(Some(&t), "opencode", "status").state,
            "not_connected"
        );
        let shown = respond(Some(&t), "opencode", "preview").preview.unwrap();
        assert!(!plugin.exists(), "a preview changes nothing");
        assert_eq!(respond(Some(&t), "opencode", "apply").state, "connected");
        assert_eq!(
            std::fs::read_to_string(&plugin).unwrap(),
            shown,
            "the file holds exactly the preview"
        );
        assert_eq!(respond(Some(&t), "opencode", "apply").state, "connected");
        assert_eq!(
            respond(Some(&t), "opencode", "remove").state,
            "not_connected"
        );
        assert!(!plugin.exists(), "the file did not exist before connect");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disconnect_restores_a_users_file_byte_for_byte_when_nothing_else_changed() {
        let d = tmpdir("b");
        let plugin = d.join("opencode/plugins/termenbox.js");
        std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        let mine = "// my own plugin\nexport default {};\n";
        std::fs::write(&plugin, mine).unwrap();
        // A file that is not ours is never overwritten, in a preview or in an apply.
        let t = Target {
            plugin: plugin.clone(),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        assert!(respond(Some(&t), "opencode", "preview").error.is_some());
        assert!(respond(Some(&t), "opencode", "apply").error.is_some());
        assert_eq!(std::fs::read_to_string(&plugin).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_user_file_that_looks_like_ours_is_restored_after_connect_disconnect() {
        let d = tmpdir("r");
        let plugin = d.join("plugins/termenbox.js");
        std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        let mine = format!("// {MARKER}, my notes\nexport default {{}};\n");
        std::fs::write(&plugin, &mine).unwrap();
        connect(&plugin, BIN, &d.join("state"), true).unwrap();
        disconnect(&plugin, &d.join("state")).unwrap();
        assert_eq!(std::fs::read_to_string(&plugin).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_second_connect_at_another_path_is_the_one_disconnect_undoes() {
        let d = tmpdir("mv");
        let state = d.join("state");
        let a = d.join("a/plugins/termenbox.js");
        let b = d.join("b/plugins/termenbox.js");
        connect(&a, BIN, &state, true).unwrap();
        connect(&b, BIN, &state, true).unwrap();
        // The journal describes one file: the one the latest connect wrote.
        assert_eq!(journal_plugin(&state), Some(b.clone()));
        disconnect(&b, &state).unwrap();
        assert!(!b.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plugin_changed_since_it_was_written_is_left_for_the_owner() {
        let d = tmpdir("edit");
        let plugin = d.join("plugins/termenbox.js");
        let state = d.join("state");
        connect(&plugin, BIN, &state, true).unwrap();
        let edited = format!(
            "{}// added by the owner\nexport const mine = true;\n",
            std::fs::read_to_string(&plugin).unwrap()
        );
        std::fs::write(&plugin, &edited).unwrap();
        let err = match disconnect(&plugin, &state) {
            Ok(_) => panic!("a changed plugin must not be removed silently"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("changed"), "{err}");
        assert_eq!(std::fs::read_to_string(&plugin).unwrap(), edited);
        assert_eq!(journal_plugin(&state), Some(plugin.clone()));
        // Once the owner removed their edited file, the journal can be cleared normally.
        std::fs::remove_file(&plugin).unwrap();
        disconnect(&plugin, &state).unwrap();
        assert_eq!(journal_plugin(&state), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_is_not_text_is_never_replaced() {
        let d = tmpdir("bin");
        let plugin = d.join("plugins/termenbox.js");
        std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        let mine = [0xff_u8, 0xfe, 0x00, 0x0a];
        std::fs::write(&plugin, mine).unwrap();
        let t = Target {
            plugin: plugin.clone(),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        // It is not our plugin just because it cannot be decoded; both actions must refuse it.
        assert!(respond(Some(&t), "opencode", "preview").error.is_some());
        assert!(respond(Some(&t), "opencode", "apply").error.is_some());
        assert_eq!(std::fs::read(&plugin).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disconnect_of_a_missing_file_is_not_an_error_and_the_journal_names_the_file() {
        let d = tmpdir("ms");
        let plugin = d.join("elsewhere/plugins/termenbox.js");
        connect(&plugin, BIN, &d.join("state"), true).unwrap();
        assert_eq!(journal_plugin(&d.join("state")), Some(plugin.clone()));
        std::fs::remove_file(&plugin).unwrap();
        assert!(disconnect(&plugin, &d.join("state")).is_ok());
        assert_eq!(journal_plugin(&d.join("state")), None);
        assert!(disconnect(&d.join("nope.js"), &d.join("state")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn requests_for_other_agents_or_without_a_target_touch_nothing() {
        let d = tmpdir("q");
        let t = Target {
            plugin: d.join("plugins/termenbox.js"),
            auto: false,
            binary: BIN.into(),
            state_dir: d.join("state"),
        };
        assert_eq!(respond(Some(&t), "claude", "apply").state, "unsupported");
        assert_eq!(respond(None, "opencode", "apply").state, "unavailable");
        assert!(respond(Some(&t), "opencode", "frobnicate").error.is_some());
        assert!(!t.plugin.exists());
        assert_eq!(
            respond(Some(&t), "opencode", "remove").state,
            "not_connected"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
