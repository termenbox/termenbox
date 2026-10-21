---
id: relays-carry-only-ciphertext
title: A relay or tunnel carries only end-to-end ciphertext keyed to the server
status: active
governed-by: 0123-connections-are-a-raced-set-of-signed-candidate-paths
grounded-in: [0057-webrtc-direct-transport]
derivation-note: Given 0057 (no tunnel provider can read the traffic) and 0123 (a relay is an untrusted rung), any path through a provider's TLS must add its own handshake pinned to the server key.
tags: [security, crypto, transport, guardrail]
---

# Guardrail: Relays Carry Only Ciphertext

## Rule

A path that passes through a third party that terminates TLS (a tunnel, a relay) **MUST** run an end-to-end handshake pinned to the server key inside it before any terminal data or credential is sent. Without it the path **MUST NOT** be offered.

## Tests Required

| Test | Description |
|---|---|
| `tunnel_path_requires_inner_handshake` | A WebSocket path whose peer fails the pinned handshake sends no data and no credential. |
| `relay_sees_no_plaintext` | A recording relay stub captures only ciphertext for a full session. |
