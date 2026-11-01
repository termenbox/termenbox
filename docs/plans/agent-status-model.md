# Agent status model: one machine, one output, proven end to end

Decisions: 0187 (one machine per session, workspace roll-up), 0188 (working, background badge), 0189 (evidence ranks),
0190 (asks), 0191 (alerts), 0192 (failures), 0193 (info tier, viewed by a human). Amends 0095. Fact 0074 is the as-is
pipeline. Guardrails: `no-alert-without-a-visible-status`, `status-changes-ship-with-a-model-transition-test`.
Procedure: `add-a-status-transition`. Branch: `fix/agent-status` (pool-01), PRs target `develop`.

**Never run this plan from the main checkout: work in a pool worktree (`sh scripts/pool.sh acquire <branch>`).** No
compiler on the machine: prefix any command with `scripts/dev.sh` (add `--full` for the browser suites).

## The problem in one paragraph

Workspace and agent status is wrong in practice: a Claude session shows working when it is not (an Esc sends no hook;
the reducer pins a turn for 30 min) or does not show working when it is (the 60 s Activity decay); a chime sounds with
no status change (the chime reads workspace Attention, the chip reads per-tab or Activity state); asks arrive without
their question (the text rides on the late `Notification` hook, matched to any open ask in the workspace, and
`AskUserQuestion` input is never read). Three parallel state paths cause it (fact 0074).

## The model in brief (the decisions hold the detail)

| Status | Meaning | Sub-reason | Carries |
|---|---|---|---|
| working | anything in progress except waiting on the human | inferring, tool(name), subagent, compacting, retrying, quiet | task title, running time |
| needs_you | waits on the human | permission, question, plan, trust, elicitation | question and options, filled in later |
| finished | turn ended well, unseen | none | title, duration, outcome |
| failed | turn ended badly, unseen | rate_limit, auth, api_error, context, crashed | title, error class |
| idle | agent present, nothing running or unseen | none | `bg: N` badge |
| unknown | no agent or evidence | none | nothing is drawn |

- Workspace roll-up, worst first: `needs_you > failed > finished > idle > working`; `unknown` ignored.
- Tiers of Attention: `needs_you` and `result` (finished, failed) alert; `info` is silent. Results and info clear only
  when a human views the tab on a visible page of any device (`ack`) or dismisses them; never by a timer.
- Failure class is decided by whether the turn ended: recovered stays working, halting and crashed are failed, Esc is idle.
- Evidence ranks hook > transcript > screen > scan; every source emits typed events, never a status.

### Transitions

| From | Event | To | Condition |
|---|---|---|---|
| idle, finished, failed | turn start | working | an unseen item stays until viewed |
| working | ask open | needs_you | raised at the first evidence, typed kind |
| needs_you | ask close (answered) | working | turn still open |
| needs_you | ask close (declined, Esc) | idle for Esc; working for declined | no hook on Esc: transcript or screen closes it |
| working | turn end (done) | finished | `bg: N` badge if background work lives |
| working | turn end (halting error) | failed | cause class |
| working | process gone | failed(crashed) | a turn was open |
| finished, failed, info | viewed by a human | idle (or cleared) | foreground, visible page |
| working | progress proof lapses | working(quiet) | window TBD (step A.3) |

## Edge-case catalogue (every row gets a model test and a tier-2 scenario)

