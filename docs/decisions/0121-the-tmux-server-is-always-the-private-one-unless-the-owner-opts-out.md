---
id: 0121-the-tmux-server-is-always-the-private-one-unless-the-owner-opts-out
title: Sessions always live on termenbox's private tmux server; the user's default tmux server is used only when the owner asks for it
status: active
date: 2026-10-19
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [server, sessions, deployment, compatibility]
---

## Decision

Accepted 2026-10-19. Amends 0120: replaces its "Existing installs keep their sessions" rule.

- **Always private.** The server uses its own tmux server (`-L termenbox`, the config it owns) on every start, new install or old. Sessions already on the user's default socket do not choose the socket, and a `tmux_socket` saved as `default` by an earlier start is replaced by `termenbox`.
- **One explicit way out.** `TERMENBOX_TMUX_SOCKET=default` keeps the user's own tmux server and config. That is the owner's choice and risk: tmux only attaches to a server of its own version, so the tmux termenbox runs must match the one that started that server.
- **Sessions already running are not moved** (tmux cannot hand them over). After the update they are no longer listed; they keep running on the default socket and end with their own work. The server logs a warning once when it moves off a saved `default`.
- The rest of 0120 stands: the private server, the config termenbox owns, `termenbox-server tmux <args>`.

## Why

0119 pins one tmux build so that termenbox behaves the same everywhere. A pinned client does not help when it attaches to a server somebody else started: a 3.8 client against a 3.2a server fails with `open terminal failed: not a terminal`, not a version message, so the install looks broken (found on a fresh `host-me` install on a machine with a running system tmux). Only a server that termenbox starts is guaranteed to be the version it runs. Keeping old sessions visible was the reason for the saved `default`; a working terminal matters more than a list entry.

Rejected: keep the default socket only when the versions match (the behaviour then depends on whatever tmux the user has, and a later tmux or bundled-tmux update breaks it again); keep the saved `default` (the same mismatch after every update that adds the bundled tmux, 0119).
