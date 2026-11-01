---
id: build-an-agent-view-slice-template-first
title: Build an Agent view slice template-first
status: active
date: 2026-11-01
tags: [client, acp, ux, ci, procedural]
---

# Procedure: Build an Agent view slice template-first

Use for each slice of `docs/plans/agent-view-programme.md` (decision 0163; guardrail `ui-features-ship-templates-tests-and-media-before-wiring`).

1. **Fixtures.** Add or extend a fixture stream in `packages/client/src/lib/agentViewFixtures.ts` for the cases the slice must show (include an unknown tool kind, an open permission and a long stream when relevant).
2. **Template.** Build the slice as components that take props only, in `packages/client/src/`. They import no store, no wire code and no server. Show them on the probe page `packages/client/e2e/agentview.html`, at every level and on phone and desktop.
3. **Tests.** Cover the pure logic, then the components (names, 44 pt targets, no glyph-free state), then the probe e2e and the audit (`docs/plans/assets/ux/audit.mjs`). Do not style yet; the tests must pass on the plain template.
4. **Style.** Apply roles and tokens (decision 0162), only DR5 sizes. Re-run every test and the audit in every skin.
5. **Scenario.** Add a case to `packages/client/e2e/video/cases.mjs` with a caption per step a user should read (plain words; no agent text, no real names). Render it: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/video/render.mjs --case <slice> --profile phone'`, and the manual: `node e2e/video/manual.mjs --case <slice>`.
6. **Review.** Show the owner the video and manual. Change the template, not the wiring, until accepted.
7. **Wire.** Connect the components to the real stream, store and server. Re-run the same scenario with `?source=live` against the fake agent, attach the new video, and run the drift check.
