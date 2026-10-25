---
id: 0137-agents-start-in-a-terminal-or-agent-view-chosen-per-agent
title: An agent starts in a Terminal view or an Agent view (ACP), chosen per agent; ACP sessions stay server-owned and a restart resumes them
status: active
amends: 0111-acp-sessions-use-one-event-model-and-per-agent-quirk-profiles
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, sessions, architecture]
---

## Decision

- **The view is chosen per agent when a session is created.** Terminal is the agent's own TUI in tmux (tiers T1 to T3). Agent view is ACP (T0), drawn by the web client. The default is Terminal; the last choice is remembered per server and agent. The control is called View, never "mode" (ACP has its own session modes).
- **ACP sessions stay server-owned, with no tmux pane and no TUI process.** The server owns the agent child (0111, 0054); reconnecting a client changes nothing.
- **A restart means resume.** The server restarts the child and runs `session/resume`, else `session/load`, from the saved `acp_id`. A turn running during the restart is lost; the owner accepts that, and it is made visible (a stream notice, one Failed attention item, a guide warning). A session that cannot be restored is kept with its reason and retried at the next restore, never dropped silently.
- **Built-in agents exist but are off by default** (`TERMENBOX_ACP=off`; `on` turns them on, or list `acp` in `TERMENBOX_BACKEND`): `claude` (`claude-agent-acp`) and `opencode` (`opencode acp`). With no `acp-agents.toml` the built-ins are the allowlist; a file is exclusive unless it sets `builtin = true`; a same-named file entry wins. Why off: with `acp` next to `tmux` the composite backend prefixes every id (`tmux:<name>`), so an upgrade would rename every existing session and orphan what the client saved under the old id; and the adapters' terms are not settled (next bullet). This amends 0111: the default-view question is answered (Terminal) and the allowlist is "the built-ins plus the owner's file".
- **Open gate (owner).** The adapters' terms of use are not settled. Anthropic's Legal and compliance page says developers building on the Agent SDK should use API keys and that third parties may not offer Claude.ai login or route requests through Free, Pro or Max credentials for users; end users may sign in to the unmodified Claude Code binary with their own subscription. The `claude-agent-acp` (Apache-2.0) and `opencode` (MIT) READMEs say nothing on terms. The default is already `off`; if the owner decides yes, turning it on by default needs stable ids first (R74).
- **Adapters are not shipped in the image.** The owner installs and logs in to them in the container. Availability is checked live on every call, so an install shows without a restart; login is not probed.
- **`max_sessions` defaults to 6 with no memory floor.** The server must handle 5 to 6 sessions freely; whoever wants more, or has less memory, sizes the box. `resource-constraints` covers termenbox's own processes (the server idles near 9 MiB, fact 0047), not agent children (a few hundred MB each). Accepted risk: six adapters on a 512 MB box can hit the OOM killer; the guide says so.
- **The ACP client uses `agent-client-protocol-schema`** for types and serde only (`default-features = false`); the transport stays ours. A message that fails typed decoding is dropped for that message, never the turn.

## Why

The ACP view existed but was hidden (it still is, until the owner turns it on): `TERMENBOX_BACKEND` defaulted to tmux only, `acp-agents.toml` had to be hand-written, and the dialog showed a Manager dropdown only with several backends. The owner wants both views as a normal, visible choice per agent. Resume after a restart costs one lost turn and needs no host process, and the authenticated owner can already run any shell through tmux, so a fixed built-in command adds no reach.

## Not chosen

- Opt-in from the dialog (a new owner-only wire message).
- Baking adapters into `docker/Dockerfile.base` (image size, releases tied to adapter versions, terms for everyone) or `npx -y` in the profile (network at first start, runs whatever the registry serves).
- Always merging built-ins into a config file (silently adds agents for owners with a restrictive file, no way to remove one) or always exclusive (the visibility problem stays).
- Defaulting to Agent view (changes what existing users get, loses the hook and screen tiers) or one choice per server.
- Draining open turns on shutdown (Docker stops after 10 s; follow-up) or a detached host process that outlives the server (rejected for now).
- Keeping `max_sessions` at 2, or a free-memory floor (it would refuse starts on small boxes instead of failing late).
- The full `agent-client-protocol` SDK (+37 crates and its own runtime glue beside tokio) or staying hand-written (tracks protocol renames by hand; ACP v2 renames fields such as `id` to `configId`).
- Awaiting `restore()` before serving (up to six sequential 30 s handshakes would block startup).

Plan and measurements: `docs/plans/acp-session-modes.md`. Follow-ups: R74.
