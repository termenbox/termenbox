---
id: 0123-connections-are-a-raced-set-of-signed-candidate-paths
title: A server is one identity reached over a raced set of signed candidate paths, so every topology connects without a mode choice
status: draft
track: product
tags: [transport, network, security, deployment, roadmap, lan]
date: 2026-10-21
---

## Decision

The server's identity is its ed25519 key and pinned DTLS fingerprint (0057). Addresses are replaceable **candidates** of a type: `lan`, `wan4`, `wan6`, `overlay`, `tunnel`. The client races the candidates it knows, staggered 150-300 ms and cheapest first, keeps the first that connects, and remembers the winner per server. The user never picks a mode.

### Candidate bundle

- Signed by the `ServerKey` (same key as directory records), with an increasing `seq` and an expiry. The canonical bytes are an explicit byte layout with shared Rust/TypeScript test vectors (not JSON).
- Delivered **in-band** after any authenticated connection and synced to the owner's devices (0068). `lan` candidates never go to the public directory.
- A first contact with no directory path (offline server, or CGNAT before a relay exists) uses a **pairing link**: `https://<client>/#/pair/<payload>` plus a QR printed on the host. The fragment never reaches a host. Trust in it comes from the channel it arrives on, as for claim links (0117).

### Path ladder (every rung free)

| Rung | Path | User setup |
|---|---|---|
| 0 | LAN candidates (also the answer where the router lacks NAT hairpin) | none |
| 1 | Direct: public IPv4 or IPv6; UPnP/NAT-PMP/PCP opens the port on routers that allow it | none |
| 2 | Hole punch: public STUN for discovery, the directory as mailbox, full ICE on the server | none; built only if measured to be worth it (0124) |
| 3 | Relay: the quick-tunnel fallback (R33), carrying an inner end-to-end handshake pinned to the server key | none if bundled; deferred while it needs an account |
| 4 | Overlay (Tailscale, WireGuard): its address is just another candidate | the user's own install |

Rungs 0, 1 and 4 need no new server transport. Rung 3 is deferred until a one-line setup exists. Until then a client away from a CGNAT server gets a diagnostic that names the missing relay and offers an overlay.

### Continuity and diagnosis

- The session lives in tmux; the transport is disposable. On a network change or a ~2 s heartbeat miss the client re-races and re-attaches with no action, and shows the active path ("Direct, LAN", "Tunnel").
- When every path fails, the client lists each path tried and why it failed, with one suggested fix.
- At start the server tests its own reachability (IPv6, UPnP, CGNAT by WAN-address comparison) and prints its topology and next step.

### Topologies covered

Server: S1 public UDP-reachable, S2 public but UDP blocked, S3 port-forwardable, S4 CGNAT, S5 IPv6, S6 on an overlay, S7 no internet, S8 inside a NAT'd VM (WSL2 NAT mode, VirtualBox NAT, Docker Desktop: the server advertises an address internal to the VM, so a LAN device cannot reach it until the VM is bridged or mirrored, or a port is forwarded; FACT-0053). Client: C1 remote, C2 same LAN, C3 restricted network, C4 on the overlay. Transient: roaming, sleep, server IP change, restart, stale directory, LAN to WAN switch.

Not solvable without a relay or overlay: S4 or S7 with the client away from the server's network. S8 is solved by the user's setup (WSL's `networkingMode=mirrored`, a bridged NIC, an overlay), not by a relay; the installer says so when it detects WSL.

## Why

A user with a home server has no path today unless the server has a reachable public UDP port. A LAN-only link (the first draft of this decision) fixes only the same-network case and goes stale when DHCP changes. Treating addresses as hints around one pinned identity covers every case with one mechanism, and keeps trust in the key, not in any address or intermediary.

## Zero trust

The directory, mailbox, STUN, relay and LAN are untrusted. Signaling messages are signed and carry a nonce and expiry. A relay sees only ciphertext. The server limits unauthenticated peers (per-source rate, half-open cap, FACT-0021) before it spends anything on them. A leaked bundle grants no access: the device certificate is still required.

## Consequences

- Amends 0057 (proposed there): optional STUN, and full ICE on the server if rung 2 is built.
- Replaces the `termenbox://` link in the first draft: a custom scheme cannot be registered from a web manifest and does not work on iOS Safari (to confirm in the spike).
- The R33 tunnel carries WebSocket over a provider's TLS; it needs the inner end-to-end handshake before it can claim 0057's "no tunnel provider can read the traffic".
- Plan: R62.
- 0126 extends the pairing link: its payload also carries the one-time claim token, and the installer prints it on every run.

## Open risks

| Risk | Mitigation |
|---|---|
| iOS Safari local-network permission and WebRTC to RFC1918 from an HTTPS page | Real-device spike first (R62 step 1) |
| Probing the same RFC1918 range on a foreign network | Fingerprint pin makes it safe; stagger and cap candidates |
| Hole punching fails on symmetric mobile CGNAT | Measure (0124) before building full ICE |
| Free STUN or tunnel terms change | Candidate types are independent; each rung is optional |
