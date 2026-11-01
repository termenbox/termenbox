//! The edge-case catalogue of `docs/plans/agent-status-model.md`, one named test per row (D.1). Each replays an event
//! script through the machine on an explicit clock and asserts the status, the roll-up and the alerts at every step.
//! `all_35_rows_have_a_test` fails when a row is missing from `CASES`.
//!
//! An alert is an effect that raises a `needs_you` or `result` item: `RaiseAsk`, `RaiseFinished`, `RaiseFailed`.
//! Rows that need more than one machine (two tabs, devices) or the pane watcher say so and test the part the machine
//! owns; the tier-2 scenarios (D.3, D.5) cover the rest through the real stack.
use termenbox_core::events::{AskKind, FailureCause, SubReason};

use super::event::*;
use super::machine::*;

/// Past `GRACE_MS` and two scans apart, so an observation is allowed to correct the machine.
const LATER: u64 = 20_000;

/// A machine with a clock and the log of every effect it returned.
struct Sim {
    m: Machine,
    log: Vec<Effect>,
    now: u64,
}

impl Sim {
    fn new() -> Self {
        Sim {
            m: Machine::new(),
            log: Vec::new(),
            now: 0,
        }
    }

    /// A session that started and sits at the prompt.
    fn started() -> Self {
        let mut s = Self::new();
        s.at(0, Event::SessionStart(SessionSource::Startup));
        s
    }

    /// A session with a turn open.
    fn working() -> Self {
        let mut s = Self::started();
        s.at(1_000, turn_start());
        s
    }

    fn at(&mut self, t: u64, e: Event) -> Vec<Effect> {
        self.now = t;
        let out = self.m.apply(e, t);
        self.log.extend(out.clone());
        out
    }

    fn tick(&mut self, t: u64) -> Vec<Effect> {
        self.now = t;
        let out = self.m.tick(t);
        self.log.extend(out.clone());
        out
    }

    fn view(&self) -> View {
        self.m.view(self.now)
    }

    fn status(&self) -> Status {
        self.view().status
    }

    fn sub(&self) -> Option<SubReason> {
        self.view().sub_reason
    }

    fn alerts(&self) -> usize {
        self.log.iter().filter(|e| is_alert(e)).count()
    }

    fn expect(&self, status: Status, what: &str) {
        assert_eq!(self.status(), status, "{what} (view {:?})", self.view());
    }
}

fn is_alert(e: &Effect) -> bool {
    matches!(
        e,
        Effect::RaiseAsk { .. } | Effect::RaiseFinished | Effect::RaiseFailed(_)
    )
}

fn turn_start() -> Event {
    Event::TurnStart {
        title: Some("fix it".into()),
    }
}
fn done(bg: u32) -> Event {
    Event::TurnEnd {
        outcome: Outcome::Done,
        bg,
    }
}
fn failed(cause: FailureCause) -> Event {
    Event::TurnEnd {
        outcome: Outcome::Failed(cause),
        bg: 0,
    }
}
fn tool(n: &str) -> Event {
    Event::ToolStart { name: n.into() }
}
fn tool_end(n: &str) -> Event {
    Event::ToolEnd { name: n.into() }
}
fn open(key: &str, kind: AskKind) -> Event {
    Event::AskOpen {
        key: key.into(),
        kind,
    }
}
fn text(key: &str, t: &str, options: &[&str]) -> Event {
    Event::AskContent {
        key: key.into(),
        text: t.into(),
        options: options.iter().map(|o| o.to_string()).collect(),
    }
}
fn close(key: &str, outcome: AskOutcome) -> Event {
    Event::AskClose {
        key: key.into(),
        outcome,
    }
}
fn screen(seen: Seen) -> Event {
    Event::Observe {
        source: Source::Screen,
        seen,
    }
}
fn transcript(seen: Seen) -> Event {
    Event::Observe {
        source: Source::Transcript,
        seen,
    }
}
fn clear_ask(key: &str) -> Effect {
    Effect::Clear(ItemId::Ask(key.into()))
}

#[test]
fn case_01_a_plain_turn_is_working_then_finished_with_one_chime() {
    let mut s = Sim::started();
    s.at(1_000, turn_start());
    s.expect(Status::Working, "after the prompt");
    assert_eq!(s.sub(), Some(SubReason::Inferring));
    assert_eq!(s.alerts(), 0, "no alert while it works");
    s.at(9_000, done(0));
    s.expect(Status::Finished, "after Stop");
    assert_eq!(s.log, vec![Effect::RaiseFinished]);
    assert_eq!(s.alerts(), 1);
}