| # | Case | Expected |
|---|---|---|
| 1 | Plain turn: prompt, inference, Stop | working then finished; one chime |
| 2 | Long silent tool (build, tests) | working(tool), no decay to idle or unknown |
| 3 | Permission allowed, denied | needs_you(permission), then working, or idle/working per outcome |
| 4 | Permission, then Esc (no hook) | needs_you then idle within the reconcile window; no stale ask |
| 5 | AskUserQuestion answered / Esc | needs_you(question) with its text and options; clears |
| 6 | ExitPlanMode approve / reject | needs_you(plan) with the plan text |
| 7 | Trust dialog at startup | needs_you(trust) |
| 8 | Elicitation (MCP) | needs_you(elicitation) |
| 9 | Two asks at once in one tab, and in two tabs | both tracked per tab and key; roll-up shows the count |
| 10 | Ask answered in the terminal, not the UI | ask clears from the transcript, hook or screen |
| 11 | `Notification` before / after / never after the request | one alert; text fills in place |
| 12 | Esc during a running turn (no hook) | working then idle (interrupted), no failed |
| 13 | Recovered error: wrong tool name, retried 429 | stays working; "N recovered errors" in detail; no alert |
| 14 | Halting 429 after N tries | failed(rate_limit) with reset time if known; alert |
| 15 | Auth failure, context overflow | failed(auth), failed(context) |
| 16 | Stuck retrying beyond the threshold | silent info item |
| 17 | Compaction mid-turn | working(compacting); `SessionStart` source=compact is not a new session |
| 18 | Subagent running at Stop | finished + `bg`; subagent stop is info |
| 19 | Backgrounded Bash after Stop, completion wakes the agent | finished + `bg`, then working, then finished |
| 20 | Scheduled wakeup or loop | idle + `bg`; the wake opens a turn |
| 21 | `kill -9` of the agent mid-turn, and a clean exit | failed(crashed); idle after a clean `SessionEnd` |
| 22 | Resume a session (`--resume`) | no restored ask; state starts idle |
| 23 | Two agent tabs in one workspace | roll-up follows the order; per-tab chips independent |
| 24 | Tab closed with a pending ask or unseen result | item removed; roll-up recomputed |
| 25 | Server restart with a pending ask | ask rebuilt from transcript or screen, or cleared; never stuck |
| 26 | Client reconnect mid-ask / mid-turn | snapshot reproduces the same status and content |
| 27 | Hook late, duplicated, out of order | machine is idempotent; no flapping |
| 28 | Hook socket down or hooks not installed | transcript and screen only; status stays correct or unknown |
| 29 | Agent without hooks or adapter (Codex) | weaker sources only; unproven is unknown |
| 30 | Unseen finished, then a new turn | stays unseen until viewed |
| 31 | Viewed on device A, device B offline, then online | B shows it cleared |
| 32 | Two devices, ask answered on one | cleared on both; no second alert |
| 33 | Split panes in one tab | defined behaviour: first agent pane drives, a second is reported |
| 34 | Screen idle while hook says finished | stays finished (no overwrite) |
| 35 | Screen-only blocked, no hook or transcript | hint only; no alert, no queue entry |

## Steps

Phase A measures. Phase B builds the server model. Phase C moves the client onto it. Phase D proves it. Phase E closes
the loop in docs.

### A.1 Measure the lag between hook, transcript and screen — [x]
Class: explore · Needs: none · Check: `test -f docs/facts/0072-claude-hook-transcript-and-screen-lag.md && ls packages/server/testdata/transcripts | grep -c claude`
Context: Decision 0189 ranks hook > transcript > screen > scan and promotes the Claude session file to a status
source, with the grace values set from measured lag. Fact 0048 measured hooks only (claude 2.1.302, n=1). The Claude
session-log adapter is `packages/server/src/session_log/claude.rs` behind the port in `session_log/mod.rs`
(decision 0157); fixtures live in `packages/server/testdata/{events,screens,transcripts}`. Use a throwaway tmux
(`tmux -L scratch`), never the owner's sessions, haiku, small prompts.
Do: Write a script under `scripts/` (kept, rerunnable) that drives `claude` in a scratch tmux through: prompt submit,
tool start and end, a permission ask, AskUserQuestion, ExitPlanMode, Esc on a dialog, Esc on a running turn, Stop,
and a long silent tool. For each, timestamp (ms, one clock) the hook arrival, the transcript file append and the first
pane redraw showing it. Run each case at least 5 times and report median and p95 per source pair. Save payloads,
transcript lines and screens as fixtures. Write fact 0072 with the table, the version and the date, and add it to
`docs/facts/INDEX.md`.
Done-when: Fact 0072 holds a lag table for every listed case with n >= 5, and fixtures from the real run exist for each.
Progress: done 2026-11-01. `scripts/status-lag.mjs` (rerunnable), fact 0072, fixtures in `testdata/{events,transcripts,screens}/lag/`. Findings for later steps: the transcript has the pending ask and the Esc cancel within 500 ms, and the transcript trails the hook by up to 400 ms (so the reconcile grace must exceed 500 ms); a foreground `sleep` is refused with no hook (A.3 must use a busy loop); `Notification` was not seen. The skipped part: the script has no case for a 20-minute tool (A.3).

