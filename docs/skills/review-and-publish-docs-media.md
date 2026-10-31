---
id: review-and-publish-docs-media
title: Review rendered documentation videos, send some back, and publish the approved ones to the site
status: active
date: 2026-10-29
tags: [docs, ci, video, ux, procedural]
---

# Procedure: Review and publish documentation media

Use after a render of the documentation videos (decision 0158), either to review one yourself or as an agent session applying the owner's notes. The owner approves; a session never ticks for them.

1. **Find the render.** The render workflow opens or updates one issue per run, labelled `docs-media`, with the Pages preview link and a checklist line per video (case, profile, input hash). Open the preview page on the device you care about (iPhone for phone cases); every video plays inline.
2. **Judge each video.** Taps visible, captions readable and not over the control being used, pacing readable, the final state is what the caption says, nothing flickers, it says it is a rendered walkthrough on scripted data. A video that fails any of these is sent back.
3. **Approve.** Tick the line (or comment `/approve <case|all>`). The review workflow checks that you have write access, copies the files to the `docs-media` branch and records the approver, date and sha256 in `manifest.json`. The next `deploy-site` publishes them (`nothing-is-published-without-a-matching-approval-record`).
4. **Send back.** Comment `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>`. The knobs re-render at once and the line is unticked. The note labels the issue `video-needs-edit`.
5. **Apply the notes (a session).** List the issue's `/regen` notes, edit the case in `packages/client/e2e/video/cases.mjs` (caption words, steps, pace constants in `lib.mjs`), keep its final assertion, run `node e2e/video/render.mjs --case <id> --profile <p> --out <dir>` locally, then dispatch the render workflow for those cases (`gh workflow run docs-media.yaml -f case=<id> -f profile=<p>`) and merge the code change first, because a workflow runs the version on `develop`. Remove the label when the new render is on the issue.
6. **Embed.** Add `<!-- media:<id> -->` where the page should show the video; run `node scripts/docs-media.mjs embed` and commit the filled block. The docs check fails while the video is unapproved.
7. **Stale videos.** A pull-request comment or a new review issue names videos whose inputs changed. Treat them like any other render: review, then approve or send back. Never approve to silence the comment.

Real-device behavior is judged by `mobile-ui-device-check`, not by a video.