#[test]
fn case_02_a_long_silent_tool_stays_working_and_never_decays() {
    let mut s = Sim::working();
    s.at(2_000, tool("Bash"));
    for minutes in [1u64, 5, 20, 60] {
        s.now = 2_000 + minutes * 60_000;
        s.expect(Status::Working, "a tool is running");
        assert_eq!(s.sub(), Some(SubReason::Tool), "at {minutes} min");
        assert_eq!(s.view().tool.as_deref(), Some("Bash"));
    }
    assert!(s.tick(2_000 + 3_600_000).is_empty(), "no notice for a tool");
    assert!(s.log.is_empty());
}

#[test]
fn case_03_a_permission_allowed_or_denied() {
    // Allowed: needs_you, then the tool runs.
    let mut s = Sim::working();
    s.at(2_000, open("Bash", AskKind::Permission));
    s.expect(Status::NeedsYou, "permission asked");
    assert_eq!(s.view().ask, Some(AskKind::Permission));
    s.at(6_000, close("Bash", AskOutcome::Answered));
    s.at(6_100, tool("Bash"));
    s.expect(Status::Working, "allowed");
    assert_eq!(s.sub(), Some(SubReason::Tool));
    assert_eq!(s.alerts(), 1, "one alert for the ask");

    // Denied: the ask clears and the agent carries on or stops, as the transcript says.
    let mut d = Sim::working();
    d.at(2_000, open("Bash", AskKind::Permission));
    d.at(6_000, close("Bash", AskOutcome::Declined));
    d.expect(Status::Working, "denied, the turn goes on");
    d.at(6_000 + LATER, transcript(Seen::Idle));
    d.expect(Status::Idle, "denied and the agent stopped");
    assert_eq!(d.alerts(), 1);
}

#[test]
fn case_04_a_permission_then_esc_clears_within_the_reconcile_window() {
    let mut s = Sim::working();
    s.at(2_000, open("Bash", AskKind::Permission));
    s.expect(Status::NeedsYou, "asked");
    // Esc fires no hook: only the screen says the prompt is gone, and one scan is not enough.
    s.at(10_000, screen(Seen::Idle));
    s.expect(Status::NeedsYou, "one scan does not close it");
    s.at(11_000, screen(Seen::Idle));
    s.expect(Status::Idle, "the second scan closes it");
    assert_eq!(s.view().asks, 0, "no stale ask");
    assert!(s.log.contains(&clear_ask("Bash")));
    assert_eq!(s.alerts(), 1, "no failed alert for an Esc");

    // The transcript alone is enough, and the explicit close names the Esc.
    let mut t = Sim::working();
    t.at(2_000, open("Bash", AskKind::Permission));
    t.at(10_000, close("Bash", AskOutcome::Escaped));
    t.expect(Status::Idle, "escaped");
}

#[test]
fn case_05_ask_user_question_carries_its_text_and_options_and_clears() {
    let mut s = Sim::working();
    s.at(2_000, open("AskUserQuestion", AskKind::Question));
    s.at(
        2_000,
        text("AskUserQuestion", "Which db?", &["pg", "sqlite"]),
    );
    s.expect(Status::NeedsYou, "question open");
    let a = s.m.ask("AskUserQuestion").expect("the ask is tracked");
    assert_eq!(a.text.as_deref(), Some("Which db?"));
    assert_eq!(a.options, vec!["pg", "sqlite"]);
    s.at(9_000, close("AskUserQuestion", AskOutcome::Answered));
    s.expect(Status::Working, "answered, the turn goes on");
    assert_eq!(s.alerts(), 1);

    let mut e = Sim::working();
    e.at(2_000, open("AskUserQuestion", AskKind::Question));
    e.at(9_000, close("AskUserQuestion", AskOutcome::Escaped));
    e.expect(Status::Idle, "Esc on the question");
}

#[test]
fn case_06_exit_plan_mode_carries_the_plan_and_both_outcomes_clear() {
    for outcome in [AskOutcome::Answered, AskOutcome::Declined] {
        let mut s = Sim::working();
        s.at(2_000, open("ExitPlanMode", AskKind::Plan));
        s.at(2_000, text("ExitPlanMode", "1. add the table", &[]));
        s.expect(Status::NeedsYou, "plan open");
        assert_eq!(s.view().ask, Some(AskKind::Plan));
        assert_eq!(
            s.m.ask("ExitPlanMode").unwrap().text.as_deref(),
            Some("1. add the table")
        );
        s.at(9_000, close("ExitPlanMode", outcome));
        assert_eq!(s.view().asks, 0, "{outcome:?}");
        assert_eq!(s.alerts(), 1);
    }
}

