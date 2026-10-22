# DECISION-0125: Three release channels (stable, beta, nightly)

**Date:** 2026-10-22
**Status:** accepted
**Supersedes:** R50 (partial)

## Context

The project currently has a single release stream triggered by `vX.Y.Z` tags. The owner requested three channels with independent update policies, trust anchors, and deployments.

## Decision

We adopt three channels:

| Channel | Branch | Tag pattern | Manifest version | Update default |
|---|---|---|---|---|
| stable | `master` | `vX.Y.Z` | `X.Y.Z` | auto |
| beta | `master` | `vX.Y.Z-rc.N`, `vX.Y.Z-beta.N` | same as tag (no `v`) | auto |
| nightly | `develop` | `vYYYY.MM.DD-nightly` | `X.Y.Z-nightly.YYYYMMDD` | notify |

Each channel has:
- Its own Ed25519 signing key with channel binding (`channel=stable|beta|nightly` in `release-keys.txt`).
- Its own GitHub Environment (stable: protected + required reviewers).
- Its own Cloudflare Pages project and domain (`app.`, `beta.app.`, `nightly.app.`).
- Its own GitHub OAuth application.
- Its own Docker tags (`latest`/`X.Y`/`X.Y.Z`, `beta`/`X.Y.Z-rc.N`, `nightly`/`X.Y.Z-nightly.YYYYMMDD`).
- Rolling alias releases: `channel-beta`, `channel-nightly` (stable uses `releases/latest`).

Trust sets:
- Stable binaries trust only stable key.
- Beta binaries trust stable + beta keys (alias may point to stable).
- Nightly binaries trust only nightly key.

## Consequences

- **Positive:** Complete isolation; compromise of nightly key cannot sign stable; independent rollback; clean PWA origins.
- **Negative:** Three keys to manage; three Pages projects/OAuth apps; shared D1 requires backward-compatible migrations; patch releases needed for API fixes on stable.
- **Migration:** Existing stable key becomes `r1 channel=stable`. New keys `b1`, `n1` generated and added. Old binaries continue working (they only know `r1`).

## Related

- GUARDRAIL-releases-are-signed
- GUARDRAIL-updates-are-verified-and-reversible
- DECISION-0057 (transport), DECISION-0062 (auth), DECISION-0076 (manifest), DECISION-0119 (bundled tmux)
- R70 (implementation plan)