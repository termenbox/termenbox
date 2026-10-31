---
id: 0161-course-videos-use-a-human-rhythm-model-a-still-poster-and-steps-generated-from-the-case
title: Course videos use a human rhythm model for their delays, show a still poster that links to the MP4, and take their numbered steps from the recorded case
status: active
date: 2026-10-31
track: process
accepted-by: Sergey Strebulaev
amends: 0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request
tags: [docs, video, ux, installer, ci]
---

## Decision

- **Rhythm model.** The recorder (`packages/client/e2e/video/lib.mjs`) models how a person behaves instead of drawing flat random delays: a reading pause scaled to the caption's length, a beat of hesitation before a choice, typing in short bursts with small stalls and some slower words, curved pointer paths that ease in and slightly overshoot, and a longer rest on each result worth noticing. It is the only default; `--pace slow` stays as a knob. It runs from a seeded generator, so a render is reproducible and its input hash does not change between runs. A chapter video runs about 45 to 90 seconds.
- **Still poster.** A chapter shows a small captioned still (one of the recorder's own stills) with a "Watch, 60 s" link that opens the MP4 in GitHub's player. Nothing autoplays. The GIF leaves the pipeline: it is no longer rendered, reviewed, picked or embedded.
- **Steps from the case.** A case lists its steps once: the caption shown on screen and the exact label tapped. The chapter's numbered steps are generated from that list between `<!-- course-steps:<case> -->` markers, and a check fails when the block is stale. The story, promise, troubleshooting box and "you can now" are written by hand around it. A renamed button breaks the build instead of the reader.
- **Install replays real output.** The Install chapter runs the real installer once in a clean container (CI already does), captures its exact output including the QR and link, and the recorder replays that in a terminal view: the command typed at human pace, the output arriving as it did. The video says it is a rendered walkthrough.

## Why

The first dialog video shows the cost of flat delays: a 17 s walkthrough with a 0.6 to 1.2 s pause after every change, typing at a steady 55 to 140 ms per key and straight pointer moves reads as a fast robot. Slowing it down only gives a slower robot; a rhythm model gives pauses where a person reads and decides. A chapter-length GIF would be 5 to 10 MB against 1.8 MB for the 17 s one, and GitHub does not play a repository MP4 inside Markdown (fact 0068), so a still that links out is the light, honest option. Generating the steps from the case is what keeps a newcomer's instructions true after a UI change.

Not chosen: making 1.6 times the default (still uniform), replaying a recorded human take (a take per chapter and a re-recording per UI change), keeping the GIF poster (page and repository weight), hosting videos outside the repository (an external dependency and a manual step outside the pull request), hand-written steps (nothing catches a stale label), generating the whole chapter (flat text, and CI may not call a model), recording the installer live (ports, timing and the QR vary per run) or leaving Install without a video.

## Consequences

- `docs/media/` holds the MP4, the stills and `render.json` of a picked video; no GIF. Decision 0158 and its embed, review page and approve script change to match (plan `docs/plans/user-course.md`).
- A new script generates and checks the steps block; the course check also enforces the anatomy.
- The captured installer output is a fixture refreshed when the installer's output changes; a stale fixture is a review finding, not a silent drift.
- Existing renders must be redone: the one video on PR #427 is flat-paced and GIF-based.