### A.2 Measure the unmeasured events and background behaviour — [x]‖
Class: explore · Needs: none · Check: `test -f docs/facts/0073-claude-events-subagents-background-and-errors.md`
Context: Fact 0048 lists what was not measured: `StopFailure`, `Elicitation`, `ElicitationResult`,
`PermissionDenied`, `idle_prompt`, plan-mode approval and parallel dialogs. Also not subscribed today
(`packages/server/src/claude_hooks.rs` `EVENTS`, 13 events): `SubagentStart`, `SubagentStop`, `PreCompact`,
`PostCompact`. Nothing detects a backgrounded Bash, a background subagent or a scheduled wakeup.
Do: In a scratch tmux, install the extra hooks in a scratch settings file and record, for each: payload fields, order
relative to `Stop`, and timing. Establish with evidence: does Claude wake on a backgrounded Bash completion and when;
what a scheduled wakeup or loop emits; what a 429 retry loop emits before `StopFailure` and its `error` values; what
`SessionStart` source=compact and resume emit; whether panes can be split inside a tab. Save fixtures under
`packages/server/testdata/events/`. Write fact 0073 and index it.
Done-when: Fact 0073 answers each question above with a measured result or "could not reproduce, how tried".
Progress: done 2026-11-01. `scripts/status-events.mjs`, fact 0073, fixtures in `testdata/events/a2/`. Key results for B.2 to B.5: `Stop` carries `background_tasks` and `session_crons` (the `bg` badge needs no scan); a finished background task or a wake opens a turn through `UserPromptSubmit`; retries emit no hook, so `retrying` is screen-only until `StopFailure` (about 3 min); `PermissionDenied` and context overflow were not reproduced (n = 1, no claude credentials for an error of that class).

### A.3 Measure the progress proofs for a silent tool and set the quiet window — [x]
Class: explore · Needs: A.1 · Check: `grep -q "quiet window" docs/decisions/0188-working-means-in-progress-except-human-input-and-background-work-is-a-badge.md && grep -q "retry threshold" docs/decisions/0192-failures-are-classed-by-whether-the-turn-ended.md`
Context: Decision 0188 shows `working(quiet)` when no progress proof arrives for a window, and 0192 raises an `info`
item when a session is `retrying` too long; both numbers are TBD. Progress proofs: hook edge, transcript append, pane
change, live child process.
Do: Using A.1's script, run a 20-minute silent tool (sleep loop, a real test run) and a stalled turn (kill the network
mid-inference). Record which proofs keep arriving. Pick the quiet window and the retry threshold with the evidence,
and write them into 0188 and 0192 as "quiet window" and "retry threshold" with the measured basis.
Done-when: Both decisions state a number and the run it comes from.
Progress: done 2026-11-01. `scripts/status-quiet.mjs` (rerunnable; the stall uses a local blackhole proxy, no host network change). Quiet window 60 s, retry threshold 120 s, written into 0188 and 0192. Findings for B.2/B.5: a 20-minute silent tool has no hook or transcript line but a live child process the whole time, so the child is the proof that matters; the raw pane is never a proof (spinner, counters, status line tick); a stalled turn ends in `StopFailure server_error` after 687 s (A.2 said about 3 min, which was for retries that kept progressing), with retry screens in between. n = 1 per run.

### B.1 Core types for sub-reason, tier, cause and bg — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test -p termenbox-core events`
Context: Shared protocol types are in `packages/core/src/events.rs` (`Activity` working/blocked/idle/unknown,
`Attention`/`AttentionKind`, `Task`, `WorkspaceActivity`; every enum has an `Unknown` fallback so an older peer ignores
values it does not know). Decisions 0187, 0188, 0192 and 0193 add data to them. The WASM build must still compile.
Do: Add `SubReason`, `FailureCause`, the Attention tier (`needs_you`, `result`, `info`), the `bg` count and the typed
ask kind (`permission|question|plan|trust|elicitation`) with `Unknown` fallbacks and serde defaults so old peers
and old clients still parse. Add wire round-trip tests including unknown values.
Done-when: `cargo test -p termenbox-core events` passes with a round-trip test for each new type and for an unknown
value of each.
Progress: done 2026-11-01. `SubReason`, `AskKind`, `FailureCause`, `AttentionTier` in `core/src/events.rs`; `Attention` gained optional `tier`, `ask`, `cause`, `WorkspaceActivity` gained `sub_reason` and `bg` (all defaulted, off the wire when empty). Writers fill them from B.2/B.6; the server literals set `None`/0 for now. Note for later steps: `cargo clippy --workspace` fails in `bridge-contract` (`explicit_counter_loop`, not ours), so clippy `-p termenbox-core` only; the two `e2e::agent_events_flow...` tests fail in the plain image on the clean baseline too.

