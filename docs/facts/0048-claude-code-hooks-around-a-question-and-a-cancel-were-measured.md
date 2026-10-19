---
id: 0048
title: Claude Code 2.1.302 sends PermissionRequest for AskUserQuestion at once, Notification up to six seconds later, and no hook at all when a dialog or a turn is cancelled with Esc
status: active
tags: [agents, hooks, claude]
date: 2026-10-19
kind: environmental
governed-by: 0122
last-verified: 2026-10-19
---

# Claude Code hooks around a question, a permission and a cancel

Measured with `claude` 2.1.302 in a private tmux, every event logged with a timestamp (n=1 per case, haiku, modes `auto` and `default`). Fixtures are in `packages/server/testdata/events/`.

| Case | Events, in order |
|---|---|
| AskUserQuestion shown, answered | `UserPromptSubmit`, `PreToolUse` (tool_use_id), `PermissionRequest` (+0.05 s, **no tool_use_id**), `Notification` `permission_prompt` (+1.5 s once, +6 s once), then `PostToolUse` (tool_use_id), `Stop`, `SubagentStop` |
| Same, Esc | `PreToolUse`, `PermissionRequest`, `Notification`; then **nothing**. The turn ended ("User declined to answer questions") with no `PostToolUse`, `PostToolUseFailure`, `PermissionDenied` or `Stop` |
| Bash permission, allowed (`1`) | `PreToolUse`, `PermissionRequest` (carries `permission_suggestions`), `Notification`, `PostToolUse`, `Stop` |
| Bash permission, Esc | `PreToolUse`, `PermissionRequest`, `Notification`; then nothing ("Interrupted") |

- `AskUserQuestion` asks through the same `PermissionRequest` as a tool permission, in `auto` mode too.
- Not measured: `StopFailure`, `Elicitation`, `ElicitationResult`, `PermissionDenied` and `idle_prompt` (the binary lists them; the fixtures for them are hand-written), plan-mode approval, and parallel dialogs.
- The screen footer of every dialog that waits (permission, question, trust) contains `Esc to cancel`; the question's also `Enter to select`.
