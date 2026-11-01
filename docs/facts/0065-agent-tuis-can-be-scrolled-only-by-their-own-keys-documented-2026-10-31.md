---
id: 0065
title: Agent TUIs can be scrolled to an earlier turn only by their own keys; none has an external jump
status: active
tags: [agents, terminal, client]
date: 2026-10-31
kind: environmental
governed-by: 0156
last-verified: 2026-10-31
---

# Agent TUIs can be scrolled to an earlier turn only by their own keys; none has an external jump

The table is read from the vendors' documentation on 2026-10-31, not run here. The session-file section below was measured by running both agents on 2026-10-31. Re-check before building on either.

| Agent | Scroll and search in the TUI | External jump to a turn |
|---|---|---|
| Claude Code, fullscreen renderer (alternate screen) | Ctrl+O opens transcript mode; `/` searches, Enter accepts, `n`/`N` step through matches; `{` `}` jump between user prompts; PgUp/PgDn, Ctrl+Home/End outside it | none: no flag, env var, command or IPC takes a message id (`--resume` restores the conversation, not the viewport) |
| Claude Code, classic renderer | Ctrl+O and Ctrl+E only; search and scroll keys undocumented | none |
| Codex | Ctrl+T transcript overlay pages with PgUp/PgDn and arrows; mouse wheel does nothing there; no documented search | none found |
| OpenCode | message navigation actions (`messages_previous`, `messages_next`, `messages_last_user`, page up/down) in a third-party list only | none found |

Which Claude Code renderer starts depends on setup (`/tui` shows it; `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` forces classic). The tmux history of a tab running a fullscreen agent holds only the shell output and the agent's current screen (fact in `docs/plans/assets/terminal-search/feasibility.md`, scenario 5).

## Session files, measured 2026-10-31

Both agents write one JSONL file per session, append-only, one JSON object per line. The vendors call the formats internal. Fixtures: `packages/server/testdata/session_log/` (README lists what is synthetic).

| | Claude Code 2.1.308 | Codex 0.166.0 |
|---|---|---|
| File | `$CLAUDE_CONFIG_DIR` or `~/.claude`, then `projects/<cwd with every non-alphanumeric as "-">/<session-id>.jsonl` | `$CODEX_HOME` or `~/.codex`, then `sessions/YYYY/MM/DD/rollout-<local time>-<session-id>.jsonl` |
| Directory link | the folder name encodes the cwd; each record also carries `cwd` | `session_meta.payload.cwd` on line 1 |
| Record | `type`, `uuid`, `parentUuid`, `sessionId`, `version`, `timestamp`, `isSidechain` | `timestamp`, `ordinal`, `type`, `payload`; line 1 is `session_meta` with `cli_version` |
| Conversation | `user` and `assistant` records, `message.content` a string or blocks (`text`, `thinking`, `tool_use`, `tool_result`) | `response_item` with `payload.type` `message` and `role` (`developer` and `user` setup messages come first); turns bracketed by `event_msg` `task_started` and `task_complete` with `turn_id` |
| Tool call | `assistant` `tool_use` block, then a `user` record with a `tool_result` block and a `toolUseResult` object (stdout, stderr) | `custom_tool_call` (`input` is code) and `custom_tool_call_output` in `response_item`, plus `event_msg` `item_completed` of type `CommandExecution` with the command and output |
| Noise to skip | `queue-operation`, `atis-latch`, `api-request*`, `last-prompt`, `cost-state`, `mode`, `attachment` (system prompts, skill and tool listings) | `world_state`, `turn_context`, `token_usage_record`, `token_count`, `thread_settings_applied`, `additional_tools` |
| Compaction | a `system` record with `subtype: compact_boundary`, `parentUuid: null` and `logicalParentUuid`; kept records listed in `compactMetadata.preservedSegment` | not measured |
| Fork | `--fork-session` writes a new file that repeats the parent's records with the same `uuid`s, so one phrase hits both files | `codex fork` is interactive only; not measured |
| Rewind | not run (no non-interactive form); the tree is by `parentUuid`, so an abandoned branch stays in the file | not measured |
| Torn last line | not observed; the writer appends whole lines, a reader must still drop an unterminated last line | same |

Measured and unexpected: a Claude `tool_use` input and a Codex `custom_tool_call` input carry the command text, which can hold a secret, so tool records are never searched or shown (guardrail `session-search-is-bounded-scrubbed-and-never-logs-the-query`). A resumed Claude session continues in the same file; a forked one does not. Codex rollout files hold the account and user ids in `session_meta`, so they are never copied into a fixture.

Hooks receive `transcript_path`.

Sources: code.claude.com/docs/en/{fullscreen,interactive-mode,keybindings,sessions,checkpointing,cli-reference}.md; openai/codex issue 36099 and transcript pager PRs; opencode.ai/docs/tui and learnopencode.com keybind reference (unofficial).
