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
**MUST** make every user-visible outcome reachable in the browser. **MUST NOT** put a feature only in a native wrapper.

**MUST** keep native code out of the web client: `packages/client` must not import Capacitor, Electron or a native plugin. Native code lives in `packages/native`; the adapter interface and its web no-op live in `packages/client`, and native implementations are injected from `packages/native`.

**MUST NOT** branch on the *native runtime* in shared client code (`isIOS`, `isAndroid`, `isElectron` wrapper checks). **MUST** ask the adapter for a capability instead. Browser feature detection — an iOS Safari install hint, a WebGL fallback — is not a native-runtime branch and stays allowed.

**MUST** give every native enhancement a web fallback: native push falls back to Web Push, background execution to foreground sync, camera/QR to file or QR input, biometrics to the device key.

## Scope
- Applies to `packages/client` (the PWA and hosted client) and to the shared code a wrapper loads.
- Does not apply to the Rust server and its installers, which are native by design.
- The wrapper's platform chrome — safe areas, back gesture, viewport, soft keyboard — is the only *feature* work with no web counterpart; it must not change what the web client can do.

## Rationale
- The product promise is zero-install, self-hosted and MIT; a wrapper is a convenience, not a requirement.
- A web build that imports native code stops being a PWA and starts depending on a platform toolchain.
- A fallback per enhancement means a user who cannot install an app loses nothing.

## Exceptions
- A feature the browser physically cannot provide (for example, waking an evicted iOS tab) may be native-only in *delivery*, but its user-visible outcome must still be reachable on the web by another path (Web Push + foreground sync).

## Enforcement
- CI (planned, own PR): `scripts/check-native-parity.mjs`, run in the client job, fails on a native import
  or a native dependency in `packages/client`. It must cover `src`, `functions`, `public`, `installer` and
  `e2e`, the vite config and `package.json`; fail when a covered path is missing rather than passing; and
  match `@capacitor/*` (including `@capacitor/core`), `@capacitor-community/*`, `@electron/*` and
  `electron`, not only wrapper package names. The import gate does not detect a platform branch;
  that rule is enforced in review against the native-runtime definition above, so browser feature detection
  is not rejected. Until that script lands, PR review is the only enforcement of this rule.
- CI (with R76): a wrapper job builds all five targets and runs the web parity suite; the PWA matrix (R36) stays green independently.
- PR review: reject a feature that only appears in a wrapper, and reject a native-runtime branch in shared client code.
