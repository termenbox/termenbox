//! Where sessions live. The server only needs four things from a session manager: list them, create one,
//! name the command that attaches a terminal to one, and survive a client detaching. tmux is built in;
//! anything with a CLI that fits that shape (zellij, screen, ...) plugs in through `CommandBackend`;
//! a server-native backend can implement `SessionBackend` directly (plan step 7.1).
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use termenbox_core::protocol::SessionInfo;
use tokio::process::Command;

use crate::control::WindowState;
use crate::events::EventEnv;
use crate::pty::Attachment;

#[async_trait]
pub trait SessionBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn list(&self) -> Result<Vec<SessionInfo>>;
    async fn create(&self, name: &str) -> Result<()>;
    /// Starts a terminal attached to session `id`. Dropping it must only detach.
    fn attach(&self, id: &str, cols: u16, rows: u16) -> Result<Attachment>;
    /// The session managers behind this backend, in the order "New session" offers them.
    fn kinds(&self) -> Vec<String> {
        vec![self.name().to_string()]
    }
    /// Creates in a named manager; a backend with a single manager ignores the name.
    async fn create_in(&self, name: &str, _backend: Option<&str>) -> Result<()> {
        self.create(name).await
    }
    /// Creates a session in a working directory, optionally starting a coding agent in it. Managers that cannot
    /// do either ignore them.
    async fn create_with(
        &self,
        name: &str,
        backend: Option<&str>,
        _dir: Option<&str>,
        _template: Option<&str>,
    ) -> Result<()> {
        self.create_in(name, backend).await
    }
    /// The visible text of the session's pane, for checking what a one-tap answer would answer.
    async fn capture(&self, _id: &str) -> Result<String> {
        Err(anyhow!("this session manager cannot read a pane"))
    }
    /// Like `capture`, with the colour and style escape codes kept (for a snapshot the client draws).
    async fn capture_styled(&self, id: &str) -> Result<String> {
        self.capture(id).await
    }
    /// `capture_styled` of one window instead of the current one.
    async fn capture_window_styled(&self, id: &str, _index: u32) -> Result<String> {
        self.capture_styled(id).await
    }
    /// Everything the session's terminal still holds, history included, as plain text with wrapped lines joined. A
    /// manager that keeps no history gives the screen.
    async fn capture_history(&self, id: &str) -> Result<String> {
        self.capture(id).await
    }
    /// Types `text` into the session's pane as if it was typed there.
    async fn send_text(&self, _id: &str, _text: &str) -> Result<()> {
        Err(anyhow!("this session manager cannot send keys"))
    }
    /// Ends the session and everything running in it. The caller has already asked the user.
    async fn kill(&self, _id: &str) -> Result<()> {
        Err(anyhow!("this session manager cannot close sessions"))
    }
    /// Sets the name clients show for a session (an empty name clears it). The id, which events and layouts key on,
    /// stays as it is.
    async fn rename(&self, _id: &str, _name: &str) -> Result<()> {
        Err(anyhow!("this session manager cannot rename sessions"))
    }
    /// The windows of a session, for the windows dropdown.
    async fn windows(&self, _id: &str) -> Result<Vec<WindowState>> {
        Err(anyhow!("this session manager has no windows"))
    }
    /// Makes window `index` the one the next client sees, whoever is attached (the user chose it).
    async fn select_window(&self, _id: &str, _index: u32) -> Result<()> {
        Err(anyhow!("this session manager has no windows"))
    }
    /// Like `select_window`, but only when no real terminal is attached to the session, so opening a tab never
    /// moves a screen somebody else is looking at. Returns whether it switched.
    async fn focus_window(&self, _id: &str, _index: u32) -> Result<bool> {
        Ok(false)
    }
    /// Closes one window; closing the last one ends the session.
    async fn kill_window(&self, _id: &str, _index: u32) -> Result<()> {
        Err(anyhow!("this session manager has no windows"))
    }
    /// Opens a window next to the others, in the directory of the session's current window, and makes it the
    /// current one.
    async fn new_window(&self, _id: &str) -> Result<()> {
        Err(anyhow!("this session manager has no windows"))
    }

    // ---- T0: ACP sessions (decision 0111). A manager that runs none refuses all of these.

    /// The agents the owner allows as ACP sessions.
    fn acp_agents(&self) -> Vec<String> {
        Vec::new()
    }
    /// Wakes when the stream of ACP session `id` has a new revision; a closed channel means it never will.
    fn acp_watch(&self, _id: &str) -> Result<tokio::sync::watch::Receiver<u64>> {
        Err(anyhow!("This session is not an ACP session"))
    }
    /// What changed in the stream since the client's revision (see `Stream::view`), and the info revision now.
    fn acp_view(
        &self,
        _id: &str,
        _after: Option<u64>,
        _epoch: Option<u64>,
        _info_seen: u64,
    ) -> Result<(termenbox_core::protocol::AcpView, u64)> {
        Err(anyhow!("This session is not an ACP session"))
    }
    /// The owner's next prompt (`session/prompt`).
    async fn acp_prompt(&self, _id: &str, _text: &str) -> Result<()> {
        Err(anyhow!("This session is not an ACP session"))
    }
    async fn acp_interrupt(&self, _id: &str) -> Result<()> {
        Err(anyhow!("This session is not an ACP session"))
    }
    /// Answers permission `prompt_id` with exactly `option_id`.
    async fn acp_answer(&self, _id: &str, _prompt_id: &str, _option_id: &str) -> Result<()> {
        Err(anyhow!("This session is not an ACP session"))
    }
    async fn acp_mode(&self, _id: &str, _mode: &str) -> Result<()> {
        Err(anyhow!("This session is not an ACP session"))
    }
}

/// Session ids/names we accept from clients; they end up in argv, never in a shell.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The display name a client may give a session: trimmed, one line, at most 64 characters, no `|` (the list format's
/// separator). `None` clears it.
pub fn clean_label(name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > 64 || name.chars().any(|c| c.is_control() || c == '|') {
        return Err(anyhow!(
            "A name is one line of at most 64 characters, without |"
        ));
    }
    Ok(Some(name.to_string()))
}

/// A working directory a client may ask for: absolute, no control characters, a sane length. It reaches tmux as
/// one argument, never through a shell.
pub fn valid_dir(dir: &str) -> bool {
    dir.starts_with('/') && dir.len() <= 512 && !dir.chars().any(|c| c.is_control())
}

/// One session per line: `id` or `id|name|...`; extra fields are ignored.
pub fn parse_list(output: &str) -> Vec<SessionInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('|').map(str::trim);
            let id = fields.next().filter(|id| !id.is_empty())?;
            let name = fields.next().filter(|n| !n.is_empty()).unwrap_or(id);
            Some(SessionInfo {
                id: id.to_string(),
                name: name.to_string(),
                agent: None,
                backend: None,
            })
        })
        .collect()
}

