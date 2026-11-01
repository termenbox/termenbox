---
id: 0163-agent-view-features-are-built-template-first-then-tested-styled-and-filmed-before-they-are-wired
title: Agent view features are built template-first, then tested, styled and filmed with a generated manual before they are wired to the real stream
status: active
date: 2026-11-01
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, ux, ci, process]
related: [0135, 0159, 0160, 0161, 0162, 0164]
---

## Decision

- **Order for every Agent view slice:** (1) **Templates**: static components that take props, shown on a probe page from fixture streams, with no store, wire or server; (2) **Tests** that cover the templates (pure logic, component, phone and desktop probe e2e, the design-rule audit); (3) **Styling** on the roles and skin tokens, tests re-run; (4) **Media**: a scripted scenario renders a captioned video and a generated manual for the slice, from the same fixtures; (5) **Owner review** of the video and manual; (6) **Wiring** to the real stream, store and server, where the same scenario runs again on live data.
- **The manual is generated, not hand-written.** The scenario steps carry the captions; the generator writes the guide section and the screenshots from them, and a check fails when the committed guide differs from what the scenarios generate. Videos and manual are labelled as scripted data, not a real device or agent.
- **Fixtures are the contract.** The fixture streams (a long run, an error, an open permission, a plan, a todo reported as a tool, unknown kinds, a 2,000-entry stress, an epoch change) are shared by tests, the probe page, the video and the manual. A wired feature must pass the same scenarios.
- **The media pipeline comes first.** Its first slice is built and working on the first template before the other slices' templates, so every later slice gets its video and manual by adding a scenario.
- **Reuse before building.** The pipeline builds on `packages/client/e2e/video/` and the audit in `docs/plans/assets/ux/audit.mjs`; when R73's scenario runner (`e2e/ux/scenarios/`) lands, scenarios move to it unchanged.

## Why

The owner wants the look, the flow and the documentation settled and reviewable before the stream, store and server are touched. Templates on fixtures are cheap to change and test; the video and manual make the review concrete and keep the documentation true.

## Not chosen

- Wiring first and styling later (rework on every pass), hand-written manuals (drift), and one video at the end (feedback arrives after the work is built).

## Consequences

- Each slice has a template step, a test step, a style step and a scenario before its wiring step, and the plan states that order in its `Needs`.
- Slices that touch the server (rules, queue, credential detection) still start with their client templates and their wire contract in the fixtures.
