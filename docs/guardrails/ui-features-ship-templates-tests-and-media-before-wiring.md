---
id: ui-features-ship-templates-tests-and-media-before-wiring
title: An Agent view slice is wired to the real stream only after its templates are tested and styled and the owner has reviewed their video
status: active
date: 2026-11-01
governed-by: 0167
grounded-in: [0167, 0158, 0135]
derivation-note: Wiring before the look, the tests and the review are settled makes every review a rework of built code, and a second media pipeline would drift from the shared one.
tags: [client, ux, process]
---

# Guardrail: An Agent view slice is wired to the real stream only after its templates are tested and styled and the owner has reviewed their video

## Rule
**MUST** build each slice as templates on fixtures first, cover them with tests, style them, and show them on a probe page that the owner has reviewed before the slice's wiring step starts. **MUST** declare the slice's recorder case (`packages/client/e2e/video/`) when wiring, and render and pick the live video, not a template one, for the docs in the final video slice of the plan (the plan names it; amended 2026-11-01 so that rendering waits for the docs-media workflow to settle). Rendering template videos before wiring is allowed but not required. **MUST** declare each case's `steps` and `inputs` once. **MUST NOT** add a recorder, render workflow, media store or guide generator for the Agent view, hand-write numbered steps a case could supply, or label a rendered walkthrough as a recording of a real device or agent.
