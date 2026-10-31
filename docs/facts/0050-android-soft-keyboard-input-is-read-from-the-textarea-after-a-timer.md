---
id: 0050
title: xterm.js 5.3 reads Android soft-keyboard input from its hidden textarea after a timer, so a microtask must not rewrite it
status: active
tags: [client, mobile, android, ios, terminal, xterm]
date: 2026-10-20
kind: environmental
last-verified: 2026-10-31
---

# Android soft-keyboard input reaches xterm after a timer

- Unchanged in @xterm/xterm 5.5.0, which the client uses since #406: `CompositionHelper.ts` is identical to 5.3.0.
- Chrome Android delivers soft-keyboard typing through an IME composition or a `keydown` whose `keyCode` is 229, not through a synchronous `keydown` with a character key.
- xterm.js 5.3 sends that text from `CompositionHelper`: `_finalizeComposition()` and `_handleAnyTextareaChanges()` read `textarea.value` inside `setTimeout(0)`. iOS sends most Latin typing as `insertText` or keydown data synchronously instead.
- A mirror that rewrote `textarea.value` on `input`/`keyup`/`compositionend` used a microtask; microtasks run before timers, so the previous phrase was written back before xterm read the field and every character went out empty (issue #250). The mirror now runs from `sendInput()`, including the `sendPaste()` path, after `words` is fed.
- A microtask queued from a `beforeinput` listener also runs before the browser's default action. Writing the field and collapsing the caret there makes `insertReplacementText` apply at the new caret (`he'llhe'll`), so the mirror defers its write with `setTimeout(0)`, not `queueMicrotask()`.
- Covered by `packages/client/src/lib/swipe.test.ts` (deferred-read regressions for composition and keyCode 229, plus the task-deferred mirror write) and `packages/client/e2e/touch.mjs` (composition, keyCode 229, iOS `insertText` and replacement). The browser run is Chromium emulation only; no real Android phone has run it yet (fact 0039).
