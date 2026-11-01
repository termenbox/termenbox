---
id: 0074
title: Claude Code status runs through three parallel paths that can disagree, and its asks lose their question text
status: active
tags: [agents, server, client, notifications]
date: 2026-11-01
kind: observed
governed-by: 0187
last-verified: 2026-11-01
---

# Agent status pipeline as it was on develop at a31db0d

Read from the code, not executed (a code-read trace of `packages/server/src` and `packages/client/src`). Superseded in shape by decisions 0187 to 0193 once the plan `docs/plans/agent-status-model.md` lands. Paths below are under `packages/`.

## Sources and who writes what

| Source | Ingested by | Can set |
|---|---|---|
| Hook (`termenbox-server --hook claude`, 13 events) | `server/src/events.rs` `deliver`, `feed`; `adapters::claude` | working, needs_input, done, error, idle; never unknown |
| tmux screen markers (2 s scan) | `agentwatch.rs`, `tabstatus.rs` `set_screen_state` | working or idle per pane; blocked only for the one chosen pane per workspace |
| Process scan | `adapters::scan_state` | working if output within 3 s, else idle; only where no profile verdict |
| Session log (transcript) | `session_log/*` | nothing: search and usage only |
| Statusline forwarder | not built (R19) | nothing |

## Three paths to the screen

- The chip and tab-strip label read the flat state, derived from Activity and Attention once the server sends either (`client/src/lib/agents.ts` `derived()`).
- Home rows, sidebar and fleet counts read `aggregateStatus` over per-tab states (`client/src/lib/workspaceTabs.ts`).
- Queue, title, badge, chime, vibration and push read the existence of a workspace Attention item (`lib/tabs.ts`, `lib/notify.ts`, `server/src/push.rs`).

## Known disagreements (code-read)

- A screen idle overwrites a hook's `done` in the ring while the `finished_unseen` item stays (`events.rs` around line 947).
- A screen-only `needs_input` makes Activity `blocked`, so the chip says "Needs you", but no Attention item exists, so there is no queue entry, chime or push.
- Reconcile runs only for the one chosen pane per workspace; a crash, or `claude` exiting, closes no tab state or Attention item.
- `derive_activity` skips a working signal older than 60 s, and the tier-0 timestamp moves only on a state change, so one long working turn can fall to a stale idle or unknown.
- `SubagentStart`, `SubagentStop`, `PreCompact` and `PostCompact` are not subscribed; background work is not detected, and `Stop` closes the turn regardless of it.
- Decision 0095 says the task is dropped on idle; `ring.task` is only ever assigned. `GRACE_MS` is duplicated in `reducer.rs` and `tabstatus.rs`.

## Ask content

`Attention.prompt` is filled from the next `Message` event, which for Claude is the `Notification` hook 1.5 to 6 s after the request, matched to the first open ask in the workspace with no prompt, in any tab. `tool_input` of `AskUserQuestion` and `ExitPlanMode` is never read. `TabPrompt` is never filled for Claude, so the tab answer branch in `workspaces.rs` is dead for it.

## Tests that exist

About 70 unit tests in `events.rs`, 12 in `reducer.rs`, 8 in `tabstatus.rs`; recorded hook fixtures in `server/testdata/events/` and screens in `server/testdata/screens/`; client unit tests for agents, notify, cards and workspaceTabs; `client/e2e/cards.mjs` and `decision.mjs` on a fake event stream. There is no scripted fake Claude that emits hooks, a transcript and screens.
