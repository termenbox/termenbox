---
id: render-a-feature-video
title: Render a captioned feature video from a UX scenario
status: active
date: 2026-10-24
tags: [client, mobile, ux, ci, video, procedural]
---

# Procedure: Render a feature video

Use when a feature needs a short user-facing video. The video is rendered from a scenario in `packages/client/e2e/ux/scenarios/` (decision 0135), so it always shows what the assessment walks.

1. **Find or add the scenario.** If the flow has no scenario, add `scenarios/<feature>.mjs` with its steps and a `caption` on each step that a user should read (plain words, no agent text, no real server names or secrets). Run it once as an assessment to confirm it passes.
2. **Choose the profile**: phone (iPhone 390 x 844) by default, desktop (1280 x 800) for shortcuts. Videos use the Chromium engine.
3. **Render**: trigger the `ux` workflow with `workflow_dispatch`, inputs `video=<feature>` and `profile=<phone|desktop>` (`gh workflow run ux.yaml -f video=<feature> -f profile=phone`), then `gh run download <run> -n ux-video-<feature>`. Locally in the pinned container: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/video.mjs --scenario <feature> --profile phone --out <dir>'`.
4. **Check the output**: an MP4 (H.264, recorded at 2x) and a GIF poster. Watch it once: taps are visible, captions do not cover the control being tapped, nothing flickers, and the run has no console error.
5. **Label it**: say in the description that it is a rendered walkthrough of the app on scripted data, not a recording of a real device or a real agent.
6. **Publish** only on the owner's word; publishing to the site is a separate step (R73 leaves it out).

**New workspace dialog (until the `ux` workflow exists).** Its videos come from `.github/workflows/dialog-videos.yaml`, dispatch only, recorded by `packages/client/e2e/video/`. Dispatch: `gh workflow run dialog-videos.yaml -f case=all -f profile=both` (add `--ref <branch>` for an unmerged branch). Download: `gh run download <run> -n dialog-videos`. Locally: `node e2e/video/render.mjs --case <case> --profile phone|desktop --out <dir>` in `packages/client` (needs ffmpeg).
