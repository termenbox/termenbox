---
id: 0166-wrappers-inject-an-optional-capability-host-and-the-client-stays-native-free
title: Wrappers inject an optional-capability host as a global, and back and keep-awake are wrapper-only chrome
status: draft
date: 2026-11-01
track: process
tags: [platform, client, mobile, parity]
---

## Decision

- **Injected host.** A wrapper loads a classic script that sets `globalThis.__termenboxPlatform`; the client reads it in `lib/platform.ts`. `packages/client` imports no native module, which is how decision 0140's adapter is realised for the first capabilities.
- **Every capability is optional.** `keepAwake`, `allowSleep` and `onBack` are independent; a host offers what it implements and the client asks for each separately. A missing one never disables the others.
- **Wrapper-only chrome.** Android back (workspace list, then close a sheet, then minimise) and keep-awake are the wrapper's chrome fixes. The PWA gains no behaviour, so keep-awake uses no Screen Wake Lock in the browser.
- **Added when used.** The adapter grows one capability at a time with the feature that needs it; the full list in R76 step 2 is a ceiling, not a to-do.

## Rejected

| Option | Why not |
|---|---|
| A web Screen Wake Lock fallback | Changes battery behaviour for every web user, which nobody asked for |
| One all-or-nothing host interface | One missing method would silently disable the rest |
| Defining every R76 capability up front | Unused interface to maintain before any feature needs it |

## Consequences

- Back logic lives in `lib/backNavigation.ts` as a pure function with tests.
- The native-import gate (`scripts/check-native-parity.mjs`) enforces the no-import rule in CI.