#[test]
fn case_07_the_trust_dialog_at_startup_needs_you() {
    let mut s = Sim::started();
    s.at(500, open("trust", AskKind::Trust));
    s.expect(Status::NeedsYou, "trust dialog");
    assert_eq!(s.view().ask, Some(AskKind::Trust));
    assert_eq!(s.alerts(), 1);
    s.at(5_000, close("trust", AskOutcome::Answered));
    assert_eq!(s.view().asks, 0);
    assert!(s.log.contains(&clear_ask("trust")));
}

#[test]
fn case_08_an_mcp_elicitation_needs_you_under_its_own_key() {
    let mut s = Sim::working();
    s.at(2_000, open("elicit:github", AskKind::Elicitation));
    s.expect(Status::NeedsYou, "elicitation");
    assert_eq!(s.view().ask, Some(AskKind::Elicitation));
    s.at(7_000, close("elicit:github", AskOutcome::Answered));
    assert_eq!(s.view().asks, 0);
    assert_eq!(s.alerts(), 1);
}

#[test]
fn case_09_two_asks_in_one_tab_and_in_two_tabs_are_tracked_by_key() {
    let mut s = Sim::working();
    s.at(2_000, open("Bash", AskKind::Permission));
    s.at(2_500, open("AskUserQuestion", AskKind::Question));
    assert_eq!(s.view().asks, 2, "both tracked");
    assert_eq!(s.alerts(), 2);
    s.at(5_000, close("Bash", AskOutcome::Answered));
    s.expect(Status::NeedsYou, "one is still open");
    assert_eq!(s.view().asks, 1);
    assert_eq!(s.view().ask, Some(AskKind::Question));

    // Two tabs: each machine holds its own ask and the roll-up stays needs_you until both clear.
    let mut a = Sim::working();
    let mut b = Sim::working();
    a.at(2_000, open("Bash", AskKind::Permission));
    b.at(2_000, open("Bash", AskKind::Permission));
    assert_eq!(rollup(&[a.status(), b.status()]), Status::NeedsYou);
    a.at(4_000, close("Bash", AskOutcome::Answered));
    assert_eq!(rollup(&[a.status(), b.status()]), Status::NeedsYou);
    b.at(5_000, close("Bash", AskOutcome::Answered));
    assert_eq!(rollup(&[a.status(), b.status()]), Status::Working);
}

#[test]
fn case_10_an_ask_answered_in_the_terminal_clears_from_any_source() {
    // Hook or transcript names the close.
    let mut h = Sim::working();
    h.at(2_000, open("Bash", AskKind::Permission));
    h.at(8_000, close("Bash", AskOutcome::Answered));
    assert_eq!(h.view().asks, 0);

    // The transcript sees the agent idle again.
    let mut t = Sim::working();
    t.at(2_000, open("Bash", AskKind::Permission));
    t.at(2_000 + LATER, transcript(Seen::Idle));
    t.expect(Status::Idle, "transcript closes it in one reading");

    // The screen alone needs two scans.
    let mut sc = Sim::working();
    sc.at(2_000, open("Bash", AskKind::Permission));
    sc.at(2_000 + LATER, screen(Seen::Idle));
    sc.at(2_000 + LATER + 1_000, screen(Seen::Idle));
    sc.expect(Status::Idle, "screen closes it on the second scan");
    for s in [&h, &t, &sc] {
        assert_eq!(s.alerts(), 1, "answering raises nothing");
    }
}

