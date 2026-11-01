---
id: 0095-agent-status-is-activity-plus-attention
title: Agent status is Activity plus Attention, with the current task, and notifications fire on Attention
status: active
date: 2026-10-15
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
supersedes: [0061]
amended-by: [0187-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up, 0188-working-means-in-progress-except-human-input-and-background-work-is-a-badge, 0189-evidence-ranks-hook-transcript-screen-scan, 0190-asks-raise-at-once-and-are-enriched-later, 0191-alerts-fire-only-from-a-rendered-attention-edge, 0192-failures-are-classed-by-whether-the-turn-ended, 0193-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them]
tags: [agents, notifications, pwa, protocol, roadmap]
---

## Decision

- **Two axes replace the flat `idle | working | needs_input | done | error` state.**
  - **Activity** is what the agent does now: `working | blocked | idle | unknown`. The server derives it per session from the freshest signal of the highest-precedence live source: hook, then screen, then scan. A source quiet for 60 s while `working` decays to `unknown`; this replaces the `stale` flag. Codex has no hook or adapter until R21, so it has scan state only.
  - **Attention** is a durable, server-owned item: `needs_input(prompt) | finished_unseen | failed`, or none. A transition raises it. `needs_input` clears only when the prompt is really gone (answered anywhere, or the agent moved on); looking does not clear it. `finished_unseen` and `failed` clear when any device shows that session in the foreground (an `ack` message), or on an explicit dismiss. Scan and screen may only hint at Attention, with low confidence, and a hint never triggers a push.
- **Activity carries the current task:** `task { title, started_at, source }`. Running time is computed in the client from `started_at` and the server clock offset. Title with a hook: the first line of the prompt (`UserPromptSubmit`), replaced by the in-progress plan item when there is one. Without a hook there is no task title: tmux's `pane_title` was measured and does not track the task (fact 0036). `task` is set on `working`, kept while `blocked`, dropped on `idle`; a finished task's title and duration stay on its `finished_unseen` item.
- **Notifications fire on a rising edge of Attention only.** Web Push (VAPID, no paid service, installed PWA) is sent when no connected device shows the session in the foreground, after a grace of about 5 s so a quick look suppresses it. One push per edge; a new edge on the same session replaces the older notification. The payload is "session X needs you" and nothing from the agent (guardrail `notifications-carry-no-agent-text`). The inbox (200 items), the tab title count, the app badge and the banner all read the same Attention items. Fallback without push: in-app inbox and toasts.
- Agent text (task title, prompt, summaries) stays untrusted: control characters stripped, capped (title 120), scrubbed, rendered as text, never audit-logged. Detection and Attention changes are audit-logged by session, kind and source only (0061 kept).
- `tool`, `plan`, `prompt` and `message` stay detail events on the status channel (0096), sent in full only for sessions a client has open or listed; other sessions get Activity, `task` and Attention.

## Why

`done` was a moment dressed as a state, and "does this need the user?" was recomputed in the client from a state plus confidence, with no read state across devices and no clear trigger for push. Making Attention a separate, acknowledgeable item gives push one trigger, the inbox one source and every device the same answer. Precedence with honest decay stops a low-confidence scan from overwriting a hook's `needs_input` and stops `working` from lasting forever. Replaces [[0061-agent-state-and-push]]; hook consent stays with [[0079-agent-hooks-are-opt-in]]; the lanes are in [[0096-terminal-and-status-use-two-data-channels]]. herdr is no longer a source ([[0097-herdr-is-removed]]).
