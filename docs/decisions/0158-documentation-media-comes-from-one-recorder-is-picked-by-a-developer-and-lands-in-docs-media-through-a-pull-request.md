---
id: 0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request
title: Documentation videos and stills come from one recorder, are picked one by one by a developer on a preview page, and land in docs/media through a pull request
status: active
amends: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
date: 2026-10-29
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [docs, ci, video, ux]
---

## Decision

- **One recorder.** `packages/client/e2e/video/` (`lib.mjs` human pacing, `cases.mjs`, `render.mjs`) is the only recorder. The five scenarios on the unmerged `user-docs` branch (connect, type, reconnect, mobile, switch) are ported into it and that branch and its `take-videos.yaml` are retired. R73 adopts this recorder as its video renderer instead of writing another; the scenario-set rule of 0135 (a video shows only what a passing scenario walks) is kept through the guardrail `ux-scenarios-assert-a-state-before-they-capture-it`.
- **Catalog.** Beyond the four New workspace dialog cases: connect the first server (add-server wizard), resume after closing the browser, answer a "needs you" decision from a push link, switch workspaces, the key row with the soft keyboard, Terminal vs Agent view, scrollback search, an unreachable server with what to fix, and updating the server. Each case declares an id, title, group, the docs page that embeds it, and the source paths its video depends on. The recorder also saves a captioned still per marked step.
- **Render.** A dispatch-only workflow records the chosen cases with human-paced delays on scripted data, deploys a Cloudflare Pages preview with one mobile-first page that plays each video inline, and opens one GitHub issue per render with a checklist, one line per video.
- **Pick.** A developer with write access ticks the lines that belong in the user docs, then comments `/approve` once (amended 2026-11-01: a tick alone starts nothing, because one run per tick raced on the branch and lost approvals). A comment `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>` re-renders one with those bounded knobs; the free-text note labels the issue `video-needs-edit`, and a session applies it to `cases.mjs` with the procedure `review-and-publish-docs-media` and dispatches again. No model and no secret beyond the existing Pages deploy token are used in CI.
- **Store.** (Amended 2026-10-31 by 0161: no GIF; the poster is a still.) `/approve` makes the review workflow push one branch for every ticked line with the video, GIF, stills and a `render.json` (case, input hash, version, run id) under `docs/media/`, and post the compare link on the issue. The developer opens it as an ordinary pull request, so the media lands in `develop` only through a squashed pull request with green CI, and the pull request is the record of who picked it.
- **Embedding.** Docs mark a place with `<!-- media:<id> -->`; a script fills the poster image and the link to the video with paths relative to the page (a still, from 0161 on), and the branch from a tick fills the markers already in place. A check fails when a marker names a video that is not in `docs/media/`.
- **Staleness.** Each video's `render.json` records a hash of its inputs (the case, the declared client source paths, the app version). A pull request touching them gets an advisory comment listing videos that may be stale; a merge to `develop` re-renders only those and opens a review issue. Nothing reaches the docs without a tick.

## Why

Four dialog videos now render in Actions, but they stop at an expiring artifact: nobody can review one on a phone, send it back, keep it, or learn that a UI change made it wrong. A second recorder was already started on a side branch, and R73 would have made a third. One recorder, one review path and one store keep the videos honest, and a pull request is the approval record the project already trusts.

Not chosen: an orphan `docs-media` branch with a checksum manifest and a gated site deploy (a second store, a second approval record and a deploy path for a few short files), release assets (links per release, no inline playback in the README), a Pages deploy as the only store (a partial deploy loses approved files), a model call to turn notes into edits (key, cost, and decision 0135), automatic republish on release (a broken video could ship unseen). Video history in `develop` grows with each regeneration; GIFs and videos stay short and only picked ones are committed, and the folder can move to an orphan branch later without changing the review path.

## Consequences

- A new dispatch-only render workflow and a review workflow replace `dialog-videos.yaml`; the render workflow uses the Pages deploy token that `deploy-site` already uses, and only runs on dispatch by someone with write access (`media-commands-come-only-from-people-with-write-access`).
- Preview deployments are public to anyone with the URL: they show scripted data only (amended 2026-11-01: they no longer carry a label saying so).
- Files enter `docs/media/` only from a ticked render whose hash matches (`nothing-is-published-without-a-matching-approval-record`); `deploy-site` is unchanged.
- Real-device checks (`mobile-ui-device-check`) stay a gate for UI changes; a picked video is a rendered walkthrough, not evidence of a device.
