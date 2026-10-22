---
id: pairing-code-guesses-are-bounded
title: The short pairing code is stretched and short-lived, and its guesses are budgeted
status: active
governed-by: 0128-a-short-pairing-code-is-a-rate-limited-pake-secret
grounded-in: [0021, 0022, 0117, 0123]
derivation-note: Given 0128 (a 40-bit code is a mailbox key and a PAKE password) and FACT-0021/0022 (unauthenticated peers need caps; the free Workers quota is shared), a guess must cost the guesser, stop at the server after a budget, and never spend the Workers quota.
tags: [security, lan, server, directory, guardrail]
---

# Guardrail: Pairing Code Guesses Are Bounded

## Rule

1. A code **MUST** expire after 15 minutes and be accepted once. Its mailbox object **MUST** be deleted on claim and **MUST** carry a signed expiry that the client enforces.
2. The code **MUST** go through a memory-hard stretch (Argon2id or scrypt) before it derives the lookup id or the key; the cost **MUST NOT** be lowered to fit a slow phone (lengthen the code instead).
3. At the server's PAKE endpoint, 5 failures from one source in a minute **MUST** block that source for 1 minute, doubling to at most 1 hour. After 20 failures in total, from any sources, the code **MUST** be burned.
4. A wrong code **MUST** get a bare refusal with no detail, and a reply **MUST NOT** be larger than its request. A miss, an expired object and an object that does not decrypt **MUST** be indistinguishable to the client.
5. Mailbox writes **MUST** be signed by the server key, limited per key and capped by an atomic daily counter; at the cap the installer **MUST** still print the link and QR. Mailbox reads **MUST NOT** run in a Worker.
6. The handler **MUST** be covered by a fuzz test, and the half-open cap of FACT-0021 applies.

The first version of this guardrail answered only private sources. The code is now checked over whichever direct path won (a public VPS included), so the budget in rule 3 is the bound.

## Tests Required

| Test | Description |
|---|---|
| `code_expires_and_is_single_use` | A code fails after 15 minutes and after one success; the object is deleted on claim and a client rejects a blob past its signed expiry. |
| `code_is_stretched` | The lookup id and key are not a plain hash of the code, and the stretch parameters are at or above the minimum. |
| `per_source_backoff` | The sixth failure in a minute from one source is blocked and the block doubles. |
| `code_burns_after_20` | The 21st guess fails even with the right code. |
| `reply_not_larger_than_request` | No reply exceeds its request in bytes. |
| `misses_are_indistinguishable` | An unknown id, an expired object and a wrong-key object give the client the same result. |
| `mailbox_write_needs_server_signature` | An upload with a missing or foreign signature is refused; the per-key limit and the daily cap refuse further writes. |
| `pairing_handler_fuzz` | Random input never panics and never yields a reply for a wrong code. |
