---
id: 0057-webrtc-direct-transport
title: The browser reaches the server over a direct WebRTC data channel
status: active
date: 2026-10-11
tags: [transport, networking, security, roadmap]
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
supersedes: [0001]
---

## Decision

The primary transport is a WebRTC data channel straight from the browser to the VPS ("webrtc-direct"):

- The server has a public IP and one open UDP port, runs ICE-lite with a fixed, persisted DTLS certificate, and multiplexes all clients on that port.
- No hole-punching: no STUN, no TURN, no signaling server. The client builds the remote session description locally from the server's IP, port and DTLS fingerprint, all taken from the signed directory record (0056). The client's own NAT needs nothing, because its outbound packet opens the path.
- DTLS is end-to-end between browser and server and the client pins the fingerprint, so no CA, no domain and no tunnel provider is involved and none can read the traffic. This reverses 0053's deferral of end-to-end encryption.
- GitHub auth and owner binding (0053) run in the first data-channel message, as they do over WebSocket now.
- Session logic is transport-agnostic. The WebSocket transport stays as the test and tunnel transport.
- The quick-tunnel supervisor (plan 8.3) is a later fallback for networks that block UDP or VPSes with no reachable UDP port. Until it exists, an unreachable server shows a clear diagnostic.

0056's directory and onboarding flows A and B stay; its record changes from a tunnel URL to IP, UDP port and fingerprint, and its choice of quick tunnel as the default transport is superseded here.

## Why

Free relays reach about 5-30 daily active users (FACT-0020); tunnels depend on a service documented as testing-only (FACT-0018); our own domains hit DNS and certificate limits (FACT-0019). A direct channel costs nothing per user at any scale.

## Open risks (plan 8.7)

Rust library support for ICE-lite on one port; browser support for locally built descriptions on iOS Safari 16+; NAT-mapped public IPs on some clouds; UDP blocked by provider firewalls or some networks; connection-flood protection for unauthenticated peers on a 512MB server.

## Amendment (accepted 2026-10-26 by 0140)

From 0123, which adds candidate paths for servers without a reachable public UDP port:

- **STUN is allowed as an optional, best-effort hint** for address discovery (free public servers, no account). It is never trusted for identity: the fingerprint pin and signed records still decide who the server is.
- **Full ICE on the server** replaces ICE-lite only if hole punching (0123 rung 2) is built after measurement (0124). ICE-lite cannot start connectivity checks, so it cannot punch.
- **Still no TURN or signaling server of the project's.** 0140 allows the owner's own Cloudflare TURN key and signaling Worker, deployed to the owner's account; the project's directory is not used.