/// Where session commands run (DECISION-0058). In container mode they run as-is. In host mode every command
/// is prefixed with `nsenter` into PID 1's namespaces (the container has `--pid=host --privileged`), and
/// for a dedicated user also with `runuser`, so tmux and the agent CLIs are the host's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runner {
    /// `nsenter ...`; empty in container mode.
    enter: Vec<String>,
    /// `runuser -u <user> --` for a dedicated user; empty for root and in container mode.
    user: Vec<String>,
    /// `env K=V -u K ...` words put in front of every command; empty normally, used by tests to give a
    /// tool a private home or socket directory without touching this process's environment.
    env: Vec<String>,
    /// The tmux this runner starts in place of `tmux` on `PATH` (decision 0119); `None` means `tmux` as named.
    tmux: Option<String>,
    /// How tmux is kept apart from the user's own (decision 0120); `None` uses the default socket and config.
    isolation: Option<Isolation>,
}

/// Where termenbox's own tmux server gets its config from (decision 0120). Every tmux this runner starts is given
/// `-L termenbox` and `-f <config>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Isolation {
    /// A config file on this machine's file system (container and native modes).
    File(String),
    /// Host mode: the file is written on the host, under tmux's own per-user directory, and named at run time
    /// because the user's uid is only known there.
    HostUser,
}

/// Runs `"$0" <args>` with termenbox's socket and the per-user config in tmux's own directory for that uid.
const HOST_TMUX: &str = r#"exec "$0" -L termenbox -f "/tmp/tmux-$(id -u)/termenbox.conf" "$@""#;

/// Writes the config for host mode into `/tmp/tmux-<uid>/` as the session user. The directory is tmux's own: it
/// is created private, and refused when it is a link or someone else's, so another user cannot plant a config.
const WRITE_HOST_CONF: &str = r#"umask 077; d=/tmp/tmux-$(id -u); mkdir -p "$d" && [ -d "$d" ] && [ ! -L "$d" ] && [ -O "$d" ] && chmod 700 "$d" && printf '%s' "$1" > "$d/termenbox.conf.$$" && mv "$d/termenbox.conf.$$" "$d/termenbox.conf""#;

static ISOLATION: std::sync::OnceLock<Isolation> = std::sync::OnceLock::new();

/// Decided once at startup (decision 0120); runners made after this keep tmux apart from the user's own.
pub fn use_private_tmux(isolation: Isolation) {
    let _ = ISOLATION.set(isolation);
}

/// The bundled tmux the installer places beside the server binary (decision 0119). Named so that it can never
/// be mistaken for, or shadow, a distribution `tmux`.
pub const BUNDLED_TMUX: &str = "termenbox-tmux";

/// What a failed tmux command says, as one line. A tmux client and a running tmux server on the same socket must
/// share a protocol number (8 in every release from 2.6 to 3.8, pinned for the bundled one in `build-tmux.sh`); when
/// they do not, tmux's own message does not say what to do.
fn tmux_stderr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim();
    if text.contains("protocol version mismatch") {
        return format!(
            "the tmux server already running on this socket is too different from the tmux termenbox runs now to talk to it (tmux says: {text}). Restart that tmux server: `termenbox-server tmux kill-server` ends its sessions"
        );
    }
    text.to_string()
}

/// The oldest tmux with the control mode the session backend needs.
const MIN_TMUX: (u32, u32) = (3, 2);

/// `Some(true)` when `tmux -V` output names a version older than 3.2, `Some(false)` when it names a newer one, `None`
/// for a build without a number (`tmux master`), which is taken as new.
fn tmux_too_old(version_output: &str) -> Option<bool> {
    let v = version_output.trim().strip_prefix("tmux")?.trim();
    let v = v.strip_prefix("next-").unwrap_or(v);
    let mut parts = v.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts
        .next()
        .map(|m| m.trim_end_matches(|c: char| c.is_ascii_alphabetic()))
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    Some((major, minor) < MIN_TMUX)
}

/// Picks the tmux to run: `explicit` (`TERMENBOX_TMUX`, never overridden, so a bad one is an error), then the
/// `bundled` copy (skipped, with a reason through `log`, when it does not run or is older than 3.2), then `tmux` on
/// `PATH`. `version_of` returns `<path> -V` output when that program runs.
pub fn resolve_tmux(
    explicit: Option<&str>,
    bundled: Option<&std::path::Path>,
    version_of: &dyn Fn(&str) -> Option<String>,
    log: &mut dyn FnMut(String),
) -> Result<String, String> {
    if let Some(path) = explicit.filter(|p| !p.is_empty()) {
        return match version_of(path).map(|v| tmux_too_old(&v)) {
            None => Err(format!("TERMENBOX_TMUX={path} cannot be run")),
            Some(Some(true)) => Err(format!(
                "TERMENBOX_TMUX={path} is older than tmux 3.2, which the session backend needs"
            )),
            Some(_) => Ok(path.to_string()),
        };
    }
    if let Some(path) = bundled.filter(|p| p.is_file()) {
        let shown = path.display().to_string();
        match version_of(&shown).map(|v| tmux_too_old(&v)) {
            None => log(format!(
                "the bundled tmux {shown} does not run; skipping it"
            )),
            Some(Some(true)) => log(format!(
                "the bundled tmux {shown} is older than 3.2; skipping it"
            )),
            Some(_) => return Ok(shown),
        }
    }
    match version_of("tmux").map(|v| tmux_too_old(&v)) {
        Some(Some(true)) => log(
            "tmux on PATH is older than 3.2; control mode is off and sessions are scanned instead"
                .into(),
        ),
        Some(_) => {}
        None => log("no tmux on PATH".into()),
    }
    Ok("tmux".to_string())
}

fn run_version(path: &str) -> Option<String> {
    let out = std::process::Command::new(path).arg("-V").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The tmux for this process, decided once from `TERMENBOX_TMUX`, the copy beside this binary and `PATH`.
fn local_tmux() -> &'static Result<String, String> {
    static CHOICE: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    CHOICE.get_or_init(|| {
        let bundled = std::env::current_exe().ok().and_then(|exe| {
            // After an update the running file shows as deleted; its directory is still the install directory.
            let exe = exe
                .to_string_lossy()
                .trim_end_matches(" (deleted)")
                .to_string();
            std::path::Path::new(&exe)
                .parent()
                .map(|d| d.join(BUNDLED_TMUX))
        });
        let chosen = resolve_tmux(
            std::env::var("TERMENBOX_TMUX").ok().as_deref(),
            bundled.as_deref(),
            &run_version,
            &mut |reason| tracing::warn!("{reason}"),
        );
        if let Ok(path) = &chosen {
            tracing::info!("tmux: {path}");
        }
        chosen
    })
}

/// Runs the rest of the command line in its own transient systemd scope when the host has systemd, so a
/// process started this way (the tmux server) is not in the container's cgroup and survives the
/// container being restarted or updated. Without systemd it runs the command directly.
const DETACH: [&str; 4] = [
    "sh",
    "-c",
    "if command -v systemd-run >/dev/null 2>&1; then exec systemd-run --scope --quiet -- \"$@\"; fi; exec \"$@\"",
    "sh",
];

/// The most of a session's history that is sent for select-all: the newest of it.
const HISTORY_TEXT_MAX: usize = 2_000_000;

/// The newest at most `max` bytes of `text`, starting at a line, without trailing blank lines.
pub(crate) fn newest_text(text: &str, max: usize) -> String {
    let text = text.trim_end();
    if text.len() <= max {
        return text.to_string();
    }
    let mut from = text.len() - max;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    // Start at the next line so the first line is whole.
    match text[from..].find('\n') {
        Some(n) => text[from + n + 1..].to_string(),
        None => text[from..].to_string(),
    }
}

