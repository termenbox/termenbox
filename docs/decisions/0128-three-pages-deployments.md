# DECISION-0128: Three independent Cloudflare Pages deployments

**Date:** 2026-10-22
**Status:** accepted

## Context

The client is a PWA deployed to Cloudflare Pages. Currently a single project (`termenbox`) deploys from `master` to `app.termenbox.dev`. The owner wants beta and nightly channels with independent deployments.

## Decision

Create three Pages projects, each on its own subdomain:

| Channel | Domain | Pages project | OAuth app | Build env vars |
|---|---|---|---|---|
| stable | `app.termenbox.dev` | `termenbox` | stable | `VITE_CHANNEL=stable`, `VITE_API_ORIGIN=https://app.termenbox.dev`, `VITE_GITHUB_CLIENT_ID=...` |
| beta | `beta.app.termenbox.dev` | `termenbox-beta` | beta | `VITE_CHANNEL=beta`, `VITE_API_ORIGIN=https://beta.app.termenbox.dev`, `VITE_GITHUB_CLIENT_ID=...` |
| nightly | `nightly.app.termenbox.dev` | `termenbox-nightly` | nightly | `VITE_CHANNEL=nightly`, `VITE_API_ORIGIN=https://nightly.app.termenbox.dev`, `VITE_GITHUB_CLIENT_ID=...` |

All three share:
- D1 database (directory records, account links).
- `CERT_SIGNING_KEY` (device certs).
- `DIRECTORY_SALT` (claim hashes).

Per-environment secrets:
- `GITHUB_CLIENT_SECRET`
- `TERMENBOX_GITHUB_CLIENT_ID` (public, also in `VITE_`)

Deployment triggers:
- stable: on stable tag push (workflow `release-stable.yaml`).
- beta: on beta/rc tag push (workflow `release-beta.yaml`).
- nightly: on nightly schedule + CI success (workflow `release-nightly.yaml`).

**Why separate origins (not path-based):**
- Service Worker scope, Cache API, and localStorage are origin-bound. Path-based channels (`/beta/`) cause cache collisions, SW interference, and shared cookies.
- OAuth callback URLs must be registered per app; separate origins avoid callback confusion.
- Independent rollback: stable domain untouched when beta/nightly roll back.
- Clean UX: user sees channel in URL bar; no hidden state.

## Consequences

- **Positive:** Full isolation; independent deployments; clean PWA semantics; separate OAuth flows.
- **Negative:** Three Pages projects to maintain; three OAuth apps; DNS records; shared D1 migration coordination; Terraform/infra updates needed.
- **Migration:** Create beta/nightly projects, OAuth apps, DNS. Update `infra/termenbox-cloud/`. Update `deploy-pages.yaml` → three workflows or reusable with channel input.

## Related

- DECISION-0125 (release channels)
- DECISION-0052 (client hosting), DECISION-0053 (auth)
- R70 (implementation plan)
- `packages/client/vite.config.ts`, `packages/client/src/lib/update.ts`, `packages/client/src/lib/settings.ts`