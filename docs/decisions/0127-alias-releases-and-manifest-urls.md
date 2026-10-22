# DECISION-0127: Alias releases and manifest-driven asset URLs

**Date:** 2026-10-22
**Status:** accepted

## Context

Channel aliases (`channel-beta`, `channel-nightly`) are rolling GitHub Releases that contain only `manifest.json` and `manifest.json.sig`. The actual binaries live under versioned tags (e.g., `v0.2.0-beta.2/`). The current updater and installer construct download URLs from a base URL (`releases/latest/download/...` or `releases/download/channel-beta/...`), which breaks for aliases because the assets don't exist there.

## Decision

1. **Manifest is the source of truth for URLs.** The updater and installer must read `manifest.binary[arch].url`, `manifest.tmux[arch].url`, and `manifest.tmux_notices[*].url` and download from those exact URLs.

2. **`make_manifest.py` receives `--tag` separately from `--version`.** The tag is used in the URL template: `https://github.com/termenbox/termenbox/releases/download/{tag}/{asset}`. The version is written to the manifest's `version` field.

3. **Alias releases copy the versioned manifest verbatim.** The alias's `manifest.json` is identical to the versioned release's manifest (same URLs, same hashes). Only the GitHub Release metadata (tag name, `prerelease`, `latest`) differs.

4. **Alias update logic:** After a successful versioned release, the channel workflow updates the alias if the new release's SemVer is greater than the alias's current manifest version. The alias release is updated with `gh release upload --clobber` for the two manifest assets.

5. **Nightly cleanup:** `gh release delete` for nightlies older than 14 days, **excluding** the release currently referenced by `channel-nightly` (checked by comparing the alias manifest's asset URL tag).

## Consequences

- **Positive:** Aliases work for all channels; nightly tag can be date-based while manifest version is SemVer; no asset duplication; installer/updater code unified.
- **Negative:** Updater/installer must parse manifest URLs (slightly more code); alias update is two-step (non-atomic) — fail closed on signature mismatch.
- **Migration:** `updater.rs` and `install.sh` changed to use manifest URLs. `make_manifest.py` signature changed.

## Related

- DECISION-0125 (release channels)
- DECISION-0126 (compile-time version)
- R70 (implementation plan)
- `packages/server/src/updater.rs`, `packages/client/public/install.sh`, `.github/scripts/make_manifest.py`