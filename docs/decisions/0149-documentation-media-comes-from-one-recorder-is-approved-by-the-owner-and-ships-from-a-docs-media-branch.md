---
id: 0149-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch
title: Documentation videos and stills come from one recorder, are approved one by one by the owner on a preview page, and ship from an orphan docs-media branch through the site deploy
status: active
amends: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
date: 2026-10-29
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [docs, ci, video, ux, site]
---

## Decision

- **One recorder.** `packages/client/e2e/video/` (`lib.mjs` human pacing, `cases.mjs`, `render.mjs`) is the only recorder. The five scenarios on the unmerged `user-docs` branch (connect, type, reconnect, mobile, switch) are ported into it and that branch and its `take-videos.yaml` are retired. R73 adopts this recorder as its video renderer instead of writing another; the scenario-set rule of 0135 (a video shows only what a passing scenario walks) is kept through the guardrail `ux-scenarios-assert-a-state-before-they-capture-it`.
- **Catalog.** Beyond the four New workspace dialog cases: connect the first server (add-server wizard), resume after closing the browser, answer a "needs you" decision from a push link, switch workspaces, the key row with the soft keyboard, Terminal vs Agent view, scrollback search, an unreachable server with what to fix, and updating the server. Each case declares an id, title, group, the docs page that embeds it, and the source paths its video depends on. The recorder also saves a captioned still per marked step.
- **Review.** Every render (dispatch-only) deploys a Cloudflare Pages preview with one mobile-first page that plays each video inline, and opens or updates one GitHub issue per render with a checklist, one line per video. Ticking a line approves that video. A comment `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>` re-renders it with those bounded knobs; the free-text note labels the issue `video-needs-edit`, and a session applies it to `cases.mjs` with the procedure `review-and-publish-docs-media` and dispatches again. No model and no secret beyond the existing Pages deploy token are used in CI.
- **Store.** Approval copies the MP4, GIF, stills and a `manifest.json` entry (case, input hash, render run id, approver, date) to the orphan branch `docs-media`, which shares no history with `develop`. `deploy-site` checks that branch out into `packages/site/media`, so the site serves approved media at stable URLs and the README and guide pages link or embed them from there.
- **Embedding.** Docs mark a place with `<!-- media:<id> -->`; a script fills the poster image and the link to the video from the manifest. A check fails when a marker names a video that is missing or not approved.
- **Staleness.** Each approved video records a hash of its inputs (the case, the declared client source paths, the app version). A pull request touching them gets an advisory comment listing videos that may be stale; a merge to `develop` re-renders only those and opens a review issue. Nothing reaches the site without a tick.

## Why

Four dialog videos now render in Actions, but they stop at an expiring artifact: nobody can review one on a phone, send it back, publish it, or learn that a UI change made it wrong. A second recorder was already started on a side branch, and R73 would have made a third. One recorder, one review path and one store keep the videos honest and cheap, and the orphan branch keeps binary history out of `develop`.

Not chosen: committing media to `develop` (binary history per regeneration), release assets (links per release, no inline playback in the README), a Pages deploy as the only store (a partial deploy loses approved files and no one is on record as approving), a model call to turn notes into edits (key, cost, and decision 0135), automatic republish on release (a broken video could ship unseen).

## Consequences

- A new dispatch-only render workflow and a review workflow replace `dialog-videos.yaml`; the render workflow uses the Pages deploy token that `deploy-site` already uses, and only runs on dispatch by someone with write access (`media-commands-come-only-from-people-with-write-access`).
- Preview deployments are public to anyone with the URL: they show scripted data only and say so.
- `deploy-site` gains a checkout of `docs-media`; an unapproved or hash-mismatched file never leaves the branch (`nothing-is-published-without-a-matching-approval-record`).
- Real-device checks (`mobile-ui-device-check`) stay a gate for UI changes; an approved video is a rendered walkthrough, not evidence of a device.
