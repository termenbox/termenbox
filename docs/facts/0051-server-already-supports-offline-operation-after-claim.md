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

Address-wise the client only needs IP, port and fingerprint (0057). Who supplies them is the open part (0123).
