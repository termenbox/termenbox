# GUARDRAIL-manifest-url-resolution

**Name:** Updater and installer download assets from manifest URLs, not constructed URLs
**Type:** blocking
**Scope:** packages/server/src/updater.rs, packages/client/public/install.sh, .github/scripts/make_manifest.py
**Detection:** code review / unit tests / integration tests

## Rule

The updater (`Updater::run`) and the installer (`fetch_release`) MUST download all assets (server binary, tmux, tmux notices) using the exact URLs provided in the verified manifest:

- `manifest.binary[arch].url`
- `manifest.tmux[arch].url`
- `manifest.tmux_notices[name].url`

Constructing URLs from a base URL and asset name (e.g., `base + "/termenbox-server-" + arch + "-linux"`) is forbidden.

## Rationale

Alias releases (`channel-beta`, `channel-nightly`) contain only the manifest and signature. The actual assets live under versioned tags. If the updater/installer constructs URLs from the alias tag, downloads will 404. Using manifest URLs makes aliases work transparently and decouples the release tag from the asset location.

## Enforcement

- `make_manifest.py` generates URLs using the `--tag` argument: `https://github.com/termenbox/termenbox/releases/download/{tag}/{asset}`. The tag is the Git tag (e.g., `v0.2.0-beta.2`, `v2026.10.22-nightly`).
- `updater.rs`: replace `format!("{base}/termenbox-server-{}-linux", arch)` with `manifest.binary(&self.arch)?.url`.
- `install.sh`: parse URLs from manifest JSON using `awk`/`sed` (JSON structure is stable and known).
- Tests: verify that a manifest with alias tag but versioned asset URLs downloads correctly.

## Exceptions

- The `release_url` field in `Updater` and `RELEASE_URL` in installer are used **only** to fetch the manifest and signature. All subsequent downloads use manifest URLs.

## Related

- DECISION-0127 (alias releases and manifest URLs)
- FACT-0054 (installer manifest URLs)
- `packages/server/src/updater.rs`, `packages/client/public/install.sh`