### B.2 The pure per-session machine and its transition table — [x]
Class: implement · Needs: B.1 · Check: `scripts/dev.sh cargo test -p termenbox-server status::`
Context: Today's logic is split across `packages/server/src/reducer.rs` (turn and obligation reducer), `tabstatus.rs`
(per-tab states with their own writers) and the ring in `events.rs` (`derive_activity`, `settle`, `raise`). Decision
0187 replaces them with one machine per (workspace, tab) session. It is pure: typed events and a clock in, state and
Attention changes out, no I/O, so it can be tested as a table.
Do: Create `packages/server/src/status/` with `event.rs` (typed events: turn start/end, tool start/end, ask
open/content/close, subagent and background start/end, compact, session start/end, process gone, observation from
screen or transcript, viewed, dismiss), `machine.rs` (state, transitions, the quiet window, bg count, ask by tab and
key, failure classes) and a table-driven test with one row per transition in this plan, including late, duplicate and
out-of-order delivery. One constant for the grace and for the disagreement count.
Done-when: The test enumerates every status, sub-reason, failure cause and event kind and fails if one has no row.
Progress: done 2026-11-01. `packages/server/src/status/{event,machine,transitions}.rs`: `Event` (20 kinds), `Machine::apply/tick/view`, `rollup`, constants `GRACE_MS` 4 s, `DISAGREE_SCANS` 2 (a transcript reading counts once), `QUIET_MS` 60 s, `RETRY_INFO_MS` 120 s. 56 transition rows plus the coverage test (`every_status_reason_cause_and_event_has_a_row`). Not read by anything yet (`#![allow(dead_code)]` in `status/mod.rs`, drop it in B.6). Choices B.3 to B.5 must honour: `TurnEnd` carries `bg` (from `Stop`); a `ToolStart` within `GRACE_MS` of a turn end is late and opens nothing; `AskContent` attaches only to an open ask of the same key; `ProcessGone` with an open turn or ask is `failed(crashed)`; screen `Blocked` with no ask is a hint only; the Esc paths close through `Observe` (screen twice, transcript once) or `AskClose{Escaped}`. Catalogue cases beyond the transitions (two tabs, restart, devices) stay for D.1/D.5.

### B.3 Adapters emit typed events, with full ask content — [x]
Class: implement · Needs: A.2, B.2 · Check: `scripts/dev.sh cargo test -p termenbox-server adapters`
Context: `packages/server/src/adapters.rs` `claude` maps 13 hooks to edges; `claude_hooks.rs` installs them.
`tool_summary` returns "" for `AskUserQuestion` and `ExitPlanMode`, so their question and plan text are lost
(fact 0074). Subagent, compaction and background events are unmapped.
Do: Map every hook to a typed event. Read `tool_input` for AskUserQuestion (question, options), ExitPlanMode (plan)
and permissions (tool, arguments); sanitize and cap as untrusted agent text (decision 0095 limits). Subscribe the
new hooks found in A.2 in `claude_hooks.rs` and keep the opt-in consent flow (decision 0079). Fixture tests use
recorded payloads from A.1 and A.2.
Done-when: Each recorded fixture maps to the expected typed event, and an AskUserQuestion fixture yields its question
and options.
Progress: done 2026-11-01. `adapters::claude_events` maps every hook to `status::event::Event` (the legacy `claude()` stays until B.6); `AskUserQuestion` question and options, the `ExitPlanMode` plan and the permission tool come from `tool_input`, cleaned and capped. `claude_hooks::EVENTS` now has 17 (added `SubagentStart/Stop`, `PreCompact/PostCompact`; the opt-in consent flow is unchanged). Choices later steps rely on: asks are keyed by tool name, elicitations by `elicit:<server>`; a `Notification` yields `AskContent` with the new `machine::ANY_ASK` key, which the machine resolves to the one open ask without text (none or two: dropped, two new transition rows); `SubagentStop` with an empty `agent_type` is noise (fires after every `Stop`) and is ignored; `Stop` `bg` = `background_tasks` + `session_crons`; `StopFailure.error` maps `rate_limit`, `server_error` (measured) and `authentication_failed`, `billing_error`, `invalid_request`, `max_output_tokens` (documented, not measured) to causes, else `Unknown`. Also fixed `profile` test that broke on A.1's `testdata/screens/lag/` directory. Not yet read by the server: the new items carry `allow(dead_code)` until B.6.