#[test]
fn case_11_a_notification_before_after_or_never_alerts_once_and_fills_in_place() {
    // After the request (the common order): the text fills in place, no second alert.
    let mut after = Sim::working();
    after.at(2_000, open("Bash", AskKind::Permission));
    let out = after.at(2_800, text(ANY_ASK, "Allow Bash?", &[]));
    assert_eq!(
        out,
        vec![Effect::FillAsk {
            key: "Bash".into(),
            text: "Allow Bash?".into(),
            options: vec![]
        }]
    );
    assert_eq!(after.alerts(), 1);

    // Never: the ask stands without words.
    let mut never = Sim::working();
    never.at(2_000, open("Bash", AskKind::Permission));
    never.expect(Status::NeedsYou, "still needs you");
    assert_eq!(never.m.ask("Bash").unwrap().text, None);
    assert_eq!(never.alerts(), 1);

    // Before: no ask is open, so the words are dropped and the request alerts once when it arrives.
    let mut before = Sim::working();
    assert!(before
        .at(1_500, text(ANY_ASK, "Allow Bash?", &[]))
        .is_empty());
    before.expect(Status::Working, "words alone are not an ask");
    before.at(2_000, open("Bash", AskKind::Permission));
    assert_eq!(before.alerts(), 1);
    assert_eq!(before.m.ask("Bash").unwrap().text, None);
}

#[test]
fn case_12_esc_during_a_running_turn_is_idle_not_failed() {
    let mut s = Sim::working();
    s.at(2_000, tool("Bash"));
    s.at(2_000 + LATER, screen(Seen::Idle));
    s.expect(Status::Working, "one idle scan is not enough");
    s.at(2_000 + LATER + 1_000, screen(Seen::Idle));
    s.expect(Status::Idle, "interrupted");
    assert!(s.log.is_empty(), "no failed, no finished: {:?}", s.log);
}

#[test]
fn case_13_a_recovered_error_keeps_working_and_does_not_alert() {
    let mut s = Sim::working();
    s.at(2_000, tool("NoSuchTool"));
    s.at(2_100, tool_end("NoSuchTool"));
    s.at(3_000, Event::Retry { active: true });
    assert_eq!(s.sub(), Some(SubReason::Retrying));
    s.at(9_000, Event::Retry { active: false });
    s.expect(Status::Working, "recovered");
    assert_eq!(s.sub(), Some(SubReason::Inferring));
    assert!(s.log.is_empty(), "no alert, no item: {:?}", s.log);
}

#[test]
fn case_14_a_halting_rate_limit_fails_with_its_cause_and_alerts() {
    let mut s = Sim::working();
    s.at(3_000, Event::Retry { active: true });
    s.at(
        90_000,
        Event::TurnEnd {
            outcome: Outcome::Failed(FailureCause::RateLimit),
            bg: 0,
        },
    );
    s.expect(Status::Failed, "gave up");
    assert_eq!(s.view().cause, Some(FailureCause::RateLimit));
    assert_eq!(s.log, vec![Effect::RaiseFailed(FailureCause::RateLimit)]);
}

#[test]
fn case_15_auth_and_context_failures_name_their_cause() {
    for cause in [FailureCause::Auth, FailureCause::Context] {
        let mut s = Sim::working();
        s.at(4_000, failed(cause));
        s.expect(Status::Failed, "failed");
        assert_eq!(s.view().cause, Some(cause));
        assert_eq!(s.alerts(), 1, "{cause:?}");
    }
}

#[test]
fn case_16_stuck_retrying_beyond_the_threshold_raises_a_silent_info_item() {
    let mut s = Sim::working();
    s.at(3_000, Event::Retry { active: true });
    assert!(s.tick(3_000 + RETRY_INFO_MS - 1).is_empty(), "not yet");
    let out = s.tick(3_000 + RETRY_INFO_MS);
    assert_eq!(out, vec![Effect::RaiseInfo(InfoKind::StuckRetrying)]);
    s.expect(Status::Working, "still working, only noted");
    assert_eq!(s.sub(), Some(SubReason::Retrying));
    assert!(s.tick(3_000 + 2 * RETRY_INFO_MS).is_empty(), "raised once");
    assert_eq!(s.alerts(), 0, "info is silent");
}

#[test]
fn case_17_compaction_mid_turn_is_working_and_not_a_new_session() {
    let mut s = Sim::working();
    s.at(2_000, Event::CompactStart);
    assert_eq!(s.sub(), Some(SubReason::Compacting));
    s.at(3_000, Event::SessionStart(SessionSource::Compact));
    s.expect(Status::Working, "compact restart keeps the turn");
    assert_eq!(s.sub(), Some(SubReason::Compacting));
    s.at(9_000, Event::CompactEnd);
    assert_eq!(s.sub(), Some(SubReason::Inferring));
    s.at(12_000, done(0));
    s.expect(Status::Finished, "the turn ended normally");
    assert_eq!(s.log, vec![Effect::RaiseFinished]);
}

