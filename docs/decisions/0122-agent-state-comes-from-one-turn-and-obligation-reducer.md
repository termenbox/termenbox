---
id: 0122-agent-state-comes-from-one-turn-and-obligation-reducer
title: Agent state is derived by one reducer from open turns and open obligations, fed alike by hooks and ACP; the screen only closes what it no longer shows
status: draft
amends: 0095-agent-status-is-activity-plus-attention
date: 2026-10-19
track: product
tags: [agents, server, hooks, protocol]
---

## Decision

- **One reducer per (workspace, tab)** (`packages/server/src/reducer.rs`) holds whether a **turn** is open and the set of open **obligations** (a permission, a question, a plan to approve, an elicitation). The state is derived, never set: an obligation open means `needs_input`; else a turn open means `working`; else the way the last turn ended (`done`, `error`, `idle`). Adapters only report edges (`Turn`, `Block`, and hook `Tool` events); the hub turns them into state. Hooks and ACP feed the same reducer, so a wait is the same thing in both. The tab takes the state its reducer holds after every edge, and closing a tab drops its reducer. The workspace state is an aggregate of its tabs: a wait beats work, work beats a result, otherwise the tab that spoke last decides. One tab's finish never erases another tab's turn or open question.
- **`working` means a turn is open and nothing waits on the human**, whether the agent is inferring or running a long tool. It never means "the pane printed something": the process scan no longer says `working` from output for an agent whose profile can tell working from idle (a redraw on attach is output, not inference).
- **An obligation ends with its resolution, never with a clock.** A permission or question stays `needs_input` for as long as it is open; a guess from the screen or the scan cannot override an open turn or obligation. The old 60 s decay of `working` (0095) still applies to a source that has gone quiet and holds no open turn.
- **Some endings send no event** (Esc on a dialog or on a running turn fires no Claude Code hook, fact 0048). The scan's screen read reconciles per tab, from that tab's own pane: an obligation or turn the profile's markers no longer show (`blocked`, `working`, `idle`) is closed after two scans in a row and only once the last event is 4 s old. The screen never opens one. A reconciliation clears a wait and raises no `finished_unseen` or `failed`.
- **Claude Code hooks installed**: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionDenied`, `Elicitation`, `ElicitationResult`, `Notification`, `Stop`, `StopFailure`. A wait is keyed by the tool name (`PermissionRequest` carries no `tool_use_id`). `Notification` arrives seconds after the dialog, so it only supplies the words and opens a wait alone when no request hook has been seen; `idle_prompt` ends a turn that sent no `Stop`.
- Settings written by an earlier version lack the new events; the next "Connect Claude Code" adds them (only ours, with a backup, 0079).

## Why

The flat state was set by whichever event arrived last, so a question showed as `working` (its `PreToolUse`) and then decayed to `idle` through the scan when the hook went quiet; it also flipped to `working` when switching sessions redrew the pane. Deriving state from open turns and obligations is what ACP gives by construction (0111), and it removes the whole class: parallel requests, late notifications and silent cancels are handled by one rule each, and are tested against payloads recorded from Claude Code 2.1.302. Whether new sessions default to ACP is a separate choice and is not made here.
