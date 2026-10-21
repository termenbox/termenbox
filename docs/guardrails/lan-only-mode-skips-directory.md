---
id: lan-only-mode-skips-directory
title: TERMENBOX_LAN_ONLY Mode Does Not Publish or Update Directory Records; the Directory Never Sees LAN Addresses
status: active
governed-by: [0123]
grounded-in: [0056, 0051]
tags: [privacy, network, deployment, lan, guardrail]
---

# Guardrail: LAN-only Mode Skips Directory

## Rule

When `TERMENBOX_LAN_ONLY=1` is set:

1. The server **MUST NOT** spawn the directory client / publisher.
2. The server **MUST NOT** make any HTTP calls to the directory service (Cloudflare Pages Functions / D1).
3. The `Record` type sent to the directory **MUST NOT** gain a `lan_addrs` field — LAN addresses never appear in directory records.
4. The LAN-link artifact is the **only** channel for LAN addresses.

## Rationale

- LAN-only deployments are explicitly off-grid; the directory is a public service.
- Leaking RFC1918 addresses to a public directory violates network privacy and serves no purpose (the directory cannot route to them).
- FACT-0051 confirms the server operates fully offline after claim — directory is optional for connectivity.

## Tests Required

| Test | Description |
|------|-------------|
| `wiremock_zero_http_calls_lan_only` | Integration: start server with `TERMENBOX_LAN_ONLY=1`; `wiremock` recorder captures zero outbound HTTP requests to directory endpoints. |
| `unit_lan_addrs_not_in_record` | Unit: serialize a `Record` → JSON does not contain `lan_addrs` field; `LANLink` is a separate type. |