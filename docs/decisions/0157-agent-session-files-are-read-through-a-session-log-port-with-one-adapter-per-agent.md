---
id: 0157-agent-session-files-are-read-through-a-session-log-port-with-one-adapter-per-agent
title: Agent session files are read through a SessionLog port with one adapter per agent, and AcpView reads one unified log
status: active
date: 2026-10-31
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, architecture, server, client, performance]
related: [0156-search-covers-every-known-agent-session-from-the-agents-own-files, 0054-server-owns-workspaces-through-a-backend-trait, 0136-a-workspace-exposes-its-agent-windows-over-acp-v1-plus-an-extension-registry, 0139-control-bytes-and-events-travel-on-separate-channels-and-events-replay-from-a-sequence-log]
---

## Decision

- **A `SessionLog` port** in the server, in the style of `WorkspaceBackend` (0054), with one adapter per agent. It offers `search(query, scope)`, `window(session, anchor, before, after)` and `page(session, cursor, direction)`. An adapter reads that agent's files and returns the unified entries `AcpView` already renders (`user`, `message`, `thought`, `tool`, `plan`, `notice`); a record it cannot map becomes a `notice`, never an error. An adapter that cannot read a file turns off search for that agent only.
- **First adapters: Claude Code and Codex.** The other agents wait until their file formats are measured (fact 0065 records what is known).
- **v1 is search plus a window around a hit.** `page` is part of the port so endless scroll-up can follow without a redesign; it is not built in v1.
- **The live event log stays authoritative** (0139). The file supplies only entries older than the ring's first entry. The join key is the agent's own turn or message id, never a timestamp, because the file can lag the stream.
- **Cursors are opaque and carry the file's fingerprint** (inode, size, mtime). A rewritten or compacted file invalidates the cursor and the client re-runs the search; it never shows shifted content.
- **The client keeps older entries apart from the reducer.** `acpStore` keeps its 400-entry cap; entries from the port sit in a separate history block above the live list, windowed and evicted so memory stays bounded, and a prepend is scroll-anchored so the view does not jump. A hit's turn is loaded and rendered before the scroll runs.
- **Replies fit the transport:** at most 48 KiB, the cap `history.rs` already uses (`REPLY_BYTES`).
- **Performance is part of the contract.** The scan reads with `pread` into one reusable buffer and finds candidates with `memchr::memmem` over a pre-folded needle, so non-matching lines are neither allocated nor parsed. JSON is parsed only for lines that match; those do allocate, and so do scrubbing and the reply. Per-file byte-offset indexes are built lazily and incrementally (the files are append-only) and kept in a small LRU. `mmap` is not used: a file truncated while mapped raises SIGBUS. CI runs a benchmark on a large fixture with budgets for time and peak RSS.
- **Contract tests:** every adapter runs recorded fixtures (as `testdata/acp/` does) through one shared suite, including a file that is truncated, torn at the last line, and in an unknown version.

## Why

Formats we do not control change. A port with a fail-soft adapter per agent confines that change to one module and gives the view one model, so the view needs no per-agent code. Keeping the ring authoritative avoids two sources of truth for the same turn, and a fingerprinted cursor is the standard defence against paging a file that moves.

## Rejected

| Option | Why not |
|---|---|
| Reload through ACP `session/load` | Spawns the agent, replays the whole conversation for one hit |
| Raise or seed the event-log ring | Changes the 0139 memory budget and still needs the file |
| Line-number or offset paging | Shifts when the file is rewritten |
| `mmap` the files | SIGBUS on truncation; `pread` is as fast for a forward scan |
| Full lazy scroll-up in v1 | A much larger client change; no search needs it |

## Consequences

- The Claude JSONL records rewinds and forks as a tree; picking the active branch is verified against fixtures in the adapter step, not assumed.
- "Zero-copy" holds for the scan; hits and replies copy. This is stated so a benchmark, not a slogan, is the check.
