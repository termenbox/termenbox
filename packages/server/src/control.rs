//! tmux control mode (plan H.3): one `tmux -C` client per workspace tells us the moment a window is added,
//! closed or renamed, or a pane changes its command or directory, so tabs and the windows dropdown follow
//! without polling. The control client only says *that* something changed; the content comes from one
//! `list-panes -s`, so no control-mode output (which can carry pane text) is ever parsed or stored.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use termenbox_core::protocol::WindowInfo;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::adapters;
use crate::backend::{valid_target, Runner};

/// Subscriptions (`refresh-client -B`) and `ignore-size` need tmux 3.2.
pub const MIN_TMUX: (u32, u32) = (3, 2);

/// Most one workspace is refreshed: a working agent changes `window_activity` every second.
const REFRESH_EVERY: Duration = Duration::from_secs(1);
/// How often the set of workspaces is looked at, to start and stop control clients.
const WORKSPACES_EVERY: Duration = Duration::from_secs(2);

/// `tmux -V` output (`tmux 3.3a`, `tmux next-3.4`, `tmux master`) as (major, minor).
pub fn parse_tmux_version(out: &str) -> Option<(u32, u32)> {
    let v = out.trim().strip_prefix("tmux")?.trim();
    if v == "master" {
        return Some((u32::MAX, 0));
    }
    let v = v.strip_prefix("next-").unwrap_or(v);
    let mut parts = v.splitn(2, '.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts
        .next()?
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()?;
    Some((major, minor))
}

pub fn control_supported(out: &str) -> bool {
    parse_tmux_version(out).is_some_and(|v| v >= MIN_TMUX)
}

/// The version number alone (`3.6`) for the client's notice.
pub fn version_label(out: &str) -> String {
    out.trim().trim_start_matches("tmux").trim().to_string()
}

pub const PANE_FORMAT: &str = "#{window_index}|#{window_id}|#{@termenbox_tab}|#{@termenbox_label}|#{window_active}|#{window_activity}|#{pane_active}|#{pane_current_command}|#{pane_current_path}|#{window_name}";

/// One window plus what the registry and the representative rule need (not sent to clients as such).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub info: WindowInfo,
    pub activity: u64,
    /// tmux's stable window id (`@12`), not the index.
    pub window_id: String,
    /// The registry tab this window carries (`@termenbox_tab`), empty for a window nobody registered.
    pub tab: String,
}

/// Window and path names come from the machine's shell and can hold anything: control characters are dropped
/// and the length capped.
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// `list-panes -s -F PANE_FORMAT` as one entry per window, taken from the window's active pane.
pub fn parse_windows(out: &str) -> Vec<WindowState> {
    let mut by_index: HashMap<u32, (WindowState, bool)> = HashMap::new();
    for line in out.lines() {
        let f: Vec<&str> = line.splitn(10, '|').collect();
        if f.len() < 10 {
            continue;
        }
        let (Ok(index), Ok(activity)) = (f[0].parse::<u32>(), f[5].parse::<u64>()) else {
            continue;
        };
        let pane_active = f[6] == "1";
        let command = clean(f[7], 64);
        let label = clean(f[3], 64);
        let state = WindowState {
            info: WindowInfo {
                index,
                name: if label.is_empty() {
                    clean(f[9], 64)
                } else {
                    label
                },
                agent: adapters::agent_name(&command).map(String::from),
                command,
                path: clean(f[8], 256),
                active: f[4] == "1",
            },
            activity,
            window_id: clean(f[1], 32),
            tab: clean(f[2], 32),
        };
        match by_index.get(&index) {
            Some((_, true)) => {}
            Some((_, false)) if !pane_active => {}
            _ => {
                by_index.insert(index, (state, pane_active));
            }
        }
    }
    let mut all: Vec<WindowState> = by_index.into_values().map(|(s, _)| s).collect();
    all.sort_by_key(|w| w.info.index);
    all
}

