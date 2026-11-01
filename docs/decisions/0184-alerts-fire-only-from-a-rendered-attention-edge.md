---
id: 0184-alerts-fire-only-from-a-rendered-attention-edge
title: A sound, vibration, push, badge or inbox item fires only on a rising edge of a server Attention item, and the client plays it from the renderer that applied the same snapshot
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, notifications, client]
---

## Decision

- **One trigger for every channel:** chime, vibration, push, app badge, tab-title count and inbox fire only on a rising edge of a server-owned Attention item of the tier `needs_you` or `result`. `info` items are silent ([[0186-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them]]).
- **Rendered first.** The server sends the status and the Attention item in one snapshot frame. The client applies the frame to the one status store, renders it, and only then plays the alert from the same code path. No separate listener, timer or alerter reads Attention on its own.
- **No Attention, no alert.** A status derived from the screen or scan alone never alerts, and is shown as a hint without the alert-bearing styling.
- **One edge, one alert,** replaced by a later edge on the same session, as in 0095. A grace of about 5 s before a push, suppressed when a device shows the session, is unchanged.

## Why

The chime runs from the client alerter on workspace-level Attention while the chip reads per-tab or Activity state, so a chime can sound with no visible change, and a "Needs you" chip can show with no alert (fact 0071). Playing the alert from the renderer that just applied the snapshot makes "sound without a status change" impossible by construction, and guardrail `no-alert-without-a-visible-status` checks it.
