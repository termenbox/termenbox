---
id: 0123-add-lan-link-mode
title: Add LAN-link mode for off-grid LAN connections to a server with no public IP
status: active
track: product
tags: [transport, network, security, deployment, roadmap, lan]
date: 2026-10-21
accepted-by: owner
---

## Decision

Add a **LAN-link** artifact — a signed, self-contained structure that carries everything a client needs to connect directly to the server on the local network without the directory.

### LANLink wire type

```rust
struct LANLink {
    server_id: String,              // base64(server public key) — same as directory
    seq: u64,                       // monotonic, persisted to ~/.local/share/termenbox/lan_link_seq
    lan_addrs: Vec<IpAddr>,         // up to 4 IPv4 RFC1918 addresses (see enumeration rules)
    udp_port: u16,                  // the WebRTC UDP port the server is bound to
    fingerprint: String,            // base64(SHA-256 of server's DTLS cert DER)
    sig: String,                    // ed25519 signature over canonical bytes, by ServerKey
}
```

Signed with the **same `ServerKey` (ed25519)** used for directory records (`directory.rs`). Canonical serialization prefix: `"termenbox-lan-link-v1"`.

### Two modes

| Mode | Trigger | Directory publish | Distribution |
|------|---------|-------------------|--------------|
| **Combined** (default) | neither flag | Yes — normal record + LAN-link emitted in-band after first auth + owner-sync propagation (DECISION-0068) | Paste/QR from client, or owner-sync to other devices |
| **LAN-only** | `TERMENBOX_LAN_ONLY=1` | **No** — directory process never spawned | Paste/QR only; no directory involvement |

### Address enumeration (server side)

- IPv4 **RFC1918 only** (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`).
- Skip: loopback, link-local (`169.254.0.0/16`), Docker/bridge (`docker*`, `br-*`, `veth*`, `cni*`), Tailscale (`tailscale*`), WireGuard (`wg*`), tap/tun (`tap*`, `tun*`).
- Maximum 4 addresses, sorted by prefix length (more specific first).
- Override: `TERMENBOX_LAN_ADDRS="192.168.1.5,10.0.0.7"` — comma-separated, validated against above rules.

### Client connection logic

1. Parse `LANLink` (from deep link, QR, or in-band push).
2. **Verify signature** against pinned `server_id` **BEFORE** preview/connect/cache.
3. Parallel race to all `lan_addrs` (WebRTC data channel per address).
4. 3-second timeout per address.
5. **Combined mode**: on all-LAN failure, fall back to public address from directory record.
6. **LAN-only mode**: on all-LAN failure, show diagnostic with the addresses that were tried (no public fallback).

### UX: Preview-and-confirm (per DECISION-0117)

- **Paste link**: `termenbox://<server_id_b64>/<lan_addr_b64>/<udp_port>/<fingerprint_b64>/<seq>/<sig_b64>`
- **QR code**: same payload, rendered as PNG.
- **Preview screen**: shows server name, LAN addresses, port, fingerprint; user confirms before connect.
- Signature verified **before** preview renders.

### Why

A user with a home server behind CGNAT / no public IP today has **no path** to connect. The transport (DECISION-0057) and auth (DECISION-0062) already work purely locally — only the bootstrap artifact was missing.

### Reuses

- **DECISION-0057**: WebRTC direct transport (unchanged)
- **DECISION-0062**: Device-key auth (unchanged)
- **DECISION-0117**: Claim link preview-and-confirm UX pattern
- **DECISION-0068**: Owner-sync for LAN-link propagation
- **DECISION-0118**: Reserved UDP port range
- **DECISION-0056**: Directory record format — **unchanged**; LAN addresses never appear in directory

### Tradeoffs: Combined vs LAN-only

| Aspect | Combined | LAN-only |
|--------|----------|----------|
| Public IP required | No (but used if available) | No |
| Directory publish | Yes | **No** |
| Server internet needed | For directory sync only | **Never** |
| Distribution | Directory + in-band + paste/QR | Paste/QR only |
| Bootstrap | Directory record → LAN-link in-band | LAN-link only |
| Failure mode | LAN race → public fallback | LAN race → diagnostic |

### Consequences

**Files touched:**
- `packages/server/src/rtc.rs` — emit LAN-link in-band, parallel accept
- `packages/server/src/main.rs` — `--lan-only`, `--print-lan-link`, `TERMENBOX_LAN_ADDRS`
- `packages/server/src/directory.rs` — extend signing, skip spawn in LAN-only
- `packages/core/src/protocol.rs` / `packages/client/src/lib/protocol.ts` — `LANLink` type
- `packages/client/src/lib/connections.ts` — parallel race, fallback, diagnostic
- `packages/server/src/control.rs` — `GET /lan-link`, `GET /lan-link/qr`
- `packages/server/src/lanaddrs.rs` — new module for interface enumeration
- `e2e/` — new test scenarios
- `.github/workflows/ci.yaml` — new e2e job

**New crates:**
- `if-addrs = "0.7"` (MIT/Apache-2.0) — interface enumeration
- `qrcode = "0.14"` (MIT) — QR PNG generation

### Open risks

| Risk | Mitigation |
|------|------------|
| iOS PWA deep-link registration fragile | Test on real devices (S13); fallback to manual paste |
| Peer-table hygiene during parallel race | str0m half-open cap + per-source rate limit (from FACT-0021) |
| Corporate LAN blocks UDP | Diagnostic shows tried addresses; user informed |
| Leaked LAN-link reveals internal addressing | No access granted — device cert + pinned server_id still required |