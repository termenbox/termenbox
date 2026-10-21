---
id: lan-link-is-signed-by-server-key
title: A LAN-link Is Signed by the Server's ed25519 Key and Verified Against the Pinned server_id
status: active
governed-by: [0123]
grounded-in: [0056, 0021, 0051]
tags: [security, auth, network, lan, guardrail]
---

# Guardrail: LAN-link Is Signed by Server Key

## Rule

**Every LAN-link artifact MUST be signed by the server's ed25519 `ServerKey` (the same key used for directory records). The client MUST verify the signature against the pinned `server_id` BEFORE showing a preview, attempting a connection, or caching the LAN-link.**

## Rationale

- The `server_id` is the trust anchor (base64 of the server's public key).
- A LAN-link carries LAN addresses and the DTLS fingerprint — it is a bootstrap credential.
- Without signature verification, a tampered LAN-link could redirect the client to an attacker-controlled address while presenting the real server's fingerprint.
- The directory record (DECISION-0056) is already signed by this key; LAN-link uses the same trust chain.

## Tests Required

| Test | Description |
|------|-------------|
| `server_signature_verifies` | Server-side: generated LAN-link signature verifies against the same `VerifyingKey` used for directory records. |
| `tampered_lan_addr_fails` | Unit: flip one bit in `lan_addrs[0]` → signature verification fails. |
| `e2e_corrupted_sig_never_previews` | E2E: serve a LAN-link with a corrupted `sig` field → preview screen never renders; client shows "invalid link" error. |