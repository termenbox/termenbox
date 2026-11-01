//! The server-side watchers behind the event layer (plan v6, P.4/P.5): the process scan and screen
//! heuristics for tmux workspaces. They report through the same hub as the
//! hooks, with low confidence.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use sha2::{Digest, Sha256};
use termenbox_core::events::{AgentState, Confidence, EventKind, Source};

use crate::adapters;
use crate::backend::Runner;
use crate::events::{now_ms, EventHub};
use crate::profile::Profiles;
use crate::reducer::Screen;
use crate::status::event::{Event, Seen, Source as Evidence};

const SCAN_EVERY: Duration = Duration::from_secs(2);
/// Never more than one `capture-pane` per this long, whatever the number of workspaces.
const PANE_SPACING: Duration = Duration::from_millis(250);

/// Per-workspace memory of the prompt heuristic: a prompt is a new one each time the matched text changes
/// or reappears after being gone.
#[derive(Default)]
pub struct PromptTracker {
    seen: HashMap<String, (String, u32)>,
}

impl PromptTracker {
    /// `matched` is the question text on screen now, if any. Returns the prompt id when it is new.
    pub fn update(&mut self, workspace: &str, matched: Option<&str>) -> Option<String> {
        let Some(text) = matched else {
            self.seen.remove(workspace);
            return None;
        };
        let hash = hex8(text);
        let (prev_hash, generation) = self.seen.get(workspace).cloned().unwrap_or_default();
        if prev_hash == hash {
            return None;
        }
        let generation = generation + 1;
        self.seen
            .insert(workspace.to_string(), (hash.clone(), generation));
        Some(format!("{generation}-{hash}"))
    }

    pub fn active(&self, workspace: &str) -> bool {
        self.seen.contains_key(workspace)
    }

    pub fn retain(&mut self, live: &HashSet<String>) {
        self.seen.retain(|k, _| live.contains(k));
    }
}

/// One pane of a scan, reduced to what the status machine needs: whose it is, whether an agent runs in it, and what
/// the screen (or, without a profile, the output scan) showed.
pub struct Reading {
    pub workspace: String,
    pub tab: String,
    pub agent: bool,
    pub seen: Option<(Evidence, Seen)>,
}

/// An event for the machine of one (workspace, tab) session.
#[derive(Debug, PartialEq, Eq)]
pub struct TabEvent {
    pub workspace: String,
    pub tab: String,
    pub event: Event,
}

/// Remembers which tabs had an agent in the last scan, so a tab whose agent is gone (its command changed, its pane or
/// window closed, tmux itself gone) says `ProcessGone` once and the machine can end an open turn as `failed(crashed)`.
#[derive(Default)]
pub struct PaneWatch {
    agents: HashSet<(String, String)>,
}

impl PaneWatch {
    /// The events of one scan. Every agent tab gets its own observation (a screen hint never raises Attention, the
    /// machine decides); with a split tab the first agent pane drives (catalogue 33).
    pub fn events(&mut self, readings: Vec<Reading>) -> Vec<TabEvent> {
        let mut out = Vec::new();
        let mut now_agents = HashSet::new();
        for r in readings.into_iter().filter(|r| r.agent) {
            let key = (r.workspace.clone(), r.tab.clone());
            if !now_agents.insert(key) {
                continue;
            }
            if let Some((source, seen)) = r.seen {
                out.push(TabEvent {
                    workspace: r.workspace,
                    tab: r.tab,
                    event: Event::Observe { source, seen },
                });
            }
        }
        for (workspace, tab) in self.agents.difference(&now_agents) {
            out.push(TabEvent {
                workspace: workspace.clone(),
                tab: tab.clone(),
                event: Event::ProcessGone,
            });
        }
        self.agents = now_agents;
        out
    }

    /// tmux has no server: every agent it held is gone.
    pub fn all_gone(&mut self) -> Vec<TabEvent> {
        self.agents
            .drain()
            .map(|(workspace, tab)| TabEvent {
                workspace,
                tab,
                event: Event::ProcessGone,
            })
            .collect()
    }
}

