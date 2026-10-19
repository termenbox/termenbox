---
id: 0120-termenbox-runs-its-own-tmux-server-with-its-own-config
title: Termenbox runs its own tmux server on a private socket with a config it owns, apart from the user's tmux
status: active
date: 2026-10-19
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [server, sessions, deployment, compatibility]
---

## Decision

Accepted 2026-10-19. Amends 0054 (the tmux backend) and 0119 (how the panes get their `TERM`).

- **Private server.** Every tmux call the server makes (`Runner`, `packages/server/src/backend.rs`, the one place that builds a tmux command line) uses `-L termenbox -f <config dir>/tmux.conf`. That is a separate tmux server, separate from any tmux the user runs themselves, and tmux reads no `~/.tmux.conf`, `/etc/tmux.conf` or XDG config for it. This holds for every runner: bundled, `PATH`, `TERMENBOX_TMUX`, Docker and host mode.
- **A config termenbox owns.** The server writes `tmux.conf` into its config dir on every start, so it always matches the running version. It holds what termenbox needs even when that contradicts a user's own tmux setup (prefix, `status`, `escape-time`, `history-limit`, `exit-empty`, and so on) and `default-terminal`, rendered from the entries the host's terminfo has among tmux-256color, screen-256color and xterm-256color. This replaces the one-off `set -g default-terminal` that was chained onto a fresh server's first command: the file is the single place for termenbox's tmux settings. The file ends with `source-file -q tmux.local.conf`, an optional file in the same dir that the owner may add; termenbox never writes it.
- **Existing installs keep their sessions.** The choice of socket is saved once in the config dir (`tmux_socket`: `termenbox` or `default`), as the UDP port is. A new install gets `termenbox`. A server that starts with no saved choice and finds sessions on the user's default socket saves `default` and behaves exactly as before: the default socket, the user's own config, no `-f`. Moving to the private socket is an explicit step by the owner, and sessions already running are not moved (tmux cannot hand them over); they end with their own work.
- **Reaching the sessions by hand.** `termenbox-server tmux <args>` runs the same tmux with the same socket and config, so `termenbox-server tmux attach` works from a shell. The uninstall text names it in place of a bare `tmux ls`.
- **Isolation is not security.** It prevents configuration clashes between termenbox and the user's tmux. Access control is still the OS user (0115) and the owner check (0053).

## Why

Termenbox attaches a PTY from a web client and relies on control mode, so a user's prefix, bindings, `default-command` or `destroy-unattached` can break it, and a setting termenbox needs can break the user's own tmux if the two share a server. A running tmux server loaded its config when it started, so per-command `set` calls cannot give a stable result on a shared server. A private socket plus an owned config file gives both sides their own settings, and one file to look at when a session behaves oddly. The saved choice keeps the change from making existing sessions vanish after an update.

Rejected: only the bundled tmux (leaves Docker and system-tmux installs coupled); per-command options on the shared server (clobbers the user's settings, no effect on a running server); moving existing sessions automatically (not possible without killing them).
