---
id: 0145-logging-never-waits-for-the-disk
title: Logging never waits for the disk
status: draft
date: 2026-10-28
track: process
tags: [server, observability, performance]
fitness-functions: [a burst into a one-line queue returns at once and kept plus reported-lost lines equal lines sent]
---

## Decision

- A log call queues its line for a writer thread and returns. A full queue drops the line and counts it; the writer logs the count (`log_lines_dropped`).
- The panic hook and a clean exit call `flush()`, which waits at most one second, so the lines before a death reach the file when the disk is healthy.
- The queue is bounded (2048 lines). Losing a line is preferred to stalling a runtime worker or growing memory (`resource-constraints`).

## Why

The first design wrote under a mutex on the calling thread, so a slow or full disk, a rotation or a prune could stall every task that logged. On 1 vCPU with few workers that can look like the hang the log is meant to explain.

## Rejected

| Option | Why not |
|---|---|
| Write under a mutex on the calling thread | A slow disk stalls the runtime. |
| Unbounded channel | Memory grows without limit when the disk stops. |
| fsync every line | Latency for a log that already has a crash file and a run marker. |

Awaiting sign-off by the owning domain; set `status: active` and `accepted-by` then.
