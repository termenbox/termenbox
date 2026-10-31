---
id: 0064
title: The updater records what it knows in update-state.json and the server only reads it
status: active
kind: observation
governed-by: 0154-a-new-release-waits-a-grace-period-for-the-owner-then-installs-or-is-skipped
tags: [server, updater, observability]
date: 2026-10-31
last-verified: 2026-10-31
---

# The updater's state file

## What is true

- **One writer.** The helper (root or user) writes `update-state.json` next to `update-status.json`, which is still written beside it for one release. The server reads it (`update_state::State::load`) and never writes it; the owner's postpone and skip reach the helper as fixed request files.
- **Where it lives.** In the parent of the requests directory, which is the server's state directory for both machine install kinds; in a container install it is `~/.config/termenbox` on the container's volume (`$ROOT/home/.config/termenbox` on the host), because the installer points `TERMENBOX_REQUESTS_DIR` there. Read with a size cap that refuses links, and anything unreadable or from another schema reads as a fresh state.
- **It keeps one result.** `last` holds the latest outcome (updated, rolled back, failed), not a history. A failed install is pinned in `skipped` like a rolled-back one, and a grace deadline puts the phase at `scheduled` with `deadline` set. A longer record is the helper's audit lines (`update`, `update_postpone`, `update_skip`, `update_grace_expired`: trigger, outcome kind and release, never the rollback reason) in its journal.
- **Who reads it.** The app's `update_state` message (polled every 20 s per connection), the push watcher (every 60 s, with nobody connected; kind and server name only), and the diagnostics bundle, which prints it scrubbed.
- **Cost.** One small file read per poll; no network.
