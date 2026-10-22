# Termenbox Release Process

This document describes how to create releases for each channel.

## Release Channels

| Channel | Branch | Tag Pattern | Published As | Pages Project |
|---------|--------|-------------|--------------|---------------|
| **Stable** | `master` | `vX.Y.Z` | Latest release | `termenbox` |
| **Beta** | `develop` | `vX.Y.Z-beta.N` or `vX.Y.Z-rc.N` | Pre-release | `termenbox-beta` |
| **Nightly** | `develop` | `vYYYY.MM.DD-nightly` (auto) | Pre-release | `termenbox-nightly` |

---

## Creating a Beta Release (any contributor)

**Prerequisites:**
- Changes merged to `develop` branch
- CI green on `develop`

**Steps:**

```bash
# 1. Ensure you're on develop and up to date
git checkout develop
git pull origin develop

# 2. Create beta tag (e.g., v0.1.46-beta.1)
git tag v0.1.46-beta.1

# 3. Push tag to origin
git push origin v0.1.46-beta.1
```

**What happens automatically:**
1. GitHub Actions `Release` workflow triggers on tag push
2. Detects branch=`develop` + tag contains `-beta.` → **beta release**
3. Builds server + tmux for x86_64 and aarch64
4. Creates manifest, signs it, verifies
5. Publishes to **GitHub Releases** as **pre-release** (with `--prerelease` flag)
6. `Deploy to Cloudflare Pages` workflow triggers on push to `develop`
7. Deploys client to **`termenbox-beta`** Pages project

**Result:**
- GitHub Release: `v0.1.46-beta.1` (marked as pre-release)
- Beta client: `https://termenbox-beta.pages.dev`
- Docker: `ghcr.io/termenbox/termenbox:v0.1.46-beta.1`

---

## Creating a Stable Release (release-managers only)

**Prerequisites:**
- `release-managers` team membership (or admin)
- Changes merged to `master` (typically via `develop` → `master` merge/ff)

**Steps:**

```bash
# 1. Ensure you're on master and up to date
git checkout master
git pull origin master

# 2. Create stable tag (e.g., v0.1.46)
git tag v0.1.46

# 3. Push tag to origin
git push origin v0.1.46
```

**What happens automatically:**
1. GitHub Actions `Release` workflow triggers
2. Detects branch=`master` + tag is `vX.Y.Z` → **stable release**
3. Builds, signs, verifies
4. Publishes to **GitHub Releases** as **latest release**
5. `Deploy to Cloudflare Pages` workflow triggers on push to `master`
6. Deploys client to **`termenbox`** Pages project

**Result:**
- GitHub Release: `v0.1.46` (marked as latest)
- Stable client: `https://termenbox.pages.dev`
- Docker: `ghcr.io/termenbox/termenbox:v0.1.46`

---

## Nightly (automatic)

- Runs daily at **00:00 UTC** from `develop` (if CI green since last nightly)
- Tag: `vYYYY.MM.DD-nightly` (e.g., `v2026.10.23-nightly`)
- Manifest version: `{core}-nightly.YYYYMMDD`
- Deploys to `termenbox-nightly` Pages project

---

## Tag Format Validation

The `Release` workflow enforces:

| Branch | Allowed Tag Pattern | Error If |
|--------|---------------------|----------|
| `develop` | `vX.Y.Z-beta.N`, `vX.Y.Z-rc.N` | Tag pushed from other branch |
| `master` | `vX.Y.Z` (no suffix) | Tag pushed from other branch |

**Example errors:**
```
::error::Beta tags (containing -beta. or -rc.) must be pushed from 'develop' branch, not 'feature/foo'
::error::Stable tags (vX.Y.Z) must be pushed from 'master' branch, not 'develop'
```

---

## Cloudflare Pages Projects

You need **three** Pages projects (one-time setup in dashboard):

| Project Name | Production Branch | Purpose |
|--------------|-------------------|---------|
| `termenbox` | `master` | Stable client |
| `termenbox-beta` | `develop` | Beta client |
| `termenbox-nightly` | `develop` (or separate) | Nightly client |

**Each project needs:**
- Same build command: `pnpm --filter termenbox-client run build`
- Output dir: `dist`
- Same Pages Functions + D1 binding + secrets (copy from stable project)

---

## Quick Reference

```bash
# Beta (from develop) — ANY CONTRIBUTOR
git checkout develop && git pull
git tag v0.1.46-beta.1
git push origin v0.1.46-beta.1

# Stable (from master) — RELEASE MANAGERS ONLY
git checkout master && git pull
git tag v0.1.46
git push origin v0.1.46
```

---

## Related

- DECISION-0125 (release channels)
- `.github/workflows/release.yaml` (build + publish)
- `.github/workflows/deploy-pages.yaml` (Cloudflare Pages deploy)
- `.github/workflows/release-nightly.yaml` (nightly schedule)
