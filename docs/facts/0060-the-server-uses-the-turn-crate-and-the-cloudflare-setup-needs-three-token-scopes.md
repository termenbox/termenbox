---
id: 0060
title: The server uses the turn crate for its relay, and the Cloudflare setup needs three token scopes
status: active
kind: observation
governed-by: 0140-assisted-connection-runs-on-the-owners-cloudflare-account
tags: [turn, cloudflare, durable-object, api-token, transport]
date: 2026-10-26
last-verified: 2026-10-26
---

# TURN client choice and Cloudflare setup surface, checked 2026-10-26

## What is true

- **TURN client:** use the `turn` crate 0.17.2 (webrtc-rs). A prototype outside the repo minted a credential, allocated a relay on `turn.cloudflare.com:3478` over a plain tokio UDP socket and got `104.30.147.203:37966` in 23 ms. The crate owns refresh and permissions. Over the server's current tree it adds about 29 crates, mostly `stun`, `webrtc-util`, `nix`, `futures`, `md-5` and `icu_*` pulled by `url`; the release prototype binary, which includes reqwest, is 6 MB.
- **Hand-rolled was not built.** The server already has `hmac`, `sha1`, `ring` and `aws-lc-rs`, so a client is feasible, but Refresh, CreatePermission and 438 stale-nonce retries are effort the crate already covers. `stun-rs` and `stun_codec` are message codecs only, with no allocation logic.
- **API token scopes that worked:** Workers Scripts Edit, Account Settings Read, Cloudflare Calls (TURN keys). TURN key create returns `uid` and `secret`.
- **Durable Object:** a SQLite-backed Durable Object accepted a WebSocket (101) through the REST API with no `wrangler`.
- **Credential endpoint:** works, but returns 403 (error 1010) without a `User-Agent`.

## Not verified

- Free plan: the owner confirmed on 2026-10-26 that the test account is on the Workers Free plan, so the Durable Object WebSocket and credential results above hold on it.
- No Android or non-symmetric-network punch (see FACT-0059).
- Two crypto libraries: `turn` uses `ring`, `str0m` uses `aws-lc-rs`; `ring` is already in the tree via rustls.
