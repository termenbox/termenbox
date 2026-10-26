---
id: no-native-apps
title: No Native Applications
status: superseded
superseded-by: 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class
governed-by: 0011
grounded-in: 0002,0003,0010
derivation-note: MIT license + web-only product goal + Svelte PWA approach = no native app distribution
tags: [product, distribution, platform]
---

# Guardrail: No Native Applications

> **Superseded** by decision 0140 and guardrail `native-wrapper-keeps-web-parity`. The rule below is
> kept as history. Native wrappers are now optional enhancements, the PWA stays the reference client,
> and the shared web client must still never import native code.

## Rule
**MUST NOT** create, maintain, or distribute native applications (iOS .ipa, Android .apk/.aab, macOS .app, Windows .exe/.msi, Linux AppImage/Flatpak/Snap).

## Scope
- All user-facing clients must be web-based (PWA)
- No Capacitor, Tauri, Electron, React Native, Flutter, or similar wrappers
- No platform-specific code in client (no `if (isIOS)` branches for native APIs)
- Server components remain native Rust binaries (not user-facing apps)

## Rationale
- **Product goal**: "No native apps. No subscriptions. Self-hosted." (DECISION-0011)
- **Distribution**: Web avoids App Store review, fees, rejection risk, update lag
- **Maintenance**: Single codebase (Svelte + WASM) vs 3+ platform codebases
- **User friction**: Zero-install, works from any browser, shareable via URL

## Exceptions
- `termenbox-hook` CLI installer (run once on server) is a native binary — acceptable as dev tool
- Server binaries (`termenbox-server`, `termenbox-agent`) are native — not user-facing apps

## Enforcement
- CI check: `grep -r "capacitor\|tauri\|electron\|react-native\|flutter" packages/client/` → fail
- PR review: Reject any native app code in client package