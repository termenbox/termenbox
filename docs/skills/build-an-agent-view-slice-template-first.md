---
id: build-an-agent-view-slice-template-first
title: Build an Agent view slice template-first
status: active
date: 2026-11-01
tags: [client, acp, ux, ci, docs, procedural]
---

# Procedure: Build an Agent view slice template-first

Use for each slice of `docs/plans/agent-view-programme.md` (decision 0167; guardrail `ui-features-ship-templates-tests-and-media-before-wiring`). The recording and review steps are the existing ones: `render-a-feature-video` and `review-and-publish-docs-media`.

1. **Fixtures.** Add or extend a fixture stream in `packages/client/src/lib/agentViewFixtures.ts` for the cases the slice must show (include an unknown tool kind, an open permission and a long stream when relevant).
2. **Template.** Build the slice as components that take props only, in `packages/client/src/`. They import no store, no wire code and no server. Show them on the probe page `packages/client/e2e/agentview.html`, at every level and on phone and desktop.
3. **Tests.** Cover the pure logic, then the components (names, 44 pt targets, no glyph-free state), then the probe e2e and the audit (`docs/plans/assets/ux/audit.mjs`). Do not style yet; the tests must pass on the plain template.
4. **Style.** Apply roles and tokens (decision 0166), only DR5 sizes. Re-run every test and the audit in every skin.
5. **Template case.** Add a case `<id>-template` (group `agent-template`) to `packages/client/e2e/video/cases.mjs` with `steps` (id, caption, label) declared once, `inputs` listing the slice's component files, and a final assertion; plain-word captions, no agent text, no real names. Render it locally: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case <id>-template --profile phone --out <dir>'`.
6. **Review.** Once the templates are on `develop`, dispatch `docs-media.yaml` for the template cases and give the owner the preview link. Change the template, not the wiring, until the owner accepts; never tick a template video for the docs.
7. **Wire.** Connect the components to the real stream, store and server. Add the live case `<id>` (same steps, real stream against the scripted fake agent) and run the slice's e2e. Add or refresh the subsection in `docs/guide/workspaces.md` with a `<!-- media:<id> -->` marker, dispatch `docs-media.yaml` for the live case, and have a developer pick it; run `node scripts/docs-media.mjs check docs/guide/workspaces.md`.
