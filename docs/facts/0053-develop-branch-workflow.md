# FACT-0053: develop branch workflow for nightly

**Date:** 2026-10-22
**Status:** accepted

## Summary

The `develop` branch is the integration branch for nightly builds. It is created once from `master` and maintained with the following rules:

## Branch rules

- **Created once:** `git branch develop master` (one-time).
- **Feature PRs target `develop`** (not `master`). This is a shift from current practice.
- **Stable/beta tags are cut from `master`**. Before a release, `develop` is merged/ff into `master` (or a release branch), then the tag is pushed to `master`.
- **Hotfixes to `master`** are back-merged to `develop` (or `develop` is rebased onto `master`).
- **Branch protection on `develop`:**
  - Required status checks (CI must pass).
  - CODEOWNERS for `.github/workflows/**`, `release-keys.txt`, `.github/scripts/make_manifest.py`.
  - No force pushes.
- **Nightly workflow trigger:**
  - Schedule: `0 0 * * *` (00:00 UTC).
  - Additionally: `on.workflow_run` of CI on `develop` with `types: [completed]` and `conclusion: success`. The workflow uses the CI run's SHA (`github.event.workflow_run.head_sha`) to ensure it builds from a green commit.
  - If schedule fires but no green CI run exists since last nightly, the workflow exits early (no empty nightly).

## Nightly versioning

- Git tag: `vYYYY.MM.DD-nightly` (UTC date at workflow start).
- Manifest version: `{core}-nightly.YYYYMMDD` where `core` is the Cargo version from `packages/server/Cargo.toml` (e.g., `0.1.36-nightly.20261022`).
- If Cargo version on `develop` is `0.2.0` (pre-release prep), nightly becomes `0.2.0-nightly.YYYYMMDD`.

## Consequences

- Nightly always builds from a CI-verified commit.
- `develop` receives features continuously; `master` receives releases.
- Hotfixes on `master` must be back-merged to `develop` to avoid nightly regressions.
- This is a lightweight GitFlow variant; simpler than full GitFlow because release branches are not used (tags on `master` suffice).

## Related

- DECISION-0125 (release channels)
- R70 (implementation plan)
- `.github/workflows/release-nightly.yaml`, `.github/workflows/ci.yaml`