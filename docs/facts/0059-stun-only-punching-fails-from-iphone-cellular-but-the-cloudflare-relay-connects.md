---
id: 0059
title: STUN-only punching failed from an iPhone on cellular to a WSL2 NAT server, and the Cloudflare TURN relay connected
status: active
kind: observation
governed-by: 0141-assisted-connection-runs-on-the-owners-cloudflare-account
tags: [network, nat, stun, turn, wsl, ios, cellular, transport]
date: 2026-10-26
last-verified: 2026-10-26
---

# STUN punching and TURN relay, measured 2026-10-26

Measured with throwaway scripts (a STUN binding probe; a Safari `RTCPeerConnection` page and an `aiortc` 1.15 peer). The peer is not the Termenbox server, so this tests the networks, not the server's ICE code (the server is still ICE-lite, 0057).

## What is true

| Platform | Network | STUN | NAT mapping | Punch (STUN only) | Relay (Cloudflare TURN) |
|---|---|---|---|---|---|
| WSL2 2.4.13.0, kernel 5.15.167.4, mirrored mode | home Wi-Fi, public IP A | works, 29 and 43 ms | endpoint-independent, port kept | not tested | not tested |
| WSL2 2.4.13.0, NAT mode (`172.18.x`) | home Wi-Fi, public IP A | works, 17 and 53 ms | endpoint-independent, port rewritten (59287 to 59011) | n/a (server side) | answered with a relay candidate |
| Windows host | home Wi-Fi, public IP A | works, 17 and 70 ms | endpoint-independent, port kept | not tested | not tested |
| iPhone, iOS 18.3, Safari 18.3 (model not recorded) | cellular, public IP B | works | **symmetric**: one local port mapped to two public ports (18485 and 18490; earlier run 6380 and 6381) | **failed** twice; ICE failed after about 23 s | **connected**, data channel open, 10 round trips 34 to 67 ms, path `relay` |
| same iPhone | home Wi-Fi, same router as the server | works | endpoint-independent | failed, but invalid: both ends shared public IP A, so it needed router hairpinning | not tested |

Measured by the same run: time to connect over the relay was 7.8 s from the page's start, but about 7 s of that was the peer's one-second mailbox polling, so the ICE exchange itself took under a second.

## Not measured

- A punch from a phone on a non-symmetric network other than the server's own (a friend's or a public Wi-Fi). This is the case STUN-only might still serve.
- Any Android device, and a second carrier. One carrier is one data point.
- The Windows host or WSL2 on a cellular hotspot.
- The relay from the Termenbox server (the server has no TURN client yet).

## Consequences

- On this carrier, STUN-only is not enough; the relay is needed, as DECISION-0141 assumes. A symmetric NAT against a port-restricted one cannot punch.
- Cloudflare's credential endpoint returns 403 (error 1010) to the default Python user agent; the server's credential call must send its own `User-Agent`.
- The mapped port in WSL2 NAT mode differs from the local port, so the server cannot assume the two match (step 2.2).
