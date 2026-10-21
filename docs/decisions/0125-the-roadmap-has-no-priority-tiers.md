---
id: 0125-the-roadmap-has-no-priority-tiers
title: The roadmap has no priority tiers; order comes from WSJF chains, dependencies and the paid-lane rule
status: active
track: process
tags: [roadmap, repo]
date: 2026-10-21
accepted-by: Sergey Strebulaev <s@vivantel.dev>
---

## Decision

Steps carry no `Tier` (P0-P3, Paid). The generated Order table sorts by lane (free before paid, 0093), then by chain score (a step takes the best bundle score of the free steps it unlocks), then by its own WSJF, never before a predecessor; pins override. Scores follow the rubric in `docs/skills/prioritise-the-roadmap-by-wsjf.md` and each step records a one-line reason per number. A `Tier` line in an old step file draws a warning, not an error. Archived steps keep theirs.

## Why

The tiers no longer carried information: P0 had shrunk to the last UX step and the device gate, P1 to P3 were ranked by taste, and a high-WSJF P2 step sat below a low-WSJF P1 one with no stated reason. Dependencies and the paid lane already express what must come first.

## Consequences

- Small, valuable steps and spikes rank high; gating steps rank by their chain score. A step that must go first anyway is pinned or has its T raised, with the reason written in its Score rationale.
- The `Chain` column in the Order table shows the number the sort uses.
- Product tiers (free, Pro, Team, Enterprise; 0089-0093) are unchanged and unrelated.
