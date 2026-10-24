---
id: 0055
title: A waiting question is pushed again on a ladder until it is answered anywhere, and a push opens its decision card
status: active
kind: decision
governed-by: 0104-decision-cards-carry-context-and-escalate
tags: [notifications, agents]
date: 2026-10-24
last-verified: 2026-10-24
---

# The escalation ladder as built (U11, PR #291)

## What is true

- **Ladder**: one task per Attention item in `packages/server/src/push.rs`. A `needs_input` is pushed after the grace, then after the wait times the multiplier, up to the cap (defaults 10 min, x2, 1 h). It ends when the item leaves Attention, so an answer on any device stops it. `finished` and `failed` are pushed once.
- **Suppression**: quiet hours and a muted agent skip a push, the first one included, and the ladder goes on to the next rung.
- **Settings**: `push_settings_get` and `push_settings_set` over the authenticated socket (no HTTP endpoint), limits in `termenbox_core::protocol::PushSettings`, kept in `push_settings.json` (0600) beside the push subscriptions. Shown in Settings only for a server whose `push_info` has `settings: true`.
- **Quiet hours are local**: the server keeps UTC; the browser sends `utc_offset_minutes` with the settings.
- **Deep link**: the push body adds `deep_link` `/#/home?focus=<percent-encoded session key>`. Home puts that session's decision card first while it still waits. The service worker follows only `/#/` links.
- **Not built**: the lock the decision mentions ("behind the app's unlock") has nothing to attach to, since the client has no app lock. The notification click is not checked on a device (U16).

Covered by `cargo test -p termenbox-server push` and `packages/client/e2e/push-link.mjs`.
