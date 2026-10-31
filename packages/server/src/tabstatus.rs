//! Per-tab agent state (decision 0149). The workspace ring stays the aggregate that Home, push and history use; this
//! map adds which tab is working, waiting or failed, and the prompt that tab is waiting on.
//!
//! A hook carries the pane it ran in (`TMUX_PANE`); the scan maps each pane to its `@termenbox_tab`, so an event from
//! one tab never colors another. A pane the scan has not seen yet simply has no tab state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use termenbox_core::events::{AgentState, EventKind};
use termenbox_core::events::{TabAgentState, TabPrompt};

/// Events newer than this are never contradicted by the screen.
const GRACE_MS: u64 = 4_000;

/// The state of every known tab and the pane -> tab mapping the hooks use.
#[derive(Default)]
pub struct TabStatus {
    panes: Mutex<HashMap<String, (String, String)>>,
    states: Mutex<HashMap<(String, String), TabAgentState>>,
    rev: AtomicU64,
}

impl TabStatus {
    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Relaxed)
    }

    pub fn set_pane(&self, pane: &str, workspace: &str, tab: &str) {
        if pane.is_empty() || tab.is_empty() {
            return;
        }
        self.panes
            .lock()
            .unwrap()
            .insert(pane.to_string(), (workspace.to_string(), tab.to_string()));
    }

    pub fn tab_for_pane(&self, pane: &str) -> Option<(String, String)> {
        self.panes.lock().unwrap().get(pane).cloned()
    }

    /// The process scan is low confidence: it may say the pane went quiet while a hook-confirmed dialog still waits.
    pub fn set_scan_state(
        &self,
        workspace: &str,
        tab: &str,
        state: AgentState,
        agent: Option<&str>,
        now: u64,
    ) {
        if tab.is_empty() {
            return;
        }
        let mut states = self.states.lock().unwrap();
        if let Some(entry) = states.get_mut(&(workspace.to_string(), tab.to_string())) {
            // Quiet output must not erase a hook's answer, failure or finished state; a new turn (Working) may.
            if matches!(
                entry.state,
                AgentState::NeedsInput | AgentState::Error | AgentState::Done
            ) && state != AgentState::Working
            {
                if let Some(agent) = agent {
                    entry.agent = Some(agent.to_string());
                }
                return;
            }
        }
        self.apply_state(&mut states, workspace, tab, state, agent, now);
    }

    /// The state the reducer derived from this tab's hook; a hook is a fact, so it overrides what the scan saw.
    pub fn set_hook_state(&self, workspace: &str, tab: &str, state: AgentState, now: u64) {
        if tab.is_empty() {
            return;
        }
        let mut states = self.states.lock().unwrap();
        self.apply_state(&mut states, workspace, tab, state, None, now);
    }

    /// The owner has seen a tab's result: `done` and `error` go back to `idle`. Anything live stays.
    pub fn clear_result(&self, workspace: &str, tab: &str) {
        let mut states = self.states.lock().unwrap();
        if let Some(entry) = states.get_mut(&(workspace.to_string(), tab.to_string())) {
            if matches!(entry.state, AgentState::Done | AgentState::Error) {
                entry.state = AgentState::Idle;
                entry.waiting_since = None;
                self.rev.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// What the tab's screen shows, for an agent whose profile tells working from idle. A redraw is output, not
    /// inference, so only the markers decide. A wait a hook opened is closed only by a screen that shows no dialog
    /// and no work, and only once the last event is `GRACE_MS` old (the screen lags a hook by a redraw).
    pub fn set_screen_state(
        &self,
        workspace: &str,
        tab: &str,
        screen: crate::reducer::Screen,
        agent: Option<&str>,
        now: u64,
    ) {
        if tab.is_empty() || screen.blocked {
            return;
        }
        let state = if screen.working {
            AgentState::Working
        } else if screen.idle {
            AgentState::Idle
        } else {
            return;
        };
        let mut states = self.states.lock().unwrap();
        if let Some(entry) = states.get(&(workspace.to_string(), tab.to_string())) {
            match entry.state {
                AgentState::Error | AgentState::Done if state != AgentState::Working => return,
                AgentState::NeedsInput if now.saturating_sub(entry.ts) < GRACE_MS => return,
                _ => {}
            }
        }
        self.apply_state(&mut states, workspace, tab, state, agent, now);
    }

    /// The state the scan saw for one tab; `agent` is the process name it recognised.
    pub fn set_state(
        &self,
        workspace: &str,
        tab: &str,
        state: AgentState,
        agent: Option<&str>,
        now: u64,
    ) {
        if tab.is_empty() {
            return;
        }
        let mut states = self.states.lock().unwrap();
        self.apply_state(&mut states, workspace, tab, state, agent, now);
    }

    fn new_state(workspace: &str, tab: &str, now: u64) -> TabAgentState {
        TabAgentState {
            workspace: workspace.to_string(),
            tab: tab.to_string(),
            state: AgentState::Unknown,
            agent: None,
            ts: now,
            waiting_since: None,
            last_response_at: None,
            prompt: None,
        }
    }

    /// Applies one state while the caller already holds the state lock. This keeps a scan or hook from taking
    /// the same mutex twice and keeps the hot path to one short critical section.
    fn apply_state(
        &self,
        states: &mut HashMap<(String, String), TabAgentState>,
        workspace: &str,
        tab: &str,
        state: AgentState,
        agent: Option<&str>,
        now: u64,
    ) {
        let entry = states
            .entry((workspace.to_string(), tab.to_string()))
            .or_insert_with(|| Self::new_state(workspace, tab, now));
        let was_working = entry.state == AgentState::Working;
        if let Some(agent) = agent {
            entry.agent = Some(agent.to_string());
        }
        entry.ts = now;
        if state != AgentState::NeedsInput {
            entry.prompt = None;
        }
        entry.waiting_since = if matches!(state, AgentState::NeedsInput | AgentState::Error) {
            Some(entry.waiting_since.unwrap_or(now))
        } else {
            None
        };
        if state == AgentState::Done
            || (was_working && matches!(state, AgentState::Idle | AgentState::NeedsInput))
        {
            entry.last_response_at = Some(now);
        }
        if entry.state != state {
            entry.state = state;
            self.rev.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The tab a pane belongs to. A pane the scan has not mapped yet falls back only when the workspace has
    /// exactly one known tab; with several, guessing would colour the wrong one.
    pub fn tab_of(&self, pane: &str, workspace: &str) -> Option<String> {
        self.tab_for_pane(pane).map(|(_, tab)| tab).or_else(|| {
            let states = self.states.lock().unwrap();
            let mut found = states.keys().filter(|(w, _)| w == workspace);
            let first = found.next().map(|(_, t)| t.clone());
            if found.next().is_none() {
                first
            } else {
                None
            }
        })
    }

    /// One hook event from a pane; the scan has already mapped the pane to its tab.
    pub fn apply_pane(&self, pane: &str, workspace: &str, kinds: &[EventKind], now: u64) {
        let Some(tab) = self.tab_of(pane, workspace) else {
            return;
        };
        let ws = workspace.to_string();
        for kind in kinds {
            match kind {
                EventKind::State { state } => {
                    let mut states = self.states.lock().unwrap();
                    self.apply_state(&mut states, &ws, &tab, *state, None, now);
                }
                EventKind::Prompt {
                    id,
                    prompt_kind,
                    choices,
                    ..
                } => {
                    let mut states = self.states.lock().unwrap();
                    let entry = states
                        .entry((ws.clone(), tab.clone()))
                        .or_insert_with(|| Self::new_state(&ws, &tab, now));
                    entry.prompt = Some(TabPrompt {
                        id: id.clone(),
                        kind: prompt_kind.clone(),
                        choices: choices.clone(),
                    });
                    entry.state = AgentState::NeedsInput;
                    entry.ts = now;
                    entry.waiting_since = Some(entry.waiting_since.unwrap_or(now));
                    self.rev.fetch_add(1, Ordering::Relaxed);
                }
                EventKind::Message { .. } => {
                    let mut states = self.states.lock().unwrap();
                    if let Some(entry) = states.get_mut(&(ws.clone(), tab.clone())) {
                        entry.last_response_at = Some(now);
                        entry.ts = now;
                    }
                }
                _ => {}
            }
        }
    }

    /// The prompt a tab's own hook confirmed, if it still waits.
    pub fn prompt(&self, workspace: &str, tab: &str) -> Option<TabPrompt> {
        self.states
            .lock()
            .unwrap()
            .get(&(workspace.to_string(), tab.to_string()))
            .filter(|s| s.state == AgentState::NeedsInput)
            .and_then(|s| s.prompt.clone())
    }

    /// Drops one tab immediately: a close must not leave its prompt or state until the next scan.
    pub fn forget_tab(&self, workspace: &str, tab: &str) {
        let mut states = self.states.lock().unwrap();
        let removed = states
            .remove(&(workspace.to_string(), tab.to_string()))
            .is_some();
        drop(states);
        self.panes
            .lock()
            .unwrap()
            .retain(|_, (w, t)| !(w == workspace && t == tab));
        if removed {
            self.rev.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drops tabs whose window the scan no longer sees (closed, or a dormant workspace gone from tmux).
    pub fn retain(&self, live: &std::collections::HashSet<(String, String)>) {
        let mut states = self.states.lock().unwrap();
        let before = states.len();
        states.retain(|key, _| live.contains(key));
        let changed = states.len() != before;
        drop(states);
        let mut panes = self.panes.lock().unwrap();
        panes.retain(|_, (workspace, tab)| live.contains(&(workspace.clone(), tab.clone())));
        if changed {
            self.rev.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> Vec<TabAgentState> {
        let mut out: Vec<TabAgentState> = self.states.lock().unwrap().values().cloned().collect();
        out.sort_by(|a, b| a.workspace.cmp(&b.workspace).then(a.tab.cmp(&b.tab)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(working: bool, idle: bool, blocked: bool) -> crate::reducer::Screen {
        crate::reducer::Screen {
            working,
            idle,
            blocked,
        }
    }

    #[test]
    fn output_alone_never_makes_a_tab_work_and_the_screen_does() {
        let status = TabStatus::default();
        status.set_state("w", "t1", AgentState::Idle, Some("claude"), 0);
        // A redraw: the screen shows an idle prompt, so nothing changes.
        status.set_screen_state(
            "w",
            "t1",
            screen(false, true, false),
            Some("claude"),
            10_000,
        );
        assert_eq!(status.snapshot()[0].state, AgentState::Idle);
        status.set_screen_state(
            "w",
            "t1",
            screen(true, false, false),
            Some("claude"),
            11_000,
        );
        assert_eq!(status.snapshot()[0].state, AgentState::Working);
        // A screen with no marker at all changes nothing.
        status.set_screen_state(
            "w",
            "t1",
            screen(false, false, false),
            Some("claude"),
            12_000,
        );
        assert_eq!(status.snapshot()[0].state, AgentState::Working);
    }

    #[test]
    fn a_wait_survives_until_the_screen_shows_no_dialog_after_the_grace() {
        let status = TabStatus::default();
        status.set_state("w", "t1", AgentState::NeedsInput, Some("claude"), 10_000);
        status.set_screen_state(
            "w",
            "t1",
            screen(false, true, false),
            Some("claude"),
            11_000,
        );
        assert_eq!(
            status.snapshot()[0].state,
            AgentState::NeedsInput,
            "within the grace"
        );
        status.set_screen_state("w", "t1", screen(false, true, true), Some("claude"), 20_000);
        assert_eq!(
            status.snapshot()[0].state,
            AgentState::NeedsInput,
            "the dialog is still shown"
        );
        status.set_screen_state(
            "w",
            "t1",
            screen(false, true, false),
            Some("claude"),
            20_000,
        );
        assert_eq!(
            status.snapshot()[0].state,
            AgentState::Idle,
            "Esc sent no hook"
        );
    }

    #[test]
    fn a_hook_state_replaces_an_error_and_seeing_the_result_clears_it() {
        let status = TabStatus::default();
        status.set_hook_state("w", "t1", AgentState::Error, 10);
        assert_eq!(status.snapshot()[0].state, AgentState::Error);
        status.clear_result("w", "t1");
        assert_eq!(status.snapshot()[0].state, AgentState::Idle);
        status.set_hook_state("w", "t1", AgentState::Working, 20);
        status.clear_result("w", "t1");
        assert_eq!(
            status.snapshot()[0].state,
            AgentState::Working,
            "a live state is not a result"
        );
        status.set_hook_state("w", "t1", AgentState::Error, 30);
        status.set_hook_state("w", "t1", AgentState::Working, 40);
        assert_eq!(status.snapshot()[0].state, AgentState::Working);
    }

    #[test]
    fn a_hook_colors_the_tab_of_its_pane_not_the_other_one() {
        let status = TabStatus::default();
        status.set_pane("%1", "w", "t1");
        status.set_pane("%2", "w", "t2");
        status.set_state("w", "t1", AgentState::Working, Some("claude"), 10);
        status.set_state("w", "t2", AgentState::Idle, Some("codex"), 10);
        status.apply_pane(
            "%1",
            "w",
            &[
                EventKind::State {
                    state: AgentState::NeedsInput,
                },
                EventKind::Prompt {
                    id: "p1".into(),
                    prompt_kind: "yes_no".into(),
                    choices: vec!["Yes".into(), "No".into()],
                    decision: None,
                },
            ],
            20,
        );
        let states = status.snapshot();
        let one = states.iter().find(|s| s.tab == "t1").unwrap();
        let two = states.iter().find(|s| s.tab == "t2").unwrap();
        assert_eq!(one.state, AgentState::NeedsInput);
        assert_eq!(one.waiting_since, Some(20));
        assert_eq!(one.prompt.as_ref().unwrap().id, "p1");
        assert_eq!(two.state, AgentState::Idle);
        assert!(two.prompt.is_none());
    }

    #[test]
    fn a_reply_starts_the_cache_and_a_new_state_clears_the_prompt() {
        let status = TabStatus::default();
        status.set_state("w", "t1", AgentState::Working, Some("claude"), 10);
        status.set_state("w", "t1", AgentState::Idle, Some("claude"), 20);
        assert_eq!(status.snapshot()[0].last_response_at, Some(20));
        status.set_pane("%1", "w", "t1");
        status.apply_pane(
            "%1",
            "w",
            &[EventKind::Prompt {
                id: "p".into(),
                prompt_kind: "choice".into(),
                choices: vec!["A".into()],
                decision: None,
            }],
            30,
        );
        assert!(status.snapshot()[0].prompt.is_some());
        status.set_state("w", "t1", AgentState::Working, Some("claude"), 40);
        assert!(status.snapshot()[0].prompt.is_none());
    }

    #[test]
    fn mapping_a_pane_does_not_advance_the_state_revision() {
        let status = TabStatus::default();
        let rev = status.rev();
        status.set_pane("%1", "w", "t1");
        assert_eq!(status.rev(), rev, "a tab movement must not push tab_states");
        status.set_state("w", "t1", AgentState::Working, Some("claude"), 10);
        assert!(status.rev() > rev, "a real state change still does");
    }

    #[test]
    fn forgetting_a_tab_drops_its_prompt_state_and_pane_mapping() {
        let status = TabStatus::default();
        status.set_pane("%1", "w", "t1");
        status.apply_pane(
            "%1",
            "w",
            &[EventKind::Prompt {
                id: "p".into(),
                prompt_kind: "yes_no".into(),
                choices: vec!["Yes".into(), "No".into()],
                decision: None,
            }],
            20,
        );
        assert!(status.prompt("w", "t1").is_some());
        let rev = status.rev();
        status.forget_tab("w", "t1");
        assert!(status.prompt("w", "t1").is_none());
        assert!(status.snapshot().is_empty());
        assert!(status.tab_for_pane("%1").is_none());
        assert!(status.rev() > rev);
    }

    #[test]
    fn a_pane_without_a_tab_is_only_guessed_when_the_workspace_has_one_tab() {
        let status = TabStatus::default();
        status.set_state("w", "t1", AgentState::Working, None, 10);
        status.apply_pane(
            "%9",
            "w",
            &[EventKind::State {
                state: AgentState::Idle,
            }],
            20,
        );
        assert_eq!(status.snapshot()[0].state, AgentState::Idle);
        status.set_state("w", "t2", AgentState::Working, None, 10);
        status.apply_pane(
            "%9",
            "w",
            &[EventKind::State {
                state: AgentState::Idle,
            }],
            30,
        );
        assert_eq!(
            status
                .snapshot()
                .iter()
                .filter(|s| s.state == AgentState::Idle)
                .count(),
            1
        );
    }
}
