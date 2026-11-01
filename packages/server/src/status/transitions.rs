//! The transition table: one row per transition of `docs/plans/agent-status-model.md`, replayed through the machine
//! with a clock. `every_status_reason_cause_and_event_has_a_row` fails when a new variant has no row.
use std::collections::HashSet;

use termenbox_core::events::{AskKind, FailureCause, SubReason};

use super::event::*;
use super::machine::*;

type Script = Vec<(u64, Event)>;

struct Row {
    name: &'static str,
    script: Script,
    /// A `tick` after the script, and the time the view is read at (default: the last event).
    tick_at: Option<u64>,
    view_at: Option<u64>,
    status: Status,
    sub: Option<SubReason>,
    tool: Option<&'static str>,
    cause: Option<FailureCause>,
    bg: u32,
    asks: usize,
    hint_blocked: bool,
    /// Every effect, in order.
    effects: Vec<Effect>,
}

fn row(name: &'static str, script: Script, status: Status, effects: Vec<Effect>) -> Row {
    Row {
        name,
        script,
        tick_at: None,
        view_at: None,
        status,
        sub: None,
        tool: None,
        cause: None,
        bg: 0,
        asks: 0,
        hint_blocked: false,
        effects,
    }
}

fn start() -> (u64, Event) {
    (0, Event::SessionStart(SessionSource::Startup))
}
fn turn(at: u64) -> (u64, Event) {
    (
        at,
        Event::TurnStart {
            title: Some("fix it".into()),
        },
    )
}
fn end(at: u64, outcome: Outcome, bg: u32) -> (u64, Event) {
    (at, Event::TurnEnd { outcome, bg })
}
fn tool(at: u64, n: &str) -> (u64, Event) {
    (at, Event::ToolStart { name: n.into() })
}
fn tool_end(at: u64, n: &str) -> (u64, Event) {
    (at, Event::ToolEnd { name: n.into() })
}
fn ask(at: u64, key: &str, kind: AskKind) -> (u64, Event) {
    (
        at,
        Event::AskOpen {
            key: key.into(),
            kind,
        },
    )
}
fn content(at: u64, key: &str, text: &str) -> (u64, Event) {
    (
        at,
        Event::AskContent {
            key: key.into(),
            text: text.into(),
            options: vec!["a".into(), "b".into()],
        },
    )
}
fn close(at: u64, key: &str, outcome: AskOutcome) -> (u64, Event) {
    (
        at,
        Event::AskClose {
            key: key.into(),
            outcome,
        },
    )
}
fn observe(at: u64, source: Source, seen: Seen) -> (u64, Event) {
    (at, Event::Observe { source, seen })
}
fn ev(at: u64, e: Event) -> (u64, Event) {
    (at, e)
}
fn raise_ask(key: &str, kind: AskKind) -> Effect {
    Effect::RaiseAsk {
        key: key.into(),
        kind,
    }
}
fn clear_ask(key: &str) -> Effect {
    Effect::Clear(ItemId::Ask(key.into()))
}
fn fill(key: &str, text: &str) -> Effect {
    Effect::FillAsk {
        key: key.into(),
        text: text.into(),
        options: vec!["a".into(), "b".into()],
    }
}

