---
id: 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class
title: Native wrappers are optional enhancements and the PWA stays the reference client
status: active
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [platform, distribution, client, mobile, desktop, product]
---

## Decision

- **The PWA stays the reference client.** Every user-visible outcome is reachable in a browser, and no feature exists only in a wrapper; delivery may be native-only where the browser physically cannot do it, as long as the outcome stays reachable on the web. Removing the wrappers must leave a complete product, including a complete client.
- **One Svelte build, one protocol.** A wrapper loads the same `packages/client` build and speaks the same wire protocol. Native code lives in a separate `packages/native` workspace; `packages/client` never imports a native module. Platform differences go through a capability adapter whose interface and web no-op live in `packages/client` and whose native implementations are injected from `packages/native`, so shared client code has no `if (isIOS)` or `if (isElectron)` branches. Browser feature detection (for example, the iOS Safari install hint) is not a native-runtime branch and stays allowed.
- **Mobile: Capacitor** for iOS and Android.
- **Desktop: `@capacitor-community/electron`** for Windows, macOS and Linux, driven by the same Capacitor CLI, so one command family builds all five targets, subject to verifying the plugin against the Capacitor version in use (see Consequences).
- **Native is an enhancement.** Native push, background execution, camera/QR and biometrics may be added, but each one has a web fallback (Web Push, foreground sync, file/QR input, device key). The wrapper's chrome fixes (safe areas, back gesture, viewport, keyboard) are the only native feature work with no web counterpart, because the web already has the behaviour.
- **Distribution.** Mobile ships through the App Store and Google Play; desktop ships signed installers. `no-paid-services` is waived strictly for the cost of distribution: the Apple Developer Program and Google Play Console accounts, desktop code-signing certificates, and the store fees for publishing the wrappers are an accepted **distribution cost**. The exception covers those fees and nothing else — it does not cover a recurring build, CI or hosting service, is not a runtime dependency, introduces no subscription and no per-user fee, and the PWA and self-hosted installs still need no paid service.
- **The install path is untouched.** A wrapper is a client, not a host: the Rust server, its installer and the 512 MB / 1 vCPU envelope stay as they are. Whether native push needs a server-side sender — the user's APNs/FCM credentials, or an intermediary — is not decided here; R76 must answer it before any server change.
- **Guardrail.** `no-native-apps` is superseded by `native-wrapper-keeps-web-parity`: the web client stays complete, the shared client stays free of native imports, and every native enhancement has a web fallback.

## Why

The owner wants the product reachable as a home-screen and desktop app with native push and real background execution; on iOS the browser cannot run the app in the background, Web Push reaches only an installed PWA (fact 0016), and its delivery is best-effort. Optional wrappers serve that without giving up zero-install web access, self-hosting or the MIT license. Keeping the native code in its own package preserves the old guardrail's real intent — that the web build must never depend on a platform — while allowing the app-store presence the owner asked for.

## Rejected

| Option | Why not |
|---|---|
| Stay web-only | Leaves native push, background execution and store discovery unreachable |
| A separate native UI codebase | Drift and double maintenance; one Svelte build is the point |
| Capacitor or Electron imports inside `packages/client` | Makes the web bundle depend on native code and breaks the zero-install path |
| Electron for mobile | Does not target iOS or Android |
| Capacitor alone for all five targets | Capacitor itself targets iOS, Android and web; desktop needs the Electron extension |
| Wrappers as the primary distribution | Drops the zero-install PWA and self-hosted users |

## Consequences

- A new workspace `packages/native` holds the Capacitor projects, the Electron target and the native capability implementations; the adapter interface and web no-op live in `packages/client`.
- CI gains a gate (`scripts/check-native-parity.mjs`, added in its own PR) that fails when `packages/client` imports a native module or depends on one.
- Desktop distribution depends on `@capacitor-community/electron`. The wrapper stack is pinned to Capacitor 8.x and `@capacitor-community/electron` 5.0.1. R76 verifies that pair before starting; the check is a precondition, and a failure blocks R76 rather than changing the target inside the step. A replacement desktop target needs an amendment to this decision.
- Native push adds Apple (APNs) or Google (FCM) as a notification intermediary and introduces device tokens and notification metadata as a new data category. The payload rule is unchanged: no agent text leaves the device (`notifications-carry-no-agent-text`), and the privacy notice must name the tokens before a wrapper ships.
- Signing keys, store credentials and provisioning profiles are release secrets and never logged (`security-secrets-handling`).
- The web client keeps its own gates: the browser matrix (R36) and the real-device pass (U16) still decide whether the PWA is done. Wrapper checks are labelled per platform (`a-phone-check-names-its-platform`).
- Store acceptance is not guaranteed. App Review guideline 4.2 (minimum functionality) treats a thin WebView shell as a rejection risk and Google Play applies similar webview guidance, so R76 must show native capability (background execution, push, offline start, bundled assets) and pass a submission dry run before store distribution counts as available.
- Decision 0005 is not reactivated: it stays deferred under 0051, and the web client's persistence remains tmux plus reconnect and replay. Only its rationale against a native wrapper is retired, which the guardrail replacement above records.
- The 512 MB / 1 vCPU envelope (guardrail `resource-constraints`) is about the server and is unchanged; a wrapper runs on the user's device.

## See also

[[0002-svelte5-client-framework]] (Svelte 5 client), [[0005-ios-mitigation-strategy]] (iOS persistence, deferred under 0051), [[0010-monorepo-cargo-pnpm]] (monorepo), [[0011-mit-license]] (MIT), [[0052-both-hosting-modes]] (hosted and self-hosted client), [[0062-device-key-credentials]] (device key and certificate), [[0076-native-releases-and-updater]] (server binary release signing), [[0112-browser-floor-ios-16-4-chrome-111]] (browser floor), guardrails `native-wrapper-keeps-web-parity`, `notifications-carry-no-agent-text` and `no-paid-services`.
