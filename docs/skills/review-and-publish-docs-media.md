---
id: review-and-publish-docs-media
title: Review rendered documentation videos, send some back, and add the picked ones to the docs
status: active
date: 2026-10-29
tags: [docs, ci, video, ux, procedural]
---

# Procedure: Review and publish documentation media

Use after a render of the documentation videos (decision 0158), either to review one yourself or as an agent session applying the owner's notes. A developer with write access picks; a session never ticks for them.

1. **Find the render.** The render workflow opens or updates one issue per run, labelled `docs-media`, with the Pages preview link and a checklist line per video (case, profile, input hash). Open the preview page on the device you care about (iPhone for phone cases); every video plays inline.
2. **Judge each video.** Taps visible, captions readable and not over the control being used, pacing readable, the final state is what the caption says, nothing flickers. A video that fails any of these is sent back.
3. **Pick.** Tick the lines you want, then comment `/approve` once (or `/approve all`, or `/approve <case>`); a tick alone starts nothing, so a batch is one run. The review workflow checks that you have write access, pushes a branch `docs/media-<run id>` with the video, poster, stills and `render.json` under `docs/media/`, fills any markers already in the docs, and comments the compare link. Open it as a pull request; its merge adds the media (`nothing-is-published-without-a-matching-approval-record`).
4. **Send back.** Comment `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>`. The knobs re-render at once and the line is unticked. The note labels the issue `video-needs-edit`.
5. **Apply the notes (a session).** List the issue's `/regen` notes, edit the case in `packages/client/e2e/video/cases.mjs` (caption words, steps, pace constants in `lib.mjs`), keep its final assertion, run `node e2e/video/render.mjs --case <id> --profile <p> --out <dir>` locally, then dispatch the render workflow for those cases (`gh workflow run docs-media.yaml -f case=<id> -f profile=<p>`) and merge the code change first, because a workflow runs the version on `develop`. Remove the label when the new render is on the issue.
6. **Embed.** Add `<!-- media:<id> -->` where the page should show the video, ideally before the tick so the branch fills it; otherwise run `node scripts/docs-media.mjs embed README.md docs/guide/*.md` after the media is on `develop`. The docs check fails while the video is missing.
7. **Upload to GitHub (chapters).** A chapter shows its video as a player on github.com through a `user-attachments` URL line. After the picked media is on `develop`, a developer with their own login runs `node scripts/docs-media-upload.mjs <issue> <case-profile>...` (or `--all <issue>`; issue #511 holds the files and stays open), then `node scripts/docs-media.mjs embed docs/course/0*.md`, and commits `docs/media/github-assets.json` and the chapters. The workflow token cannot attach files. `docs-media.mjs check docs/course/0*.md` fails a chapter whose upload is missing or older than its `render.json`, so a re-render is not finished until this step is.
8. **Stale videos.** A pull-request comment or a new review issue names videos whose inputs changed. Treat them like any other render: review, then pick or send back. Never tick to silence the comment.

Real-device behavior is judged by `mobile-ui-device-check`, not by a video.
