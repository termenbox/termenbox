---
id: 0113-host-me-needs-no-root
title: The install for the current user needs no root; anything that does is optional and printed, not run
status: active
amends: 0078-install-for-the-current-user
date: 2026-10-17
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [deployment, security, server]
---

## Decision

- `host-me` runs entirely as the user who ran the command, with no `sudo`. It is a **user** systemd service (`systemctl --user`): the binary in `~/.local/bin`, state in `~/.config/termenbox`, the change journal in `~/.local/state/termenbox`, the unit files in `~/.config/systemd/user`. `host-user` and `host` keep the system service and still need root (0075).
- The signed daily update check and the request handler (0076, 0077) are user units of the same user, run as `termenbox-server --root-helper … --user-helper`. That helper refuses to run as root, restarts the service with `systemctl --user`, and touches only that user's files. Revert and uninstall run as that user.
- The journal (0067) covers it: a user unit is removed only from `~/.config/systemd/user` of the user running the revert, and lingering the installer switched on is recorded and switched off again.
- Starting at boot needs lingering. The installer tries `loginctl enable-linger` for the user (systemd's default policy allows a user to do that to themselves). If that is denied, the install still succeeds, says the server starts at login only, and prints the one command to run as root.
- Everything else that needs root is optional and **printed, never run**: that command, opening the UDP port when ufw or firewalld is active. The installer does not call `sudo` in this mode.
- `tmux` must already be installed. When it is missing the installer stops before changing anything and prints the install command (R23 would remove this limit).
- A machine that already has the system-service install from 0078 keeps working: re-running the installer on it takes the old route. Add Server marks each host option as needing root or not.

## Why

Running as the owner already works without root: the server binds a high UDP port, keeps state in the home, and the signed update and uninstall only need the user's own files. Asking for `sudo` was for a boot-time system unit, a firewall rule and a package that most hosts either have or can do in one printed line. Making root optional removes the adoption friction of a shared or locked-down account, keeps the server from ever holding root (guardrail `unprivileged-server-never-holds-root`), and leaves `host-user` and `host` for the cases that want a system service.

Amended by [[0118-user-installs-use-a-reserved-udp-range-and-say-what-needs-root-first]]: the server binds its port from a reserved range, and the installer says what needs root before it changes anything.
