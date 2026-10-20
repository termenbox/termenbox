//! The turn-and-obligation reducer: one small state machine per session that turns what an agent says (hooks,
//! ACP requests) into `working`, `needs_input` or an ending, the way ACP does by construction.
//!
//! * A **turn** is open from the prompt until the agent answers; `working` means a turn is open and nothing waits
//!   on the human, whatever the agent is busy with (inference, a long tool).
//! * An **obligation** (a permission, a question, an elicitation) is open until it is resolved; any open one makes
//!   the state `needs_input`, and it stays so until it resolves. No clock ends it.
//! * Some endings send no event (Esc on a dialog or on a running turn fires no Claude Code hook), so the screen
//!   reconciles: an obligation or a turn the screen no longer shows is closed, but only after the screen has
//!   disagreed twice in a row and the last event is a few seconds old.
use std::collections::BTreeSet;

use termenbox_core::events::AgentState;

/// Obligations kept per session; the oldest are never dropped silently, new ones past this are not tracked.
const MAX_BLOCKS: usize = 32;
/// A screen must disagree this many scans in a row before it closes anything.
const DISAGREE_SCANS: u8 = 2;
/// Events newer than this are never contradicted by the screen: the screen lags a hook by a redraw.
const GRACE_MS: u64 = 4_000;
/// A turn or wait no event has touched for this long no longer pins the state against a guess: the agent or its
/// hooks are gone.
const PIN_MAX_MS: u64 = 30 * 60 * 1000;
/// A key that only a notification vouches for; it never opens once a real request hook has been seen.
pub const WEAK_KEY: &str = "notify";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edge {
    /// A new prompt: whatever was open before is over.
    TurnOpen,
    TurnClose(AgentState),
    BlockOpen(String),
    BlockClose(String),
    /// A tool started: the agent is busy, unless something waits on the human.
    ToolStart,
    /// A tool of this name finished, was denied or failed: a block it named is resolved.
    ToolEnd(String),
}

/// What the screen shows, for the agents that have a profile with markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub working: bool,
    pub idle: bool,
    pub blocked: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Reducer {
    turn_open: bool,
    blocks: BTreeSet<String>,
    /// The state the reducer last derived, so only a change is reported.
    last: Option<AgentState>,
    /// A real request hook (not a notification) has opened a block in this session.
    request_hook_seen: bool,
    /// ms of the last edge, on the server's clock.
    last_edge_at: u64,
    no_block_scans: u8,
    no_work_scans: u8,
}

impl Reducer {
    fn derive(&self) -> Option<AgentState> {
        if !self.blocks.is_empty() {
            Some(AgentState::NeedsInput)
        } else if self.turn_open {
            Some(AgentState::Working)
        } else {
            self.last
        }
    }

    /// Records the derived state; returns it when it differs from the last one reported.
    fn settle(&mut self, forced: Option<AgentState>) -> Option<AgentState> {
        let next = forced.or_else(|| self.derive())?;
        if self.last == Some(next) {
            return None;
        }
        self.last = Some(next);
        Some(next)
    }

    /// Applies one edge; returns the new state if it changed.
    pub fn apply(&mut self, edge: Edge, now: u64) -> Option<AgentState> {
        self.last_edge_at = now;
        self.no_block_scans = 0;
        self.no_work_scans = 0;
        match edge {
            Edge::TurnOpen => {
                self.turn_open = true;
                self.blocks.clear();
                self.settle(None)
            }
            Edge::TurnClose(outcome) => {
                self.turn_open = false;
                self.blocks.clear();
                self.settle(Some(outcome))
            }
            Edge::BlockOpen(key) => {
                if key == WEAK_KEY {
                    if self.request_hook_seen {
                        return None;
                    }
                } else {
                    self.request_hook_seen = true;
                }
                if self.blocks.len() < MAX_BLOCKS {
                    self.blocks.insert(key);
                }
                self.turn_open = true;
                self.settle(None)
            }
            Edge::BlockClose(key) => {
                if !self.blocks.remove(&key) {
                    return None;
                }
                self.settle(None)
            }
            Edge::ToolStart => {
                self.turn_open = true;
                self.settle(None)
            }
            Edge::ToolEnd(name) => {
                let closed = self.blocks.remove(&name) | self.blocks.remove(WEAK_KEY);
                if !closed && !self.turn_open {
                    // A late `PostToolUse` after the turn ended must not start it again.
                    return None;
                }
                self.settle(None)
            }
        }
    }

    /// Whether the reducer holds a fact that a low-confidence guess must not override: something waits on the
    /// human, or a turn is open.
    pub fn pins(&self) -> bool {
        self.turn_open || !self.blocks.is_empty()
    }

    /// `pins`, unless nothing has been heard for `PIN_MAX_MS`.
    pub fn pins_at(&self, now: u64) -> bool {
        self.pins() && now.saturating_sub(self.last_edge_at) < PIN_MAX_MS
    }

