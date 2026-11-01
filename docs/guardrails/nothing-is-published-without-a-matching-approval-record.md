---
id: nothing-is-published-without-a-matching-approval-record
title: A video or still enters docs/media only from a render a developer ticked, and its render.json hash is the ticked one
status: active
governed-by: 0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request
grounded-in: [0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request, 0066, ux-scenarios-assert-a-state-before-they-capture-it]
derivation-note: The point of the review ledger is that a person saw what ships; a file that changed after the tick, or arrived without one, would put unreviewed media into the public docs.
tags: [ci, docs, video, site]
---

# Guardrail: No tick, no media in the docs

## Rule
The review workflow **MUST** copy a render into `docs/media/` only for a line ticked (or named by `/approve`) by a person with write access (`media-commands-come-only-from-people-with-write-access`), from the artifact of the render that line points to, and **MUST** fail when the `hash` in that render's `render.json` differs from the hash on the ticked line.
It **MUST** deliver the files on a branch for a human to open as a pull request, never push to `develop`, and every file in `docs/media/` **MUST** have its `render.json` beside it.
`scripts/docs-media.mjs check` **MUST** fail on a marker whose video, GIF or `render.json` is absent.
Every published video and still **MUST** have its `render.json` beside it, recording the case and the input hash. It no longer **MUST** carry a label that it is a rendered walkthrough (owner's decision, 2026-11-01); the render's source stays recorded in the review and in `render.json`.
