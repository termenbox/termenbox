# GUARDRAIL-stable-pages-on-tags

**Name:** Stable client and Pages Functions deploy only on stable tag
**Type:** blocking
**Scope:** .github/workflows/deploy-pages.yaml, .github/workflows/release-stable.yaml, infra/termenbox-cloud/
**Detection:** CI gate / workflow inspection

## Rule

The stable Cloudflare Pages project (`termenbox` → `app.termenbox.dev`) deploys **only** when a stable tag (`vX.Y.Z`) is pushed. The deployment is part of the stable release workflow (`release-stable.yaml`), not a separate push-to-master workflow.

## Consequences

- API/Functions fixes wait for a patch release (tag `vX.Y.Z+1`).
- Stable client is truly frozen between releases.
- Rollback = previous tag.

## Patch release process

To keep API fixes fast:
1. Fix lands on `master` (or `develop` → `master`).
2. Maintainer creates patch tag: `git tag vX.Y.Z+1 && git push origin vX.Y.Z+1`.
3. Stable workflow builds client + server + manifest, deploys Pages, publishes GitHub Release.
4. Total time: CI duration (~10-15 min).

## Shared D1 migrations

Since beta/nightly deploy their own Pages Functions (with their own migrations) to the same D1:
- Migrations must be **backward-compatible** (additive only: new tables, new columns with defaults, new indexes).
- No `DROP`, `RENAME`, or breaking `ALTER` in beta/nightly before stable has the compatible code.
- Stable migration runs on tag deploy; beta/nightly migrations run on their deploys.
- If a breaking migration is needed, it requires a coordinated release: stable tag + beta/nightly deploy simultaneously.

## Exceptions

- Beta and nightly Pages projects deploy on their respective tags/schedule (not gated by this guardrail).
- Hotfix to stable Pages outside a tag requires a patch tag (no exception).

## Related

- DECISION-0125 (release channels)
- DECISION-0128 (three Pages deployments)
- FACT-0053 (develop branch workflow)
- `.github/workflows/release-stable.yaml`