    pub fn blocked(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// Compares the reducer with the screen. Closes what the screen no longer shows, never opens anything.
    pub fn reconcile(&mut self, screen: Screen, now: u64) -> Option<AgentState> {
        if now.saturating_sub(self.last_edge_at) < GRACE_MS || !self.pins() {
            self.no_block_scans = 0;
            self.no_work_scans = 0;
            return None;
        }
        if self.blocked() {
            if screen.blocked {
                self.no_block_scans = 0;
                return None;
            }
            self.no_block_scans = self.no_block_scans.saturating_add(1);
            if self.no_block_scans < DISAGREE_SCANS {
                return None;
            }
            self.blocks.clear();
            self.no_block_scans = 0;
            if screen.idle && !screen.working {
                self.turn_open = false;
                return self.settle(Some(AgentState::Idle));
            }
            return self.settle(None);
        }
        // Nothing waits: an open turn that the screen shows as idle and not working is over.
        if screen.idle && !screen.working && !screen.blocked {
            self.no_work_scans = self.no_work_scans.saturating_add(1);
            if self.no_work_scans >= DISAGREE_SCANS {
                self.turn_open = false;
                self.no_work_scans = 0;
                return self.settle(Some(AgentState::Idle));
            }
        } else {
            self.no_work_scans = 0;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentState::*;

    fn r() -> Reducer {
        Reducer::default()
    }

    #[test]
    fn a_prompt_works_and_an_answer_ends_it() {
        let mut r = r();
        assert_eq!(r.apply(Edge::TurnOpen, 0), Some(Working));
        assert_eq!(
            r.apply(Edge::ToolStart, 1),
            None,
            "still working: no change"
        );
        assert_eq!(r.apply(Edge::TurnClose(Done), 2), Some(Done));
    }

    #[test]
    fn an_obligation_blocks_until_it_resolves_and_then_the_turn_resumes() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        assert_eq!(r.apply(Edge::BlockOpen("Bash".into()), 1), Some(NeedsInput));
        assert_eq!(
            r.apply(Edge::ToolStart, 2),
            None,
            "a parallel tool does not unblock"
        );
        assert_eq!(r.apply(Edge::ToolEnd("Bash".into()), 3), Some(Working));
    }

    #[test]
    fn two_obligations_need_both_resolved() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        r.apply(Edge::BlockOpen("Bash".into()), 1);
        r.apply(Edge::BlockOpen("Edit".into()), 2);
        assert_eq!(r.apply(Edge::ToolEnd("Bash".into()), 3), None);
        assert_eq!(r.apply(Edge::ToolEnd("Edit".into()), 4), Some(Working));
    }

    #[test]
    fn a_new_prompt_or_the_end_of_the_turn_drops_every_obligation() {
        let mut r = r();
        r.apply(Edge::BlockOpen("Bash".into()), 0);
        assert_eq!(r.apply(Edge::TurnOpen, 1), Some(Working));
        r.apply(Edge::BlockOpen("Bash".into()), 2);
        assert_eq!(r.apply(Edge::TurnClose(Done), 3), Some(Done));
        assert!(!r.pins());
    }

    #[test]
    fn a_late_tool_end_does_not_restart_a_finished_turn() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        r.apply(Edge::TurnClose(Done), 1);
        assert_eq!(r.apply(Edge::ToolEnd("Bash".into()), 2), None);
    }

    #[test]
    fn a_notification_alone_blocks_but_never_after_a_request_hook() {
        let mut a = r();
        a.apply(Edge::TurnOpen, 0);
        assert_eq!(
            a.apply(Edge::BlockOpen(WEAK_KEY.into()), 1),
            Some(NeedsInput)
        );
        assert_eq!(a.apply(Edge::ToolEnd("Bash".into()), 2), Some(Working));

        let mut b = r();
        b.apply(Edge::TurnOpen, 0);
        b.apply(Edge::BlockOpen("Bash".into()), 1);
        b.apply(Edge::ToolEnd("Bash".into()), 2);
        // The notification arrives seconds after the dialog was answered.
        assert_eq!(b.apply(Edge::BlockOpen(WEAK_KEY.into()), 3), None);
        assert!(!b.blocked());
    }

    fn screen(working: bool, idle: bool, blocked: bool) -> Screen {
        Screen {
            working,
            idle,
            blocked,
        }
    }

    #[test]
    fn a_dialog_dismissed_without_an_event_is_closed_after_two_scans_that_do_not_show_it() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        r.apply(Edge::BlockOpen("AskUserQuestion".into()), 1_000);
        // Inside the grace window the screen is not believed.
        assert_eq!(r.reconcile(screen(false, true, false), 2_000), None);
        assert_eq!(
            r.reconcile(screen(false, true, false), 6_000),
            None,
            "one scan is not enough"
        );
        assert_eq!(r.reconcile(screen(false, true, false), 8_000), Some(Idle));
        assert!(!r.pins());
    }

    #[test]
    fn a_dialog_still_on_screen_stays_blocked_for_as_long_as_it_is() {
        let mut r = r();
        r.apply(Edge::BlockOpen("Bash".into()), 0);
        for t in (5_000..600_000).step_by(2_000) {
            assert_eq!(r.reconcile(screen(false, false, true), t), None);
        }
        assert!(r.blocked(), "no clock ends an obligation");
    }

    #[test]
    fn a_dismissed_dialog_with_the_agent_still_busy_goes_back_to_working() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        r.apply(Edge::BlockOpen("Bash".into()), 0);
        r.reconcile(screen(true, false, false), 5_000);
        assert_eq!(
            r.reconcile(screen(true, false, false), 7_000),
            Some(Working)
        );
    }

    #[test]
    fn an_interrupted_turn_with_no_stop_event_ends_when_the_screen_is_idle_twice() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        assert_eq!(r.reconcile(screen(false, true, false), 5_000), None);
        assert_eq!(r.reconcile(screen(false, true, false), 7_000), Some(Idle));
    }

    #[test]
    fn a_working_screen_resets_the_disagreement() {
        let mut r = r();
        r.apply(Edge::TurnOpen, 0);
        r.reconcile(screen(false, true, false), 5_000);
        r.reconcile(screen(true, false, false), 7_000);
        assert_eq!(r.reconcile(screen(false, true, false), 9_000), None);
        assert!(r.pins());
    }

    #[test]
    fn the_reducer_is_bounded() {
        let mut r = r();
        for i in 0..200 {
            r.apply(Edge::BlockOpen(format!("k{i}")), i);
        }
        assert!(r.blocks.len() <= MAX_BLOCKS);
    }
}