### B.4 Transcript as an evidence source — [x]
Class: implement · Needs: A.1, B.2 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log`
Context: `packages/server/src/session_log/claude.rs` parses the transcript for search and usage only. Decision 0189
makes it a reconciler and the source of ask text. The contract for adapters is `session_log/contract.rs`.
Do: Expose turn start and end, tool calls with and without results, pending asks and error stops as typed
observations through the port, tail the file of each live session, and feed the machine. Keep the adapter behind the
port; an unreadable or changed format degrades to hook and screen, logged without content (guardrail
`audit-logging-no-secrets`).
Done-when: Fixture transcripts from A.1 produce the expected observations, and a malformed transcript produces none
and no panic.

Progress: done 2026-11-01. `session_log/observe.rs`: `Tail` reads the complete lines appended since its last call and returns `status::event::Event`s; `SessionLog::observe` is the port method (default `Disabled`), `ClaudeLog` implements it. Transcript turn start, assistant text and `end_turn` are `Observe{Transcript, Working|Idle}` only; tool calls are `ToolStart/ToolEnd`; `AskUserQuestion`/`ExitPlanMode` calls open an ask with question and options (same cleaning as the hook); a refused tool result waits for the next record: `[Request interrupted by user` makes `AskClose{Escaped}` plus `Observe Idle`, anything else `Declined`. An `isApiErrorMessage` record is `TurnEnd{Failed}` (field names unmeasured: no credentials for that error, fixture is hand-written). A shrunk file is `StaleCursor`, a half line waits, a bad line is counted and skipped (warn, no content). Not yet read by the server: B.6 wires `Tail` per live session (start `Tail::from(len)`), and a transcript `TurnEnd{Failed}` must be weighed against the hook there. The two `e2e::terminal_bytes_use_their_own_lane...` tests fail in the plain image (environment).

### B.5 Screen and scan reconcile every agent pane; the process gone closes the turn — [x]
Class: implement · Needs: B.2 · Check: `scripts/dev.sh cargo test -p termenbox-server agentwatch`
Context: `packages/server/src/agentwatch.rs` reconciles only the one chosen pane per workspace (active pane
preferred), and skips a pane whose command is no longer an agent without closing its state (fact 0074).
Do: Run reconcile per agent pane, emit observation events instead of setting state, and emit `process gone` when an
agent pane's command stops being an agent or its process exits. A screen hint never raises Attention.
Done-when: A test with two agent tabs reconciles both, and one that kills the agent mid-turn ends with `failed(crashed)`.
Progress: done 2026-11-01. `agentwatch::PaneWatch` turns one scan into `TabEvent`s: an `Observe{Screen|Scan}` per agent tab (every agent pane is captured now, with or without the tab store), and one `ProcessGone` when a tab that had an agent no longer does (command changed, pane or window closed) or tmux has no server (`all_gone`). `scan_tmux_once` returns them; nothing consumes them until B.6 (the spawned loop drops them, the legacy writers still run). Choices for B.6: a split tab is driven by its first agent pane (catalogue 33); a screen the profile does not recognise yields no observation; a scan failure other than "no server" yields no events, so a flaky `list-panes` never ends a turn. Tests: two tabs, split tab, kill mid-turn ending `Failed` through a `Machine`, closed tab and server gone. The hook `SessionEnd` and the pane check can both close a turn; the machine's `ProcessGone` after a clean end is already a no-op (B.2 rows).

### B.6 Retire the parallel state paths — [ ]
Class: implement · Needs: B.3, B.4, B.5 · Check: `scripts/dev.sh cargo test --workspace`
Context: `TabStatus` writers (`set_hook_state`, `set_screen_state`, `set_scan_state`, `apply_pane`), the ring's flat
`state`, `derive_activity`'s 60 s decay and `PIN_MAX_MS` are the paths that disagree (fact 0074). Attention is raised
in `events.rs` `raise`, `settle`, `settle_tab`.
Do: Make the machine the only writer. Derive the workspace roll-up from the machines in the order of decision 0187.
Raise and clear Attention only from the machine, per tier (decision 0193), clear only on a viewed or dismiss event.
Drop `task` on idle. Remove the dead `TabPrompt` branch or fill it from the ask content, attached by tab and key.
Delete the duplicated `GRACE_MS`. Keep the ack and the push ladder behaviour.
Done-when: `cargo test --workspace` passes and `grep -n "set_screen_state\|PIN_MAX_MS" packages/server/src` finds no
writer outside the machine.
Progress: split on 2026-11-01 after reading the code: it is one diff across `events.rs` (3.5k lines, `feed`,
`settle_tab`, `reconcile_at`, `push_reduced`, `derive_activity`, ~60 tests that pin today's behaviour), `tabstatus.rs`,
`reducer.rs`, `agentwatch.rs` and `workspaces.rs` (the 500 ms poll), too large to land safely as one commit. Do it as
three commits, each green on its own:
- **B.6a Machines in the hub, fed beside the old paths.** `EventHub` owns one `Machine` per (workspace, tab) behind one
  lock; the hook route feeds it `adapters::claude_events`, the scan feeds it `PaneWatch` events, a `Tail` per live
  session feeds it transcript events (start `Tail::from(len)`). Nothing reads the machines yet, except a test that
  runs each A.1/A.2 fixture through both paths and lists the disagreements. Done-when: the new test passes and the
  disagreement list is committed as a fixture, so B.6b changes are visible as a diff.
- **B.6b Read from the machines**, in three green commits (split on 2026-11-01 after reading `events.rs`: Activity,
  Attention and `TabStatus` each have their own tests and writers):
  - **B.6b-1 Activity.** The workspace `Activity` of a workspace with machines is `rollup` of them, with `sub_reason`
    and `bg` (drops the 60 s decay there). A workspace no machine has a status for (ACP, Codex, no hook yet) keeps
    `derive_activity` until B.6c decides what feeds those.
  - **B.6b-2 Attention.** Raised and cleared from `Effect`s (`RaiseAsk`, `FillAsk`, `RaiseFinished`, `RaiseFailed`,
    `RaiseInfo`, `Clear`), clear only on `Viewed`/`Dismiss`, for machine-backed tabs; the legacy `settle_tab`/`settle`
    raise and clear skip them. Ask text fills in place.
  - **B.6b-3 Tab view.** `TabStatus` serves `Machine::view`; `task` dropped on idle; `TabPrompt` filled from the ask.
  Port the `events.rs` tests that pin behaviour the decisions keep, delete those that pin behaviour they replace (say
  which in the commit body). Done-when: `cargo test --workspace` passes.
- **B.6c Delete the old paths.** Remove `Reducer`, `reducer.rs` (keep `Screen` and `WEAK_KEY` where used),
  `set_*_state`, `apply_pane`, `PIN_MAX_MS`, the duplicated `GRACE_MS`, `#![allow(dead_code)]` in `status/mod.rs`.
  Done-when: the original grep finds nothing and `cargo test --workspace` passes.
B.6a done 2026-11-01: `status/feed.rs` `Machines` (one machine per (workspace, tab), plus a `Tail` per followed transcript, started at the end of the file) is owned by `EventHub`. `deliver` feeds `adapters::claude_events` and follows `transcript_path` (resolved under the Claude root); the scan loop feeds the `PaneWatch` events, polls the tails on a blocking thread and ticks the machines; `forget_tab` and `forget_except` drop them. Nothing reads them yet. The test `events::tests::old_paths_and_machine_disagreements_are_listed` replays every A.1/A.2 run through both paths and pins the disagreements in `testdata/status/disagreements.txt` (`UPDATE_DISAGREEMENTS=1` rewrites it): after an Esc or a 401 the hooks alone leave the machine `working` where the old path says `unknown`, so B.6b must rely on the transcript or screen to close those (the fixtures hold hooks only). Env note: the 22 tmux-dependent tests in `cargo test --workspace` fail in this image (same as before B.6a); clippy `-p termenbox-server` has 17 errors in files this step did not touch.
B.6b-1 done 2026-11-01: `refresh` reads `Machines::views_for` through `machine_activity` (roll-up, sub-reason of the deciding tab, summed `bg`); the hub refreshes after a hook feeds the machines and after each scan tick. No test of the old behaviour changed: they feed the ring, not the machines. Env note: run cargo with a target dir per worktree (`CARGO_TARGET_DIR=/cache/target-sergei-p01`): every worktree mounts at `/work`, so the shared `target-sergei` served a stale `termenbox-core`. `control::a_real_tmux_pushes_window_and_directory_changes` is flaky under the parallel run (fails on the clean tree too).
Not started: B.6b-2, B.6b-3, B.6c.

### B.7 One snapshot frame on the status channel — [ ]
Class: implement · Needs: B.6 · Check: `scripts/dev.sh cargo test -p termenbox-server e2e`
Context: Status and Attention travel as separate messages (`activity`, `attention`, `tab_states`, polled every 500
ms in `packages/server/src/workspaces.rs`). Decision 0191 needs the status and the Attention item in the same frame.
Do: Send per-session status, sub-reason, cause, `bg`, tier, and ask content in one ordered frame on the status
channel (decision 0096); keep old messages for old clients until they are retired. Version it and test an old
client against the new server.
Done-when: A server e2e test shows a hook producing exactly one frame with both the status and the Attention item,
and an old-client fixture still parses.

### C.1 One status store and selectors in the client — [ ]
Class: implement · Needs: B.7 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test -- status'`
Context: Three readers today: `lib/agents.ts` `derived()` (chip, tab strip), `lib/workspaceTabs.ts` `aggregateStatus`
(Home, sidebar, fleet counts), and `lib/tabs.ts`, `lib/notify.ts` (queue, title, badge, chime). Components:
`WorkspaceCard.svelte`, `TabStrip.svelte`, `WorkspaceList.svelte`, `shell/BottomBar.svelte`, `lib/home.ts`.
Do: Add `lib/status.ts`, the only store applying the snapshot frame, with selectors for each surface and the roll-up
of decision 0187 (counts, first-ask tab). Switch every surface to the selectors and delete `derived()` and the mix in
`rowOf`. Drawing nothing for `unknown`.
Done-when: `grep -rn "aggregateStatus\|derived(" packages/client/src` finds no use outside `status.ts` and its tests,
and the status unit tests pass.

### C.2 Alerts play from the renderer — [ ]
Class: implement · Needs: C.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test -- notify'`
Context: `lib/notify.ts` `createAlerter` and `createNotifier` fire on workspace Attention from `App.svelte` around
line 500; `lib/bell.ts`, `lib/chimes.ts` play. Decision 0191 requires alerts to fire from the code that applied the
same snapshot, only for `needs_you` and `result` tiers.
Do: Move the alerter into the status store's apply step, run after render, key it on the rising edge of the tier,
and keep the freshness window and the kept-sound behaviour. `info` never alerts. Expose a test hook that records every
alert for the e2e helper.
Done-when: Unit tests prove an alert never fires for a status absent from the store, and `info` is silent.

### C.3 The ask card: typed fallback, in-place enrichment, open terminal — [ ]
Class: implement · Needs: C.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test -- decision'`
Context: `lib/decision.ts` (`decisionFor`, fallback 'Needs your answer'), `DecisionProbe.svelte`, `AcpEntryView.svelte`;
banner choices need 1 to 3 high-confidence choices.
Do: Show the typed fallback per kind until content arrives, fill the card in place without a new alert, always offer
"open terminal", offer one-tap answers only from structured content.
Done-when: A unit test renders each kind with and without content and shows no empty card.

### C.4 Viewed by a human only — [ ]
Class: implement · Needs: C.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test -- agents'`
Context: `lib/agents.ts` ack rules (foreground, visible page, not `needs_you`, the tab visible); server
`EventHub::ack` clears non-`needs_you` items and `TabStatus::clear_result`.
Do: Send `ack` only on a human view (foreground, visible page, tab visible), for `result` and `info` items; never on
a timer or a background tab; make the idle transition follow it. Tests cover the hidden page and the two-device case.
Done-when: A unit test shows no ack from a hidden page or a timer, and an ack on one device clears all.

### D.1 Complete the model test with the edge-case catalogue — [ ]‖
Class: implement · Needs: B.2 · Check: `scripts/dev.sh cargo test -p termenbox-server status::catalogue`
Context: The catalogue above has 35 cases; B.2 covers the transitions, not each case as a replayed script.
Do: Add `status/catalogue.rs`: one event script per case, replayed through the machine, asserting status, roll-up,
Attention and alerts at every step. Cases that need real timing use a fake clock.
Done-when: Each of the 35 rows has a named test, and a missing row fails the build.

### D.2 The scripted fake-Claude harness — [ ]
Class: implement · Needs: A.2 · Check: `scripts/dev.sh node scripts/fake-claude.test.mjs`
Context: There is no fake Claude that emits hooks, a transcript and screens; today only a renamed `dash` or `sleep`
binary so the scan detects "claude" (`agentwatch.rs` tests, `client/e2e/hookoffer.mjs`) and `fake-acp-agent.mjs` for ACP.
Do: Write `scripts/fake-claude.mjs`, started in a tmux pane under the process name `claude`, that reads a scenario
file and plays it: runs the real `termenbox-server --hook claude` with recorded payloads, appends transcript lines to
the session file path the server reads, and draws recorded screens (`packages/server/testdata/screens/`). Supports
delays, Esc (no hook), kill, and speed control. Scenarios are plain data in `packages/client/e2e/status-scenarios/`.
Done-when: The self-test plays a scenario in a scratch tmux and the server reports the expected status.

### D.3 Tier-2 scenarios through the real server and client — [ ]
Class: implement · Needs: D.2, C.1, C.2, C.3, C.4 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/status.mjs'`
Context: Browser e2e lives in `packages/client/e2e/` (`realserver.mjs` runs a real server and tmux; `cards.mjs` and
`decision.mjs` use a fake event stream). Browser suites are sharded in CI (PR #413).
Do: Add `e2e/status.mjs`, running every catalogue case that is observable in the client through the real server,
the fake Claude and the real client. Each step waits for and asserts the state (guardrail
`ux-scenarios-assert-a-state-before-they-capture-it`) and runs the cross-surface helper from D.4.
Done-when: All catalogue cases pass in the real stack, in the desktop and the phone viewport.

### D.4 The cross-surface invariant helper — [ ]
Class: implement · Needs: C.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/status-invariants.test.mjs'`
Context: Guardrail `no-alert-without-a-visible-status` is checked by a helper that every status scenario calls.
Do: Write `e2e/status-invariants.mjs`: after each step read chip, tab strip, sidebar, Home row, title count, badge and
inbox and assert they agree with the roll-up; read the alert log from C.2 and assert each alert follows a rendered
rising edge and each edge produced exactly one. Add a self-test with a deliberately broken surface.
Done-when: The self-test fails on a surface that disagrees and on an alert without a status.

### D.5 Multi-device, reconnect and restart scenarios — [ ]
Class: implement · Needs: D.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/status-devices.mjs'`
Context: Ack is global across devices (`EventHub::ack`); a snapshot is sent on connect; a server restart loses
in-memory state. Catalogue rows 25, 26, 31, 32.
Do: Two clients on one server: answer on one, view on one, one offline then online, reconnect mid-ask, server restart
with a pending ask. Assert the same status everywhere and one alert per edge.
Done-when: All four scenarios pass with the helper active.

### D.6 CI wiring — [ ]
Class: implement · Needs: D.3 · Check: `grep -rn "status.mjs" .github/workflows`
Context: CI skips jobs a pull request cannot affect and shards the browser suites (PR #413, #415). Tier 1 runs in
`cargo test --workspace`.
Do: Add the status suites to the browser shard list, add path filters for `packages/server/src/status/**`,
`adapters.rs`, `session_log/**`, `packages/client/src/lib/status.ts` and the fixtures, and keep the run under the
shard time budget.
Done-when: A PR touching only `status/machine.rs` runs tier 1 and tier 2; one touching docs runs neither.

### D.7 Tier 3: real-Claude smoke and drift check — [ ]
Class: implement · Needs: A.1, D.3 · Check: `sh scripts/status-smoke.sh --dry-run`
Context: Tier 2 is only as right as the last measurement; the drift started that way (fact 0048 n=1). Real Claude
needs credentials and costs money, so it runs on demand or nightly, never as a required check.
Do: Write `scripts/status-smoke.sh`: runs a short set of real-Claude cases (plain turn, permission, AskUserQuestion,
Esc, a forced error if reproducible) on haiku in a scratch tmux, records hook, transcript and screen, and compares the
events and ordering with the tier-2 fixtures; a difference fails the run and prints a diff. Add a manual workflow.
Done-when: `--dry-run` lists the cases and the fixtures they compare, and one real run passes against current fixtures.

### E.1 Close the docs loop — [ ]
Class: implement · Needs: B.7, C.4, D.5 · Check: `lint`
Context: Decision 0095 still says `task` is dropped on idle and that Codex has scan state only; both are wrong or
changed (fact 0074). The roadmap in `docs/plans/termenbox-roadmap.md` does not list this work.
Do: Mark fact 0074 as historical with a pointer to the decisions, update 0095's text for the points above, record the
results of A.1 to A.3 in the decisions that name a TBD, add a roadmap item with the owner's go-ahead, and run `lint`.
Done-when: `lint` reports no dangling reference or stale cross-reference for 0074 and 0187 to 0193.
