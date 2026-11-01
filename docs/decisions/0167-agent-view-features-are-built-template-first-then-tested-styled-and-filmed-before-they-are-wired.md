---
id: 0167-agent-view-features-are-built-template-first-then-tested-styled-and-filmed-before-they-are-wired
title: Agent view features are built template-first, then tested, styled and filmed on the shared recorder before they are wired to the real stream
status: active
date: 2026-11-01
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, ux, ci, process, docs]
related: [0135, 0158, 0159, 0160, 0161, 0163, 0164, 0165, 0168]
---

## Decision

- **Order for every Agent view slice:** (1) **Templates**: static components that take props, shown on a probe page from fixture streams, with no store, wire or server; (2) **Tests** that cover the templates (pure logic, component, phone and desktop probe e2e, the design-rule audit); (3) **Styling** on the roles and skin tokens, tests re-run; (4) **Preview video**: a case on the shared recorder films the styled templates; (5) **Owner review** of that video; (6) **Wiring** to the real stream, store and server, where the same case runs again on live data and that video is picked for the docs.
- **One recorder, one pipeline, no second one.** Cases live in `packages/client/e2e/video/cases.mjs` and render with `render.mjs` (decision 0158). A case declares its `title`, `group`, `page`, `server`, `inputs` and `steps` once (decision 0161); `h.step` performs them in order and the case ends with an assertion of its final state. Review is the `docs-media.yaml` render and the preview page; a pick lands the media in `docs/media/` through a pull request. Agent view adds no recorder, workflow, store or generator of its own.
- **Template videos are for review only.** They show scripted fixtures, not the product, so they use the group `agent-template`, are rendered for the owner's review and are never ticked for the docs. The live case of the same slice (same steps, real stream against the scripted fake agent) is the one picked.
- **The manual is the existing docs.** The reference is a subsection of `docs/guide/workspaces.md`, written by hand and held in step by the docs checks of decision 0159 (links, anchors, `<!-- media:<id> -->` markers, drift). The teaching chapters belong to the course's Master part (decision 0160), whose numbered steps are generated from the same live cases (decision 0161). The `inputs` of each case list the slice's component files, so a later change flags its video as stale.
- **Fixtures are the contract.** The fixture streams (a long run, an error, an open permission, a plan, a todo reported as a tool, unknown kinds, a 2,000-entry stress, an epoch change, an away stretch, a queue) are shared by the tests, the probe page and the cases. A wired feature must pass the same cases.
- **Media comes early.** The first slice's template case is built right after the fixtures, so every later slice only adds cases.

- **Amendment 2026-11-01 (owner): film last.** Owner review before wiring is on the probe page (the built `e2e/<slice>.html`, served locally, with the audit's screenshots); a template video is no longer a gate. Cases are declared with their slice and rendered, reviewed and picked for the docs in one final slice of the plan, after the docs-media workflow settles. The one-recorder rule is unchanged.

## Why

The owner wants the look, the flow and the documentation settled and reviewable before the stream, store and server are touched. Templates on fixtures are cheap to change and test; a video on the shared recorder makes the review concrete without a second pipeline to keep alive, and the live case keeps the documentation true.

## Not chosen

- Wiring first and styling later (rework on every pass), a separate Agent view video and manual generator (a duplicate of decisions 0158 to 0161), hand-written teaching steps (nothing catches a stale label), and one video at the end (feedback arrives after the work is built).

## Consequences

- Each slice has a template step, a test step, a style step and a template case before its wiring step, and the plan states that order in its `Needs`.
- Slices that touch the server (rules, queue, credential detection) still start with their client templates and their wire contract in the fixtures.
- The probe page is a start option of a case, next to the scripted and real servers; its code is test support, not shipped.
