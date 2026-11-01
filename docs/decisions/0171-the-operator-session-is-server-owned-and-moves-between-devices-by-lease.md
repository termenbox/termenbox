---
id: 0171-the-operator-session-is-server-owned-and-moves-between-devices-by-lease
title: The operator session (cursor, open card, drafts, window, mode) is owned by the conductor and moves between devices by lease; answers are idempotent by card id
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, client, mobile, server, protocol]
related: [0095, 0100, 0169, 0170, 0175]
---

## Decision

- **One operator session per actor** lives on the conductor: stream cursor, the open card, drafts (typed and dictated), the sliding window (0174), mode (focus, commute, desk) and last-seen cursors. It is the thing that "proceeds from the same point".
- **A device attaches and resumes at the same card within one second.** Handoff is an explicit "continue here" (one tap, QR or nearby). The previous device goes passive after a lease, so two devices never answer the same card; a passive device shows the stream read-only with a "take over" action.
- **Answers are idempotent by card id.** The first answer wins; any other device shows "answered on <device>". Drafts are kept per card, not per device.
- **The operator session survives every device being off.** Rules, contracts and the deputy (0172, 0173) run on the conductor and use the same cursor model, so what the human sees on return is a digest (0168) from their cursor.
- **Every action records an actor id**, separate from the owner id (`operator-actions-carry-an-actor-id-apart-from-the-owner`).

## Why

Attention is server-owned already (0095), but the operator's own position is per device, so changing device loses the place. A leased single session is simple, conflict-free for one operator, and is the base on which team mode (0175) adds more actors.

## Not chosen

- Per-device state with synced drafts (the place is approximate), and all devices live at once (needs conflict handling for little gain with one operator).
