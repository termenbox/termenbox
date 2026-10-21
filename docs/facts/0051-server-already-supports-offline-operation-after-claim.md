---
id: 0051
title: Server Already Supports Offline Operation After Claim
status: active
tags: [transport, auth, network, deployment]
date: 2026-10-21
---

# Server Already Supports Offline Operation After Claim

The server already works offline after the initial claim flow. No transport or auth changes are required for LAN-only mode — only the address source changes.

## Transport

- `main.rs` `--host` defaults to `0.0.0.0`.
- `rtc.rs::bind_transport` binds the UDP socket on that address → the socket accepts LAN packets on any interface.

## Auth

- `auth.rs::AuthState` + `CertKeys::verify` validate the device certificate locally using the server's keys — no GitHub API call at connect time.
- Owner identity is stored in `owner.json` on the server; no external lookup needed.

## Conclusion

LAN-only mode requires **zero changes** to the transport layer or auth logic. The only difference is where the client gets the server's address and fingerprint (directory record vs. LAN-link artifact).