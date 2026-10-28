---
id: 0061
title: The server logs to stderr only and leaves no record of its own death
status: active
tags: [server, observability, deployment]
date: 2026-10-28
kind: environmental
governed-by: 0142
last-verified: 2026-10-28
---

# The server logs to stderr only and leaves no record of its own death

Read from the code on 2026-10-28 (`packages/server/src/main.rs`, `Cargo.toml`, `docker/`, `packages/client/installer/install.sh`).

| Question | Answer |
|---|---|
| Where do logs go? | `tracing_subscriber::fmt()` to stderr, plain text, filter from `RUST_LOG` (default `info`). No file, no JSON. Retention is whatever journald or the container log driver does. |
| Does the audit-logging guardrail hold? | No. `audit-logging-no-secrets` asks for JSON lines, 30 days / 100 MB and a redaction layer; none is implemented. |
| What happens on a panic? | The release profile unwinds (no `panic = "abort"`), so a panic in a tokio task stays in that task. There is no panic hook, so the message goes to stderr only. A panic on the main task, an OOM kill or a signal leaves nothing. |
| What restarts it? | systemd units use `Restart=on-failure`; Docker uses `restart: unless-stopped` and a `/health` check every 60 s. No crash-loop backoff or safe mode. |
| What survives a restart? | ACP workspaces are restored from `acp-workspaces.json` with `session/resume` or `session/load`; a turn that was running gets a notice (plan `acp-session-modes`, choice E). |
| What does a user report carry? | The client's scrubbed text (decisions 0074, 0086). Nothing from the server's logs. |

## Incident of 2026-10-28

While working in an ACP workspace the owner's termenbox server went down. No log existed afterwards, so the cause is unknown (panic, OOM kill and agent-child fault are all possible). See `docs/incidents/2026-10-28-server-down-in-acp-mode.md`.
