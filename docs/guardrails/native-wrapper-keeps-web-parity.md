---
id: native-wrapper-keeps-web-parity
title: Native wrappers are optional and the web client stays complete
status: active
governed-by: 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class
grounded-in: 0002,0003,0010,0140
derivation-note: An optional wrapper is only safe when the web build stays complete, so the shared client cannot import native code and every native enhancement has a web fallback.
tags: [client, platform, distribution, parity]
---

# Guardrail: Native wrappers are optional and the web client stays complete

## Rule
**MUST** ship every user-facing feature in the browser. **MUST NOT** put a feature only in a native wrapper.

**MUST** keep native code out of the web client: `packages/client` must not import Capacitor, Electron, Tauri or a native plugin. Native code lives in `packages/native`, behind a capability adapter.

**MUST NOT** branch on the platform in shared client code (`isIOS`, `isAndroid`, `isElectron`). **MUST** ask the adapter for a capability instead.

**MUST** give every native enhancement a web fallback: native push falls back to Web Push, background execution to foreground sync, camera/QR to file or QR input, biometrics to the device key.

## Scope
- Applies to `packages/client` (the PWA and hosted client) and to the shared code a wrapper loads.
- Does not apply to the Rust server and its installers, which are native by design.
- The wrapper's platform chrome — safe areas, back gesture, viewport, soft keyboard — is the only work with no web counterpart; it must not change what the web client can do.

## Rationale
- The product promise is zero-install, self-hosted and MIT; a wrapper is a convenience, not a requirement.
- A web build that imports native code stops being a PWA and starts depending on a platform toolchain.
- A fallback per enhancement means a user who cannot install an app loses nothing.

## Exceptions
- A feature the browser physically cannot provide (for example, waking an evicted iOS tab) may be native-only in *delivery*, but its user-visible outcome must still be reachable on the web by another path (Web Push + foreground sync).

## Enforcement
- CI: the client job's gate `the web client imports no native wrapper` fails on a native import in `packages/client/src` or `packages/client/vite.config.ts`, or a native package in `packages/client/package.json`.
- CI (with R76): a wrapper job builds all five targets and runs the web parity suite; the PWA matrix (R36) stays green independently.
- PR review: reject a feature that only appears in a wrapper, and reject a platform branch in shared client code.