impl Runner {
    /// Sets (`Some`) or removes (`None`) environment variables for every command this runner starts.
    #[cfg(test)]
    pub fn with_env(mut self, vars: &[(&str, Option<&str>)]) -> Self {
        for (k, v) in vars {
            match v {
                Some(v) => self.env.push(format!("{k}={v}")),
                // `env` reads its options before the first NAME=value.
                None => {
                    self.env.insert(0, k.to_string());
                    self.env.insert(0, "-u".to_string());
                }
            }
        }
        self
    }

    /// Applies the generated tmux config to a tmux server that is already running. tmux reads its config only when its
    /// server starts, and sessions outlive the termenbox server, so an upgrade that changes the config (hiding the
    /// status line, say) would otherwise wait for every session to end. Where no tmux server runs this does nothing.
    pub async fn reload_tmux_config(&self) {
        let Some(Isolation::File(conf)) = &self.isolation else {
            return;
        };
        let args = ["source-file", "-q", conf.as_str()].map(String::from);
        let _ = self.run("tmux", &args).await;
    }

    /// Host mode: writes the private server's config on the host (decision 0120), as the session user.
    pub async fn install_host_tmux_conf(&self, content: &str) -> Result<()> {
        let args = ["-c", WRITE_HOST_CONF, "sh", content].map(String::from);
        let out = self.run("sh", &args).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "cannot write tmux's config on the host: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    /// For commands run on this machine as this process: container and native modes. Uses the tmux that
    /// `local_tmux` chose (a bad `TERMENBOX_TMUX` is reported by the startup self-check, not here).
    pub fn local() -> Self {
        Self {
            tmux: local_tmux().as_ref().ok().cloned(),
            isolation: ISOLATION.get().cloned(),
            ..Self::default()
        }
    }

    /// `user` is `root` or the name of an unprivileged host user.
    pub fn host(user: &str) -> Self {
        let enter = ["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "-p"]
            .map(String::from)
            .to_vec();
        let user = if user == "root" {
            vec![]
        } else {
            ["runuser", "-u", user, "--"].map(String::from).to_vec()
        };
        Self {
            enter,
            user,
            env: vec![],
            tmux: None,
            isolation: ISOLATION.get().cloned(),
        }
    }

    /// The program and arguments to actually execute for `program args`. `detach` is for commands that
    /// start a long-lived server (tmux new-session).
    pub fn wrap(&self, detach: bool, program: &str, args: &[String]) -> (String, Vec<String>) {
        let mut argv: Vec<String> = self.enter.clone();
        if detach && !self.enter.is_empty() {
            argv.extend(DETACH.iter().map(|s| s.to_string()));
        }
        argv.extend(self.user.iter().cloned());
        if !self.env.is_empty() {
            argv.push("env".to_string());
            argv.extend(self.env.iter().cloned());
        }
        let program_path = match (&self.tmux, program) {
            (Some(path), "tmux") => path.clone(),
            _ => program.to_string(),
        };
        match (&self.isolation, program) {
            (Some(Isolation::File(conf)), "tmux") => {
                argv.push(program_path);
                argv.extend(["-L", crate::tmuxconf::SOCKET, "-f", conf].map(String::from));
            }
            (Some(Isolation::HostUser), "tmux") => {
                argv.extend(["sh", "-c", HOST_TMUX].map(String::from));
                argv.push(program_path);
            }
            _ => argv.push(program_path),
        }
        argv.extend(args.iter().cloned());
        let first = argv.remove(0);
        (first, argv)
    }

    pub async fn run(&self, program: &str, args: &[String]) -> Result<std::process::Output> {
        self.run_as(false, program, args).await
    }

    async fn run_as(
        &self,
        detach: bool,
        program: &str,
        args: &[String],
    ) -> Result<std::process::Output> {
        let (program, args) = self.wrap(detach, program, args);
        Command::new(&program)
            .args(&args)
            .envs(crate::locale::utf8_env())
            .output()
            .await
            .map_err(|e| anyhow!("cannot run {program}: {e}"))
    }

    /// A long-lived child with piped stdin/stdout (a control-mode client); it dies with its handle.
    pub fn spawn_piped(&self, program: &str, args: &[String]) -> Result<tokio::process::Child> {
        let (program, args) = self.wrap(false, program, args);
        Command::new(&program)
            .args(&args)
            .envs(crate::locale::utf8_env())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("cannot run {program}: {e}"))
    }

    fn attach(&self, program: &str, args: &[String], cols: u16, rows: u16) -> Result<Attachment> {
        let (program, args) = self.wrap(false, program, args);
        Attachment::spawn(&program, &args, cols, rows)
    }

    /// Startup self-check for the native mode: this process must run as the chosen user and tmux must exist.
    pub async fn check_native(&self, want_root: bool) -> Result<()> {
        let out = self.run("id", &["-u".to_string()]).await?;
        let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if want_root != (uid == "0") {
            return Err(anyhow!(
                "the service runs as uid {uid}, not as the {} you chose; check User= in the systemd unit",
                if want_root { "root user" } else { "dedicated user" }
            ));
        }
        if let Err(why) = local_tmux() {
            return Err(anyhow!("{why}"));
        }
        let tmux = self.run("tmux", &["-V".to_string()]).await;
        if !tmux.map(|o| o.status.success()).unwrap_or(false) {
            return Err(anyhow!(
                "no tmux to run: none beside the server ({BUNDLED_TMUX}), none on PATH, and TERMENBOX_TMUX is not set"
            ));
        }
        Ok(())
    }

    /// Startup self-check for host mode: the prefix must work, run as the chosen user, and find tmux.
    pub async fn check_host(&self, want_root: bool) -> Result<()> {
        let out = self.run("id", &["-u".to_string()]).await.map_err(|e| {
            anyhow!("host mode could not run commands on the host ({e}); start the container with --pid=host --privileged")
        })?;
        if !out.status.success() {
            return Err(anyhow!(
                "host mode could not run commands on the host: {}; start the container with --pid=host --privileged and check that the user exists",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if want_root != (uid == "0") {
            return Err(anyhow!(
                "host sessions run as uid {uid}, not as the {} you chose",
                if want_root {
                    "root user"
                } else {
                    "dedicated user"
                }
            ));
        }
        let tmux = self.run("tmux", &["-V".to_string()]).await;
        if !tmux.map(|o| o.status.success()).unwrap_or(false) {
            return Err(anyhow!("tmux is not installed on the host"));
        }
        Ok(())
    }
}

/// Chosen at install and fixed for the life of the container; no message or update can change it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunMode {
    Container,
    /// `user` is `root` or the dedicated unprivileged user.
    Host {
        user: String,
    },
    /// The server itself runs on the host as `user` (decision 0075): commands run as they are, with no `nsenter`.
    Native {
        user: String,
    },
}

impl RunMode {
    pub fn parse(run: &str, user: &str) -> Result<Self> {
        match run {
            "container" => Ok(Self::Container),
            "host" if user.is_empty() => {
                Err(anyhow!("TERMENBOX_RUN_USER must name a user or root"))
            }
            "host" => Ok(Self::Host {
                user: user.to_string(),
            }),
            "native" if user.is_empty() => {
                Err(anyhow!("TERMENBOX_RUN_USER must name a user or root"))
            }
            "native" => Ok(Self::Native {
                user: user.to_string(),
            }),
            other => Err(anyhow!(
                "unknown TERMENBOX_RUN `{other}` (container, host or native)"
            )),
        }
    }

    /// Shown to the owner in the app and written in audit entries.
    pub fn label(&self) -> String {
        match self {
            Self::Container => "container".into(),
            // The app shows the same badge for both ways of running on the host.
            Self::Host { user } | Self::Native { user } => format!("host:{user}"),
        }
    }

    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host { .. } | Self::Native { .. })
    }

    pub fn runner(&self) -> Runner {
        match self {
            Self::Container | Self::Native { .. } => Runner::local(),
            Self::Host { user } => Runner::host(user),
        }
    }

    pub async fn self_check(&self) -> Result<()> {
        match self {
            Self::Container => Ok(()),
            Self::Host { user } => self.runner().check_host(user == "root").await,
            Self::Native { user } => self.runner().check_native(user == "root").await,
        }
    }
}

#[derive(Default)]
pub struct TmuxBackend {
    runner: Runner,
    events: Option<EventEnv>,
    /// Added to the session name wherever events refer to it (`tmux:` when several managers run).
    event_prefix: String,
}

impl TmuxBackend {
    pub fn new(runner: Runner) -> Self {
        Self {
            runner,
            events: None,
            event_prefix: String::new(),
        }
    }