/// The window a workspace's tab speaks for: the window tmux has current (the one the user last had open, so a
/// tab switch lands where they left), else the agent window used last (ties go to the leftmost), else the first.
pub fn representative(windows: &[WindowState]) -> Option<u32> {
    windows
        .iter()
        .find(|w| w.info.active)
        .or_else(|| {
            windows
                .iter()
                .filter(|w| w.info.agent.is_some())
                .max_by(|a, b| {
                    a.activity
                        .cmp(&b.activity)
                        .then(b.info.index.cmp(&a.info.index))
                })
        })
        .or_else(|| windows.first())
        .map(|w| w.info.index)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowUpdate {
    pub workspace: String,
    pub windows: Vec<WindowInfo>,
    pub representative: Option<u32>,
    /// The tmux window id of each entry in `windows`, in the same order.
    pub window_ids: Vec<String>,
    /// The registry tab id of each entry in `windows`, in the same order.
    pub tab_ids: Vec<String>,
}

impl WindowUpdate {
    /// The registry tab of the window tmux has current, when that window carries one. A window made in tmux
    /// directly has no tab until the registry adopts it, and a session with no current window names none.
    pub fn active_tab(&self) -> Option<&str> {
        let at = self.windows.iter().position(|w| w.active)?;
        self.tab_ids
            .get(at)
            .map(String::as_str)
            .filter(|tab| !tab.is_empty())
    }
}

/// The latest windows of every tmux workspace, and a stream of changes.
#[derive(Clone)]
pub struct WindowHub {
    workspaces: Arc<Mutex<HashMap<String, WindowUpdate>>>,
    tx: broadcast::Sender<WindowUpdate>,
}

impl Default for WindowHub {
    fn default() -> Self {
        Self {
            workspaces: Arc::default(),
            tx: broadcast::channel(256).0,
        }
    }
}

impl WindowHub {
    pub fn subscribe(&self) -> broadcast::Receiver<WindowUpdate> {
        self.tx.subscribe()
    }

    /// Stores the windows of `session` and tells subscribers only when something a client shows has changed.
    pub fn publish(&self, workspace: &str, states: &[WindowState]) -> bool {
        let update = WindowUpdate {
            workspace: workspace.to_string(),
            windows: states.iter().map(|w| w.info.clone()).collect(),
            representative: representative(states),
            window_ids: states.iter().map(|w| w.window_id.clone()).collect(),
            tab_ids: states.iter().map(|w| w.tab.clone()).collect(),
        };
        let mut workspaces = self.workspaces.lock().unwrap();
        if workspaces.get(workspace) == Some(&update) {
            return false;
        }
        workspaces.insert(workspace.to_string(), update.clone());
        let _ = self.tx.send(update);
        true
    }

    pub fn all(&self) -> Vec<WindowUpdate> {
        let mut all: Vec<_> = self.workspaces.lock().unwrap().values().cloned().collect();
        all.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        all
    }

    pub fn representative(&self, workspace: &str) -> Option<u32> {
        self.workspaces
            .lock()
            .unwrap()
            .get(workspace)
            .and_then(|u| u.representative)
    }

    pub fn forget(&self, workspace: &str) {
        self.workspaces.lock().unwrap().remove(workspace);
    }
}

/// Control-mode lines that mean "windows or panes changed". Everything else (replies, `%output`, ...) is ignored.
pub fn is_trigger(line: &str) -> bool {
    let word = line.split_whitespace().next().unwrap_or("");
    matches!(
        word,
        "%window-add"
            | "%window-close"
            | "%window-renamed"
            | "%unlinked-window-add"
            | "%unlinked-window-close"
            | "%unlinked-window-renamed"
            | "%session-window-changed"
            | "%window-pane-changed"
            | "%subscription-changed"
    )
}

/// `list-panes -s` of one workspace, parsed; an error means the workspace is gone.
pub async fn read_windows(runner: &Runner, name: &str) -> Result<Vec<WindowState>> {
    if !valid_target(name) {
        return Err(anyhow!("Unknown workspace"));
    }
    let args = [
        "list-panes",
        "-s",
        "-t",
        &format!("={name}"),
        "-F",
        PANE_FORMAT,
    ]
    .map(String::from);
    let out = runner.run("tmux", &args).await?;
    if !out.status.success() {
        return Err(anyhow!("the workspace has no windows"));
    }
    Ok(parse_windows(&String::from_utf8_lossy(&out.stdout)))
}

/// Watches one workspace until it ends or its control client exits.
async fn watch(hub: WindowHub, runner: Runner, prefix: String, name: String) {
    let key = format!("{prefix}{name}");
    let args = [
        "-C",
        "attach-session",
        "-f",
        "no-output,ignore-size",
        "-t",
        &format!("={name}"),
    ]
    .map(String::from);
    let Ok(mut child) = runner.spawn_piped("tmux", &args) else {
        return;
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return;
    };
    // Every pane, so a command or directory change in any window is heard (tmux checks about once a second).
    let sub = "refresh-client -B \"termenbox:%*:#{pane_current_command}|#{pane_current_path}|#{window_activity}\"\n";
    if stdin.write_all(sub.as_bytes()).await.is_err() {
        return;
    }
    let mut lines = BufReader::new(stdout).lines();
    let mut dirty = true;
    let mut next_allowed = Instant::now();
    let mut failures = 0u8;
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => if is_trigger(&l) { dirty = true },
                _ => break,
            },
            _ = tokio::time::sleep_until(next_allowed), if dirty => {
                dirty = false;
                next_allowed = Instant::now() + REFRESH_EVERY;
                match read_windows(&runner, &name).await {
                    // An empty list means the pane is not there yet; publishing it would hide the workspace.
                    Ok(states) if states.is_empty() => {
                        failures += 1;
                        if failures > 10 { break; }
                        dirty = true;
                        next_allowed = Instant::now() + Duration::from_millis(200);
                    }
                    Ok(states) => { failures = 0; hub.publish(&key, &states); }
                    // The session may still be starting: ask again shortly instead of losing the watch.
                    Err(_) => {
                        failures += 1;
                        if failures > 10 { break; }
                        dirty = true;
                        next_allowed = Instant::now() + Duration::from_millis(200);
                    }
                }
            }
        }
    }
    // `stdin` is dropped here, which ends the control client; the child is killed with it.
    hub.forget(&key);
}

