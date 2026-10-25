---
id: no-paid-services
title: No Paid Services in Required Stack
status: active
governed-by: [0011, 0140]
grounded-in: 0001,0013
derivation-note: MIT license + self-hosted goal + VPS deployment = no external paid dependencies
tags: [product, architecture, cost]
---

# Guardrail: No Paid Services in Required Stack

## Rule
**MUST NOT** require any paid external service for core functionality.

**Scope note (decision 0140).** Platform store developer accounts (Apple, Google) and desktop
code-signing certificates are an accepted **distribution cost** for the optional native wrapper. They are
not a runtime dependency: the PWA and self-hosted installs still need no paid service, and no
subscription or per-user fee is introduced. Everything below still applies to the required stack.

## Scope
- No paid APIs (Pusher, Ably, Firebase, Supabase, etc.)
- No paid databases (PlanetScale, Neon, etc.) — use embedded/self-hosted only
- No paid authentication (Auth0, Clerk, etc.) — SSH keys only
- No paid push notification services (OneSignal, etc.) — self-hosted Web Push/APNs
- No paid tunneling (ngrok, Cloudflare Tunnel) — user manages own VPS networking
- No paid CI/CD (GitHub Actions free tier acceptable)

## Allowed Free/self-hosted
- PostgreSQL/SQLite/Redis (self-hosted)
- Let's Encrypt for TLS certs (free)
- GitHub Actions / GitLab CI (free tiers)
- Docker Hub / GHCR (free tiers)

## Rationale
- **Self-hosted principle**: User deploys on their own VPS; no vendor lock-in
- **Cost predictability**: $5/month VPS only; no per-user/per-session fees
- **Privacy**: No data leaves user's infrastructure
- **License compatibility**: MIT encourages commercial self-hosting without license fees

## Enforcement
- CI check: `grep -r "api_key\|secret_key\|Bearer " packages/` → fail (except test fixtures)
- Dependency audit: `cargo deny check` + `pnpm audit` in CI
- Architecture review: New external dependency requires ADR with cost analysis