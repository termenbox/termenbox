---
id: 0138-a-workspace-holds-windows-and-a-session-is-an-agent-conversation
title: A workspace is the sandbox that holds windows; a session is one agent conversation (an ACP sessionId)
status: active
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, protocol, architecture, roadmap]
---

## Decision

- **Workspace** is termenbox's own sandbox, much like a tmux session: a named, directory-bound container of **windows**, with a **focus** pointer (by default the base agent window). It replaces what the code, wire and UI call "session" today. "Container" was rejected as the name because it collides with the Docker deployment ("restart the container").
- **Window** is a process in a workspace and has a kind: **agent** or **terminal** (shell, lazygit, mc). A workspace can hold several agent windows and each can be addressed on its own.
- **Session** now means one agent conversation, exactly one ACP `sessionId`. An agent window has one session; a terminal window has none. This matches ACP's vocabulary and routing: the event stream is multiplexed by `sessionId`.
- **Hosting.** tmux stays under the workspace abstraction (0054): windows are tmux windows or panes; a native ACP child is an agent window with no pane. An existing single-agent "session" is the one-agent-window workspace, so nothing migrates by hand.
- **The rename is complete at once** in code, wire, UI copy and docs, as one mechanical pull request with no behaviour change, landed before protocol work touches the same files. Old clients are served by a **legacy shim** on the wire only: the hello carries a protocol version, a new server answers an old client with the legacy `session*` and `acp_view` messages for one release window, a new client against an old server falls back to the legacy view. The shim lives in one module and is deleted at the end of the window.
- **Persistence.** The workspace layout (windows, kinds, focus, directory) is persisted; agent windows come back by `session/resume` or `session/load` from the saved `acp_id` (plan `acp-session-modes`), terminal windows by tmux. A turn running during a server restart is lost and made visible.

## Why

The owner's model is a sandbox of several processes with one agent in focus, and wants to address any agent window, which a one-agent "session" cannot express. Keeping "session" for the agent conversation aligns with the protocol and avoids having two things called session. A staged rename was considered and rejected by the owner in favour of one atomic change; the legacy shim is what keeps hosted-client versus self-hosted-server skew from locking owners out.

## Rejected

| Option | Why not |
|---|---|
| Name it "container" | Ambiguous with the Docker container in docs and support |
| Staged rename (new names in protocol first, aliases for the rest) | Owner's call: one clean break; aliases would live on in code |
| Session id is the ACP sessionId and extra agents are sub-sessions | Second agents become second-class and addressing nests, against the need to address any one |
| Hard cut, lockstep release | Skew between the hosted client and self-hosted servers that have not updated |
| Own PTY multiplexer instead of tmux | Large rewrite of persistence, scrollback and resize; a risk with no gain for this goal; the host stays replaceable behind the port (0136) |
