---
id: 0072
title: Claude Code 2.1.308 shows a dialog on the screen and in the transcript within 80 ms of its hook, but the transcript lags the hook by 150 to 400 ms elsewhere, and Esc leaves a trace in the transcript and on the screen only
status: active
tags: [agents, hooks, claude, status]
date: 2026-11-01
kind: environmental
governed-by: 0189
last-verified: 2026-11-01
---

# Lag between a hook, the transcript append and the pane redraw

Measured with `claude` 2.1.308 (haiku, `--permission-mode default`) by `scripts/status-lag.mjs`, rerunnable: 9 cases, n = 5 each, one clock (`Date.now()`; the hook script stamps `date +%s%3N`), pane polled every 15 ms and the transcript tailed in the same loop, so every value has about 15 to 30 ms of resolution. Fixtures from the first run of each case are in `packages/server/testdata/{events,transcripts,screens}/lag/`. Values are median / p95 in ms; positive means the second source is later.

| Mark | transcript - hook | screen - hook | screen - transcript |
|---|---|---|---|
| turn start (prompt submitted) | 398 / 402 | 63 / 70 | -330 / -290 |
| turn end (`Stop`, end_turn) | 140 / 156 | 118 / 139 | -17 / -16 |
| tool start (`PreToolUse`) | 156 / 204 | 51 / 65 | -105 / -99 |
| tool end (`PostToolUse`) | 214 / 293 | 1545 / 1726 (collapsed summary line) | 1252 / 1512 |
| permission dialog opens | 62 / 67 | 46 / 51 | -16 / -15 |
| AskUserQuestion dialog opens | 114 / 173 | 48 / 51 | -99 / -16 |
| ExitPlanMode dialog opens | 89 / 94 | 78 / 102 | -16 / 13 |
| permission answered (`1`) | 216 / 302 | -126 / -112 | -342 / -309 |
| AskUserQuestion answered (Enter) | 259 / 284 | 74 / 105 | -185 / -168 |
| long silent tool (25 s), start / end | 143 / 156, 227 / 269 | 50 / 57, 1658 / 2588 | -93, 1439 |

Cancelled with Esc (no hook arrives at all, so time is from the key sent):

| Case | transcript line | dialog gone from the screen |
|---|---|---|
| permission dialog | 297 / 347 | 190 / 271 |
| AskUserQuestion dialog | 335 / 392 | 232 / 284 |
| ExitPlanMode dialog | 344 / 486 | 296 / 385 |
| running turn (tool in flight) | 329 / 383 | 140 / 197 |

- A dialog is visible on the screen and in the transcript (a `tool_use` line, with no `tool_result`) within 80 ms of its `PermissionRequest`. The transcript line is the pending ask, so the ask text survives a missed hook.
- Esc writes an "interrupted" or "declined" line to the transcript within 500 ms and the dialog leaves the screen first. This is the only source for the cancel; confirms fact 0048 with n = 5 and adds the plan dialog and a running turn.
- The transcript flushes in batches: dozens of lines share one timestamp, so line order is reliable and per-line time is not.
- At turn start the transcript is about 400 ms behind the hook; the screen spinner (`✽ Word… (`) is the earliest sign after the hook (about 60 ms). `esc to interrupt` is no longer on the spinner line.
- A collapsed tool result ("Ran 1 shell command") is drawn 1.5 to 2.6 s after `PostToolUse`, so it is no proof of tool end; the spinner line going away is.
- `Notification` was not seen in the 1.5 s the dialog stayed open (fact 0048 saw it at 1.5 s and 6 s); `SubagentStop` followed `Stop` in some runs without a subagent.
- In plan mode the model ran read tools (`PreToolUse`/`PostToolUse`) before `ExitPlanMode`; its `PermissionRequest` carried `tool_name` `ExitPlanMode`.
- A foreground `sleep N; cmd` is refused by the tool ("Blocked: ... use Monitor") with no `PreToolUse` or `PostToolUse` hook; the silent-tool cases use a busy loop in `node -e` instead.
- The trust dialog defaults to "No, exit", so a script must send Down then Enter.
- The status-line footer in 2.1.308 reads "manual mode on" in `default` mode, and a new session starts in the settings' default mode, not `default`: pass `--permission-mode` explicitly.

The hook script itself adds a process start of a few ms, included above.
