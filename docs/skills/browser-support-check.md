---
id: browser-support-check
title: Tell engine automation from real-browser and real-iPhone checks
status: active
date: 2026-10-24
tags: [client, browser-support, compatibility, procedural]
---

# Procedure: Check browser support, and say which kind of check it was

The CI job "Browser matrix" runs the built client in Chromium, Firefox and WebKit. A Playwright engine is not the browser people install, so it does not prove the support floor (fact 0003, decision 0112). Use this list to see what each kind of check proves and what stays manual.

| Kind | What runs it | Proves | Does not prove |
|---|---|---|---|
| Engine automation | CI `browsers` job: `e2e/suite.mjs desktop` on Chromium, Firefox and WebKit at 1024x768 @1x and 1440x900 @2x, `e2e/suite.mjs phone` on WebKit at 390 px @3x | Layout, keys, sidebar, palette, resize, copy and the clipboard fallback behave the same in each engine's current build | The minimum version, Edge, Safari's own UI and policies, a real soft keyboard, safe areas, gestures |
| Real desktop browser | A person, by hand, once per release and after a change to input, layout or the terminal | The real Chrome, Edge, Firefox and macOS Safari the app supports | Anything on a phone |
| Real iPhone | The owner, with `mobile-ui-device-check` | iOS Safari and the installed PWA | Android |

1. Read the CI summary of each matrix job: it names the engine and version, viewport and pixel ratio. A failure leaves `<engine>-<script>.png` and a Playwright trace in the `e2e-failure-...` artifact (`npx playwright show-trace <file>.zip`).
2. Run the real desktop pass on the oldest and the newest supported build of each browser in fact 0003 that you can get: Chrome, Edge, Firefox (current and current-2), macOS Safari 16.4+. Check what automation cannot: the shortcut list and Ctrl/Cmd+K, a selection copied and pasted into another app, a mouse-reporting program with Shift-drag, the sidebar drag and keyboard resize, and a reload that keeps the layout.
3. Zoom the page to 200% (Ctrl/Cmd and +) at 1440 px wide and again at 1024 px. Confirm the terminal refits, no control is cut off, nothing scrolls sideways and the layout switches to the phone layout under 721 CSS px. Record the browser, version and result.
4. Record the results in the pull request, one line per browser and version. Label every Chromium-, Firefox- or WebKit-engine result as engine automation, per `a-phone-check-names-its-platform`; only a real browser or phone counts as a device check.
5. Run `mobile-ui-device-check` for the iPhone and Android parts; this list does not replace it.

Not in the matrix: the throughput and resource gates and the WebRTC scenarios run once, on Chromium, in the server suite (`e2e/suite.mjs server`).
