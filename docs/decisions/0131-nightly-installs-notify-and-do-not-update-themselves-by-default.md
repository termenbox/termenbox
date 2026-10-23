---
id: 0131-nightly-installs-notify-and-do-not-update-themselves-by-default
title: Each server has an update policy, and a nightly install notifies instead of updating itself by default
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [release, updater, installer]
date: 2026-10-23
---

## Decision

- **Per-server policy**, root-owned, in `update.toml` beside the journal (`/var/lib/termenbox-root` or the user's `~/.local/state/termenbox`), written by the installer: `[update] channel = "stable|beta|nightly"`, `mode = "auto|notify|hold"`, optional `maintenance_window = "03:00-04:00"` (local time, may wrap midnight). Defaults: `stable` and `auto`. A file that does not parse stops the updater; a typo never reads as `auto`.
- **A nightly install defaults to `notify`**; `--auto-update` on the installer opts into `auto`. Stable and beta default to `auto`. An unattended daily restart on a server that only wanted to try nightly is a surprise, and a nightly can break a session the owner is in.
- **Modes.** `auto`: the daily check installs, inside the maintenance window if one is set, and never across a major version. `notify`: the daily check records the available version in `update-status.json` in the server's state directory; the owner's `UpdateNow` request installs it. `hold`: nothing happens, whoever asks, and the manifest is not even fetched. `UpdateNow` ignores the window and counts as the owner's confirmation of a major version.
- **Channel rules.** A stable server never installs beta or nightly; a beta server installs beta and stable (through the `channel-beta` alias); a nightly server installs only nightly. The configured channel, not the running binary, selects the trust set of decision 0130, so moving a server to another channel is editing `update.toml` or re-running the installer with `--channel`.
- **Changing the channel is a host-level act**, never a web action: the app cannot write `update.toml`.

## Why

Auto-update on one stream suits stable. Beta and nightly users want to watch, hold or choose when. Keeping the policy in a root-owned file keeps a compromised unprivileged server (or a hijacked browser session) from moving a host to a less reviewed stream.

Not chosen: auto for nightly with an opt-out (surprise restarts); a channel switch in the app (a web session would decide what runs as root).

## Consequences

- Refines decision 0063 (auto-update is opt-in in host mode) and 0076 (the updater): the owner's policy decides, the signature and rollback rules are unchanged.
- The client notice that reads `update-status.json` is step R70.
