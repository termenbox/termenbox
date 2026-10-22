---
id: 0052
title: CPace over ristretto255 fits the pairing code on both ends with no WASM; the phone half of the spike is still open
status: draft
kind: measurement
governed-by: 0128-a-short-pairing-code-is-a-rate-limited-pake-secret
tags: [pake, security, client, server, onboarding, spike]
date: 2026-10-22
last-verified: 2026-10-22
---

# CPace over ristretto255 fits the pairing code; the phone half is open

Plan step 1.1 (R65). **The PAKE half is done; the iPhone and Android half has not been run, so the step stays `[ ]`.** Throwaway code in the session scratchpad, not in the repo.

## PAKE verdict: a fit

| Question | Result |
|---|---|
| Rust crate | `pake-cpace` 0.1.7 (jedisct1, ISC, `no_std`, no unsafe, curve25519-dalek 4.1 which is already in `Cargo.lock` as 4.1.3). Port of CPace draft-01; last release 2023-12. `spake2` 0.4 (RustCrypto) also builds; not chosen, since no browser side matches it and it needs its own port. |
| Browser | Pure TypeScript with `@noble/curves` 2.4.0 (`ristretto255.Point`, `ristretto255_hasher.deriveToCurve`) and `@noble/hashes` sha512. About 35 kB minified, 14 kB gzip for the whole ed25519 module, `--target=safari16`. No WASM. |
| Interop | A 40-line TS client (draft-01 byte layout of `pake-cpace`) against the Rust `step2`: with the same code both sides derived equal k1 and k2; with a code differing in the last character they differed. |
| Cost | Rust, both sides of one handshake: 0.31 ms (CPace), 0.46 ms (spake2), on the build host. A JS scalar multiplication takes 2.4 ms in Node 24; not measured on a phone. |
| Memory | Nothing measurable against 512 MB: a handshake holds one scalar and three points. |

## Findings that change the design

- **The crates give a shared key, not authentication.** A wrong code yields different keys silently. The claim proof needs a key-confirmation step (a MAC of the transcript under k1/k2 from each side) and must put the server's DTLS fingerprint in the associated data `ad`, so the key is bound to the channel.
- **`pake-cpace` is a single-maintainer port of an old draft.** The current spec is draft-irtf-cfrg-cpace-21 (April 2026 in a mirror; the search found no RFC number). Using the crate pins us to draft-01's layout; implementing the current draft on curve25519-dalek and noble, with the draft's test vectors shared by Rust and TypeScript as `bundle` does, is the alternative. Decided in 0128: the current draft.
- **No WASM in the client today.** The CSP in `packages/client/public/_headers` is `script-src 'self'` with no `'wasm-unsafe-eval'`, and WASM stays deferred in the architecture summary. The pure-TypeScript path avoids both.
- **Possible floor problem, not checked on a device:** `lib/bundle.ts` verifies bundles with WebCrypto `Ed25519`. The browser floor is iOS 16.4 and Chrome 111 (0112). I believe WebCrypto Ed25519 arrived later than that (Safari 17, Chrome 137, Firefox 129 are my recollection; a search did not confirm versions). If so, bundle verification fails on the floor and needs the noble ed25519 fallback, which this spike would already have in the bundle. To confirm on the MDN compatibility table and on an iOS 16.4 device.

## Not done

- iPhone (Safari and PWA) and Android: reaching a LAN server over WebSocket and WebRTC from the hosted HTTPS client, the Local Network prompt, and what the page sees when it is denied. Record each result labelled by platform (GUARDRAIL-a-phone-check-names-its-platform).
- JS timing of the handshake on a phone.
