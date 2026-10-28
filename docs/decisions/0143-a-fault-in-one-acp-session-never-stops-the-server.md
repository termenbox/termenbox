---
id: 0143-a-fault-in-one-acp-session-never-stops-the-server
title: A fault in one ACP session never stops the server, and a crashing server restarts with backoff and a safe mode
status: active
date: 2026-10-28
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [server, agents, architecture, performance]
fitness-functions: [a test panics inside one session's task and the other sessions keep answering; a test restarts after two unclean exits and the live workspace is restored stopped, not started]
---

## Decision

- **Supervised sessions.** Each ACP session's tasks (reader, writer, event pump, agent child) run under one supervisor. A panic, a closed pipe or an agent-child exit marks that workspace `failed` with a reason and a one-tap restart; no other workspace is touched. Shared state behind a `std::sync` lock is `parking_lot` or non-poisoning, so one panic cannot poison it for the rest.
- **Restart policy.** Installed units use `Restart=always`, `RestartSec=2` with a start-rate limit that ends in a visible failed state rather than a silent loop; the compose file keeps `unless-stopped`.
- **Safe mode.** The run marker (0142) counts consecutive unclean exits. A workspace that was live at the last two unclean exits is restored stopped, with a notice saying why, instead of resuming and possibly crashing the server again.
- **Health that means something.** `/health` also fails when the ACP supervisor or the event loop has not ticked for 30 s, so Docker's check and external monitors see a hang.

## Why

The owner's goal is maximum viability: one bad agent or turn must cost one workspace, not every workspace. Unwinding panics already isolate tokio tasks (fact 0061); this makes isolation a stated, tested property and covers the cases unwinding does not (poisoned locks, repeated crash on restore, hangs).

## Rejected

| Option | Why not |
|---|---|
| Process per session | Strongest, but RAM per process against the 512 MB envelope and a large refactor; revisit if supervised tasks prove insufficient. |
| Isolation without restart policy | A crash loop would still burn the box and hide the cause. |
