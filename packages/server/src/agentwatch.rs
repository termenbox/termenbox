//! The server-side watchers behind the event layer (plan v6, P.4/P.5): the process scan and screen
//! heuristics for tmux sessions. They report through the same hub as the
//! hooks, with low confidence.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use sha2::{Digest, Sha256};
use termenbox_core::events::{AgentState, Confidence, EventKind, Source};

use crate::adapters;
use crate::backend::Runner;
use crate::events::EventHub;
use crate::profile::Profiles;
use crate::reducer::Screen;

const SCAN_EVERY: Duration = Duration::from_secs(2);
/// Never more than one `capture-pane` per this long, whatever the number of sessions.
const PANE_SPACING: Duration = Duration::from_millis(250);

/// Per-session memory of the prompt heuristic: a prompt is a new one each time the matched text changes
/// or reappears after being gone.
#[derive(Default)]
pub struct PromptTracker {
    seen: HashMap<String, (String, u32)>,
}

impl PromptTracker {
    /// `matched` is the question text on screen now, if any. Returns the prompt id when it is new.
    pub fn update(&mut self, session: &str, matched: Option<&str>) -> Option<String> {
        let Some(text) = matched else {
            self.seen.remove(session);
            return None;
        };
        let hash = hex8(text);
        let (prev_hash, generation) = self.seen.get(session).cloned().unwrap_or_default();
        if prev_hash == hash {
            return None;
        }
        let generation = generation + 1;
        self.seen
            .insert(session.to_string(), (hash.clone(), generation));
        Some(format!("{generation}-{hash}"))
    }

    pub fn active(&self, session: &str) -> bool {
        self.seen.contains_key(session)
    }

    pub fn retain(&mut self, live: &HashSet<String>) {
        self.seen.retain(|k, _| live.contains(k));
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

pub fn hex8(s: &str) -> String {
    Sha256::digest(s.as_bytes())[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now_s() -> u64 {
    crate::events::now_ms() / 1000
}

/// What tmux prints when no server is running (or its socket is gone): the sessions it held are gone with it.
fn no_tmux_server(stderr: &str) -> bool {
    let e = stderr.to_ascii_lowercase();
    e.contains("no server running")
        || e.contains("error connecting to")
        || e.contains("failed to connect to server")
}

/// One scan cycle over tmux: states from the process scan, then prompts from the screens of agent sessions.
pub async fn scan_tmux_once(
    hub: &EventHub,
    runner: &Runner,
    prefix: &str,
    tracker: &mut PromptTracker,
) {
    let args = [
        "list-panes",
        "-a",
        "-F",
        "#{session_name}|#{pane_current_command}|#{window_activity}",
    ]
    .map(String::from);
    let Ok(out) = runner.run("tmux", &args).await else {
        return;
    };
    if !out.status.success() {
        // Only a tmux that says there is no server means every session is gone. Any other failure is this scan's, and
        // ending every task in flight on it would cut them all in half.
        if no_tmux_server(&String::from_utf8_lossy(&out.stderr)) {
            hub.forget_except(prefix, &[]);
            tracker.retain(&HashSet::new());
        }
        return;
    }
    let mut panes = adapters::parse_panes(&String::from_utf8_lossy(&out.stdout), now_s());
    // Events know a session by the id clients see: the manager prefix plus tmux's name.
    let names: HashMap<String, String> = panes
        .iter()
        .map(|p| (format!("{prefix}{}", p.session), p.session.clone()))
        .collect();
    for p in &mut panes {
        p.session = format!("{prefix}{}", p.session);
    }
    let live: HashSet<String> = panes.iter().map(|p| p.session.clone()).collect();
    hub.forget_except(prefix, &live.iter().cloned().collect::<Vec<_>>());
    tracker.retain(&live);
    let mut seen = HashSet::new();
    for p in panes
        .iter()
        .filter(|p| adapters::agent_name(&p.command).is_some())
    {
        let tmux_name = names[&p.session].clone();
        if !seen.insert(p.session.clone()) || !crate::backend::valid_name(&tmux_name) {
            continue;
        }
        let capture = ["capture-pane", "-p", "-t", &format!("={tmux_name}:")].map(String::from);
        let screen = match runner.run("tmux", &capture).await {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => String::new(),
        };
        if let Some(agent) = adapters::agent_name(&p.command) {
            hub.set_agent(&p.session, agent);
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
        if let Some(id) = tracker.update(&p.session, info.as_ref().map(|i| i.question.as_str())) {
            let info = info.expect("a new prompt has a screen");
            // The screen says what the question is; only the agent's own hook says it is really waiting.
            let confirmed = hub.hook_says_needs_input(&p.session);
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
                &p.session,
                Source::Screen,
                Confidence::Low,
                EventKind::State {
                    state: AgentState::NeedsInput,
                },
            );
            hub.report(
                &p.session,
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
                    &p.session,
                    Source::Screen,
                    Confidence::Low,
                    EventKind::State { state },
                );
            }
            hub.reconcile(
                &p.session,
                Screen {
                    working: v.working,
                    idle: v.idle,
                    blocked: v.blocked,
                },
            );
        }
        // While a question is on screen the scan must not flip the state back and forth.
        if !tracker.active(&p.session) {
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
                    &p.session,
                    Source::Scan,
                    Confidence::Low,
                    EventKind::State { state },
                );
            }
        }
        tokio::time::sleep(PANE_SPACING).await;
    }
}

pub fn spawn_tmux_scan(hub: EventHub, runner: Runner, prefix: String) {
    tokio::spawn(async move {
        let mut tracker = PromptTracker::default();
        loop {
            scan_tmux_once(&hub, &runner, &prefix, &mut tracker).await;
            tokio::time::sleep(SCAN_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_tmux_server_means_the_sessions_are_gone() {
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
        scan_tmux_once(&hub, &runner, "tmux:", &mut tracker).await;
        let ev = hub.since("tmux:scantest", 0);
        let agent = hub
            .snapshot()
            .into_iter()
            .find(|s| s.session == "tmux:scantest")
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
}
