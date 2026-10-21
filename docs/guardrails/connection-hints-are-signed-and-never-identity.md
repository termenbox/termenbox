---
id: connection-hints-are-signed-and-never-identity
title: Candidate bundles are signed by the server key, and no address or intermediary ever decides who the server is
status: active
governed-by: 0123-connections-are-a-raced-set-of-signed-candidate-paths
grounded-in: [0057-webrtc-direct-transport, 0051]
derivation-note: Given 0123 (addresses are replaceable hints) and 0057 (the pinned fingerprint is the identity), a bundle must be verified before use and a connection must fail unless the pinned fingerprint matches.
tags: [security, auth, network, lan, guardrail]
---

# Guardrail: Hints Are Signed and Never Identity

## Rule

1. A candidate bundle **MUST** be signed by the server's ed25519 key. The client **MUST** verify the signature, and for a known server that `seq` is higher than the stored one, before it previews, connects to or caches any candidate.
2. A connection **MUST** fail unless the DTLS fingerprint (or, over a relay, the inner handshake key) matches the one pinned for that server, whatever path or address delivered it.
3. A first-contact pairing link is trusted only as far as the channel it arrived on; the UI **MUST** show the server name and fingerprint before the first connect.

The signature gives integrity and rollback protection for a known server. It does not authenticate an unknown one: a link carries its own `server_id`.

## Tests Required

| Test | Description |
|---|---|
| `bundle_signature_verifies` | A generated bundle verifies against the directory `VerifyingKey`; Rust and TypeScript agree on shared vectors. |
| `tampered_bundle_fails` | Flipping any bit of an address, port, `seq` or fingerprint fails verification. |
| `stale_seq_rejected` | A bundle with `seq` at or below the stored one is ignored for a known server. |
| `wrong_fingerprint_never_connects` | A candidate whose DTLS fingerprint differs from the pin never reaches auth. |
| `e2e_corrupted_sig_never_previews` | A pairing link with a corrupted `sig` shows "invalid link", never the preview. |
