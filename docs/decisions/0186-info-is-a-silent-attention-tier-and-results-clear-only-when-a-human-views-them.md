---
id: 0186-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them
title: Info is a silent Attention tier, and finished, failed and info items clear only when a human views them
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, notifications, client, server]
---

## Decision

- **Three tiers of Attention item:**

| Tier | Items | Chime, vibration, push | Clears |
|---|---|---|---|
| `needs_you` | an ask | yes | when answered anywhere, or the agent moved on |
| `result` | `finished`, `failed` | yes (`finished` uses the "done" chime) | when a human views it |
| `info` | a background task completed, a stuck-retry notice, an agent note worth reading | no | when a human views it |

  `info` items appear in the inbox and on the card and may count toward the in-app badge; they never make a sound or a push.
- **Viewed means a human action.** A `result` or `info` item clears only when a person views its tab or workspace in the foreground of a visible page on any device (the `ack` message), or dismisses it. No timer clears it, and a new turn does not. After it clears, the tab is `idle`.
- **The ack is global:** viewing on one device clears it on all, as today.
- **`needs_you` is never cleared by looking;** it clears only when the wait is really over.

## Why

A finished result that vanishes by itself is a result the owner may never have read. Silent `info` gives the background completions and stuck-retry notices a place to be read without a second chime for the same work, which keeps the number of interruptions per agent low. The ack rule is the one 0095 already has; this decision states it as human-only and adds the new items to it.