    pub fn with_event_prefix(mut self, prefix: &str) -> Self {
        self.event_prefix = prefix.to_string();
        self
    }

    /// New sessions get the variables agent tools use to report events (plan v6).
    pub fn with_events(mut self, events: EventEnv) -> Self {
        self.events = Some(events);
        self
    }
}

#[async_trait]
impl SessionBackend for TmuxBackend {
    fn name(&self) -> &str {
        "tmux"
    }

    async fn list(&self) -> Result<Vec<SessionInfo>> {
        let args = [
            "list-sessions",
            "-F",
            "#{session_name}|#{?@termenbox_name,#{@termenbox_name},#{session_name}}",
        ]
        .map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            let stderr = tmux_stderr(&out.stderr);
            // No server yet simply means no sessions.
            if stderr.contains("no server running") || stderr.contains("No such file") {
                return Ok(vec![]);
            }
            return Err(anyhow!("tmux list-sessions failed: {stderr}"));
        }
        Ok(parse_list(&String::from_utf8_lossy(&out.stdout)))
    }

    async fn create(&self, name: &str) -> Result<()> {
        self.create_with(name, None, None, None).await
    }

    async fn create_with(
        &self,
        name: &str,
        _backend: Option<&str>,
        dir: Option<&str>,
        template: Option<&str>,
    ) -> Result<()> {
        if !valid_name(name) {
            return Err(anyhow!(
                "Session names use letters, digits, - and _ (max 64)"
            ));
        }
        // Only the agents the scan knows may be started from a client; a plain shell otherwise.
        let template = match template.filter(|t| !t.is_empty() && *t != "shell") {
            Some(t) => Some(
                crate::adapters::AGENTS
                    .into_iter()
                    .find(|a| *a == t)
                    .ok_or_else(|| anyhow!("Unknown agent template"))?,
            ),
            None => None,
        };
        let mut args: Vec<String> = ["new-session", "-d", "-s", name].map(String::from).into();
        if let Some(dir) = dir.filter(|d| !d.is_empty()) {
            if !valid_dir(dir) {
                return Err(anyhow!("The directory must be an absolute path"));
            }
            args.extend(["-c".to_string(), dir.to_string()]);
        }
        for var in self
            .events
            .iter()
            .flat_map(|e| e.vars(&format!("{}{name}", self.event_prefix)))
        {
            args.extend(["-e".to_string(), var]);
        }
        let out = self.runner.run_as(true, "tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "tmux new-session failed: {}",
                tmux_stderr(&out.stderr)
            ));
        }
        if let Some(agent) = template {
            // Typed into the new shell, so quitting the agent leaves the shell (and the session) alive.
            self.send_text(name, &format!("{agent}\r")).await?;
        }
        Ok(())
    }

    async fn capture(&self, id: &str) -> Result<String> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["capture-pane", "-p", "-t", &format!("={id}:")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("the pane cannot be read"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn capture_history(&self, id: &str) -> Result<String> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        // `-S -` and `-E -` are the start and end of the history; `-J` joins the lines the width wrapped.
        let args = [
            "capture-pane",
            "-p",
            "-J",
            "-S",
            "-",
            "-E",
            "-",
            "-t",
            &format!("={id}:"),
        ]
        .map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("the pane cannot be read"));
        }
        Ok(newest_text(
            &String::from_utf8_lossy(&out.stdout),
            HISTORY_TEXT_MAX,
        ))
    }

    async fn capture_styled(&self, id: &str) -> Result<String> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["capture-pane", "-e", "-p", "-t", &format!("={id}:")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("the pane cannot be read"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn capture_window_styled(&self, id: &str, index: u32) -> Result<String> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["capture-pane", "-e", "-p", "-t", &format!("={id}:{index}")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("the pane cannot be read"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn send_text(&self, id: &str, text: &str) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        // `\r` is Enter; everything else is literal (`-l`), so no tmux key name can be smuggled in.
        for (i, part) in text.split('\r').enumerate() {
            if i > 0 {
                let args = ["send-keys", "-t", &format!("={id}:"), "Enter"].map(String::from);
                self.runner.run("tmux", &args).await?;
            }
            if !part.is_empty() {
                let args =
                    ["send-keys", "-t", &format!("={id}:"), "-l", "--", part].map(String::from);
                self.runner.run("tmux", &args).await?;
            }
        }
        Ok(())
    }

    async fn rename(&self, id: &str, name: &str) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let label = clean_label(name)?;
        // A trailing colon: `set-option` takes a pane target, and `=name` alone is not one.
        let target = format!("={id}:");
        let args: Vec<String> = match &label {
            Some(l) => ["set-option", "-t", &target, "@termenbox_name", l]
                .map(String::from)
                .into(),
            None => ["set-option", "-u", "-t", &target, "@termenbox_name"]
                .map(String::from)
                .into(),
        };
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "tmux set-option failed: {}",
                tmux_stderr(&out.stderr)
            ));
        }
        Ok(())
    }

    async fn kill(&self, id: &str) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["kill-session", "-t", &format!("={id}")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "tmux kill-session failed: {}",
                tmux_stderr(&out.stderr)
            ));
        }
        Ok(())
    }

    async fn windows(&self, id: &str) -> Result<Vec<WindowState>> {
        crate::control::read_windows(&self.runner, id).await
    }

    async fn select_window(&self, id: &str, index: u32) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["select-window", "-t", &format!("={id}:{index}")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("that window is gone"));
        }
        Ok(())
    }

    async fn focus_window(&self, id: &str, index: u32) -> Result<bool> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = [
            "list-clients",
            "-t",
            &format!("={id}"),
            "-F",
            "#{client_control_mode}",
        ]
        .map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        let real_client = String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l.trim() == "0");
        if !out.status.success() || real_client {
            return Ok(false);
        }
        self.select_window(id, index).await?;
        Ok(true)
    }

    async fn new_window(&self, id: &str) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        // tmux does not expand a format in -c, so ask for the current window's directory first. It is passed
        // as one argument, never through a shell, and only when it is a sane absolute path.
        let ask = [
            "display-message",
            "-p",
            "-t",
            &format!("={id}:"),
            "#{pane_current_path}",
        ]
        .map(String::from);
        let asked = self.runner.run("tmux", &ask).await?;
        let dir = String::from_utf8_lossy(&asked.stdout).trim().to_string();
        let mut args = vec!["new-window".to_string(), "-t".into(), format!("={id}:")];
        if asked.status.success() && valid_dir(&dir) {
            args.extend(["-c".to_string(), dir]);
        }
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("could not open a window"));
        }
        Ok(())
    }

    async fn kill_window(&self, id: &str, index: u32) -> Result<()> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let args = ["kill-window", "-t", &format!("={id}:{index}")].map(String::from);
        let out = self.runner.run("tmux", &args).await?;
        if !out.status.success() {
            return Err(anyhow!("that window is gone"));
        }
        Ok(())
    }

    fn attach(&self, id: &str, cols: u16, rows: u16) -> Result<Attachment> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        // `=` makes the target an exact name match.
        let args = ["attach-session", "-t", &format!("={id}")].map(String::from);
        self.runner.attach("tmux", &args, cols, rows)
    }
}

