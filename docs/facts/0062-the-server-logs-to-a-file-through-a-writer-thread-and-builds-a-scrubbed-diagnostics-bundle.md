---
id: 0062
title: The server logs to a file through a writer thread and builds a scrubbed diagnostics bundle on request
status: active
tags: [server, observability, performance, privacy]
date: 2026-10-28
kind: environmental
governed-by: 0142
last-verified: 2026-10-28
---

# Logging and diagnostics as built

Replaces fact 0061 (the state before plan `server-viability-and-logging`). Read from `packages/server/src/{logging,crash,bundle,workspaces}.rs` and measured on 2026-10-28.

| Question | Answer |
|---|---|
| Where do logs go? | stderr, and JSON Lines in `<config-dir>/logs/termenbox.jsonl` (10 MB files; 30 days / 100 MB kept). Only on the server run path. |
| Does a log call wait for the disk? | No. The layer queues the line (2048, about 1 MB) for a `log-writer` thread. A full queue drops the line; the writer then logs `log_lines_dropped` with a count. `logging::flush()` waits up to 1 s; the panic hook and a clean exit call it. |
| What does the file leave out? | Fields named like a secret are `[redacted]`; fields named like session text are `[omitted]` (`SECRET_NAMES`, `PAYLOAD_NAMES`). Values are cut at 2048 characters. |
| What survives a death? | `crash-<ms>.json` from a panic; `run.marker` for an OOM kill or SIGKILL. The next start logs `previous run ended uncleanly` and the owner's app is told (`previous_exit`). |
| What is the bundle? | The owner sends `diagnostics_bundle`; the server answers with parts of at most 8 KiB: log tail (2 MB), newest crash file, previous-exit report, version, channel, mode, health history. The direct transport carries 64 KiB per message, so one reply could not hold it. |
| How is it scrubbed? | JSON lines by field name (the log lists plus session, workspace, window, device, address and key names), the labels, and the ids that are not minted, of the live workspaces as whole tokens (case-sensitive, three characters or more); a minted id (`ws-` and 32 hex) is kept and the word after `session:`/`workspace:` in free text, free text by shape (certificates, tokens, long secrets, IPv4/IPv6, `host:port`, home and config paths, non-termenbox URLs, control characters). |
| Cost? | Idle RSS of the release build 8.7 MiB at the merge-base, 9.7 MiB on the branch (3 runs, 8 s after `/health`); binary 9.8 to 11.0 MB; one more thread. |

## Known gaps

- A workspace made before decision 0146 has its typed name as its id. Its name is scrubbed from free text only while the workspace is live, or after `session:`/`workspace:`; a deleted one in a bare message, or one of two characters or fewer, can still pass. New workspaces have a random id and no typed name in any error, and the `Label` type keeps it out of `{:?}`.
- Two devices that rename the same workspace at once: the last write wins, with no warning to the first.
- While the diagnostics dialog waits, any server `error` is shown as its answer, since replies carry no request id.
- Not measured: the writer's effect on latency, and idle RSS with the writer thread added.
