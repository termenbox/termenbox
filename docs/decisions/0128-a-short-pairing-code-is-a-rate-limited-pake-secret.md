---
id: 0128-a-short-pairing-code-is-a-rate-limited-pake-secret
title: A short pairing code is a mailbox key and a PAKE password, bounded by its life, a stretch and a guess budget
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [onboarding, lan, security, protocol, server, directory]
date: 2026-10-22
---

## Decision

Rewrites the first version of this decision (a typed code sent to LAN addresses the browser guesses), which could not work: a browser needs the server's DTLS fingerprint to open WebRTC, so an IP and a code are not enough, and a page cannot scan a subnet. The code is now a lookup key for the server's signed candidate bundle.

- **The code.** The installer prints an 8-character base32 code (about 40 bits, e.g. `K7QM-4TXD`) beside the link and a short fingerprint of the server. It lives 15 minutes and works once.
- **The mailbox.** A server with a directory path uploads its signed candidate bundle (0123), encrypted, under a lookup id. `K = stretch(code)` (Argon2id or scrypt, a public context string as the salt, about 100 ms on a desktop; the parameter is set from a phone measurement when built). `id = HKDF(K, "id")`, `key = HKDF(K, "enc")`, AEAD under `key`. The claim token is never in the blob.
- **Typing a code.** The client derives `K`, fetches `id`, decrypts, checks the bundle signature and its expiry, and races the candidates as 0123 does. A wrong code, an expired blob and an unknown id look the same: not found.
- **Claim proof.** Over the direct connection the client and server run CPace (the current CFRG draft, ristretto255, SHA-512) with the code as the password and the server's DTLS fingerprint and ids as associated data, then confirm the key with a MAC from each side. The server answers under the guess budget of guardrail `pairing-code-guesses-are-bounded`. The result: a fake server that does not know the code cannot complete it, and the key is bound to the channel. A scanned or pasted link still carries the claim token, a bearer secret (0117).
- **Confirm shows the fingerprint.** The step shows the server name and the short fingerprint the installer printed, and asks the user to compare them when the server was found by a code. This catches a mailbox or directory that substituted a blob after cracking the code (the attacker's server has a different fingerprint).
- **Where the mailbox lives (free tier, FACT-0022).** Writes go through one Worker route: a signature by the server key, a per-key limit and an atomic daily counter in D1. Reads are plain GETs on a public R2 bucket under a custom domain, so they cost no Worker requests and a flood of guesses cannot starve login or claims. Objects are deleted on claim; R2 lifecycle rules remove the rest within a day, so the client checks the signed expiry.
- **Not for LAN-only.** With `TERMENBOX_LAN_ONLY=1` the server makes no directory request (guardrail `lan-addresses-never-reach-the-directory`), so it has no mailbox entry; such a server is added by its link or QR.
- **Dropped:** browser-side guessing of LAN addresses, and a typed IP plus code.
- If a later check finds the stretch too slow on the slowest supported phone, the code gets longer, not the stretch weaker.

## Stated limits

- Anyone who holds a blob (the directory operator, or whoever finds the id) can test codes against it offline. Cost per guess is the stretch, and the code is dead after 15 minutes. Estimated, not measured: at 100 ms a full search is about 3,500 CPU-years; at 1 ms it is about 35. Cracking an average live code inside the window takes about 6·10⁷ cores at 100 ms and about 6·10⁵ at 1 ms. After the window a cracked blob shows addresses and the fingerprint, not a claim.
- The mailbox sees the ciphertext, the id, the uploader's address and the timing; the bucket is not listable.
- A flood of lookups has no hard cap on any free path. Here it lands on R2 reads (about 7.8M a month of headroom, FACT-0022), not on the Workers quota. Objects can stay up to a day.
- A stranger on the LAN can burn a code by guessing at the server's PAKE endpoint; the cost is one installer re-run. Guessing is bounded to about 20 in 2^40 per code.
- The link and QR are bearer secrets: whoever holds one and confirms first becomes owner (0117). A screenshot or shared scrollback is a larger exposure than guessing.
- The PAKE endpoint is unauthenticated attack surface; the limits do not protect against a parsing or logic bug in it. It gets a minimal reply and fuzz tests.
- CPace libraries give a shared key, not authentication: wrong codes yield different keys silently, so key confirmation is part of the protocol, not optional (FACT-0052).

## Why

A long link is hard to carry from a terminal to a phone without a camera, and a bare command (0126) leaves the browser nothing to wait for. A short code is typeable, but its entropy is low, so it is only safe as a key behind a stretch, a short life and a hard guess budget, with the fingerprint shown as the final check.

Not chosen: a code compared directly on the server (a LAN attacker could impersonate the server and learn the claim); the code sent to guessed LAN addresses (cannot work in a browser); a Worker serving the lookups (each guess would spend the shared Workers quota); ntfy.sh, Magic Wormhole or Nostr relays as the mailbox (a third party's quota and availability, or a publicly enumerable store); no short code (the link and QR remain the fallback).

## Consequences

- Needs, before the code field is built: the mailbox routes and R2 bucket in `infra/termenbox-cloud` (check the provider supports an R2 custom domain), CPace with the draft's test vectors shared by Rust and TypeScript, the installer printing the fingerprint, and the phone spike for the stretch cost (R65 step 1.1).
- Amends guardrail `lan-addresses-never-reach-the-directory` (ciphertext only) and rewrites `pairing-code-guesses-are-bounded`.
- Plan: R65.