/// Several session managers at once. Session ids become `manager:name` so a tmux `work` and a zellij `work`
/// stay apart; the client shows the manager next to the name.
pub struct CompositeBackend {
    parts: Vec<(String, Arc<dyn SessionBackend>)>,
}

impl CompositeBackend {
    pub fn new(parts: Vec<(String, Arc<dyn SessionBackend>)>) -> Self {
        Self { parts }
    }

    fn part(&self, kind: &str) -> Result<&Arc<dyn SessionBackend>> {
        self.parts
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, b)| b)
            .ok_or_else(|| anyhow!("Unknown session manager"))
    }
}

#[async_trait]
impl SessionBackend for CompositeBackend {
    fn name(&self) -> &str {
        "mixed"
    }

    /// A manager that fails to answer (zellij not installed, say) only hides its own sessions.
    async fn list(&self) -> Result<Vec<SessionInfo>> {
        let mut all = Vec::new();
        for (kind, b) in &self.parts {
            match b.list().await {
                Ok(list) => all.extend(list.into_iter().map(|s| SessionInfo {
                    id: format!("{kind}:{}", s.id),
                    backend: Some(kind.clone()),
                    ..s
                })),
                Err(e) => tracing::warn!("{kind}: cannot list sessions: {e}"),
            }
        }
        Ok(all)
    }

    async fn create(&self, name: &str) -> Result<()> {
        self.create_in(name, None).await
    }

    async fn create_in(&self, name: &str, backend: Option<&str>) -> Result<()> {
        self.create_with(name, backend, None, None).await
    }

    async fn create_with(
        &self,
        name: &str,
        backend: Option<&str>,
        dir: Option<&str>,
        template: Option<&str>,
    ) -> Result<()> {
        let b = match backend {
            Some(k) => self.part(k)?,
            None => &self.parts[0].1,
        };
        b.create_with(name, backend, dir, template).await
    }

    fn attach(&self, id: &str, cols: u16, rows: u16) -> Result<Attachment> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.attach(name, cols, rows)
    }

    fn kinds(&self) -> Vec<String> {
        self.parts.iter().map(|(k, _)| k.clone()).collect()
    }

    async fn capture(&self, id: &str) -> Result<String> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.capture(name).await
    }

    async fn capture_styled(&self, id: &str) -> Result<String> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.capture_styled(name).await
    }

    async fn capture_history(&self, id: &str) -> Result<String> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.capture_history(name).await
    }

    async fn capture_window_styled(&self, id: &str, index: u32) -> Result<String> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.capture_window_styled(name, index).await
    }

    async fn send_text(&self, id: &str, text: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.send_text(name, text).await
    }

    async fn kill(&self, id: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.kill(name).await
    }

    async fn rename(&self, id: &str, name: &str) -> Result<()> {
        let (kind, name_in) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.rename(name_in, name).await
    }

    async fn windows(&self, id: &str) -> Result<Vec<WindowState>> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.windows(name).await
    }

    async fn select_window(&self, id: &str, index: u32) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.select_window(name, index).await
    }

    async fn focus_window(&self, id: &str, index: u32) -> Result<bool> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.focus_window(name, index).await
    }

    async fn kill_window(&self, id: &str, index: u32) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.kill_window(name, index).await
    }

    async fn new_window(&self, id: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.new_window(name).await
    }

    fn acp_agents(&self) -> Vec<String> {
        self.parts
            .iter()
            .flat_map(|(_, b)| b.acp_agents())
            .collect()
    }

    fn acp_watch(&self, id: &str) -> Result<tokio::sync::watch::Receiver<u64>> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.acp_watch(name)
    }

    fn acp_view(
        &self,
        id: &str,
        after: Option<u64>,
        epoch: Option<u64>,
        info_seen: u64,
    ) -> Result<(termenbox_core::protocol::AcpView, u64)> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.acp_view(name, after, epoch, info_seen)
    }

    async fn acp_prompt(&self, id: &str, text: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.acp_prompt(name, text).await
    }

    async fn acp_interrupt(&self, id: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.acp_interrupt(name).await
    }

    async fn acp_answer(&self, id: &str, prompt_id: &str, option_id: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?
            .acp_answer(name, prompt_id, option_id)
            .await
    }

    async fn acp_mode(&self, id: &str, mode: &str) -> Result<()> {
        let (kind, name) = id
            .split_once(':')
            .ok_or_else(|| anyhow!("Unknown session"))?;
        self.part(kind)?.acp_mode(name, mode).await
    }
}

/// Any session manager driven by three command lines (no shell; split on spaces).
/// `{id}` and `{name}` in an argument are replaced by the validated session id/name.
/// list prints one session per line (`id` or `id|name`); create makes a detached session;
/// attach runs in a PTY and must repaint the session when it starts and only detach when killed.
pub struct CommandBackend {
    label: String,
    list: Vec<String>,
    create: Vec<String>,
    attach: Vec<String>,
    runner: Runner,
}

impl CommandBackend {
    #[cfg(test)]
    pub fn new(label: &str, list: &str, create: &str, attach: &str) -> Result<Self> {
        Self::with_runner(label, list, create, attach, Runner::default())
    }

    pub fn with_runner(
        label: &str,
        list: &str,
        create: &str,
        attach: &str,
        runner: Runner,
    ) -> Result<Self> {
        let split = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let backend = Self {
            label: label.to_string(),
            list: split(list),
            create: split(create),
            attach: split(attach),
            runner,
        };
        for (what, cmd) in [
            ("list", &backend.list),
            ("create", &backend.create),
            ("attach", &backend.attach),
        ] {
            if cmd.is_empty() {
                return Err(anyhow!("backend command `{what}` is empty"));
            }
        }
        Ok(backend)
    }

    fn fill(template: &[String], id: &str) -> Vec<String> {
        template
            .iter()
            .map(|a| a.replace("{id}", id).replace("{name}", id))
            .collect()
    }
}