#[allow(clippy::vec_init_then_push)]
fn rows() -> Vec<Row> {
    use AskKind::*;
    let mut r = Vec::new();

    r.push(row(
        "a fresh machine has no evidence",
        vec![],
        Status::Unknown,
        vec![],
    ));
    r.push(row(
        "a session start is an agent that is present and idle",
        vec![start()],
        Status::Idle,
        vec![],
    ));
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a turn start is working",
            vec![start(), turn(1_000)],
            Status::Working,
            vec![],
        )
    });
    r.push(row(
        "a turn end is finished and raises one result",
        vec![start(), turn(1_000), end(5_000, Outcome::Done, 0)],
        Status::Finished,
        vec![Effect::RaiseFinished],
    ));
    r.push(row(
        "a duplicate Stop raises nothing more",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Done, 0),
            end(5_100, Outcome::Done, 0),
        ],
        Status::Finished,
        vec![Effect::RaiseFinished],
    ));
    r.push(row(
        "a tool start a moment after the turn ended is late, not a new turn",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Done, 0),
            tool(5_100, "Bash"),
            tool_end(5_200, "Bash"),
        ],
        Status::Finished,
        vec![Effect::RaiseFinished],
    ));
    r.push(Row {
        sub: Some(SubReason::Tool),
        tool: Some("Bash"),
        view_at: Some(20 * 60_000),
        ..row(
            "a long silent tool stays working(tool) and never goes quiet",
            vec![start(), turn(1_000), tool(2_000, "Bash")],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "the tool end returns to inferring",
            vec![
                start(),
                turn(1_000),
                tool(2_000, "Bash"),
                tool_end(3_000, "Bash"),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Quiet),
        view_at: Some(1_000 + QUIET_MS),
        ..row(
            "no progress proof for the quiet window is working(quiet)",
            vec![start(), turn(1_000)],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        view_at: Some(1_000 + QUIET_MS),
        ..row(
            "a progress proof keeps it from going quiet",
            vec![
                start(),
                turn(1_000),
                ev(30_000, Event::Progress),
                ev(59_000, Event::Progress),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Compacting),
        ..row(
            "compaction is working(compacting)",
            vec![start(), turn(1_000), ev(2_000, Event::CompactStart)],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a compact session start is not a new session and compaction end returns",
            vec![
                start(),
                turn(1_000),
                ev(2_000, Event::CompactStart),
                ev(3_000, Event::SessionStart(SessionSource::Compact)),
                ev(4_000, Event::CompactEnd),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Subagent),
        ..row(
            "a running subagent is working(subagent)",
            vec![start(), turn(1_000), ev(2_000, Event::SubagentStart)],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Retrying),
        ..row(
            "a retry screen is working(retrying)",
            vec![
                start(),
                turn(1_000),
                ev(2_000, Event::Retry { active: true }),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a retry that ends is inferring again",
            vec![
                start(),
                turn(1_000),
                ev(2_000, Event::Retry { active: true }),
                ev(3_000, Event::Retry { active: false }),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        tick_at: Some(2_000 + RETRY_INFO_MS),
        sub: Some(SubReason::Retrying),
        ..row(
            "retrying past the threshold raises a silent info item once",
            vec![
                start(),
                turn(1_000),
                ev(2_000, Event::Retry { active: true }),
            ],
            Status::Working,
            vec![Effect::RaiseInfo(InfoKind::StuckRetrying)],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Retrying),
        ..row(
            "retrying under the threshold raises nothing",
            vec![
                start(),
                turn(1_000),
                ev(2_000, Event::Retry { active: true }),
            ],
            Status::Working,
            vec![],
        )
    });

    // Asks.
    r.push(Row {
        asks: 1,
        ..row(
            "a permission request is needs_you at the first evidence",
            vec![start(), turn(1_000), ask(2_000, "p1", Permission)],
            Status::NeedsYou,
            vec![raise_ask("p1", Permission)],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "a duplicate request raises one ask",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                ask(2_100, "p1", Permission),
            ],
            Status::NeedsYou,
            vec![raise_ask("p1", Permission)],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "content fills the open ask in place",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "q1", Question),
                content(3_000, "q1", "which?"),
            ],
            Status::NeedsYou,
            vec![raise_ask("q1", Question), fill("q1", "which?")],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "the same content twice fills once",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "q1", Question),
                content(3_000, "q1", "which?"),
                content(3_100, "q1", "which?"),
            ],
            Status::NeedsYou,
            vec![raise_ask("q1", Question), fill("q1", "which?")],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "content for another key does not attach to the open ask",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "q1", Question),
                content(3_000, "other", "x"),
            ],
            Status::NeedsYou,
            vec![raise_ask("q1", Question)],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "late content for a closed ask is dropped",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "q1", Question),
                close(3_000, "q1", AskOutcome::Answered),
                content(9_000, "q1", "late"),
            ],
            Status::Working,
            vec![raise_ask("q1", Question), clear_ask("q1")],
        )
    });
    r.push(Row {
        asks: 2,
        ..row(
            "two asks in a tab are both tracked",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                ask(2_100, "p2", Plan),
            ],
            Status::NeedsYou,
            vec![raise_ask("p1", Permission), raise_ask("p2", Plan)],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "closing one of two asks keeps needs_you",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                ask(2_100, "p2", Trust),
                close(3_000, "p1", AskOutcome::Answered),
            ],
            Status::NeedsYou,
            vec![
                raise_ask("p1", Permission),
                raise_ask("p2", Trust),
                clear_ask("p1"),
            ],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "an elicitation is an ask kind",
            vec![start(), turn(1_000), ask(2_000, "e1", Elicitation)],
            Status::NeedsYou,
            vec![raise_ask("e1", Elicitation)],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "an answered ask returns to working",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                close(3_000, "p1", AskOutcome::Answered),
            ],
            Status::Working,
            vec![raise_ask("p1", Permission), clear_ask("p1")],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a declined ask returns to working",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                close(3_000, "p1", AskOutcome::Declined),
            ],
            Status::Working,
            vec![raise_ask("p1", Permission), clear_ask("p1")],
        )
    });
    r.push(row(
        "an ask closed by Esc is idle, not failed",
        vec![
            start(),
            turn(1_000),
            ask(2_000, "p1", Permission),
            close(3_000, "p1", AskOutcome::Escaped),
        ],
        Status::Idle,
        vec![raise_ask("p1", Permission), clear_ask("p1")],
    ));
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a new prompt ends whatever was asked",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                turn(3_000),
            ],
            Status::Working,
            vec![raise_ask("p1", Permission), clear_ask("p1")],
        )
    });
    r.push(Row {
        asks: 1,
        ..row(
            "a viewed event never clears an ask",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                ev(3_000, Event::Viewed),
            ],
            Status::NeedsYou,
            vec![raise_ask("p1", Permission)],
        )
    });
    r.push(row(
        "an unknown key or tool end is harmless",
        vec![
            start(),
            close(1_000, "nope", AskOutcome::Answered),
            tool_end(1_100, "Bash"),
        ],
        Status::Idle,
        vec![],
    ));

    // Endings.
    r.push(row(
        "Esc on a running turn is idle, no failed",
        vec![start(), turn(1_000), end(5_000, Outcome::Interrupted, 0)],
        Status::Idle,
        vec![],
    ));
    for (cause, name) in [
        (FailureCause::RateLimit, "a halting rate limit"),
        (FailureCause::Auth, "an auth failure"),
        (FailureCause::ApiError, "an api error"),
        (FailureCause::Context, "a context overflow"),
    ] {
        r.push(Row {
            cause: Some(cause),
            ..row(
                name,
                vec![start(), turn(1_000), end(5_000, Outcome::Failed(cause), 0)],
                Status::Failed,
                vec![Effect::RaiseFailed(cause)],
            )
        });
    }
    r.push(Row {
        cause: Some(FailureCause::Crashed),
        ..row(
            "a process gone mid-turn is failed(crashed)",
            vec![start(), turn(1_000), ev(2_000, Event::ProcessGone)],
            Status::Failed,
            vec![Effect::RaiseFailed(FailureCause::Crashed)],
        )
    });
    r.push(Row {
        cause: Some(FailureCause::Crashed),
        ..row(
            "a process gone with an ask open is failed(crashed) and clears the ask",
            vec![
                start(),
                turn(1_000),
                ask(2_000, "p1", Permission),
                ev(3_000, Event::ProcessGone),
            ],
            Status::Failed,
            vec![
                raise_ask("p1", Permission),
                clear_ask("p1"),
                Effect::RaiseFailed(FailureCause::Crashed),
            ],
        )
    });
    r.push(row(
        "a process gone with no turn is unknown",
        vec![start(), ev(1_000, Event::ProcessGone)],
        Status::Unknown,
        vec![],
    ));
    r.push(row(
        "a clean session end closes the turn as idle",
        vec![start(), turn(1_000), ev(2_000, Event::SessionEnd)],
        Status::Idle,
        vec![],
    ));
    r.push(row(
        "a resume starts idle with no restored ask",
        vec![
            start(),
            turn(1_000),
            ask(2_000, "p1", Permission),
            ev(3_000, Event::SessionStart(SessionSource::Resume)),
        ],
        Status::Idle,
        vec![raise_ask("p1", Permission), clear_ask("p1")],
    ));

    // Unseen results.
    r.push(row(
        "viewing a finished result clears it and the tab is idle",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Done, 0),
            ev(6_000, Event::Viewed),
        ],
        Status::Idle,
        vec![Effect::RaiseFinished, Effect::Clear(ItemId::Result)],
    ));
    r.push(row(
        "dismissing a failed result clears it",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Failed(FailureCause::Auth), 0),
            ev(6_000, Event::Dismiss),
        ],
        Status::Idle,
        vec![
            Effect::RaiseFailed(FailureCause::Auth),
            Effect::Clear(ItemId::Result),
        ],
    ));
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "an unseen result stays through a new turn",
            vec![
                start(),
                turn(1_000),
                end(5_000, Outcome::Done, 0),
                turn(9_000),
            ],
            Status::Working,
            vec![Effect::RaiseFinished],
        )
    });
    r.push(row(
        "nothing to view is nothing to clear",
        vec![start(), ev(1_000, Event::Viewed)],
        Status::Idle,
        vec![],
    ));

    // Background work and info.
    r.push(Row {
        bg: 2,
        ..row(
            "a finished turn with background work carries the badge",
            vec![start(), turn(1_000), end(5_000, Outcome::Done, 2)],
            Status::Finished,
            vec![Effect::RaiseFinished],
        )
    });
    r.push(Row {
        bg: 1,
        ..row(
            "a background completion after the turn is silent info",
            vec![
                start(),
                turn(1_000),
                end(5_000, Outcome::Done, 2),
                ev(20_000, Event::Background { count: 1 }),
            ],
            Status::Finished,
            vec![
                Effect::RaiseFinished,
                Effect::RaiseInfo(InfoKind::BackgroundDone),
            ],
        )
    });
    r.push(row(
        "a subagent stop after the turn is silent info",
        vec![
            start(),
            turn(1_000),
            ev(2_000, Event::SubagentStart),
            end(5_000, Outcome::Done, 0),
            ev(9_000, Event::SubagentEnd),
        ],
        Status::Finished,
        vec![
            Effect::RaiseFinished,
            Effect::RaiseInfo(InfoKind::SubagentDone),
        ],
    ));
    r.push(row(
        "viewing clears the info items too",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Done, 1),
            ev(20_000, Event::Background { count: 0 }),
            ev(21_000, Event::Viewed),
        ],
        Status::Idle,
        vec![
            Effect::RaiseFinished,
            Effect::RaiseInfo(InfoKind::BackgroundDone),
            Effect::Clear(ItemId::Result),
            Effect::Clear(ItemId::Info(InfoKind::BackgroundDone)),
        ],
    ));
    r.push(Row {
        bg: 1,
        sub: Some(SubReason::Inferring),
        ..row(
            "a wake after a finish opens a new turn",
            vec![
                start(),
                turn(1_000),
                end(5_000, Outcome::Done, 1),
                turn(30_000),
            ],
            Status::Working,
            vec![Effect::RaiseFinished],
        )
    });

    // Observations from the transcript, the screen and the scan.
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "one idle screen does not close a turn",
            vec![
                start(),
                turn(1_000),
                observe(10_000, Source::Screen, Seen::Idle),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(row(
        "two idle screens close the turn as interrupted",
        vec![
            start(),
            turn(1_000),
            observe(10_000, Source::Screen, Seen::Idle),
            observe(10_500, Source::Screen, Seen::Idle),
        ],
        Status::Idle,
        vec![],
    ));
    r.push(row(
        "one idle transcript reading closes the turn",
        vec![
            start(),
            turn(1_000),
            observe(10_000, Source::Transcript, Seen::Idle),
        ],
        Status::Idle,
        vec![],
    ));
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "an agreeing reading resets the disagreement count",
            vec![
                start(),
                turn(1_000),
                observe(10_000, Source::Screen, Seen::Idle),
                observe(10_500, Source::Screen, Seen::Working),
                observe(11_000, Source::Screen, Seen::Idle),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        ..row(
            "a reading inside the grace never contradicts an event",
            vec![
                start(),
                turn(1_000),
                observe(1_500, Source::Transcript, Seen::Idle),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(row(
        "an idle screen after a hook finish keeps it finished",
        vec![
            start(),
            turn(1_000),
            end(5_000, Outcome::Done, 0),
            observe(20_000, Source::Screen, Seen::Idle),
            observe(21_000, Source::Screen, Seen::Idle),
        ],
        Status::Finished,
        vec![Effect::RaiseFinished],
    ));
    r.push(row(
        "an ask the screen no longer shows closes after two scans",
        vec![
            start(),
            turn(1_000),
            ask(2_000, "p1", Permission),
            observe(10_000, Source::Screen, Seen::Idle),
            observe(10_500, Source::Screen, Seen::Idle),
        ],
        Status::Idle,
        vec![raise_ask("p1", Permission), clear_ask("p1")],
    ));
    r.push(Row {
        sub: Some(SubReason::Inferring),
        hint_blocked: true,
        ..row(
            "a blocked screen with no ask is a hint only",
            vec![
                start(),
                turn(1_000),
                observe(10_000, Source::Screen, Seen::Blocked),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(Row {
        sub: Some(SubReason::Inferring),
        view_at: Some(12_000),
        ..row(
            "two working screens reopen a turn whose start hook was missed",
            vec![
                start(),
                observe(10_000, Source::Screen, Seen::Working),
                observe(10_500, Source::Scan, Seen::Working),
            ],
            Status::Working,
            vec![],
        )
    });
    r.push(row(
        "a working screen with no agent present is ignored",
        vec![
            observe(10_000, Source::Screen, Seen::Working),
            observe(10_500, Source::Screen, Seen::Working),
        ],
        Status::Unknown,
        vec![],
    ));
    r
}

fn replay(r: &Row) -> (Machine, Vec<Effect>, u64) {
    let mut m = Machine::new();
    let mut effects = Vec::new();
    let mut now = 0;
    for (at, e) in r.script.clone() {
        now = at;
        effects.extend(m.apply(e, at));
    }
    if let Some(t) = r.tick_at {
        effects.extend(m.tick(t));
        effects.extend(m.tick(t + 1_000));
        now = t;
    }
    (m, effects, r.view_at.unwrap_or(now))
}

#[test]
fn every_transition_row_holds() {
    for r in rows() {
        let (m, effects, at) = replay(&r);
        let v = m.view(at);
        assert_eq!(v.status, r.status, "{}: status", r.name);
        assert_eq!(v.sub_reason, r.sub, "{}: sub-reason", r.name);
        assert_eq!(v.tool.as_deref(), r.tool, "{}: tool", r.name);
        assert_eq!(v.cause, r.cause, "{}: cause", r.name);
        assert_eq!(v.bg, r.bg, "{}: bg", r.name);
        assert_eq!(v.asks, r.asks, "{}: asks", r.name);
        assert_eq!(v.hint_blocked, r.hint_blocked, "{}: hint", r.name);
        assert_eq!(effects, r.effects, "{}: effects", r.name);
    }
}

#[test]
fn every_status_reason_cause_and_event_has_a_row() {
    let mut statuses = HashSet::new();
    let mut subs = Vec::new();
    let mut causes = Vec::new();
    let mut kinds = HashSet::new();
    for r in rows() {
        statuses.insert(r.status);
        subs.extend(r.sub);
        causes.extend(r.cause);
        kinds.extend(r.script.iter().map(|(_, e)| e.kind()));
    }
    for s in Status::ALL {
        assert!(statuses.contains(&s), "no row ends in status {s:?}");
    }
    for s in [
        SubReason::Inferring,
        SubReason::Tool,
        SubReason::Subagent,
        SubReason::Compacting,
        SubReason::Retrying,
        SubReason::Quiet,
    ] {
        assert!(subs.contains(&s), "no row shows sub-reason {s:?}");
    }
    for c in [
        FailureCause::RateLimit,
        FailureCause::Auth,
        FailureCause::ApiError,
        FailureCause::Context,
        FailureCause::Crashed,
    ] {
        assert!(causes.contains(&c), "no row shows failure cause {c:?}");
    }
    for k in Kind::ALL {
        assert!(kinds.contains(&k), "no row feeds event {k:?}");
    }
    for k in [
        AskKind::Permission,
        AskKind::Question,
        AskKind::Plan,
        AskKind::Trust,
        AskKind::Elicitation,
    ] {
        let used = rows().iter().any(|r| {
            r.effects
                .iter()
                .any(|e| matches!(e, Effect::RaiseAsk { kind, .. } if *kind == k))
        });
        assert!(used, "no row raises an ask of kind {k:?}");
    }
}

#[test]
fn the_rollup_is_worst_first_and_ignores_unknown() {
    use Status::*;
    assert_eq!(rollup(&[]), Unknown);
    assert_eq!(rollup(&[Unknown, Unknown]), Unknown);
    assert_eq!(rollup(&[Working, Idle]), Idle);
    assert_eq!(rollup(&[Idle, Finished, Working]), Finished);
    assert_eq!(rollup(&[Finished, Failed, Idle]), Failed);
    assert_eq!(rollup(&[Failed, NeedsYou, Finished, Unknown]), NeedsYou);
    assert_eq!(rollup(&[Unknown, Working]), Working);
}

#[test]
fn a_replayed_script_is_deterministic_and_a_duplicate_delivery_does_not_flap() {
    let script = vec![
        start(),
        turn(1_000),
        tool(2_000, "Bash"),
        tool_end(2_500, "Bash"),
        end(5_000, Outcome::Done, 0),
    ];
    let run = |dup: bool| {
        let mut m = Machine::new();
        let mut out = Vec::new();
        for (at, e) in script.clone() {
            out.extend(m.apply(e.clone(), at));
            if dup {
                out.extend(m.apply(e, at + 1));
            }
        }
        (m.status(), out)
    };
    assert_eq!(run(false), run(false));
    assert_eq!(run(true), run(false));
}
