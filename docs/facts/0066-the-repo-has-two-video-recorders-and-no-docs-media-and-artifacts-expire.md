---
id: 0066
title: The repo has two video recorders and no docs media, and workflow artifacts expire
status: active
kind: observation
governed-by: 0158-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch
tags: [docs, ci, video, site]
date: 2026-10-29
last-verified: 2026-10-29
---

# Two recorders, no docs media

## What is true

- **Two recorders exist.** `packages/client/e2e/video/` (merged in PR #360: `lib.mjs` with a touch spot or cursor, eased moves, human typing and captions; `cases.mjs`; `render.mjs`) records four New workspace dialog cases on phone and desktop. The unmerged branch `user-docs` (a registered worktree at `origin/develop` in the checkout) holds `.github/workflows/take-videos.yaml` and `video-connect|mobile|reconnect|switch|type.mjs`, which use a different mechanism. R73 and decision 0135 plan a third (`e2e/ux/`), not built.
- **The docs carry no media.** `README.md`, `docs/guide/{configuration,development,install,networking,safety,workspaces}.md` and `packages/site/index.html` reference no video or screenshot. The only machine check on them is `scripts/check-site.mjs` (metadata, local assets, the install command).
- **A render run's output (run 40786291939, 2026-10-29).** 8 MP4s (H.264, 13 to 24 s) total 3.7 MB; 8 GIFs total 14 MB (the largest 2.5 MB). They live in the artifact `dialog-videos`, kept 30 days.
- **The site deploys from `master`.** `.github/workflows/deploy-site.yaml` runs on a push to `master` that touches `packages/site/**`, `scripts/check-site.mjs` or `README.md`, and on dispatch, with Wrangler and the Pages token.