#[test]
fn case_18_a_subagent_running_at_stop_finishes_with_bg_and_its_end_is_info() {
    let mut s = Sim::working();
    s.at(2_000, Event::SubagentStart);
    assert_eq!(s.sub(), Some(SubReason::Subagent));
    s.at(9_000, done(1));
    s.expect(Status::Finished, "the turn ended");
    assert_eq!(s.view().bg, 1);
    let out = s.at(30_000, Event::SubagentEnd);
    assert_eq!(out, vec![Effect::RaiseInfo(InfoKind::SubagentDone)]);
    assert_eq!(s.alerts(), 1, "only the finish alerts");
}

#[test]
fn case_19_a_background_task_completion_wakes_the_agent_for_a_second_turn() {
    let mut s = Sim::working();
    s.at(9_000, done(1));
    s.expect(Status::Finished, "first turn");
    assert_eq!(s.view().bg, 1);
    s.at(10_000, Event::Viewed);
    s.expect(Status::Idle, "viewed, background work lives");
    assert_eq!(s.view().bg, 1);
    // The completion opens a turn through UserPromptSubmit.
    s.at(60_000, turn_start());
    s.expect(Status::Working, "woken");
    s.at(70_000, done(0));
    s.expect(Status::Finished, "second turn");
    assert_eq!(s.view().bg, 0);
    assert_eq!(s.alerts(), 2, "one chime per finished turn");
}

#[test]
fn case_20_a_scheduled_wakeup_is_idle_with_bg_and_the_wake_opens_a_turn() {
    let mut s = Sim::working();
    s.at(9_000, done(1));
    s.at(10_000, Event::Viewed);
    s.expect(Status::Idle, "waiting on a schedule");
    assert_eq!(s.view().bg, 1);
    s.at(600_000, turn_start());
    s.expect(Status::Working, "the wake");
    assert_eq!(s.sub(), Some(SubReason::Inferring));
}

#[test]
fn case_21_kill_nine_fails_crashed_and_a_clean_exit_is_idle() {
    let mut k = Sim::working();
    k.at(2_000, tool("Bash"));
    k.at(9_000, Event::ProcessGone);
    k.expect(Status::Failed, "killed mid-turn");
    assert_eq!(k.view().cause, Some(FailureCause::Crashed));
    assert_eq!(k.log, vec![Effect::RaiseFailed(FailureCause::Crashed)]);

    let mut c = Sim::working();
    c.at(9_000, done(0));
    c.at(10_000, Event::Viewed);
    c.at(11_000, Event::SessionEnd);
    c.expect(Status::Idle, "clean exit");
    assert_eq!(c.view().cause, None);
    assert_eq!(c.alerts(), 1, "only the finish");
}

#[test]
fn case_22_a_resumed_session_restores_no_ask_and_starts_idle() {
    let mut s = Sim::new();
    let out = s.at(0, Event::SessionStart(SessionSource::Resume));
    assert!(out.is_empty());
    s.expect(Status::Idle, "resumed");
    assert_eq!(s.view().asks, 0);

    // A resume on a machine that held an ask closes it.
    let mut r = Sim::working();
    r.at(2_000, open("Bash", AskKind::Permission));
    r.at(9_000, Event::SessionStart(SessionSource::Resume));
    r.expect(Status::Idle, "no restored ask");
    assert!(r.log.contains(&clear_ask("Bash")));
}

#[test]
fn case_23_two_agent_tabs_roll_up_in_order_and_stay_independent() {
    let mut a = Sim::working();
    let mut b = Sim::started();
    assert_eq!(rollup(&[a.status(), b.status()]), Status::Idle);
    a.at(9_000, done(0));
    assert_eq!(a.status(), Status::Finished);
    assert_eq!(b.status(), Status::Idle, "b is untouched");
    assert_eq!(rollup(&[a.status(), b.status()]), Status::Finished);
    b.at(10_000, failed(FailureCause::Auth));
    // b had no open turn, so a late Stop failure changes nothing.
    assert_eq!(b.status(), Status::Idle);
    b.at(11_000, turn_start());
    b.at(12_000, failed(FailureCause::Auth));
    assert_eq!(rollup(&[a.status(), b.status()]), Status::Failed);
    a.at(13_000, open("Bash", AskKind::Permission));
    assert_eq!(rollup(&[a.status(), b.status()]), Status::NeedsYou);
}

