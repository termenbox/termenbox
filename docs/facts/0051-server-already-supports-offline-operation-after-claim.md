---
id: 0051
title: The server needs no external call to accept a connection after claim
status: active
kind: decision
governed-by: 0057-webrtc-direct-transport
tags: [transport, auth, network, deployment]
date: 2026-10-21
last-verified: 2026-10-21
---

# The server needs no external call to accept a connection after claim

Read from the code; not yet tested with the directory and internet blocked (R62 step 1 does that).

- `--host` / `TERMENBOX_HOST` defaults to `0.0.0.0` (`main.rs`), and `rtc.rs::bind_transport` binds the UDP socket there, so LAN packets are accepted on every interface.
- The server is ICE-lite (`rtc.rs`, `set_ice_lite(true)`): it answers checks but cannot start them, so it cannot hole-punch.
- Device certificates are checked locally (`auth.rs`); owner identity is in `owner.json`.

- Since R62, `TERMENBOX_LAN_ONLY=1` starts the server with no public IP lookup and no directory publisher, and `--print-pairing` prints a signed link with its LAN address. CI runs that on loopback (`tests/pairing_cli.rs`, `e2e/pair.mjs`: zero directory requests, connected over the LAN candidate). It has not been run on a phone: the iOS local-network case is still the open spike.

Address-wise the client only needs IP, port and fingerprint (0057). Who supplies them is the open part (0123).
