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

**Exception (decision 0140).** This guardrail does **not** apply to paying for developer accounts
(Apple Developer Program, Google Play Console), desktop code-signing certificates, or the supporting
infrastructure needed to publish the native wrappers in those stores. Those are a distribution cost of the
optional wrapper, not a runtime dependency. Everything else in this guardrail stays in force: no paid runtime
service, no paid API, no subscription and no per-user fee is introduced, and the PWA and self-hosted installs
still need no paid service.

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