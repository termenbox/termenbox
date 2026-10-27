---
id: 0141-assisted-connection-runs-on-the-owners-cloudflare-account
title: A server that cannot be reached directly is helped by free public STUN, a signaling Worker and a TURN key that live in the owner's Cloudflare account, never in the project's
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [transport, network, security, deployment, cloudflare, wsl]
date: 2026-10-26
amends: [0057-webrtc-direct-transport, 0123-connections-are-a-raced-set-of-signed-candidate-paths]
---

## Decision

Rungs 2 (hole punch) and 3 (relay) of 0123 are built now, as one "assisted" path for servers with no reachable public UDP port (home routers, CGNAT, WSL2 NAT mode, VirtualBox NAT; S3, S4, S8 in 0123). 0123's "only if measured to be worth it (0124)" gate is lifted for the build. 0124 telemetry still follows, to tune ordering and defaults.

- **STUN: free public servers, best effort.** Defaults `stun.cloudflare.com:3478` then `stun.l.google.com:19302`, overridable with `TERMENBOX_STUN`. Used only when the reachability check (R64) finds the server not directly reachable. Off under `TERMENBOX_LAN_ONLY` and paranoid mode. STUN never decides identity (0057 amendment, accepted here).
- **One socket.** STUN binding requests leave the same UDP socket that serves WebRTC, so the discovered mapping is the one that gets punched.
- **ICE.** Direct sessions stay ICE-lite. An assisted session uses full ICE on the same port, per session, so the common path costs nothing more.
- **Signaling lives in the owner's account.** A small Worker plus a hibernating Durable Object, deployed to the owner's Cloudflare account, relays opaque messages between browser and server. The server holds one outbound WebSocket to it. It holds no secret and no key. Messages are signed by the sender, carry a nonce and an expiry, and have a size cap; the Durable Object rate-limits per source. The Worker URL travels in the signed bundle and the pairing link.
- **TURN: the owner's own Cloudflare Realtime TURN key** (free to 1,000 GB a month, FACT-0058). The server verifies the client's device certificate (or claim token on first contact), then mints short-lived credentials through the Cloudflare API and returns them over the signaling channel. Browser and server both allocate relays, so only outbound traffic is needed on each side; this is the path that works from WSL2 NAT mode and behind symmetric NAT.
- **Spend cap.** The server counts relayed bytes and stops minting at a monthly cap (default 500 GB), so the owner is not billed past the free allowance unnoticed. Cloudflare has no hard cap of its own.
- **Setup.** `termenbox-server --setup-cloudflare` reads an API token from stdin (scopes: Workers Scripts edit, Realtime TURN edit), creates the TURN key, uploads the Worker and Durable Object embedded in the binary, prints the URL and discards the token. Only the TURN key id and its narrow credential-minting token persist, in a `0600` file in the config directory. `--teardown` removes the Worker, Durable Object and key.
- **Race order.** The client tries LAN, cached winner and direct first (0123), then the punch, and uses the relay when the punch has not connected within its window. The path report names the winner ("Direct", "Punched", "Relay").
- **No project resource.** Nothing in this path calls the project's directory, R2 mailbox or Workers quota. A server without the owner setup behaves as today.

## Why

A home or WSL2 server has no path today unless it has a reachable public UDP port (FACT-0053). Free STUN fixes most home NATs; a relay is the only fix for symmetric NAT and a NAT'd VM. Putting the relay and signaling in the owner's account keeps the project's free-tier budget (FACT-0018, FACT-0022) and its guardrail on paid services intact: any overage bills the owner, who set the cap, never the project.

Not chosen: project-minted TURN credentials through the directory (shared 1 TB allowance, overage risk, directory becomes a relay service); a polled R2/D1 mailbox for signaling (spends the shared request budget, adds seconds of latency); TURN-only with no punching (every NAT'd session spends relay bandwidth); a bundled coturn installer (a second product to run).

## Zero trust

STUN, the Worker, the Durable Object and the relay are untrusted. The DTLS fingerprint pin and the signed bundle still decide who the server is (GUARDRAIL connection-hints-are-signed-and-never-identity). A relay sees ciphertext only. A client without a valid device certificate gets no TURN credential.

## Consequences

- Amends 0057 (the STUN and full-ICE amendment is accepted; TURN is allowed, but only as the owner's own key) and 0123 (rungs 2 and 3, the Consequences, the ungated build).
- The server needs a TURN client (UDP) with allocation refresh; `str0m` is sans-IO and has none (spike in the plan).
- The client reaches the Worker over `wss:`, which the hosted CSP (`connect-src 'self' wss:`, `packages/client/public/_headers`) already allows; a plain `https:` fetch to it would need a CSP change, so the channel is WebSocket only.
- A Cloudflare account is required for the assisted path only; it stays optional, and rungs 0, 1 and 4 need none.
- Plan: R77, `docs/plans/assisted-connect-stun-turn.md`.

## Open risks

| Risk | Mitigation |
|---|---|
| Free STUN endpoints change terms or disappear (Google publishes none, FACT-0058) | Two defaults plus `TERMENBOX_STUN`; STUN is a hint, so a failure falls through to the relay |
| Punching fails on symmetric mobile CGNAT | The relay is the fallback; the plan measures the rate (step 1.1) |
| Unauthenticated floods at the owner's Worker burn the owner's free quota | Size cap, per-source rate limit in the Durable Object, and the server answers only signed requests |
| The setup token is powerful | Read from stdin once, never stored, never logged; the persisted TURN-key token can only mint credentials |
| Cloudflare changes the free TURN allowance | The cap is a setting; the server falls back to punch-only when no key is configured |
