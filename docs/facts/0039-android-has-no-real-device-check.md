---
id: 0039
title: Android is in the support floor but has never had a real-device check
status: active
tags: [client, mobile, android]
date: 2026-10-16
kind: environmental
governed-by: 0107
last-verified: 2026-10-16
---

# Android is in the support floor but has never had a real-device check

The support floor lists Chrome Android 100+ (guardrail `minimum-browser-support`, fact 0003). The owner tests on an iPhone only (roadmap cold start); `docs/skills/mobile-ui-device-check.md` is iPhone-specific; emulated Chrome is never claimed as a phone check. So no Android behaviour (install prompt, back gesture, Web Push, dictation, keyboard resize) has been verified on a device as of 2026-10-16.

The soft-keyboard bug in issue #250 was diagnosed from the xterm.js source and fixed with Chromium-emulated coverage (fact 0050); that does not count as an Android phone check.
