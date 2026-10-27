---
id: no-paid-services
title: No Paid Services in Required Stack
status: active
governed-by: [0011, 0140]
grounded-in: 0001,0013
derivation-note: MIT license + self-hosted goal + VPS deployment = no external paid dependencies; decision 0140 carves out only developer-account and code-signing fees for optional native distribution
tags: [product, architecture, cost]
---

# Guardrail: No Paid Services in Required Stack

## Rule
**MUST NOT** require any paid external service for core functionality.

**Exception (decision 0140).** This guardrail does **not** apply to paying for developer accounts
(Apple Developer Program, Google Play Console) or desktop code-signing certificates. Those are a
distribution cost of the optional wrapper, not a runtime dependency. The exception covers those fees only,
including the annual renewal of a developer account; it does not cover a recurring build, CI or hosting
service. Everything else in this guardrail stays in force:
no paid runtime service, no paid API, no subscription and no per-user fee is introduced, and the PWA and
self-hosted installs still need no paid service.

## Scope
- No paid APIs (Pusher, Ably, Supabase, Firebase Realtime Database/Firestore, etc.); FCM is the free push transport named below, not a paid database or API
- No paid databases (PlanetScale, Neon, etc.) — use embedded/self-hosted only
- No paid authentication (Auth0, Clerk, etc.) — SSH keys only
- No paid push notification services (OneSignal, etc.) — self-hosted Web Push, or free APNs/FCM. Free store
  push for the optional wrapper is not a paid service; the path a self-hosted user uses is decided by R76,
  and the payload rule is `notifications-carry-no-agent-text`.
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
- **Privacy**: No data leaves the user's infrastructure for the core stack, except push payloads, which reach the browser vendor on the web path and APNs/FCM in a wrapper (see `notifications-carry-no-agent-text`)
- **License compatibility**: MIT encourages commercial self-hosting without license fees

## Enforcement
- CI check: `grep -r "api_key\|secret_key\|Bearer " packages/` → fail (except test fixtures)
- Dependency audit: `cargo deny check` + `pnpm audit` in CI
- Architecture review: New external dependency requires ADR with cost analysis