/// What a recognised screen says: a waiting dialog, a working agent, or an idle one. Nothing when it shows none.
fn screen_state(v: &crate::profile::ScreenView) -> Option<AgentState> {
    if v.blocked {
        Some(AgentState::NeedsInput)
    } else if v.working {
        Some(AgentState::Working)
    } else if v.idle {
        Some(AgentState::Idle)
    } else {
        None
    }
}

/// What a recognised screen shows, as the machine's reading; nothing when the profile recognises none.
fn seen_on_screen(v: &crate::profile::ScreenView) -> Option<Seen> {
    if v.blocked {
        Some(Seen::Blocked)
    } else if v.working {
        Some(Seen::Working)
    } else if v.idle {
        Some(Seen::Idle)
    } else {
        None
    }
}

pub fn hex8(s: &str) -> String {
    Sha256::digest(s.as_bytes())[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now_s() -> u64 {
    crate::events::now_ms() / 1000
}

/// What tmux prints when no server is running (or its socket is gone): the workspaces it held are gone with it.
fn no_tmux_server(stderr: &str) -> bool {
    let e = stderr.to_ascii_lowercase();
    e.contains("no server running")
        || e.contains("error connecting to")
        || e.contains("failed to connect to server")
}

/// The text of one pane; `None` when tmux refuses (the pane is gone) or the id is not a tmux pane id.
async fn capture_pane(runner: &Runner, pane_id: &str) -> Option<String> {
    if !(pane_id.starts_with('%')
        && pane_id.len() > 1
        && pane_id[1..].chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let args = ["capture-pane", "-p", "-t", pane_id].map(String::from);
    match runner.run("tmux", &args).await {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => None,
    }
}

/// One scan cycle over tmux: states from the process scan, then prompts from the screens of agent workspaces.
pub async fn scan_tmux_once(
    hub: &EventHub,
    runner: &Runner,
    prefix: &str,
    tracker: &mut PromptTracker,
    watch: &mut PaneWatch,
) -> Vec<TabEvent> {
    let args = [
        "list-panes",
        "-a",
        "-F",
        "#{session_name}|#{pane_current_command}|#{window_activity}|#{pane_id}|#{window_active}|#{pane_active}|#{@termenbox_tab}",
    ]
    .map(String::from);
    let Ok(out) = runner.run("tmux", &args).await else {
        return Vec::new();
    };
    if !out.status.success() {
        // Only a tmux that says there is no server means every workspace is gone. Any other failure is this scan's, and
        // ending every task in flight on it would cut them all in half.
        if no_tmux_server(&String::from_utf8_lossy(&out.stderr)) {
            hub.forget_except(prefix, &[]);
            tracker.retain(&HashSet::new());
            return watch.all_gone();
        }
        return Vec::new();
    }
    let mut panes = adapters::parse_panes(&String::from_utf8_lossy(&out.stdout), now_s());
    // Events know a workspace by the id clients see: the manager prefix plus tmux's name.
    let names: HashMap<String, String> = panes
        .iter()
        .map(|p| (format!("{prefix}{}", p.workspace), p.workspace.clone()))
        .collect();
    for p in &mut panes {
        p.workspace = format!("{prefix}{}", p.workspace);
    }
    let live: HashSet<String> = panes.iter().map(|p| p.workspace.clone()).collect();
    hub.forget_except(prefix, &live.iter().cloned().collect::<Vec<_>>());
    tracker.retain(&live);
    // The hooks name a pane (`TMUX_PANE`); this is where panes learn their tab, and every agent pane reports its
    // own process state so a workspace with several tabs never shows one tab's work on another (decision 0149).
    let status = hub.tab_status();
    if let Some(status) = &status {
        let live_tabs: HashSet<(String, String)> = panes
            .iter()
            .filter(|p| !p.tab.is_empty())
            .map(|p| (p.workspace.clone(), p.tab.clone()))
            .collect();
        status.retain(&live_tabs);
    }
    let mut readings = Vec::new();
    for p in &panes {
        if let Some(status) = &status {
            status.set_pane(&p.pane_id, &p.workspace, &p.tab);
        }
        let Some(agent) = adapters::agent_name(&p.command) else {
            readings.push(Reading {
                workspace: p.workspace.clone(),
                tab: p.tab.clone(),
                agent: false,
                seen: None,
            });
            continue;
        };
        let mut seen = None;
        let (profile, _) = Profiles::builtin().for_agent(Some(agent));
        if profile.has_state_markers() {
            // The screen decides: a redraw (a client attaching, a resize) is output on every pane at once.
            if let Some(screen) = capture_pane(runner, &p.pane_id)
                .await
                .filter(|s| !s.is_empty())
            {
                let v = profile.read(&screen);
                seen = seen_on_screen(&v).map(|s| (Evidence::Screen, s));
                if let Some(status) = &status {
                    status.set_screen_state(
                        &p.workspace,
                        &p.tab,
                        Screen {
                            working: v.working,
                            idle: v.idle,
                            blocked: v.blocked,
                        },
                        Some(agent),
                        now_ms(),
                    );
                }
                tokio::time::sleep(PANE_SPACING).await;
            }
        } else if let Some(state) = adapters::scan_state(p) {
            seen = Some((
                Evidence::Scan,
                match state {
                    AgentState::Working => Seen::Working,
                    _ => Seen::Idle,
                },
            ));
            if let Some(status) = &status {
                status.set_scan_state(&p.workspace, &p.tab, state, Some(agent), now_ms());
            }
        }
        readings.push(Reading {
            workspace: p.workspace.clone(),
            tab: p.tab.clone(),
            agent: true,
            seen,
        });
    }
    let tab_events = watch.events(readings);
    // One agent pane speaks for a workspace, and it is the one the workspace shows (the active pane of its active
    // window), so the process, the screen and the state all come from the same pane. Without an active agent pane
    // the first one is used, as before.
    let mut chosen: Vec<&adapters::PaneScan> = Vec::new();
    for p in panes
        .iter()
        .filter(|p| adapters::agent_name(&p.command).is_some())
    {
        match chosen.iter().position(|c| c.workspace == p.workspace) {
            None => chosen.push(p),
            Some(i) if p.active && !chosen[i].active => chosen[i] = p,
            Some(_) => {}
        }
    }
    for p in chosen {
        let tmux_name = names[&p.workspace].clone();
        if !crate::backend::valid_name(&tmux_name) {
            continue;
        }
        // The pane itself when tmux named it (`%12`); the workspace's active pane otherwise.
        let target = if p.pane_id.starts_with('%')
            && p.pane_id[1..].chars().all(|c| c.is_ascii_digit())
            && p.pane_id.len() > 1
        {
            p.pane_id.clone()
        } else {
            format!("={tmux_name}:")
        };
        let capture = ["capture-pane", "-p", "-t", &target].map(String::from);
        let screen = match runner.run("tmux", &capture).await {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => String::new(),
        };
        if let Some(agent) = adapters::agent_name(&p.command) {
            hub.set_agent(&p.workspace, agent);
        }
        // Where the agent has a profile that tells working from idle, the screen decides both what the scan says
        // (a redraw on attach is output, not inference) and whether the reducer's turn or wait is still on screen.
        let agent = adapters::agent_name(&p.command);
        let (profile, _) = Profiles::builtin().for_agent(agent);
        let view =
            (profile.has_state_markers() && !screen.is_empty()).then(|| profile.read(&screen));
        // The question heuristic reads the last lines of the screen, which in an idle or working agent are its own
        // transcript: a `(y/n)` it printed earlier, or a numbered list in a reply, is not a dialog that waits. A screen
        // the profile recognises as idle or working (and not blocked) therefore has no prompt; only a blocked or an
        // unrecognised screen can have one.
        let recognised_without_dialog = view
            .as_ref()
            .is_some_and(|v| (v.working || v.idle) && !v.blocked);
        let info = adapters::parse_prompt(&screen).filter(|_| !recognised_without_dialog);
        if let Some(id) = tracker.update(&p.workspace, info.as_ref().map(|i| i.question.as_str())) {
            let info = info.expect("a new prompt has a screen");
            // The screen says what the question is; only the agent's own hook says it is really waiting.
            let confirmed = hub.hook_says_needs_input(&p.workspace);
            let confidence = if confirmed {
                Confidence::High
            } else {
                Confidence::Low
            };
            let prompt_kind = if info.choices.len() == 2 && info.choices[0].1.ends_with('\r') {
                "yes_no"
            } else {
                "choice"
            };
            let choices = info
                .choices
                .iter()
                .map(|(label, _)| label.clone())
                .collect();
            hub.report(
                &p.workspace,
                Source::Screen,
                Confidence::Low,
                EventKind::State {
                    state: AgentState::NeedsInput,
                },
            );
            hub.report(
                &p.workspace,
                Source::Screen,
                confidence,
                EventKind::Prompt {
                    id,
                    prompt_kind: prompt_kind.into(),
                    choices,
                    decision: None,
                },
            );
        }
        if let Some(v) = &view {
            // Without hooks the screen is the only witness: a dialog that waits is `needs_input`, as a hint (never
            // a push), and the screen says so again when it is gone, so the hint cannot outlive the dialog.
            if let Some(state) = screen_state(v) {
                hub.report(
                    &p.workspace,
                    Source::Screen,
                    Confidence::Low,
                    EventKind::State { state },
                );
            }
            hub.reconcile(
                &p.workspace,
                Some(p.tab.as_str()).filter(|t| !t.is_empty()),
                Screen {
                    working: v.working,
                    idle: v.idle,
                    blocked: v.blocked,
                },
            );
        }
        // While a question is on screen the scan must not flip the state back and forth.
        if !tracker.active(&p.workspace) {
            // A screen the profile does not recognise (a newer UI, a blank start-up) falls back to the output
            // heuristic: an agent is still reported, and the reducer's pin guards an open turn against it.
            let scanned = match &view {
                Some(v) if v.working => Some(AgentState::Working),
                Some(v) if v.idle => Some(AgentState::Idle),
                Some(v) if v.blocked => None,
                _ => adapters::scan_state(p),
            };
            if let Some(state) = scanned {
                hub.report(
                    &p.workspace,
                    Source::Scan,
                    Confidence::Low,
                    EventKind::State { state },
                );
            }
        }
        tokio::time::sleep(PANE_SPACING).await;
    }
    tab_events
}

pub fn spawn_tmux_scan(hub: EventHub, runner: Runner, prefix: String) {
    tokio::spawn(async move {
        let mut tracker = PromptTracker::default();
        let mut watch = PaneWatch::default();
        loop {
            // The machines are fed beside the legacy writers; a machine-backed tab's Attention comes from them.
            let events = scan_tmux_once(&hub, &runner, &prefix, &mut tracker, &mut watch).await;
            let now = now_ms();
            for e in events {
                hub.feed_machine(&e.workspace, &e.tab, e.event, now);
            }
            let h = hub.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let now = now_ms();
                let mut routed = h.machines().poll_transcripts(now);
                routed.extend(h.machines().tick(now));
                h.apply_effects(routed, now);
            })
            .await;
            tokio::time::sleep(SCAN_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(ws: &str, tab: &str, agent: bool, seen: Option<Seen>) -> Reading {
        Reading {
            workspace: ws.into(),
            tab: tab.into(),
            agent,
            seen: seen.map(|s| (Evidence::Screen, s)),
        }
    }

    #[test]
    fn two_agent_tabs_are_each_observed() {
        let mut w = PaneWatch::default();
        let ev = w.events(vec![
            reading("tmux:a", "t1", true, Some(Seen::Working)),
            reading("tmux:a", "t2", true, Some(Seen::Idle)),
        ]);
        assert_eq!(ev.len(), 2);
        assert!(ev.iter().any(|e| e.tab == "t1"
            && e.event
                == Event::Observe {
                    source: Evidence::Screen,
                    seen: Seen::Working
                }));
        assert!(ev.iter().any(|e| e.tab == "t2"
            && e.event
                == Event::Observe {
                    source: Evidence::Screen,
                    seen: Seen::Idle
                }));
    }

    #[test]
    fn a_split_tab_is_driven_by_its_first_agent_pane() {
        let mut w = PaneWatch::default();
        let ev = w.events(vec![
            reading("tmux:a", "t1", false, None),
            reading("tmux:a", "t1", true, Some(Seen::Working)),
            reading("tmux:a", "t1", true, Some(Seen::Idle)),
        ]);
        assert_eq!(ev.len(), 1);
    }

    #[test]
    fn a_screen_with_no_reading_emits_nothing_but_the_agent_is_still_present() {
        let mut w = PaneWatch::default();
        assert!(w
            .events(vec![reading("tmux:a", "t1", true, None)])
            .is_empty());
        // Still an agent next scan: no ProcessGone.
        assert!(w
            .events(vec![reading("tmux:a", "t1", true, None)])
            .is_empty());
    }

    #[test]
    fn killing_the_agent_mid_turn_ends_it_failed_crashed() {
        use crate::status::machine::{Machine, Status};
        let mut w = PaneWatch::default();
        let mut m = Machine::new();
        m.apply(Event::TurnStart { title: None }, 0);
        assert_eq!(m.status(), Status::Working);
        w.events(vec![reading("tmux:a", "t1", true, Some(Seen::Working))]);
        // The pane's command is a shell now.
        let ev = w.events(vec![reading("tmux:a", "t1", false, None)]);
        assert_eq!(
            ev,
            vec![TabEvent {
                workspace: "tmux:a".into(),
                tab: "t1".into(),
                event: Event::ProcessGone
            }]
        );
        for e in ev {
            m.apply(e.event, 10_000);
        }
        assert_eq!(m.status(), Status::Failed);
        // Said once.
        assert!(w
            .events(vec![reading("tmux:a", "t1", false, None)])
            .is_empty());
    }

    #[test]
    fn a_closed_tab_and_a_gone_tmux_server_both_say_process_gone() {
        let mut w = PaneWatch::default();
        w.events(vec![
            reading("tmux:a", "t1", true, None),
            reading("tmux:a", "t2", true, None),
        ]);
        let ev = w.events(vec![reading("tmux:a", "t2", true, None)]);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].tab, "t1");
        let ev = w.all_gone();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].tab, "t2");
        assert!(w.all_gone().is_empty());
    }

    #[test]
    fn only_a_missing_tmux_server_means_the_workspaces_are_gone() {
        assert!(no_tmux_server(
            "no server running on /tmp/tmux-1000/default\n"
        ));
        assert!(no_tmux_server(
            "error connecting to /tmp/tmux-1000/default (No such file or directory)"
        ));
        assert!(!no_tmux_server("server exited unexpectedly"));
        assert!(!no_tmux_server(""));
    }

    #[test]
    fn a_changed_or_returning_prompt_gets_a_fresh_id_and_a_steady_one_does_not() {
        let mut t = PromptTracker::default();
        let a = t.update("s", Some("Do you want to proceed?")).unwrap();
        assert_eq!(t.update("s", Some("Do you want to proceed?")), None);
        let b = t.update("s", Some("Overwrite file? (y/n)")).unwrap();
        assert_ne!(a, b);
        assert_eq!(t.update("s", None), None);
        assert!(!t.active("s"));
        let c = t.update("s", Some("Overwrite file? (y/n)")).unwrap();
        assert_ne!(b, c, "the same question asked again is a new prompt");
        assert!(t.update("other", Some("Overwrite file? (y/n)")).is_some());
    }

    /// A real tmux (private socket directory) running a program named `claude` that shows a question.
    async fn scan_a_question(
        hook_said_needs_input: bool,
    ) -> Option<Vec<(Source, Confidence, EventKind)>> {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return None;
        }
        let dir = std::env::temp_dir().join(format!(
            "tm-scan-{}-{}",
            std::process::id(),
            hook_said_needs_input
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // A binary named like the agent (a copy of dash, which is not a multicall binary), started after the question is printed.
        let fake = dir.join("claude");
        std::fs::copy("/bin/dash", &fake).unwrap();
        let launcher = format!(
            "echo 'Deploy to production? (y/n)'; exec {} -c 'read answer'",
            fake.display()
        );
        let tmpdir = dir.to_str().unwrap();
        let runner = Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir)), ("TMUX", None)]);
        let tmux = |args: &[&str]| {
            std::process::Command::new("env")
                .args(["-u", "TMUX"])
                .arg(format!("TMUX_TMPDIR={tmpdir}"))
                .arg("tmux")
                .args(args)
                .output()
                .unwrap()
        };
        tmux(&[
            "new-session",
            "-d",
            "-s",
            "scantest",
            "-x",
            "80",
            "-y",
            "24",
            &launcher,
        ]);
        tokio::time::sleep(Duration::from_millis(600)).await;
        let hub = EventHub::new([1; 32]);
        if hook_said_needs_input {
            hub.report(
                "tmux:scantest",
                Source::Hook,
                Confidence::High,
                EventKind::State {
                    state: AgentState::NeedsInput,
                },
            );
        }
        let mut tracker = PromptTracker::default();
        scan_tmux_once(
            &hub,
            &runner,
            "tmux:",
            &mut tracker,
            &mut PaneWatch::default(),
        )
        .await;
        let ev = hub.since("tmux:scantest", 0);
        let agent = hub
            .snapshot()
            .into_iter()
            .find(|s| s.workspace == "tmux:scantest")
            .and_then(|s| s.agent);
        assert_eq!(
            agent.as_deref(),
            Some("claude"),
            "the scan names the agent it found"
        );
        tmux(&["kill-server"]);
        let kinds: Vec<_> = ev
            .iter()
            .map(|e| (e.source, e.confidence, e.kind.clone()))
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        Some(kinds)
    }

    #[tokio::test]
    async fn a_question_on_screen_alone_is_a_low_confidence_prompt() {
        let Some(kinds) = scan_a_question(false).await else {
            return;
        };
        assert!(
            kinds.iter().any(|(s, c, k)| *s == Source::Screen
                && *c == Confidence::Low
                && matches!(k, EventKind::Prompt { .. })),
            "{kinds:?}"
        );
        assert!(kinds.iter().any(|(_, _, k)| *k
            == EventKind::State {
                state: AgentState::NeedsInput
            }));
    }

    #[tokio::test]
    async fn the_same_question_with_the_agents_own_hook_waiting_is_high_confidence() {
        let Some(kinds) = scan_a_question(true).await else {
            return;
        };
        assert!(
            kinds.iter().any(|(s, c, k)| *s == Source::Screen
                && *c == Confidence::High
                && matches!(k, EventKind::Prompt { .. })),
            "{kinds:?}"
        );
    }

    #[test]
    fn a_recorded_claude_screen_maps_to_its_state() {
        let (p, _) = Profiles::builtin().for_agent(Some("claude"));
        let read = |n: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("testdata/screens")
                .join(format!("{n}.txt"));
            screen_state(&p.read(&std::fs::read_to_string(path).unwrap()))
        };
        for dialog in [
            "claude-question",
            "claude-question-freetext",
            "claude-permission",
        ] {
            assert_eq!(read(dialog), Some(AgentState::NeedsInput), "{dialog}");
        }
        assert_eq!(read("claude-working"), Some(AgentState::Working));
        assert_eq!(read("claude-idle"), Some(AgentState::Idle));
    }

    /// A status line under the input box is the user's own text. Token counts and a model's "(thinking)" there read
    /// as the spinner and made an idle agent `working` on every scan.
    #[test]
    fn a_status_line_under_the_input_box_is_not_the_agent_working() {
        let (p, _) = Profiles::builtin().for_agent(Some("claude"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/screens");
        let idle = std::fs::read_to_string(path.join("claude-idle.txt")).unwrap();
        let working = std::fs::read_to_string(path.join("claude-working.txt")).unwrap();
        for status in [
            "  Opus 4.1 (thinking) | 4% ctx",
            "  Opus | 120k tokens · $1.20",
            "  Opus | (1.2M tokens)",
        ] {
            let with = |screen: &str| {
                let mut lines: Vec<&str> = screen.lines().collect();
                let at = lines.len() - 2;
                lines[at] = status;
                lines.join("\n")
            };
            let v = p.read(&with(&idle));
            assert_eq!(screen_state(&v), Some(AgentState::Idle), "{status}");
            // The real spinner above the box still counts.
            let v = p.read(&with(&working));
            assert_eq!(screen_state(&v), Some(AgentState::Working), "{status}");
        }
    }
}
