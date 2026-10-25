---
id: 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class
title: Native wrappers are optional enhancements and the PWA stays the reference client
status: active
supersedes-guardrail: [no-native-apps]
amends-guardrail: [no-paid-services]
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [platform, distribution, client, mobile, desktop, product]
---

## Decision

- **The PWA stays the reference client.** Every user-facing feature works in a browser, and no feature exists only in a wrapper. Removing the wrappers must leave a complete product, including a complete client.
- **One Svelte build, one protocol.** A wrapper loads the same `packages/client` build and speaks the same wire protocol. Native code lives in a separate `packages/native` workspace; `packages/client` never imports a native module. Platform differences go through a capability adapter that the web build implements as a no-op, so shared client code has no `if (isIOS)` or `if (isElectron)` branches.
- **Mobile: Capacitor** for iOS and Android.
- **Desktop: `@capacitor-community/electron`** for Windows, macOS and Linux, driven by the same Capacitor CLI, so one command family builds all five targets.
- **Native is an enhancement.** Native push, background execution, camera/QR and biometrics may be added, but each one has a web fallback (Web Push, foreground sync, file/QR input, device key). The wrapper's chrome fixes (safe areas, back gesture, viewport, keyboard) are the only native work with no web counterpart, because the web already has the behaviour.
- **Distribution.** Mobile ships through the App Store and Google Play; desktop ships signed installers. The store developer accounts are a **distribution cost**, not a runtime dependency: the PWA and self-hosted installs still need no paid service (`no-paid-services`, scope note). No subscription and no per-user fee is introduced.
- **The server is untouched.** The Rust server and its install path stay as they are; a wrapper is a client, not a host.
- **Guardrail.** `no-native-apps` is superseded by `native-wrapper-keeps-web-parity`: the web client stays complete, the shared client stays free of native imports, and every native enhancement has a web fallback.

## Why

The owner wants the product reachable as a home-screen and desktop app with native push and real background execution, which is the one thing the browser cannot do on iOS (decision 0005, fact 0016). Optional wrappers serve that without giving up zero-install web access, self-hosting or the MIT license. Keeping the native code in its own package preserves the old guardrail's real intent — that the web build must never depend on a platform — while allowing the app-store presence the owner asked for.

## Rejected

| Option | Why not |
|---|---|
| Stay web-only | Leaves native push, background execution and store discovery unreachable |
| A separate native UI codebase | Drift and double maintenance; one Svelte build is the point |
| Capacitor or Electron imports inside `packages/client` | Makes the web bundle depend on native code and breaks the zero-install path |
| Tauri or Electron for mobile | Neither covers iOS and Android |
| Capacitor alone for all five targets | Capacitor itself targets iOS, Android and web; desktop needs the Electron extension |
| Wrappers as the primary distribution | Drops the zero-install PWA and self-hosted users |

## Consequences

- A new workspace `packages/native` holds the Capacitor projects, the Electron target and the capability adapter.
- CI gains a gate that fails when `packages/client/src` imports a native module, replacing the old guardrail's `packages/client/` grep.
- Signing keys, store credentials and provisioning profiles are release secrets and never logged (`security-secrets-handling`).
- The web client keeps its own gates: the browser matrix (R36) and the real-device pass (U16) still decide whether the PWA is done. Wrapper checks are labelled per platform (`a-phone-check-names-its-platform`).
- Decision 0005's "no native wrapper" rationale is superseded; its persistence layers still apply to the web client.
- The 512 MB / 1 vCPU envelope (guardrail `resource-constraints`) is about the server and is unchanged; a wrapper runs on the user's device.

## See also

[[0002]] (Svelte 5 client), [[0005]] (iOS persistence), [[0006]] (device keys), [[0010]] (monorepo), [[0011]] (MIT), [[0052]] (hosted and self-hosted client), [[0076]] (signed releases), [[0112]] (browser floor), guardrails `native-wrapper-keeps-web-parity` and `no-paid-services`.
