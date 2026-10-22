---
id: 0128-a-short-pairing-code-is-a-rate-limited-pake-secret
title: A short pairing code finds a LAN server only through a PAKE and a bounded number of guesses
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
expires: dropped if the PAKE spike (R65 step 1) finds no fit
tags: [onboarding, lan, security, protocol, server]
date: 2026-10-22
---

## Decision

- The installer prints an 8-character base32 code (about 40 bits, e.g. `K7QM-4TXD`) beside the link. A client with no camera or link can type it; it sends the code to the candidate LAN addresses it can guess, or to the directory when online.
- The server **proves it knows the code with a PAKE** (CPace or similar), so the client authenticates the server and a fake server on the LAN cannot test guesses offline. A bare comparison or MAC of the code is not enough, because the client cannot pin a fingerprint it has not yet seen.
- Guesses are bounded (guardrail `pairing-code-guesses-are-bounded`): 15-minute lifetime and one use; 5 failures per source IP per minute then a block doubling from 1 minute to 1 hour; 20 failures in total and the code is burned; only private sources are answered; a reply is never larger than the request.
- If the PAKE spike finds no fit, the short code is dropped and the pasteable link remains.

## Stated limits

- A stranger on the LAN can burn a code by guessing; the cost is one installer re-run.
- Guessing is bounded to about 20 in 2^40 per code, which is negligible but not zero.
- The link and QR are bearer secrets: whoever holds one and confirms first becomes owner (0117). A screenshot or shared scrollback is a larger exposure than guessing.
- The new unauthenticated endpoint is attack surface in its own right; the limits do not protect against a parsing or logic bug in it. It gets a minimal reply and fuzz tests.

## Why

A long link is hard to carry from a terminal on one machine to a phone without a camera. A short code is typeable, but its entropy is low, so it is only safe behind a PAKE and a hard attempt budget.

Not chosen: a short code compared directly on the server (a LAN attacker could impersonate the server and learn the claim); no short code (kept as the fallback if the spike fails).

## Consequences

- Plan: R65.
