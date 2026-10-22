---
id: lan-addresses-never-reach-the-directory
title: Private (LAN) addresses never reach the public directory or telemetry
status: active
governed-by: 0123-connections-are-a-raced-set-of-signed-candidate-paths
grounded-in: [0056-automatic-connectivity, 0051]
derivation-note: Given 0123 (LAN candidates travel in-band and in owner-synced data) and 0056 (the directory is a public service that cannot route to them), private addresses must not be published there in the clear; 0128 allows them in the pairing mailbox only as ciphertext keyed by the pairing code.
tags: [privacy, network, lan, guardrail]
---

# Guardrail: LAN Addresses Never Reach the Directory

## Rule

1. A `lan` or `overlay` candidate **MUST NOT** appear in a directory record or in a telemetry event (0124). A pairing mailbox object (0128) **MUST NOT** hold one in plaintext: the bundle in it is encrypted under a key derived from the pairing code.
2. With `TERMENBOX_LAN_ONLY=1` the server **MUST NOT** start the directory publisher or make any request to the directory.

## Tests Required

| Test | Description |
|---|---|
| `record_has_no_private_candidates` | A serialized `Record` contains no `lan` or `overlay` candidate, including when the bundle has them. |
| `mailbox_object_is_ciphertext` | The bytes uploaded for a bundle with `lan` and `overlay` candidates contain none of their addresses and do not parse as a bundle without the key. |
| `lan_only_makes_no_directory_calls` | With `TERMENBOX_LAN_ONLY=1` a recording stub directory receives zero requests. |