#[async_trait]
impl SessionBackend for CommandBackend {
    fn name(&self) -> &str {
        &self.label
    }

    async fn list(&self) -> Result<Vec<SessionInfo>> {
        let out = self.runner.run(&self.list[0], &self.list[1..]).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "{} list failed: {}",
                self.label,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_list(&String::from_utf8_lossy(&out.stdout))
            .into_iter()
            .filter(|s| valid_name(&s.id))
            .collect())
    }

    async fn create(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            return Err(anyhow!(
                "Session names use letters, digits, - and _ (max 64)"
            ));
        }
        let argv = Self::fill(&self.create, name);
        let out = self.runner.run_as(true, &argv[0], &argv[1..]).await?;
        if !out.status.success() {
            return Err(anyhow!(
                "{} create failed: {}",
                self.label,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    fn attach(&self, id: &str, cols: u16, rows: u16) -> Result<Attachment> {
        if !valid_name(id) {
            return Err(anyhow!("Unknown session"));
        }
        let argv = Self::fill(&self.attach, id);
        self.runner.attach(&argv[0], &argv[1..], cols, rows)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tmux_protocol_mismatch_says_what_to_do_and_other_errors_pass_through() {
        let msg = super::tmux_stderr(b"protocol version mismatch (client 8, server 7)\n");
        assert!(msg.contains("termenbox-server tmux kill-server"), "{msg}");
        assert!(msg.contains("client 8, server 7"), "{msg}");
        assert_eq!(
            super::tmux_stderr(b"no server running on /tmp/x\n"),
            "no server running on /tmp/x"
        );
    }

    use super::*;

    #[test]
    fn parses_session_lines() {
        let infos = parse_list("main|Main one\nwork\n\n|x\n");
        assert_eq!(
            infos
                .iter()
                .map(|i| (i.id.as_str(), i.name.as_str()))
                .collect::<Vec<_>>(),
            [("main", "Main one"), ("work", "work")]
        );
    }

    /// Two fake managers sharing a session name stay apart.
    #[tokio::test]
    async fn the_composite_keeps_same_named_sessions_of_different_managers_apart() {
        let a: Arc<dyn SessionBackend> = Arc::new(
            CommandBackend::new("a", r"printf work|Work\n", "true {name}", "cat").unwrap(),
        );
        let b: Arc<dyn SessionBackend> = Arc::new(
            CommandBackend::new("b", r"printf work|Work\nother\n", "true {name}", "cat").unwrap(),
        );
        let c = CompositeBackend::new(vec![("one".into(), a), ("two".into(), b)]);
        let list = c.list().await.unwrap();
        let ids: Vec<_> = list
            .iter()
            .map(|s| (s.id.as_str(), s.backend.as_deref().unwrap()))
            .collect();
        assert_eq!(
            ids,
            [
                ("one:work", "one"),
                ("two:work", "two"),
                ("two:other", "two")
            ]
        );
        assert_eq!(c.kinds(), ["one", "two"]);
        c.create_in("fresh", Some("two")).await.unwrap();
        assert!(c.create_in("fresh", Some("nope")).await.is_err());
        assert!(
            c.attach("work", 80, 24).is_err(),
            "an id without a manager is refused"
        );
        assert!(c.attach("zzz:work", 80, 24).is_err());
        assert!(c.attach("two:work", 80, 24).is_ok());
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(valid_name("agent-1_a"));
        assert!(!valid_name(""));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a:b"));
        assert!(!valid_name("=x"));
        assert!(!valid_name("a;rm"));
        assert!(!valid_name(&"a".repeat(65)));
    }

    #[test]
    fn the_native_mode_runs_commands_as_they_are_and_shows_the_host_badge() {
        let native = RunMode::parse("native", "termenbox").unwrap();
        assert_eq!(native.label(), "host:termenbox");
        assert!(native.is_host());
        let (prog, argv) = native
            .runner()
            .wrap(true, "tmux", &["new-session".to_string()]);
        assert_eq!(
            (prog.as_str(), argv),
            ("tmux", vec!["new-session".to_string()]),
            "no nsenter, no wrapper"
        );
        assert!(RunMode::parse("native", "").is_err());
    }

    #[test]
    fn run_modes_parse_and_default_to_the_container() {
        assert_eq!(
            RunMode::parse("container", "termenbox").unwrap(),
            RunMode::Container
        );
        let host = RunMode::parse("host", "root").unwrap();
        assert_eq!((host.label().as_str(), host.is_host()), ("host:root", true));
        assert_eq!(RunMode::Container.label(), "container");
        assert!(RunMode::parse("host", "").is_err());
        assert!(RunMode::parse("vm", "x").is_err());
    }

    #[test]
    fn host_commands_enter_pid_1_and_drop_to_the_chosen_user() {
        let args = ["-V".to_string()];
        let (prog, argv) = Runner::host("root").wrap(false, "tmux", &args);
        assert_eq!(prog, "nsenter");
        assert_eq!(argv.join(" "), "-t 1 -m -u -i -n -p tmux -V");
        let (_, argv) = Runner::host("termenbox").wrap(false, "tmux", &args);
        assert_eq!(
            argv.join(" "),
            "-t 1 -m -u -i -n -p runuser -u termenbox -- tmux -V"
        );
        let (prog, argv) = Runner::default().wrap(true, "tmux", &args);
        assert_eq!((prog.as_str(), argv), ("tmux", args.to_vec()));
    }

    #[test]
    fn a_server_start_leaves_the_container_cgroup_before_dropping_privileges() {
        let args = ["new-session".to_string()];
        let (_, argv) = Runner::host("termenbox").wrap(true, "tmux", &args);
        let line = argv.join(" ");
        let scope = line.find("systemd-run --scope").expect("scope");
        let user = line.find("runuser -u termenbox").expect("user");
        assert!(
            scope < user,
            "systemd-run runs as root, then runuser: {line}"
        );
        assert!(
            line.ends_with("sh runuser -u termenbox -- tmux new-session"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn the_host_self_check_rejects_the_wrong_user_and_a_missing_prefix() {
        // No prefix: commands run as this (non-root) test user, so asking for root must fail.
        let me = Runner::default();
        if std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| o.stdout)
            .map(|b| b == b"0\n")
            .unwrap_or(false)
        {
            return;
        }
        let err = me.check_host(true).await.unwrap_err().to_string();
        assert!(err.contains("not as the root user"), "{err}");
        // A prefix that cannot start gives the actionable hint.
        let broken = Runner {
            enter: vec!["/nonexistent-nsenter".into()],
            user: vec![],
            env: vec![],
            tmux: None,
            isolation: None,
        };
        let err = broken.check_host(false).await.unwrap_err().to_string();
        assert!(err.contains("--pid=host --privileged"), "{err}");
    }

    #[test]
    fn tmux_versions_are_compared_against_3_2() {
        for (out, old) in [
            ("tmux 3.8\n", Some(false)),
            ("tmux 3.2\n", Some(false)),
            ("tmux 3.3a\n", Some(false)),
            ("tmux next-3.4\n", Some(false)),
            ("tmux 3.1c\n", Some(true)),
            ("tmux 2.9a\n", Some(true)),
            ("tmux 1.8\n", Some(true)),
            ("tmux 4\n", Some(false)),
            ("tmux master\n", None),
            ("screen 4.9\n", None),
        ] {
            assert_eq!(tmux_too_old(out), old, "{out:?}");
        }
    }

    #[test]
    fn tmux_resolution_order_is_explicit_then_bundled_then_path() {
        let dir = std::env::temp_dir().join(format!("termenbox-tmux-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bundled = dir.join(BUNDLED_TMUX);
        std::fs::write(&bundled, "x").unwrap();
        let b = bundled.to_str().unwrap();
        let mut logs = vec![];

        // an explicit path wins over both of the others, even when they are newer
        let all = |p: &str| match p {
            "/opt/tmux" => Some("tmux 3.2".to_string()),
            _ => Some("tmux 3.8".to_string()),
        };
        assert_eq!(
            resolve_tmux(Some("/opt/tmux"), Some(&bundled), &all, &mut |m: String| {
                logs.push(m)
            }),
            Ok("/opt/tmux".to_string())
        );
        // no explicit one: the bundled copy wins over PATH
        assert_eq!(
            resolve_tmux(None, Some(&bundled), &all, &mut |m: String| logs.push(m)),
            Ok(b.to_string())
        );
        assert_eq!(
            resolve_tmux(Some(""), Some(&bundled), &all, &mut |m: String| logs
                .push(m)),
            Ok(b.to_string()),
            "an empty variable is not a choice"
        );
        // nothing bundled (or the file is missing): PATH
        assert_eq!(
            resolve_tmux(None, None, &all, &mut |m: String| logs.push(m)),
            Ok("tmux".into())
        );
        assert_eq!(
            resolve_tmux(None, Some(&dir.join("absent")), &all, &mut |m: String| logs
                .push(m)),
            Ok("tmux".into())
        );
        assert!(logs.iter().all(|l| !l.contains("skipping")), "{logs:?}");

        // a bundled copy older than 3.2 or one that does not run is skipped with a reason
        for v in ["tmux 3.1c", ""] {
            let mut skipped = vec![];
            let old = |p: &str| match p {
                "tmux" => Some("tmux 3.4".to_string()),
                _ => (!v.is_empty()).then(|| v.to_string()),
            };
            assert_eq!(
                resolve_tmux(None, Some(&bundled), &old, &mut |m| skipped.push(m)),
                Ok("tmux".into())
            );
            assert_eq!(skipped.len(), 1, "{skipped:?}");
            assert!(skipped[0].contains("skipping"), "{skipped:?}");
        }

        // an explicit one that is too old or does not run fails loudly, and never falls through
        let old = |p: &str| match p {
            "/opt/old" => Some("tmux 3.0a".to_string()),
            "/opt/gone" => None,
            _ => Some("tmux 3.8".to_string()),
        };
        let err = resolve_tmux(Some("/opt/old"), Some(&bundled), &old, &mut |m: String| {
            logs.push(m)
        })
        .unwrap_err();
        assert!(err.contains("older than tmux 3.2"), "{err}");
        let err = resolve_tmux(Some("/opt/gone"), Some(&bundled), &old, &mut |m: String| {
            logs.push(m)
        })
        .unwrap_err();
        assert!(err.contains("cannot be run"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_resolved_tmux_replaces_only_the_word_tmux() {
        let r = Runner {
            tmux: Some("/home/me/.local/bin/termenbox-tmux".into()),
            ..Runner::default()
        };
        let args = ["-V".to_string()];
        let (prog, argv) = r.wrap(false, "tmux", &args);
        assert_eq!(prog, "/home/me/.local/bin/termenbox-tmux");
        assert_eq!(argv, ["-V"]);
        assert_eq!(r.wrap(false, "id", &args).0, "id");
        // host mode (the container reaching into the host) keeps the host's own tmux
        assert_eq!(Runner::host("root").tmux, None);
    }

    #[test]
    fn command_backend_needs_all_three_commands() {
        assert!(CommandBackend::new("x", "ls", "", "attach {id}").is_err());
        assert!(CommandBackend::new("x", "ls", "new {name}", "attach {id}").is_ok());
    }

    /// A backend with no multiplexer binary at all, only coreutils: proves the seam works for other tools.
    #[tokio::test]
    async fn command_backend_drives_arbitrary_tools() {
        let b = CommandBackend::new("echoer", r"printf one|First\ntwo\n", "true {name}", "cat")
            .unwrap();
        // printf itself expands the backslash escapes, so the argument stays one word.
        let list = b.list().await.unwrap();
        assert_eq!(
            list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        b.create("new-one").await.unwrap();
        assert!(b.create("bad name").await.is_err());
        let mut att = b.attach("one", 80, 24).unwrap();
        att.write(b"hello\n").unwrap();
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(3), att.rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&chunk).contains("hello"));
    }

    /// Real tmux: text sent to a pane is typed there (with Enter for `\r`), and capture reads it back.
    #[tokio::test]
    async fn tmux_send_text_types_into_the_pane_and_capture_reads_it() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let b = TmuxBackend::default();
        let name = format!("snd{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
        b.create(&name).await.unwrap();
        b.send_text(&name, "echo typed$((40+2))\r").await.unwrap();
        let mut seen = String::new();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            seen = b.capture(&name).await.unwrap();
            if seen.contains("typed42") {
                break;
            }
        }
        assert!(seen.contains("typed42"), "{seen}");
        assert!(b.send_text("bad name", "x").await.is_err());
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &format!("={name}")])
            .output();
    }

    /// Real tmux: the styled capture keeps the colour codes the plain one drops.
    #[tokio::test]
    async fn tmux_styled_capture_keeps_colour_codes() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let b = TmuxBackend::default();
        let name = format!("sty{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
        b.create(&name).await.unwrap();
        b.send_text(&name, "printf '\\033[31mredword\\033[0m\\n'\r")
            .await
            .unwrap();
        let mut styled = String::new();
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            styled = b.capture_styled(&name).await.unwrap();
            // The typed command line echoes the word too; the output is the part that carries the escape code.
            if styled.contains("\x1b[31m") {
                break;
            }
        }
        let plain = b.capture(&name).await.unwrap();
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &format!("={name}")])
            .output();
        assert!(
            styled.contains("\x1b[31m") && styled.contains("redword"),
            "{styled:?}"
        );
        assert!(
            !plain.contains("\x1b["),
            "the plain capture has no escape codes"
        );
    }

    #[test]
    fn only_absolute_clean_directories_are_accepted() {
        assert!(valid_dir("/home/u/app"));
        assert!(!valid_dir("app"));
        assert!(!valid_dir("/tmp/a\nb"));
        assert!(!valid_dir(&format!("/{}", "a".repeat(600))));
    }

    #[tokio::test]
    async fn a_new_session_starts_in_the_asked_directory_and_unknown_agents_are_refused() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tm-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmpdir = dir.to_string_lossy().into_owned();
        let runner =
            Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir.as_str())), ("TMUX", None)]);
        let b = TmuxBackend::new(runner.clone());
        assert!(b
            .create_with("nd", None, Some("relative"), None)
            .await
            .is_err());
        assert!(b
            .create_with("nd", None, None, Some("rm -rf"))
            .await
            .is_err());
        b.create_with("nd", None, Some("/tmp"), None).await.unwrap();
        let mut path = String::new();
        for _ in 0..20 {
            path = b.windows("nd").await.unwrap()[0].info.path.clone();
            if path == "/tmp" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(path, "/tmp");
        b.kill("nd").await.unwrap();
        assert!(b.list().await.map(|l| l.is_empty()).unwrap_or(true));
        let _ = runner.run("tmux", &["kill-server".to_string()]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_history_includes_the_lines_that_scrolled_off_the_screen() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tm-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmpdir = dir.to_string_lossy().into_owned();
        let runner =
            Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir.as_str())), ("TMUX", None)]);
        let b = TmuxBackend::new(runner.clone());
        // tmux pads the lines to the pane width.
        let has_line = |text: &str, want: &str| text.lines().any(|l| l.trim_end() == want);
        b.create_with("hist", None, Some("/tmp"), None)
            .await
            .unwrap();
        // Far more lines than any screen holds, so the first ones are only in the scrollback.
        let args = [
            "send-keys",
            "-t",
            "=hist:",
            "seq 1 500; echo done-marker",
            "Enter",
        ]
        .map(String::from);
        runner.run("tmux", &args).await.unwrap();
        let mut all = String::new();
        for _ in 0..30 {
            all = b.capture_history("hist").await.unwrap();
            // Only the output line: the typed command line also contains the marker.
            if has_line(&all, "done-marker") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let screen = b.capture("hist").await.unwrap();
        assert!(has_line(&all, "1"), "the first line is in the history");
        assert!(!has_line(&screen, "1"), "and no longer on the screen");
        assert!(has_line(&all, "500"));
        let _ = b.kill("hist").await;
        let _ = runner.run("tmux", &["kill-server".to_string()]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_history_sent_is_the_newest_whole_lines_that_fit() {
        assert_eq!(newest_text("a\nb\n\n\n", 100), "a\nb");
        assert_eq!(newest_text("aaaa\nbbbb\ncccc", 10), "bbbb\ncccc");
        assert_eq!(
            newest_text("aaaa\nbbbb\ncccc", 9),
            "cccc",
            "a line that does not fit whole is dropped"
        );
        assert_eq!(newest_text("héllo\nwörld", 7), "wörld");
        assert_eq!(
            newest_text("abcdefghij", 4),
            "ghij",
            "one long line is cut, not dropped"
        );
    }

    #[test]
    fn a_display_name_is_one_short_line_without_the_list_separator() {
        assert_eq!(
            clean_label("  my work ").unwrap().as_deref(),
            Some("my work")
        );
        assert_eq!(clean_label("   ").unwrap(), None);
        assert!(clean_label("a|b").is_err());
        assert!(clean_label("a\nb").is_err());
        assert!(clean_label(&"x".repeat(65)).is_err());
        assert!(clean_label(&"x".repeat(64)).is_ok());
    }

    #[tokio::test]
    async fn renaming_a_session_changes_the_name_clients_show_and_not_its_id() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tm-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmpdir = dir.to_string_lossy().into_owned();
        let runner =
            Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir.as_str())), ("TMUX", None)]);
        let b = TmuxBackend::new(runner.clone());
        b.create("rn").await.unwrap();
        b.rename("rn", "Release  notes").await.unwrap();
        let l = b.list().await.unwrap();
        assert_eq!(
            (l[0].id.as_str(), l[0].name.as_str()),
            ("rn", "Release  notes")
        );
        b.rename("rn", "").await.unwrap();
        let l = b.list().await.unwrap();
        assert_eq!((l[0].id.as_str(), l[0].name.as_str()), ("rn", "rn"));
        assert!(b.rename("nope", "x").await.is_err());
        let _ = runner.run("tmux", &["kill-server".to_string()]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tmux_is_given_the_private_socket_and_config_and_other_programs_are_left_alone() {
        let args = ["list-sessions".to_string()];
        let mut r = Runner::default();
        assert_eq!(r.wrap(false, "tmux", &args).1, ["list-sessions"]);
        r.isolation = Some(Isolation::File("/c/tmux.conf".into()));
        let (prog, argv) = r.wrap(false, "tmux", &args);
        assert_eq!(
            format!("{prog} {}", argv.join(" ")),
            "tmux -L termenbox -f /c/tmux.conf list-sessions"
        );
        r.tmux = Some("/b/termenbox-tmux".into());
        assert_eq!(r.wrap(false, "tmux", &args).0, "/b/termenbox-tmux");
        assert_eq!(r.wrap(false, "id", &["-u".to_string()]).1, ["-u"]);
        // Host mode: the config path needs the user's uid, so a shell names it where the user runs.
        let mut h = Runner::host("termenbox");
        h.isolation = Some(Isolation::HostUser);
        let (_, argv) = h.wrap(false, "tmux", &args);
        let line = argv.join(" ");
        assert!(line.contains("runuser -u termenbox -- sh -c"), "{line}");
        assert!(line.ends_with("tmux list-sessions"), "{line}");
        assert!(line.contains("-L termenbox -f \"/tmp/tmux-$(id -u)/termenbox.conf\""));
    }

    #[tokio::test]
    async fn sessions_live_on_a_private_server_that_reads_termenbox_config_and_not_the_users() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tm-priv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tmpdir = dir.to_string_lossy().into_owned();
        // The user's own config, which must not be read, and the config termenbox writes.
        std::fs::write(
            dir.join(".tmux.conf"),
            "set -g default-terminal xterm\nset -g history-limit 77\n",
        )
        .unwrap();
        let conf = crate::tmuxconf::write_config(&dir, "screen-256color").unwrap();
        let env = [
            ("TMUX_TMPDIR", Some(tmpdir.as_str())),
            ("TMUX", None),
            ("HOME", Some(tmpdir.as_str())),
        ];
        let mut private = Runner::default().with_env(&env);
        private.isolation = Some(Isolation::File(conf.to_string_lossy().into_owned()));
        let user_own = Runner::default().with_env(&env);
        let b = TmuxBackend::new(private.clone());
        b.create_with("p1", None, None, None).await.unwrap();
        let show = |r: &Runner, opt: &str| {
            let (r, args) = (r.clone(), ["show-options", "-gv", opt].map(String::from));
            async move {
                String::from_utf8_lossy(&r.run("tmux", &args).await.unwrap().stdout)
                    .trim()
                    .to_string()
            }
        };
        assert_eq!(show(&private, "default-terminal").await, "screen-256color");
        assert_ne!(show(&private, "history-limit").await, "77");
        // A tmux server that started before the config said `status off` keeps its status line until the config is
        // applied to it again, which the server does at every start.
        assert_eq!(show(&private, "status").await, "off");
        private
            .run(
                "tmux",
                &["set-option", "-g", "status", "on"].map(String::from),
            )
            .await
            .unwrap();
        assert_eq!(show(&private, "status").await, "on");
        private.reload_tmux_config().await;
        assert_eq!(show(&private, "status").await, "off");
        // The user's tmux does not see it, and the session list of the private one does.
        let theirs = TmuxBackend::new(user_own.clone());
        assert!(theirs.list().await.map(|l| l.is_empty()).unwrap_or(true));
        assert_eq!(b.list().await.unwrap().len(), 1);
        let _ = private.run("tmux", &["kill-server".to_string()]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
