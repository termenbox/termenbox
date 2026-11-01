---
id: render-a-feature-video
title: Render a captioned feature video from a UX scenario
status: active
date: 2026-10-24
tags: [client, mobile, ux, ci, video, procedural]
---

# Procedure: Render a feature video

Use when a feature needs a short user-facing video. The video is rendered by the one recorder, `packages/client/e2e/video/` (decision 0158, which amends 0135), from a case that ends with an assertion of its final state, so it always shows what a passing run walks.

1. **Find or add the case.** If the flow has no case, add it to `packages/client/e2e/video/cases.mjs` with its id, title, group, the docs page that embeds it, the source paths it depends on, and a `caption` on each step a user should read (plain words, no agent text, no real server names or secrets). It must assert its final state before it is recorded (`ux-scenarios-assert-a-state-before-they-capture-it`).
2. **Choose the profile**: phone (iPhone 390 x 844) by default, desktop (1280 x 800) for shortcuts. Videos use the Chromium engine.
3. **Render**: dispatch the render workflow once it exists (`gh workflow run docs-media.yaml -f case=<id> -f profile=<phone|desktop>`; until then `dialog-videos.yaml` for the dialog cases), then `gh run download <run> -n <artifact>`. Locally in the pinned container: `node e2e/video/render.mjs --case <id> --profile phone --out <dir>` in `packages/client` (needs ffmpeg).
4. **Check the output**: an MP4 (H.264, recorded at 2x), a poster still (`<case>-<profile>-poster.png`) and `render.json`. Watch it once: taps are visible, captions do not cover the control being tapped, nothing flickers, and the run has no console error.
5. **Describe it**: say in the description which case and profile it shows and what it proves. No on-video label is required (owner's decision, 2026-11-01).
6. **Review and publish** with the procedure `review-and-publish-docs-media`: the owner approves each video on the preview page; nothing reaches the site unapproved.
