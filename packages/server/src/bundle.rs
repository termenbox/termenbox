//! The server's diagnostics bundle (decision 0144): the tail of the JSONL log, the newest crash file, the previous-exit
//! report, versions and the health history, as one scrubbed text. It is built when the signed-in owner asks, handed
//! to that connection and nowhere else; the app shows it before it is downloaded or attached to an issue.
//!
//! The log layer already leaves secrets and session text out of new lines; the scrub here is a second pass for what it
//! cannot know (an older file, a free-text message, a backtrace): certificates, tokens, addresses, ports, home
//! paths, and every field that can hold session text or a name (guardrail `diagnostics-never-include-secrets`).

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use termenbox_core::protocol::ServerMessage;

/// How much of the log the bundle carries.
const LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// A part stays below the 64 KiB the direct transport carries even if every byte were escaped to six (`\u001b`);
/// `scrub` also replaces control characters, so that cannot happen.
const PART_BYTES: usize = 8 * 1024;

/// Field names whose value is left out whole, besides the log layer's own `PAYLOAD_NAMES`: names a user gave to things.
const OMITTED_EXTRA: &[&str] = &[
    "session",
    "session_id",
    "workspace",
    "workspace_id",
    "window",
    "device",
    "device_id",
    "socket",
    "name",
    "title",
    "cwd",
    "path",
    "dir",
    "command",
    "args",
    "login",
    "account_id",
    "server_name",
];
/// Names replaced besides the log layer's own `SECRET_NAMES`. The log keeps a key fingerprint; a bundle is public.
const SECRETS_EXTRA: &[&str] = &["cert", "fingerprint", "key"];
/// Field names that hold an address.
const ADDRESSES: &[&str] = &[
    "port",
    "ip",
    "addr",
    "address",
    "host",
    "hostname",
    "remote",
    "peer",
    "url",
    "candidate",
    "candidates",
];

fn is_secret_name(name: &str) -> bool {
    crate::logging::SECRET_NAMES
        .iter()
        .chain(SECRETS_EXTRA)
        .any(|s| name.contains(s))
}

fn is_address_name(name: &str) -> bool {
    ADDRESSES.contains(&name)
        || name.ends_with("_port")
        || name.ends_with("_addr")
        || name.ends_with("_ip")
        || name.ends_with("_host")
        || name.ends_with("_url")
}

/// What the bundle is made from. Paths named here are replaced wherever they appear in the text.
pub struct Inputs<'a> {
    pub config_dir: &'a Path,
    pub version: &'a str,
    pub mode: &'a str,
    pub previous_exit: Option<&'a ServerMessage>,
    pub health: Option<ServerMessage>,
}

/// The bundle as scrubbed text.
pub fn build(inputs: &Inputs<'_>) -> String {
    let logs = inputs.config_dir.join("logs");
    let channel =
        format!("{:?}", crate::release::Channel::of_version(inputs.version)).to_lowercase();
    let mut out = String::new();
    out.push_str("termenbox server diagnostics\n");
    out.push_str(&format!(
        "version: {}\nchannel: {channel}\nmode: {}\nos: {} {}\n",
        inputs.version,
        inputs.mode,
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str("\nprevious exit:\n");
    out.push_str(&match inputs.previous_exit {
        Some(m) => json_line(m),
        None => "none (the last run ended cleanly)".into(),
    });
    out.push_str("\n\nhealth history:\n");
    out.push_str(&match &inputs.health {
        Some(m) => json_line(m),
        None => "not sampled".into(),
    });
    out.push_str("\n\nnewest crash file:\n");
    out.push_str(&newest_crash(&logs).unwrap_or_else(|| "none".into()));
    out.push_str("\n\nlog (newest last):\n");
    out.push_str(&log_tail(&logs));
    out.push('\n');
    let known = known_paths(inputs.config_dir);
    scrub_known(&scrub_lines(&out), &known)
}

fn json_line(m: &ServerMessage) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "unavailable".into())
}

/// The paths this process knows are private to the machine, longest first so a directory inside another goes first.
fn known_paths(config_dir: &Path) -> Vec<String> {
    let mut paths = vec![config_dir.to_string_lossy().into_owned()];
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(home.to_string_lossy().into_owned());
    }
    paths.retain(|p| p.len() > 1);
    paths.sort_by_key(|p| std::cmp::Reverse(p.len()));
    paths
}

