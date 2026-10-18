---
id: 0046
title: One machine can already hold a system install and a user install for each user; what coexists and what conflicts
status: active
tags: [deployment, server]
date: 2026-10-17
kind: environmental
governed-by: 0115
last-verified: 2026-10-17
---

# One machine can hold a system install and a user install per user

Read from `packages/client/public/install.sh` and `packages/server/src/main.rs`; the installer was **not run** with two users. A two-user CI test is plan step 1.6.

**Separate per instance:** binary (`/usr/local/bin` vs `~/.local/bin`), state and keys (`/var/lib/termenbox` vs `~/.config/termenbox`), service unit (system vs `~/.config/systemd/user`), journal (`/var/lib/termenbox-root` vs `~/.local/state/termenbox`), stored ports, tmux sockets (per uid). The "already installed" checks test those exact paths, so one install does not hide another. Each has its own server key and owner.

**Conflicts found:**
- The port check uses `ss`, which minimal images lack; without it the check passes and two instances both pick UDP 4443 and TCP 8080. Nothing reserves a chosen port before it is bound.
- A user without root cannot open the firewall; with ufw or firewalld active the instance is unreachable until the admin opens the port.
- Only one system install (`host`, `host-user`) exists per machine: fixed unit and path names, one run user.
- Starting at boot needs lingering; self-service lingering can be denied by the host's policy.
- Every instance defaults to the hostname as its name (0081).

Idle memory seen on one dev machine: two `termenbox-server` processes at about 8 MB and 12 MB RSS. One reading of unknown processes, not a benchmark.
