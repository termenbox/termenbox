---
id: add-a-status-transition
title: Add or change a status transition, event or evidence source
status: active
date: 2026-11-01
tags: [agents, server, client, procedural]
---

# Procedure: Add or change a status transition

Use when a new agent event, a new status, sub-reason or failure cause, a new evidence source, or a status rendering changes. The model is in decisions 0180 to 0186; the machine and its table-driven test live in `packages/server/src/status/` (created by the plan `docs/plans/agent-status-model.md`).

1. Say which status or event changes, and which decision (0180 to 0186) it falls under. If it contradicts one, amend that decision first.
2. Record the evidence from a real run: start the agent in a throwaway tmux (`tmux -L scratch`), never the owner's sessions, and save the hook payloads to `packages/server/testdata/events/`, the transcript lines to `packages/server/testdata/transcripts/` and the screens to `packages/server/testdata/screens/`. Note the agent version and the timing of each source (hook, transcript, screen) relative to the first one.
3. Teach the adapter to emit the typed event (never a status). Add a fixture test that maps the recorded payload to the event.
4. Add the row to the transition test: from-state, event, to-state, condition, and the alert it must or must not produce. Include the case where the event arrives late, twice or out of order.
5. Add or extend an edge-case scenario of the catalogue in the plan, run on the fake-Claude harness, with the cross-surface helper asserting chip, tab strip, sidebar, Home, title, badge and inbox.
6. If a surface renders the status, change the one status store and its selectors, not a surface; check guardrail `no-alert-without-a-visible-status`.
7. Run `cargo test -p termenbox-server status` and the status e2e shard; update fact 0071 or its successor if the pipeline's shape changed.
