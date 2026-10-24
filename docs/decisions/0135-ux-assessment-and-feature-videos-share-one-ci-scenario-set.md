---
id: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
title: UX assessment and feature videos are rendered from one set of scripted scenarios in a CI job; hard rules block, the rest is advisory
status: active
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, ux, ci, video]
date: 2026-10-24
---

## Decision

- **One scenario set, two outputs.** Scripted scenarios in `packages/client/e2e/ux/scenarios/` drive the real client against the scripted-WebSocket harness (`e2e/app-lib.mjs`). The same file yields the assessment report (taps, timings, target sizes, overflow, contrast, focus order, screenshots) and, rendered with captions and tap overlays, a feature video. A video can never show a flow the assessment does not walk.
- **Engines and profiles.** Chromium and Playwright WebKit, through the R36 launcher `e2e/browser.mjs` (engine from `E2E_ENGINE`, `tap`, failure traces), which R73 extends with named device profiles instead of adding a second launcher. Profiles: iPhone (390 x 844 @2x), a Pixel profile, desktop (1280 x 800) and tablet or landscape. Tablet and landscape are assessed only (overflow, target size, keyboard); no tablet layout is designed before the real-device pass. Every result is labelled emulated (`a-phone-check-names-its-platform`); Linux WebKit is not iOS Safari, has no soft keyboard and cannot deliver touch (fact 0056).
- **Trigger.** A pull request that touches `packages/client/src`, `packages/client/e2e` or the UX assets (path-filtered), and `workflow_dispatch` for ad-hoc runs and video renders. Results are a CI artifact plus one sticky pull request comment.
- **Comparison is re-rendered, not stored.** The job builds the base (`develop`) and the head in one run, runs the scenarios on both and diffs metrics and screenshots. No baseline images or numbers are committed.
- **Hard rules block, the rest is advisory** (guardrail `ux-hard-rules-block-the-pr-the-rest-is-advisory`).
- **Videos** are captioned MP4 with a GIF poster, produced from a scenario by Playwright recording plus an in-page tap and caption overlay and `ffmpeg`. No voice-over. Publishing to the site is a later step.
- **No model in CI.** The job needs no API key and calls no service. An agent (or the owner) analyses the downloaded artifact in a session and improves the UI with the procedure `improve-the-ux-from-ci`; the job only measures.
- Report artifacts are kept 14 days, videos 30.

## Why

The owner wants mobile menus, views, paths, keyboard, shortcuts and navigation analysed and improved, and short videos per feature. The pieces exist but are separate: a component gallery (`gallery.mjs`), a scripted-WebSocket app harness, the spike's `shoot.mjs`, `audit.mjs` and `measure.mjs`, and a manual iPhone checklist. Nothing runs them on every UI change, the R36 Browser matrix covers WebKit for functional scripts but not UX measurement, and nothing produces video. One scenario set keeps assessment and videos from drifting apart; re-rendering the base avoids baselines that rot or bloat the repository (the same reasoning as decision 0129 against stored state). Builds on [[0107-design-spike-precedes-the-build]] and [[0069-ui-foundation-tailwind-shadcn]]; targets from [[ux-acceptance-targets]].

Not chosen: Chromium only (blind to Safari layout, the owner's device); committed screenshot baselines (churn, flaky across font updates); a model call in CI (secrets, cost, and the no-paid-services rule); a voice-over (needs a TTS dependency).

## Consequences

- UI pull requests gain about twice the build time of the UX job (base plus head), only on the filtered paths.
- The UX job installs Playwright WebKit as the R36 Browser matrix does (`pnpm exec playwright-core install --with-deps webkit`) and adds `ffmpeg`.
- Emulation cannot replace `mobile-ui-device-check`; real-device passes (R03, U16) stay a gate.