#[test]
fn case_24_a_closed_tab_drops_its_items_and_the_rollup_is_recomputed() {
    // The pane watcher reports ProcessGone; the ask is cleared with the crash.
    let mut s = Sim::working();
    s.at(2_000, open("Bash", AskKind::Permission));
    s.at(9_000, Event::ProcessGone);
    assert_eq!(s.view().asks, 0);
    assert!(s.log.contains(&clear_ask("Bash")));
    // Forgetting the machine removes the tab from the roll-up.
    let finished = {
        let mut f = Sim::working();
        f.at(9_000, done(0));
        f.status()
    };
    assert_eq!(rollup(&[finished, Status::Working]), Status::Finished);
    assert_eq!(rollup(&[Status::Working]), Status::Working);
    assert_eq!(rollup(&[]), Status::Unknown);
}

#[test]
fn case_25_a_restart_rebuilds_a_pending_ask_from_the_transcript_or_clears_it() {
    // The new machine has no memory; the transcript tail replays the open ask with its words.
    let mut s = Sim::new();
    s.expect(Status::Unknown, "nothing known after a restart");
    s.at(100, tool("Read"));
    s.at(150, open("AskUserQuestion", AskKind::Question));
    s.at(150, text("AskUserQuestion", "Which db?", &["pg", "sqlite"]));
    s.expect(Status::NeedsYou, "rebuilt");
    assert_eq!(
        s.m.ask("AskUserQuestion").unwrap().text.as_deref(),
        Some("Which db?")
    );
    // It is never stuck: the answer in the terminal closes it from the transcript.
    s.at(150 + LATER, transcript(Seen::Idle));
    s.expect(Status::Idle, "cleared by a later reading");
}

#[test]
fn case_26_a_snapshot_reproduces_the_same_status_and_content() {
    let mut s = Sim::working();
    s.at(2_000, open("AskUserQuestion", AskKind::Question));
    s.at(2_100, text("AskUserQuestion", "Which db?", &["pg"]));
    let before = s.view();
    let snapshot = s.m.clone();
    // Reading is pure: a view at the same time is the same however often a device asks.
    assert_eq!(s.m.view(s.now), before);
    assert_eq!(snapshot.view(s.now), before);
    assert_eq!(
        snapshot.ask("AskUserQuestion"),
        s.m.ask("AskUserQuestion"),
        "the content is part of the snapshot"
    );
    assert!(s.tick(s.now + 1).is_empty(), "a tick invents nothing");
}

#[test]
fn case_27_late_duplicate_and_out_of_order_events_do_not_flap() {
    let mut s = Sim::working();
    s.at(2_000, tool("Bash"));
    s.at(2_500, tool_end("Bash"));
    s.at(3_000, done(0));
    // A tool event that arrives after the Stop is late and opens nothing.
    s.at(3_200, tool("Bash"));
    s.expect(Status::Finished, "late ToolStart");
    s.at(3_300, tool_end("Bash"));
    // A duplicate Stop raises nothing.
    s.at(3_400, done(0));
    s.expect(Status::Finished, "duplicate Stop");
    assert_eq!(s.log, vec![Effect::RaiseFinished]);

    let mut a = Sim::working();
    a.at(2_000, open("Bash", AskKind::Permission));
    a.at(2_000, open("Bash", AskKind::Permission));
    assert_eq!(a.alerts(), 1, "a duplicate request alerts once");
    a.at(3_000, tool_end("Bash"));
    a.expect(Status::NeedsYou, "a stray ToolEnd closes no ask");
}

#[test]
fn case_28_without_hooks_the_transcript_and_screen_keep_the_status_right() {
    // No hook ever arrived: the first evidence is a transcript tool call.
    let mut s = Sim::new();
    s.expect(Status::Unknown, "no evidence");
    s.at(1_000, tool("Bash"));
    s.expect(Status::Working, "the transcript saw a tool");
    s.at(1_500, tool_end("Bash"));
    s.at(1_000 + LATER, transcript(Seen::Idle));
    s.expect(Status::Idle, "the transcript says the turn ended");
    assert_eq!(s.alerts(), 0, "no hook, no finished claim");

    // Hooks gone, screens only, from a session that was seen once.
    let mut w = Sim::started();
    w.at(LATER, screen(Seen::Working));
    w.expect(Status::Idle, "one scan proves nothing");
    w.at(LATER + 1_000, screen(Seen::Working));
    w.expect(Status::Working, "two scans agree");
}

