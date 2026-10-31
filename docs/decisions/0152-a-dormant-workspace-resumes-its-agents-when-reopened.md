---
id: 0152-a-dormant-workspace-resumes-its-agents-when-reopened
title: A dormant workspace resumes its agents when it is reopened
status: draft
date: 2026-10-31
track: product
tags: [workspaces, tabs, agents, hooks, server]
amends: [0149-a-workspace-is-a-server-record-and-each-tab-is-a-tmux-window]
related: [0122-agent-state-comes-from-one-turn-and-obligation-reducer, 0079-connector-settings-are-edited-with-a-backup, 0054-server-owns-workspaces-through-a-backend-trait]
---

## Decision

- **A tab record keeps its agent's session id.** `TabRecord` gains an optional `agent_session`: the id the agent reports in its own hooks (`session_id` for Claude Code and Codex, `sessionID` for OpenCode; `adapters::agent_session_id` already reads it). It is written when the first hook event of a session names the tab and replaced when a new session starts in the same tab. It holds the id only, never the conversation or the prompt.
- **Reopening a dormant workspace resumes, it does not just recreate shells.** For each tab that has an `agent_session` and a known agent, the new window runs that agent's resume command in the tab's directory: `claude --resume <id>`, `codex resume <id>`, `opencode --session <id>`. The command table lives next to the tab templates and is the only place that knows the flags. A tab without a session id, or of an agent with no resume command, opens as before.
- **A resume that cannot happen falls back to a shell and says so.** If the agent's session file is gone, the command fails, or the agent is not installed, the tab stays a shell with its label and directory, and the reopen reply lists each tab as `resumed` or `shell` with a short reason. The server never starts a fresh agent conversation under a tab that had another one.
- **The restore is one explicit step.** Reopening a dormant workspace is the user's action; nothing resumes agents on server start or in the background, so a restart with no one watching spends no model tokens.
- **Scrollback is not restored in this slice.** tmux history dies with the tmux server. Saving a capped capture per tab on an interval would cost disk and could hold secrets, so it needs its own decision.
- **Ids stay in the registry file** next to the labels and order, written atomically like the rest of it (0149), and are never logged or put in an audit entry.

## Why

0149 makes the workspace layout survive a tmux server that dies, but the agents are the point of a tab: after a reboot the owner has to find each session id and resume each agent by hand in every tab. The server already receives the session id in every hook event, so keeping the last one on the tab costs one field and gives a restart the same result as a reattach.

## Consequences

- The registry format gains a field; an older server ignores it and a record without it reads as today.
- A resume command is an agent contract that can change. A flag that stops working turns into a visible shell fallback, not a broken tab.
- The ids are not secrets, but a resumed session sees the full previous conversation, so a workspace opened by a device the owner has already authorised is the only path that runs it.
- Open: whether a restart that the owner caused on purpose (an update) should resume without the explicit reopen step. Not decided here.
