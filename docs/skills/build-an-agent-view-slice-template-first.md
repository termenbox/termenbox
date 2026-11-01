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
4. **Style.** Apply roles and tokens (decision 0181), only DR5 sizes. Re-run every test and the audit in every skin.
5. **Template case (declare now, render at the end).** Add a case `<id>-template` (group `agent-template`) to `packages/client/e2e/video/cases.mjs` with `steps` (id, caption, label) declared once, `inputs` listing the slice's component files, and a final assertion; plain-word captions, no agent text, no real names. Rendering waits for the plan's final video slice (decision 0167, amended 2026-11-01); to try a case early, render it locally: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case <id>-template --profile phone --out <dir>'`.
6. **Review.** Push the branch and dispatch `gh workflow run template-review.yaml -f pages=<page>` (the probe page must be on `develop`: the workflow builds the default branch unless you pass `--ref`) (workflow `template-review.yaml`, script `scripts/template-review.mjs`); give the owner the issue link it opens. The owner opens the Cloudflare Pages preview and ticks `accept <page>`: one tap. Check with `gh issue list --label template-review --state all --json body | node scripts/template-review.mjs status <page>`. A session never ticks. No video is needed. Change the template, not the wiring, until the owner accepts; never tick a template video for the docs.
7. **Wire.** Connect the components to the real stream, store and server. Declare the live case `<id>` (same steps, real stream against the scripted fake agent) and run the slice's e2e. Dispatching `docs-media.yaml`, picking the video and the `<!-- media:<id> -->` markers happen in the final video slice.
