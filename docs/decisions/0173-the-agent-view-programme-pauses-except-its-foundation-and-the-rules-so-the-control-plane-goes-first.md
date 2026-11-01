---
id: 0173-the-agent-view-programme-pauses-except-its-foundation-and-the-rules-so-the-control-plane-goes-first
title: The Agent view programme pauses except its foundation (R79) and rules (R80); the control plane goes first
status: active
date: 2026-11-01
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, roadmap, acp, client]
related: [0158, 0159, 0160, 0161, 0163, 0165]
---

## Decision

- **Paused**: R81 (default flip), R82 (reading), R83 (orientation), R84 (inspector and composer). They are marked blocked on this decision and resume after the dogfood milestone M3 of `docs/plans/attention-control-plane.md`, or earlier if the owner says so. Their decisions (0158 to 0160, 0162, 0164) stay active.
- **Kept**: R79, because the template-first rule (0163) needs its media pipeline and fixtures for any new UI, and R80, because contracts and the deputy extend the rules engine. R80 no longer waits for R82 and R84; its Rules UI lives in the Home stream and a Rules sheet, and the inspector's Rules tab returns with R84.
- The per-agent view stays the drill-down from a card.

## Why

The control plane changes the number that matters (0165); the reading experience of one agent does not. Pausing it avoids building two big UI programmes against one shared set of files.