fn scrub_known(text: &str, known: &[String]) -> String {
    known
        .iter()
        .fold(text.to_string(), |t, p| t.replace(p.as_str(), "[path]"))
}

fn newest_crash(logs: &Path) -> Option<String> {
    let newest = fs::read_dir(logs)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ms: u64 = name
                .strip_prefix("crash-")?
                .strip_suffix(".json")?
                .parse()
                .ok()?;
            Some((ms, e.path()))
        })
        .max()?;
    let (text, _) = tail_of(&newest.1, 256 * 1024)?;
    // The file is pretty-printed; one line per object lets the field-name cleaning apply to it.
    Some(match serde_json::from_str::<Value>(&text) {
        Ok(value) => value.to_string(),
        Err(_) => text,
    })
}

/// The last `LOG_TAIL_BYTES` of the log, filled from the newest rotated file when the current one is short.
fn log_tail(logs: &Path) -> String {
    let current = logs.join("termenbox.jsonl");
    let (mut text, cut) = tail_of(&current, LOG_TAIL_BYTES).unwrap_or_default();
    let short = LOG_TAIL_BYTES.saturating_sub(text.len() as u64);
    // A current file that was cut already holds the newest 2 MB; joining an older file to it would hide a gap.
    if !cut && short > 0 {
        let rotated = fs::read_dir(logs)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n != "termenbox.jsonl" && n.starts_with("termenbox-") && n.ends_with(".jsonl")
            })
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path());
        if let Some((older, _)) = rotated.and_then(|p| tail_of(&p, short)) {
            text = format!("{older}{text}");
        }
    }
    if text.is_empty() {
        "(no log file)".into()
    } else {
        text
    }
}

/// The last `max` bytes of a file as whole lines (a cut first line is dropped), and whether anything was cut.
fn tail_of(path: &Path, max: u64) -> Option<(String, bool)> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let cut = len > max;
    if cut {
        file.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut bytes = Vec::new();
    file.take(max).read_to_end(&mut bytes).ok()?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if cut {
        text = text.split_once('\n').map_or(String::new(), |r| r.1.into());
    }
    Some((text, cut))
}

/// Splits the text into messages of at most [`PART_BYTES`], at line ends where there is one.
pub fn parts(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(PART_BYTES);
        if end < rest.len() {
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            if let Some(nl) = rest[..end].rfind('\n') {
                end = nl + 1;
            }
        }
        let (head, tail) = rest.split_at(end);
        parts.push(head.to_string());
        rest = tail;
    }
    if parts.is_empty() {
        parts.push(String::new());
    }
    parts
}

/// Scrubs a whole text, line by line. A line that is a JSON object is cleaned by field name first.
pub fn scrub_lines(text: &str) -> String {
    text.lines().map(scrub_line).collect::<Vec<_>>().join("\n")
}

fn scrub_line(line: &str) -> String {
    if line.trim_start().starts_with('{') {
        if let Ok(mut value) = serde_json::from_str::<Value>(line) {
            clean(&mut value, "");
            return value.to_string();
        }
    }
    scrub(line)
}

fn clean(value: &mut Value, key: &str) {
    if !key.is_empty() {
        if crate::logging::PAYLOAD_NAMES.contains(&key) || OMITTED_EXTRA.contains(&key) {
            *value = Value::from("[omitted]");
            return;
        }
        if is_secret_name(key) {
            *value = Value::from("[redacted]");
            return;
        }
        if is_address_name(key) {
            *value = Value::from("[address]");
            return;
        }
    }
    match value {
        Value::String(s) => *s = scrub(s),
        Value::Array(items) => items.iter_mut().for_each(|v| clean(v, key)),
        Value::Object(map) => map.iter_mut().for_each(|(k, v)| clean(v, k)),
        _ => {}
    }
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || "\"'(){}<>,;=|\\".contains(c)
}

/// Replaces certificates, tokens, secrets, addresses, ports and home paths in free text.
pub fn scrub(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    // The token before a delimiter named a secret or was `Bearer`: the next one is its value.
    let mut redact_next = false;
    for c in text.chars() {
        if !is_delimiter(c) {
            token.push(c);
            continue;
        }
        flush(&mut out, &mut token, &mut redact_next, c == '=');
        out.push(c);
    }
    flush(&mut out, &mut token, &mut redact_next, false);
    out
}

