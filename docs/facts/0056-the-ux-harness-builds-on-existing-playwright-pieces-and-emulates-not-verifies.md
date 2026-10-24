---
id: 0056
title: The UX harness builds on existing Playwright pieces, and CI emulates a phone, it does not verify one
status: active
kind: observation
governed-by: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
tags: [client, mobile, ux, ci]
date: 2026-10-24
last-verified: 2026-10-24
---

# The UX harness builds on existing pieces and emulates a phone

## What is true

Existing, in `packages/client/e2e` and `docs/plans/assets/ux`:

- `app-lib.mjs` serves the built `App.svelte` and answers its WebSocket from a script (`page.routeWebSocket`), so the real hub, connection, terminal and session view run without a server binary. It does not prove real tmux, resize handling or agent hooks (`run.mjs` covers the server).
- `gallery.mjs` audits the component gallery at 390 px for the design rules (target size DR1, spacing DR2, type scale DR5, contrast DR8, overflow DR15); `shoot.mjs`, `audit.mjs` and `measure.mjs` in `docs/plans/assets/ux` did the same for the design-spike prototypes.
- `e2e/browser.mjs` (R36) is the one launcher: engine from `E2E_ENGINE` (chromium, firefox, webkit), viewport and pixel ratio from `E2E_VIEWPORT` and `E2E_DPR`, failure screenshots and traces under `E2E_ARTIFACTS`. `.github/workflows/ci.yaml` runs a Browser matrix (desktop on all three engines, phone on WebKit at 390 px) through `e2e/suite.mjs`. There are no UX measurement scenarios, no `ffmpeg` step and no video output.

Limits that no CI run removes:

- Playwright WebKit on Linux is not iOS Safari: it has no soft keyboard, no safe-area or Local Network behaviour, cannot deliver touch input (`browser.mjs` `tap` clicks there; only Chromium proves touch), and its `visualViewport` keyboard changes can only be simulated by resizing.
- An emulator is never reported as a phone (`a-phone-check-names-its-platform`; Android has no real-device check, fact 0039).
- First-paint timings from a shared runner vary; a throttled run needs several samples and a median.
