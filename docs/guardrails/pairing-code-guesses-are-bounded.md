---
id: pairing-code-guesses-are-bounded
title: Guesses of the short pairing code are rate-limited, burned and answered only to private sources
status: active
governed-by: 0128-a-short-pairing-code-is-a-rate-limited-pake-secret
grounded-in: [0021, 0117, 0123]
derivation-note: Given 0128 (a 40-bit code on an unauthenticated LAN endpoint) and FACT-0021 (unauthenticated peers need caps), the endpoint must bound guesses and cost.
tags: [security, lan, server, guardrail]
---

# Guardrail: Pairing Code Guesses Are Bounded

## Rule

1. A code **MUST** expire after 15 minutes and be accepted once.
2. Per source IP, 5 failures in a minute **MUST** block that source for 1 minute, doubling to at most 1 hour.
3. After 20 failures in total, from any sources, the code **MUST** be burned.
4. The endpoint **MUST** answer only RFC1918, ULA and link-local sources, and a reply **MUST NOT** be larger than its request.
5. A wrong code **MUST** get a bare refusal with no detail.
6. The handler **MUST** be covered by a fuzz test, and the half-open cap of FACT-0021 applies.

## Tests Required

| Test | Description |
|---|---|
| `code_expires_and_is_single_use` | A code fails after 15 minutes and after one success. |
| `per_source_backoff` | The sixth failure in a minute from one source is blocked and the block doubles. |
| `code_burns_after_20` | The 21st guess fails even with the right code. |
| `public_source_unanswered` | A public source address gets no reply. |
| `reply_not_larger_than_request` | No reply exceeds its request in bytes. |
| `pairing_handler_fuzz` | Random input never panics and never yields a reply for a wrong code. |
