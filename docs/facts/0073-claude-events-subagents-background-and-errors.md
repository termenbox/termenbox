---
id: 0073
title: Claude Code 2.1.308 reports background work in the Stop payload, wakes the session with a UserPromptSubmit, and ends a failing turn with StopFailure after about three minutes of retries
status: active
tags: [agents, hooks, claude, status]
date: 2026-11-01
kind: environmental
governed-by: 0182
last-verified: 2026-11-01
---

# Claude Code events not measured by fact 0048

Measured with `claude` 2.1.308 (haiku) by `scripts/status-events.mjs` (n = 1 per case, all 17 hook events subscribed); payloads are in `packages/server/testdata/events/a2/`. Times are ms from the prompt.

| Question | Result |
|---|---|
| Does `Stop` say whether work lives on? | Yes. Its payload has `background_tasks` (id, type `shell`, status `running`, description) and `session_crons` (id, cron `schedule`, `recurring`). `SubagentStop` carries the same two fields. This is the source of the `bg: N` badge |
| Backgrounded Bash (`run_in_background`) | `PreToolUse` 2656, `PostToolUse` at once (empty output), `Stop` 4241 with one running task; on completion (14999) a new turn opens with `UserPromptSubmit` whose prompt is a `<task-notification>`, then `Stop` with `background_tasks=[]` |
| Background subagent (`Agent` tool) | `SubagentStart` 2936, `PostToolUse(Agent)` 3 ms later, `SubagentStop` 4857, `Stop` 5105, then a `<task-notification>` turn opens at 5312. `SubagentStop` also fires after most plain `Stop`s with an `agent_id` and no subagent (the title or summary agent) |
| Scheduled wakeup (`ScheduleWakeup`) | `PostToolUse` returns `scheduledFor`; `Stop` lists the wake in `session_crons` (minute granularity, the 60 s ask became 90 to 105 s); at 94727 `UserPromptSubmit` with the wake prompt opens a turn and `session_crons` empties |
| `SessionStart` after `/compact` | Order: `PreCompact` (`trigger` manual), `SubagentStop`, `SessionStart` `source=compact`, `PostCompact` (with `compact_summary`). No `UserPromptSubmit` or `Stop` |
| `SessionStart` on `--continue` | `source=resume`, with `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`; no restored ask |
| `/exit` | `SessionEnd` `reason=prompt_input_exit` |
| `idle_prompt` | `Notification` `notification_type=idle_prompt`, "Claude is waiting for your input", 60 s after `Stop` |
| Elicitation (MCP, form) | After the tool's own permission dialog: `Elicitation` (`mcp_server_name`, `message`, `mode=form`, `requested_schema`), then on Esc `ElicitationResult` (`action=cancel`), `Notification` `elicitation_response`, `PostToolUse`. The `Elicitation` hook carries the full question |
| Parallel tool dialogs | Shown one at a time: `PreToolUse`, `PermissionRequest`, answer, `PostToolUse`, then the next tool's pair. Never two open dialogs from one message |
| Tool disallowed by settings | No `PreToolUse`, `PostToolUse` or `PermissionDenied`; the model is told and the turn ends with `Stop`. `PermissionDenied` was not reproduced (it needs an auto-mode classifier denial) |
| Splitting panes in a tab | Claude Code has no panes; a pane split is a tmux action on the tab's window, not an event the agent reports |

## API failures (local server answering with the status, `ANTHROPIC_BASE_URL`)

| Status | Retries | `StopFailure` | Screen while retrying, after |
|---|---|---|---|
| 429 | 13 requests over 150 s (gaps 1, 1, 2, 5, 10, 20 s then ~38 s) | `error=rate_limit` at 182 s | "API Error: Server is temporarily limiting requests (not your usage limit)" |
| 500 | 13 over 139 s | `error=server_error` at 171 s | "API Error: 500 ..." |
| 529 | 13 over 150 s | `error=server_error` at 189 s | "API Error: 529 ..." |
| 401 | 11 requests to 105 s, still retrying at 120 s | none within 120 s | `✻ 401 ... · Retrying in 17s · attempt 9/10` |

- During the retries no hook arrives at all (only `UserPromptSubmit` before and `StopFailure` after): the retry state exists on the screen alone, as `Retrying in Ns · attempt k/10`.
- The failed turn leaves "API Error: ..." as the message and `last_assistant_message`; the transcript records a `turn_duration` system line, not a `Stop`.
- A 529 and a 500 give the same `error` value (`server_error`); `overloaded` is told apart only by the message text.
- Not reproduced: context overflow and a stuck-forever state.
