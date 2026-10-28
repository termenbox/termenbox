---
id: 0142-the-server-keeps-its-own-jsonl-logs-and-a-crash-record
title: The server keeps its own JSONL logs and a crash record
status: active
date: 2026-10-28
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [server, observability, security, deployment]
fitness-functions: [a test kills a server process uncleanly and the next start logs "previous run ended uncleanly"; a log sanitizer test injects a secret and an ACP payload and finds neither in the file]
---

## Decision

- **Own log files.** Besides stderr, the server writes JSON Lines (one object per line: `ts, level, target, event, fields`) to `<config-dir>/logs/termenbox.jsonl`, rotated by size and age (the existing cap: 100 MB total, 30 days). It works the same under systemd, Docker and a bare run; under Docker the config dir is the volume, so logs outlive the container.
- **Level.** `info` by default; `TERMENBOX_LOG` (an `EnvFilter` string) overrides `RUST_LOG`. ACP and terminal payloads are never logged at any level (guardrail `audit-logging-no-secrets`).
- **Panic hook.** A panic writes its message, location, thread and backtrace to the log and to `<config-dir>/logs/crash-<ts>.json`, flushed before unwinding goes on.
- **Run marker.** `<config-dir>/run.marker` is created at start and removed on a clean exit. If it is found at start, the server logs `previous run ended uncleanly` with the last marker timestamp, the last health sample (RSS, event-log bytes) and the newest crash file if any. That is how an OOM kill or SIGKILL becomes visible after the fact.
- **Errors, not silence.** A failure to open the log directory is a warning on stderr; the server still starts.

## Why

The 2026-10-28 incident left no log (fact 0061). journald and container logs are the host's, with retention the owner does not control; a file under the config dir is ours and can be bundled (0144). A run marker needs no second process, which matters under the 512 MB budget.

## Rejected

| Option | Why not |
|---|---|
| Rely on journald / docker logs | It is what failed. |
| In-memory ring dumped on crash | Lost on OOM kill and SIGKILL. |
| Panic hook only | Misses OOM and signals, the likeliest deaths on a small box. |
| External supervisor process | A second process and its RAM; the marker gives the same detection. |
