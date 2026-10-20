---
id: 0029
title: What Claude Code and Codex expose for agent events
status: active
tags: [agents, hooks, server]
date: 2026-10-13
kind: environmental
governed-by: 0061
last-verified: 2026-10-13
---

# What Claude Code and Codex expose for agent events

Measured on the dev VPS, 2026-10-13, in a throwaway working directory and a throwaway herdr home (the owner's sessions were not touched). Claude Code 2.1.296. herdr was measured too and is dropped (decision 0097; see git history). Codex is not installed there, so its row is documentation only and unverified.

| Tool | Surface | Verdict |
|---|---|---|
| Claude Code | `hooks` in a settings file (`--settings FILE` or `~/.claude/settings.json`): `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionDenied`, `Elicitation`, `ElicitationResult`, `Notification` (typed: `permission_prompt`, `idle_prompt`, `auth_success`, `elicitation_dialog`), `Stop`, `StopFailure` (what we install; fact 0048). Each hook command receives one JSON object on stdin. | Primary source for Claude Code, opt-in (0079). |
| Codex | A `notify` program in `config.toml` is documented to receive one JSON argument per event (turn complete). Not verified here. | Unverified; scan state only until P.5 can test it. |

## Claude Code samples (real)

Every event carries `session_id`, `transcript_path`, `cwd`, `hook_event_name`. Tool events add `tool_name`, `tool_input`, `tool_use_id`; `PostToolUse` adds `tool_response` (stdout, stderr, `interrupted`) and `duration_ms`; `Stop` adds `last_assistant_message`.

```
PreToolUse  {"tool_name":"Bash","tool_input":{"command":"echo hello","description":"Print hello"},"tool_use_id":"toolu_01…","permission_mode":"default"}
PostToolUse {"tool_name":"Bash","tool_response":{"stdout":"hello","stderr":"","interrupted":false},"duration_ms":76}
Stop        {"stop_hook_active":false,"last_assistant_message":"The command printed `hello`. Done."}
```

Captured interactively (tmux): `Notification` fires when a permission prompt appears (sample in `packages/server/testdata/events/claude-notification.json`), and the screen at that moment is `testdata/screens/claude-permission.txt`. Not captured: a crash or kill (no `Stop` is delivered, so `done` must also come from the process scan).

Usage is in no hook payload: none carries tokens, cost, cache or rate-limit windows (`StopFailure` names `error: rate_limit`, nothing more). The transcript a hook names does: each assistant line has `message.usage` with `input_tokens`, `output_tokens`, `cache_read_input_tokens` and `cache_creation_input_tokens`, checked on a real transcript (2026-10-20), written once per content block with the same `message.id`, so a count must be taken once per id. ACP agents report tokens in the `session/prompt` result (`usage`: `inputTokens`, `outputTokens`, `cachedReadTokens`; fixtures under `testdata/acp/`); no recorded agent sends a `cost`.

`tool_input.command` and `prompt` can hold secrets, and `last_assistant_message` is model text: all of it is untrusted (plan v6 rules).

## Hook behaviour

- A hook that exits 1 or whose command does not exist does not fail the turn.
- Per-hook `timeout` is in seconds. One measurement (n=1): a hook sleeping 6 s with `timeout: 1` still delayed the end of a one-line run from about 3.8 s to 8.6 s, so a slow hook costs real seconds; our hook must be a single fast write to a local socket and exit 0.
- The session is learned from the environment: the server will export `TERMENBOX_SESSION`, `TERMENBOX_EVENT_SOCKET` and a per-session token into the pane; hooks read them. Claude's own `session_id` is not our session name.


## Scan and screen notes (tmux 3.6)

- `tmux list-panes -a -F '#{session_name}|#{pane_current_command}|#{window_activity}'` is one process for all sessions. A native agent binary shows as `claude`; a script shows as its interpreter (`sh`), and Codex, if it runs as a node script, will show as `node`, so the scan cannot see it by name.
- A session is a `capture-pane` target only as `=name:` (trailing colon); `=name` alone fails with "can't find pane".
- Real permission screen (`testdata/screens/claude-permission.txt`): the line `Do you want to proceed?` with numbered choices `1. Yes` … `4. No`. Claude Code also asks "trust this folder" (`claude-trust.txt`) with unnumbered choices, which the heuristic does not match.
- A Unix socket path is limited to about 108 bytes; the event socket falls back to a private directory under the temp dir when the state directory path is longer.
- Memory measured for plan v6: 20 sessions with full rings of 50 events use about 165 KB as JSON for typical events (more with 120-character summaries, still under 500 KB). 24,000 events (20 sessions, 20 per second, one minute) are processed in about 0.8 s in a debug build.