#[test]
fn case_29_an_agent_without_an_adapter_is_unknown_when_nothing_is_proven() {
    let mut s = Sim::new();
    for t in [LATER, LATER + 1_000, LATER + 2_000] {
        s.at(
            t,
            Event::Observe {
                source: Source::Scan,
                seen: Seen::Working,
            },
        );
    }
    s.expect(Status::Unknown, "a scan alone does not prove a session");
    assert!(s.log.is_empty());
    s.at(LATER + 3_000, screen(Seen::Blocked));
    assert!(s.log.is_empty(), "a hint is not an alert");
}

#[test]
fn case_30_an_unseen_result_survives_a_new_turn_until_it_is_viewed() {
    let mut s = Sim::working();
    s.at(9_000, done(0));
    s.expect(Status::Finished, "unseen");
    let out = s.at(10_000, turn_start());
    assert!(
        !out.contains(&Effect::Clear(ItemId::Result)),
        "a new turn does not clear the result"
    );
    s.expect(Status::Working, "the new turn shows");
    s.at(11_000, done(0));
    s.expect(Status::Finished, "still unseen");
    let out = s.at(12_000, Event::Viewed);
    assert_eq!(out, vec![Effect::Clear(ItemId::Result)]);
    s.expect(Status::Idle, "viewed");
}

#[test]
fn case_31_a_view_on_one_device_is_final_for_a_device_that_comes_online_later() {
    let mut s = Sim::working();
    s.at(9_000, done(0));
    s.at(10_000, Event::Viewed);
    s.expect(Status::Idle, "cleared on device A");
    // Device B reads the same machine after it reconnects; its own view changes nothing and raises nothing.
    assert_eq!(s.m.view(500_000).status, Status::Idle);
    assert!(s.at(500_000, Event::Viewed).is_empty(), "idempotent");
    s.expect(Status::Idle, "B shows it cleared");
}

#[test]
fn case_32_an_ask_answered_on_one_device_clears_once_and_alerts_once() {
    let mut s = Sim::working();
    s.at(2_000, open("Bash", AskKind::Permission));
    let first = s.at(5_000, close("Bash", AskOutcome::Answered));
    assert_eq!(first, vec![clear_ask("Bash")]);
    // The second device's answer (or the hook echoing it) finds no ask.
    assert!(s.at(5_200, close("Bash", AskOutcome::Answered)).is_empty());
    assert_eq!(s.alerts(), 1);
}

#[test]
fn case_33_a_split_tab_is_one_stream_so_a_second_pane_cannot_flap_it() {
    // `agentwatch::tests::a_split_tab_is_driven_by_its_first_agent_pane` pins which pane drives; the machine sees
    // one stream, so a long run of agreeing readings changes nothing.
    let mut s = Sim::working();
    s.at(2_000, tool("Bash"));
    for i in 0..10 {
        s.at(LATER + i * 1_000, screen(Seen::Working));
    }
    s.expect(Status::Working, "steady");
    assert_eq!(s.sub(), Some(SubReason::Tool));
    assert!(s.log.is_empty());
}

#[test]
fn case_34_a_screen_that_looks_idle_does_not_overwrite_a_finished_turn() {
    let mut s = Sim::working();
    s.at(9_000, done(0));
    for i in 0..5 {
        s.at(LATER + i * 1_000, screen(Seen::Idle));
        s.at(LATER + i * 1_000 + 500, transcript(Seen::Idle));
    }
    s.expect(Status::Finished, "still finished");
    assert_eq!(s.log, vec![Effect::RaiseFinished]);
}

#[test]
fn case_35_a_screen_only_blocked_state_is_a_hint_with_no_alert_and_no_item() {
    let mut s = Sim::started();
    s.at(LATER, screen(Seen::Blocked));
    s.at(LATER + 1_000, screen(Seen::Blocked));
    s.expect(Status::Idle, "no ask backs it");
    assert!(s.view().hint_blocked);
    assert_eq!(s.view().asks, 0);
    assert!(s.log.is_empty(), "no item, no alert: {:?}", s.log);
    // A later hook-backed event drops the hint.
    s.at(LATER + 2_000, turn_start());
    assert!(!s.view().hint_blocked);
}