fn flush(out: &mut String, token: &mut String, redact_next: &mut bool, before_equals: bool) {
    if token.is_empty() {
        return;
    }
    let t = std::mem::take(token);
    let lower = t.to_lowercase();
    let names_secret = is_secret_name(&lower);
    if *redact_next {
        // `Authorization: Bearer x`: the scheme is not the secret.
        if lower == "bearer" {
            out.push_str(&t);
            return;
        }
        *redact_next = false;
        out.push_str("[redacted]");
        return;
    }
    // `password=x` and `token: x` (the colon stays on the key) and `Bearer x`.
    if (names_secret && (before_equals || t.ends_with(':'))) || lower == "bearer" {
        *redact_next = true;
        out.push_str(&t);
        return;
    }
    out.push_str(&classify(&t));
}

fn classify(t: &str) -> String {
    // Trailing sentence punctuation stays outside what is judged.
    let core = t.trim_end_matches(['.', ':', '!', '?']);
    let tail = &t[core.len()..];
    format!("{}{tail}", classify_core(core))
}

fn is_home_path(t: &str) -> bool {
    ["/home/", "/Users/", "/root/", "~/"]
        .iter()
        .any(|p| t.contains(p))
        || t == "/root"
        || t == "~"
}

fn classify_core(t: &str) -> String {
    if is_home_path(t) {
        return "[path]".into();
    }
    if t.contains("://") {
        // Only the project's own host is kept: any other can name the owner's account or machine.
        let no_query = t.split(['?', '#']).next().unwrap_or(t);
        if no_query.starts_with("https://github.com/termenbox/") {
            return no_query.to_string();
        }
        return "[url]".into();
    }
    if is_ipv4(t) || is_ipv6(t) {
        return "[ip]".into();
    }
    if has_port(t) {
        return "[host]".into();
    }
    if t.starts_with("gh") && t.len() > 20 && t.chars().nth(3) == Some('_')
        || t.starts_with("github_pat_")
    {
        return "[token]".into();
    }
    if let Some((a, b)) = t.split_once('.') {
        let long = |s: &str| s.len() >= 16 && s.chars().all(is_b64url);
        if long(a) && long(b) {
            return "[cert]".into();
        }
    }
    if t.len() >= 32 && t.chars().all(is_b64url) {
        return "[secret]".into();
    }
    t.to_string()
}

