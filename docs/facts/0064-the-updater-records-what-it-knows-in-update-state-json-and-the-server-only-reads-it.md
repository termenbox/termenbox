---
id: 0064
title: The updater records what it knows in update-state.json and the server only reads it
status: active
kind: observation
governed-by: 0153-refine-the-update-policy-with-a-grace-period-and-per-release-owner-actions
tags: [server, updates, observability]
date: 2026-10-31
last-verified: 2026-10-31
---

# The updater's state file

## What is true

- **One writer.** The helper (root or user) writes `update-state.json` next to `update-status.json`, which is still written beside it for one release. The server reads it (`update_state::State::load`) and never writes it; the owner's postpone and skip reach the helper as fixed request files.
- **Where it lives.** In the parent of the requests directory, which is the server's state directory for both install kinds. Read with a size cap that refuses links, and anything unreadable or from another schema reads as a fresh state.
- **It keeps one result.** `last` holds the latest outcome (updated, rolled back, failed), not a history. A longer record is the helper's audit lines (`update`, `update_postpone`, `update_skip`: trigger, outcome kind and release, never the rollback reason) in its journal.
- **Who reads it.** The app's `update_state` message (polled every 20 s per connection), the push watcher (every 60 s, with nobody connected; kind and server name only), and the diagnostics bundle, which prints it scrubbed.
- **Cost.** One small file read per poll; no network.