/// Starts and stops one control client per tmux workspace. Without tmux or with an old one nothing runs and the
/// caller keeps the scan.
pub fn spawn(hub: WindowHub, runner: Runner, prefix: String) {
    tokio::spawn(async move {
        let mut running: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            let args = ["list-sessions", "-F", "#{session_name}"].map(String::from);
            let names: Vec<String> = match runner.run("tmux", &args).await {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|n| valid_target(n))
                    .map(String::from)
                    .collect(),
                _ => vec![],
            };
            running.retain(|name, handle| {
                let keep = names.contains(name) && !handle.is_finished();
                if !keep {
                    handle.abort();
                    hub.forget(&format!("{prefix}{name}"));
                }
                keep
            });
            for name in names {
                if !running.contains_key(&name) {
                    running.insert(
                        name.clone(),
                        tokio::spawn(watch(hub.clone(), runner.clone(), prefix.clone(), name)),
                    );
                }
            }
            tokio::time::sleep(WORKSPACES_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        index: u32,
        active: u8,
        activity: u64,
        pane_active: u8,
        cmd: &str,
        path: &str,
        name: &str,
    ) -> String {
        format!("{index}|@{index}|||{active}|{activity}|{pane_active}|{cmd}|{path}|{name}")
    }

    #[test]
    fn versions_parse_and_the_floor_is_3_2() {
        assert_eq!(parse_tmux_version("tmux 3.6\n"), Some((3, 6)));
        assert_eq!(parse_tmux_version("tmux 3.3a"), Some((3, 3)));
        assert_eq!(parse_tmux_version("tmux next-3.4"), Some((3, 4)));
        assert_eq!(parse_tmux_version("tmux 2.7"), Some((2, 7)));
        assert_eq!(parse_tmux_version("zellij 0.40"), None);
        assert!(control_supported("tmux 3.2"));
        assert!(control_supported("tmux master"));
        assert!(!control_supported("tmux 3.1c"));
        assert!(!control_supported("tmux 3.0a"));
        assert!(!control_supported("garbage"));
        assert_eq!(version_label("tmux 3.0a\n"), "3.0a");
    }

    #[test]
    fn a_window_is_described_by_its_active_pane_and_names_are_cleaned() {
        let out = [
            row(0, 1, 100, 0, "bash", "/home/u", "main"),
            row(0, 1, 100, 1, "claude", "/home/u/app", "ed\u{7}it"),
            row(1, 0, 90, 1, "lazygit", "/home/u/app", "git"),
            "garbage line".to_string(),
        ]
        .join("\n");
        let w = parse_windows(&out);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].info.command, "claude");
        assert_eq!(w[0].info.name, "edit", "control characters are dropped");
        assert_eq!(w[0].info.agent.as_deref(), Some("claude"));
        assert!(w[0].info.active && !w[1].info.active);
        assert_eq!(w[1].info.agent, None);
    }

    #[test]
    fn a_name_with_a_bar_survives() {
        let w = parse_windows(&row(2, 0, 5, 1, "bash", "/tmp", "a|b"));
        assert_eq!(w[0].info.name, "a|b");
    }

    #[test]
    fn the_current_window_then_the_agent_used_last_speaks_for_the_workspace() {
        let w = parse_windows(
            &[
                row(0, 0, 500, 1, "bash", "/a", "sh"),
                row(1, 0, 300, 1, "claude", "/b", "c"),
                row(2, 0, 400, 1, "codex", "/c", "x"),
            ]
            .join("\n"),
        );
        assert_eq!(representative(&w), Some(2), "most recent output wins");
        let current = parse_windows(
            &[
                row(0, 1, 500, 1, "bash", "/a", "sh"),
                row(1, 0, 300, 1, "claude", "/b", "c"),
            ]
            .join("\n"),
        );
        assert_eq!(representative(&current), Some(0), "the current window wins");
        let tie = parse_windows(
            &[
                row(1, 0, 300, 1, "claude", "/b", "c"),
                row(2, 0, 300, 1, "codex", "/c", "x"),
            ]
            .join("\n"),
        );
        assert_eq!(representative(&tie), Some(1), "a tie goes to the leftmost");
    }

    #[test]
    fn without_an_agent_or_a_current_window_the_first_one_speaks() {
        let w = parse_windows(
            &[
                row(0, 0, 1, 1, "bash", "/a", "a"),
                row(3, 1, 1, 1, "vim", "/b", "b"),
            ]
            .join("\n"),
        );
        assert_eq!(representative(&w), Some(3));
        let none_active = parse_windows(&[row(4, 0, 1, 1, "bash", "/a", "a")].join("\n"));
        assert_eq!(representative(&none_active), Some(4));
        assert_eq!(representative(&[]), None);
    }

    #[test]
    fn the_hub_announces_only_what_changed() {
        let hub = WindowHub::default();
        let mut rx = hub.subscribe();
        let a = parse_windows(&row(0, 1, 100, 1, "bash", "/a", "w"));
        assert!(hub.publish("s", &a));
        assert!(rx.try_recv().is_ok());
        // Only the output time moved: nothing a client shows changed.
        let later = parse_windows(&row(0, 1, 200, 1, "bash", "/a", "w"));
        assert!(!hub.publish("s", &later));
        assert!(rx.try_recv().is_err());
        let cd = parse_windows(&row(0, 1, 200, 1, "bash", "/b", "w"));
        assert!(hub.publish("s", &cd));
        assert_eq!(hub.all().len(), 1);
        assert_eq!(hub.representative("s"), Some(0));
        hub.forget("s");
        assert!(hub.all().is_empty());
    }

    #[test]
    fn the_current_window_names_the_tab_it_carries() {
        fn update(windows: &[(u32, bool)], tabs: &[&str]) -> WindowUpdate {
            WindowUpdate {
                workspace: "w".into(),
                windows: windows
                    .iter()
                    .map(|&(index, active)| WindowInfo {
                        index,
                        name: String::new(),
                        command: String::new(),
                        path: String::new(),
                        active,
                        agent: None,
                    })
                    .collect(),
                representative: None,
                window_ids: (0..windows.len()).map(|i| format!("@{i}")).collect(),
                tab_ids: tabs.iter().map(|t| t.to_string()).collect(),
            }
        }
        let u = update(&[(0, false), (1, true), (2, false)], &["t1", "t2", "t3"]);
        assert_eq!(u.active_tab(), Some("t2"));
        // A window made in tmux directly carries no tab until the registry adopts it.
        let u = update(&[(0, false), (1, true)], &["t1", ""]);
        assert_eq!(u.active_tab(), None);
        // No current window at all.
        let u = update(&[(0, false)], &["t1"]);
        assert_eq!(u.active_tab(), None);
    }

    #[test]
    fn only_window_and_pane_changes_trigger_a_refresh() {
        assert!(is_trigger("%window-add @1"));
        assert!(is_trigger("%unlinked-window-close @1"));
        assert!(is_trigger(
            "%subscription-changed termenbox $0 @0 0 %0 : bash|/tmp|1"
        ));
        assert!(!is_trigger("%output %0 secret\\040text"));
        assert!(!is_trigger("%begin 1 2 0"));
        assert!(!is_trigger(""));
    }

    /// A real tmux (private socket directory): the control client hears windows come and go and a `cd`.
    #[tokio::test]
    async fn a_real_tmux_pushes_window_and_directory_changes() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tm-ctl-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let tmpdir = dir.to_string_lossy().into_owned();
        let runner =
            Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir.as_str())), ("TMUX", None)]);
        let version = runner.run("tmux", &["-V".to_string()]).await.unwrap();
        if !control_supported(&String::from_utf8_lossy(&version.stdout)) {
            return;
        }
        let tmux = |args: &[&str]| {
            let runner = runner.clone();
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            async move { runner.run("tmux", &args).await.unwrap() }
        };
        assert!(
            tmux(&["new-session", "-d", "-s", "ctl", "-x", "80", "-y", "24"])
                .await
                .status
                .success()
        );
        let hub = WindowHub::default();
        let mut rx = hub.subscribe();
        spawn(hub.clone(), runner.clone(), String::new());

        async fn next(
            rx: &mut broadcast::Receiver<WindowUpdate>,
            want: impl Fn(&WindowUpdate) -> bool,
        ) -> WindowUpdate {
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let u = rx.recv().await.unwrap();
                    if want(&u) {
                        return u;
                    }
                }
            })
            .await
            .expect("the update arrives")
        }
        next(&mut rx, |u| u.windows.len() == 1).await;
        tmux(&["new-window", "-t", "=ctl:", "-n", "second"]).await;
        let u = next(&mut rx, |u| u.windows.len() == 2).await;
        assert_eq!(u.windows[1].name, "second");
        tmux(&["send-keys", "-t", "=ctl:second", "cd /tmp", "Enter"]).await;
        next(&mut rx, |u| u.windows.iter().any(|w| w.path == "/tmp")).await;
        tmux(&["kill-window", "-t", "=ctl:second"]).await;
        next(&mut rx, |u| u.windows.len() == 1).await;
        tmux(&["kill-server"]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