fn is_b64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `1.2.3.4`, `1.2.3.4:80`, `1.2.3.4/24`.
fn is_ipv4(t: &str) -> bool {
    let host = t.split(['/', ':']).next().unwrap_or(t);
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

/// An IPv6 address, bracketed or not, with an optional port: it has `::` or eight groups, and only address characters.
/// A time of day (`11:51:09`) has neither.
fn is_ipv6(t: &str) -> bool {
    let colons = t.matches(':').count();
    (t.contains("::") || colons >= 7)
        && t.chars()
            .all(|c| c.is_ascii_hexdigit() || ":.[]%".contains(c))
        && t.chars().any(|c| c.is_ascii_hexdigit())
}

/// `name:80`, `host.example.com:443`.
fn has_port(t: &str) -> bool {
    let Some((host, port)) = t.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && (2..=5).contains(&port.len())
        && port.chars().all(|c| c.is_ascii_digit())
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termenbox-bundle-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("logs")).unwrap();
        dir
    }

    const CERT: &str = "MEUCIQDabcdefghijklmnop12345.AiEA0123456789abcdefghijklmnop";
    const CLAIM: &str = "q3JvdGF0ZWRfY2xhaW1fc2VjcmV0X3RoYXRfaXNfNDM";
    const GH: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn bundle_scrub_removes_secrets_addresses_paths_and_session_text() {
        let dir = temp_dir();
        let logs = dir.join("logs");
        let lines = [
            // what the log layer writes, with fields that must not survive a second look
            format!(
                r#"{{"ts":"2026-10-28T11:51:09.638Z","level":"info","target":"audit","fields":{{"event":"sign_in","login":"alice-the-owner","remote":"203.0.113.9:51234","session":"my-secret-project","workspace_id":"client-x-deploy","device":"devkey-9f3a","message":"signed in from 203.0.113.9:51234 with {GH}"}}}}"#
            ),
            // ACP message text under the names the layer would have dropped, and in nested values
            r#"{"ts":"2026-10-28T11:51:10.000Z","level":"debug","target":"acp","fields":{"text":"please rotate the prod database password","prompt":"ssh root@10.1.2.3","content":["rm -rf /home/alice/work"],"event":"acp_update"}}"#.into(),
            // free text with an address, a home path, a cert and a claim secret
            format!("warn: bind 192.168.1.20:4433 failed, retry [2001:db8::1]:4433 or fe80::1; cert {CERT} claim {CLAIM} cwd /home/alice/work/app password=hunter2 Authorization: Bearer abc123"),
            format!("{{\"ts\":\"t\",\"level\":\"warn\",\"fields\":{{\"message\":\"config in {}/owner.json\",\"password\":\"hunter2\",\"port\":4433}}}}", dir.display()),
        ];
        fs::write(logs.join("termenbox.jsonl"), lines.join("\n") + "\n").unwrap();
        fs::write(
            logs.join("crash-1000.json"),
            "{\n  \"message\": \"called unwrap at /home/alice/.claude/projects/x\",\n  \"session\": \"client-x-deploy\",\n  \"token\": \"tok-9\",\n  \"location\": \"packages/server/src/acp.rs:12\",\n  \"backtrace\": \"at /home/alice/.cargo/registry/src/foo.rs:9 wss://relay.owner.workers.dev/s/route1\"\n}",
        )
        .unwrap();
        let previous = ServerMessage::PreviousExit {
            at_ms: 1700000000000,
            streak: 2,
            crashed: true,
        };
        let text = build(&Inputs {
            config_dir: &dir,
            version: "0.9.0-beta.1",
            mode: "systemd",
            previous_exit: Some(&previous),
            health: None,
        });
        for secret in [
            CERT,
            CLAIM,
            GH,
            "hunter2",
            "abc123",
            "203.0.113.9",
            "51234",
            "192.168.1.20",
            "10.1.2.3",
            "2001:db8",
            "fe80::1",
            "4433",
            "/home/alice",
            "alice",
            "my-secret-project",
            "client-x-deploy",
            "tok-9",
            "workers.dev",
            "devkey-9f3a",
            "rotate the prod database",
            "rm -rf",
            dir.to_str().unwrap(),
        ] {
            assert!(
                !text.contains(secret),
                "{secret:?} is in the bundle:\n{text}"
            );
        }
        for kept in [
            "0.9.0-beta.1",
            "channel: beta",
            "systemd",
            "previous exit",
            "acp.rs:12",
            "sign_in",
            "\"streak\":2",
        ] {
            assert!(text.contains(kept), "{kept:?} is missing:\n{text}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_time_of_day_and_a_rust_path_are_not_taken_for_addresses() {
        assert_eq!(
            scrub("at 11:51:09 in std::io::Error"),
            "at 11:51:09 in std::io::Error"
        );
        assert_eq!(scrub("1.2.3.4:80 and ::1"), "[ip] and [ip]");
    }

    #[test]
    fn the_log_tail_is_cut_at_a_line_and_parts_stay_small() {
        let dir = temp_dir();
        let line = format!("{{\"fields\":{{\"event\":\"{}\"}}}}\n", "x".repeat(200));
        let count = (LOG_TAIL_BYTES as usize / line.len()) + 50;
        fs::write(dir.join("logs/termenbox.jsonl"), line.repeat(count)).unwrap();
        let tail = log_tail(&dir.join("logs"));
        assert!(tail.len() as u64 <= LOG_TAIL_BYTES);
        assert!(
            tail.lines().all(|l| l.ends_with("}}")),
            "a cut line was kept"
        );
        let all = parts(&scrub_lines(&tail));
        assert!(all.iter().all(|p| p.len() <= PART_BYTES));
        assert_eq!(all.concat(), scrub_lines(&tail));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_characters_are_replaced_and_a_cut_log_is_not_joined_to_an_older_one() {
        assert_eq!(scrub("a\u{1b}[31mred"), "a?[31mred");
        let dir = temp_dir();
        let logs = dir.join("logs");
        fs::write(
            logs.join("termenbox-old.jsonl"),
            "{\"fields\":{\"event\":\"older\"}}\n",
        )
        .unwrap();
        let line = format!("{{\"fields\":{{\"event\":\"{}\"}}}}\n", "x".repeat(200));
        let count = (LOG_TAIL_BYTES as usize / line.len()) + 50;
        fs::write(logs.join("termenbox.jsonl"), line.repeat(count)).unwrap();
        assert!(!log_tail(&logs).contains("older"));
        fs::write(logs.join("termenbox.jsonl"), line).unwrap();
        assert!(log_tail(&logs).contains("older"));
        let _ = fs::remove_dir_all(&dir);
    }
}
