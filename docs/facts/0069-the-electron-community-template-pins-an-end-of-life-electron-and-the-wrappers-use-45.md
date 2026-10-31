---
id: 0069
title: The Electron community template pins an end-of-life Electron 26, and the wrappers use Electron 45
status: active
kind: environmental
governed-by: 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class
tags: [desktop, platform, security]
date: 2026-11-01
last-verified: 2026-11-01
---

# The template's Electron is end of life; the wrappers use 45

- `@capacitor-community/electron` 5.0.1 generates a project on Electron 26, which is out of support.
- `packages/native/electron` pins Electron 45.0.0 and electron-builder 26.18.0, and drops the template's `electron-rebuild` step.
- `pnpm audit` reports three moderate `uuid` advisories through the Capacitor CLI's `xcode` dependency. They are build-time only and have no upstream fix yet (README, checked at the wrappers merge, 2026-10-31).
- Verified by: `packages/native/electron/package.json` and `packages/native/README.md` on `develop`.
