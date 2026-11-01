---
id: 0187-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up
title: Agent status comes from one server-owned machine per agent session, and the workspace is a pure roll-up of its tabs
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, server, protocol, client]
---

## Decision

- **One machine per agent session.** The unit of truth is one agent session in one tab. A single server-owned state machine takes typed events from every evidence source ([[0189-evidence-ranks-hook-transcript-screen-scan]]) and outputs the status below. Tab strip, sidebar, Home, card, title, badge, inbox, chime and push all read that one output. No other state is kept: the workspace ring, `TabStatus` and the client's `derived()` mix are retired.
- **Statuses.** What the user sees is exactly one of:

| Status | Meaning | Sub-reason while it holds | Carries |
|---|---|---|---|
| `working` | Anything is in progress except waiting on the human ([[0188-working-means-in-progress-except-human-input-and-background-work-is-a-badge]]) | inferring, tool (name), subagent, compacting, retrying | task title, running time |
| `needs_you` | The agent waits on the human | permission, question, plan, trust, elicitation | question text and options, filled in later ([[0190-asks-raise-at-once-and-are-enriched-later]]) |
| `finished` | A turn ended well and its result is unseen | none | task title, duration, outcome |
| `failed` | A turn ended badly and its result is unseen ([[0192-failures-are-classed-by-whether-the-turn-ended]]) | cause class | task title, error class |
| `idle` | An agent is present, nothing runs, nothing is unseen | none | `bg: N` badge when background work runs |
| `unknown` | No agent, or no evidence | none | nothing: no text and no badge is drawn |

  `info` is a silent tier of Attention items, not a status ([[0193-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them]]).
- **The workspace is derived.** Its status is the roll-up of its tabs, never stored or set on its own, so the two levels cannot disagree. Order, worst first: `needs_you > failed > finished > idle > working`. `unknown` tabs are ignored. Idle outranks working because an idle agent is capacity the owner is not using, while a working one is the expected state. The roll-up carries counts ("2 working, 1 asks") and the tab of the first ask, so one tap opens it.
- **The Activity and Attention axes of [[0095-agent-status-is-activity-plus-attention]] stay as the wire shape.** The machine fills them; it adds the sub-reason, the failure cause, the `bg` count and the `info` tier. The flat `AgentState` stops being a status source.

## Why

Today the chime keys off workspace Attention, the chip off per-tab flat states or Activity, and Home off a mix of both (fact 0074). Three paths mean three answers: a chime with no visible change, a "Needs you" chip with no alert, a "working" tab that is not. One machine with one output removes the disagreement instead of patching each case. The roll-up order replaces the aggregate rule in `docs/plans/workspace-tabs.md`.
