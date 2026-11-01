---
id: 0185-failures-are-classed-by-whether-the-turn-ended
title: A failure is recovered, halting, crashed or interrupted, decided by what the agent did next and not by the error
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, server, client]
---

## Decision

The same error can be harmless or fatal, so the class depends on whether the turn ended.

| Class | Rule | Status | Alert |
|---|---|---|---|
| **Recovered** | The agent continues: a wrong tool name it corrects, a retried 429, a tool error it works around | stays `working`; sub-reason `retrying` while it backs off; the detail view counts "N recovered errors" | none |
| **Halting** | The turn ended on an error: repeated 429 then stop, quota, auth, API error, context overflow (`StopFailure`, or an error stop in the transcript) | `failed`, cause `rate_limit` (with a reset time when known), `auth`, `api_error` or `context` | yes, like `finished` |
| **Crashed** | The agent process is gone with a turn open | `failed`, cause `crashed` | yes |
| **Interrupted** | The human pressed Esc | `idle`, not failed | none |

- A `failed` item follows the lifecycle of `finished`: it stays until a human views it ([[0186-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them]]), then the tab is `idle`.
- A session in `retrying` for longer than a threshold raises a silent `info` item ("stuck retrying"), so a retry loop cannot look like work forever. The threshold is **TBD**, measured in plan step A.3.
- Recovered failures are never an alert and never a status of their own; they are history.

## Why

A 429 is the case that shows why error type cannot decide: Claude retries it and carries on, or gives up after N attempts. Only the second needs the human. Classing by the outcome keeps the alert rate honest at scale while still telling the owner that a stopped agent stopped for a reason they may fix, such as a reset time.