/// Row number and the name of its test; the list must stay 1..=35 in order.
const CASES: [(u8, &str); 35] = [
    (
        1,
        "case_01_a_plain_turn_is_working_then_finished_with_one_chime",
    ),
    (
        2,
        "case_02_a_long_silent_tool_stays_working_and_never_decays",
    ),
    (3, "case_03_a_permission_allowed_or_denied"),
    (
        4,
        "case_04_a_permission_then_esc_clears_within_the_reconcile_window",
    ),
    (
        5,
        "case_05_ask_user_question_carries_its_text_and_options_and_clears",
    ),
    (
        6,
        "case_06_exit_plan_mode_carries_the_plan_and_both_outcomes_clear",
    ),
    (7, "case_07_the_trust_dialog_at_startup_needs_you"),
    (8, "case_08_an_mcp_elicitation_needs_you_under_its_own_key"),
    (
        9,
        "case_09_two_asks_in_one_tab_and_in_two_tabs_are_tracked_by_key",
    ),
    (
        10,
        "case_10_an_ask_answered_in_the_terminal_clears_from_any_source",
    ),
    (
        11,
        "case_11_a_notification_before_after_or_never_alerts_once_and_fills_in_place",
    ),
    (12, "case_12_esc_during_a_running_turn_is_idle_not_failed"),
    (
        13,
        "case_13_a_recovered_error_keeps_working_and_does_not_alert",
    ),
    (
        14,
        "case_14_a_halting_rate_limit_fails_with_its_cause_and_alerts",
    ),
    (15, "case_15_auth_and_context_failures_name_their_cause"),
    (
        16,
        "case_16_stuck_retrying_beyond_the_threshold_raises_a_silent_info_item",
    ),
    (
        17,
        "case_17_compaction_mid_turn_is_working_and_not_a_new_session",
    ),
    (
        18,
        "case_18_a_subagent_running_at_stop_finishes_with_bg_and_its_end_is_info",
    ),
    (
        19,
        "case_19_a_background_task_completion_wakes_the_agent_for_a_second_turn",
    ),
    (
        20,
        "case_20_a_scheduled_wakeup_is_idle_with_bg_and_the_wake_opens_a_turn",
    ),
    (
        21,
        "case_21_kill_nine_fails_crashed_and_a_clean_exit_is_idle",
    ),
    (
        22,
        "case_22_a_resumed_session_restores_no_ask_and_starts_idle",
    ),
    (
        23,
        "case_23_two_agent_tabs_roll_up_in_order_and_stay_independent",
    ),
    (
        24,
        "case_24_a_closed_tab_drops_its_items_and_the_rollup_is_recomputed",
    ),
    (
        25,
        "case_25_a_restart_rebuilds_a_pending_ask_from_the_transcript_or_clears_it",
    ),
    (
        26,
        "case_26_a_snapshot_reproduces_the_same_status_and_content",
    ),
    (
        27,
        "case_27_late_duplicate_and_out_of_order_events_do_not_flap",
    ),
    (
        28,
        "case_28_without_hooks_the_transcript_and_screen_keep_the_status_right",
    ),
    (
        29,
        "case_29_an_agent_without_an_adapter_is_unknown_when_nothing_is_proven",
    ),
    (
        30,
        "case_30_an_unseen_result_survives_a_new_turn_until_it_is_viewed",
    ),
    (
        31,
        "case_31_a_view_on_one_device_is_final_for_a_device_that_comes_online_later",
    ),
    (
        32,
        "case_32_an_ask_answered_on_one_device_clears_once_and_alerts_once",
    ),
    (
        33,
        "case_33_a_split_tab_is_one_stream_so_a_second_pane_cannot_flap_it",
    ),
    (
        34,
        "case_34_a_screen_that_looks_idle_does_not_overwrite_a_finished_turn",
    ),
    (
        35,
        "case_35_a_screen_only_blocked_state_is_a_hint_with_no_alert_and_no_item",
    ),
];

#[test]
fn all_35_rows_have_a_test() {
    let source = include_str!("catalogue.rs");
    for (i, (n, name)) in CASES.iter().enumerate() {
        assert_eq!(*n as usize, i + 1, "CASES must run 1..=35 in order");
        assert!(
            name.starts_with(&format!("case_{n:02}_")),
            "row {n}: {name} does not carry its number"
        );
        assert!(
            source.contains(&format!("fn {name}()")),
            "row {n} has no test named {name}"
        );
    }
    let needle = ["fn ", "case_"].concat();
    let tests = source.matches(needle.as_str()).count();
    assert_eq!(tests, CASES.len(), "a case test is missing from CASES");
}
