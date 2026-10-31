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

Read from the vendors' documentation on 2026-10-31, not run here. Re-check before building on it.

| Agent | Scroll and search in the TUI | External jump to a turn |
|---|---|---|
| Claude Code, fullscreen renderer (alternate screen) | Ctrl+O opens transcript mode; `/` searches, Enter accepts, `n`/`N` step through matches; `{` `}` jump between user prompts; PgUp/PgDn, Ctrl+Home/End outside it | none: no flag, env var, command or IPC takes a message id (`--resume` restores the conversation, not the viewport) |
| Claude Code, classic renderer | Ctrl+O and Ctrl+E only; search and scroll keys undocumented | none |
| Codex | Ctrl+T transcript overlay pages with PgUp/PgDn and arrows; mouse wheel does nothing there; no documented search | none found |
| OpenCode | message navigation actions (`messages_previous`, `messages_next`, `messages_last_user`, page up/down) in a third-party list only | none found |

Which Claude Code renderer starts depends on setup (`/tui` shows it; `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` forces classic). The tmux history of a tab running a fullscreen agent holds only the shell output and the agent's current screen (fact in `docs/plans/assets/terminal-search/feasibility.md`, scenario 5).

Claude Code transcripts are JSONL files at `~/.claude/projects/<project>/<session-id>.jsonl`; the docs call the entry format internal and subject to change. Hooks receive `transcript_path`.

Sources: code.claude.com/docs/en/{fullscreen,interactive-mode,keybindings,sessions,checkpointing,cli-reference}.md; openai/codex issue 36099 and transcript pager PRs; opencode.ai/docs/tui and learnopencode.com keybind reference (unofficial).
