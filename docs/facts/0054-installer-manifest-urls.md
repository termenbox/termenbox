# FACT-0054: Installer parses asset URLs from manifest

**Date:** 2026-10-22
**Status:** accepted

## Summary

The installer (`packages/client/public/install.sh`) no longer constructs download URLs from a base URL. Instead, it downloads the manifest, verifies the signature, and extracts the exact asset URLs from the manifest JSON.

## Details

- **Manifest download:** `curl "$RELEASE_URL/manifest.json"` (where `RELEASE_URL` points to the channel's alias or versioned release, e.g., `https://github.com/termenbox/termenbox/releases/download/channel-beta`).
- **Signature verification:** unchanged (uses `ssh-keygen -Y verify` with `RELEASE_KEYS`).
- **Asset URL extraction:** uses `awk`/`sed` to parse `manifest.binary[arch].url`, `manifest.tmux[arch].url`, and `manifest.tmux_notices[*].url` from the JSON. The URLs are absolute (include `https://github.com/...`).
- **Version extraction:** fixed to capture the full SemVer string (not just `X.Y.Z`). The old regex `[0-9.]*` truncated `0.2.0-beta.1` to `0.2.0`.

## Example manifest URLs

```json
{
  "version": "0.2.0-beta.2",
  "channel": "beta",
  "binaries": {
    "x86_64": {
      "url": "https://github.com/termenbox/termenbox/releases/download/v0.2.0-beta.2/termenbox-server-x86_64-linux",
      "sha256": "..."
    }
  },
  "tmux": {
    "x86_64": {
      "url": "https://github.com/termenbox/termenbox/releases/download/v0.2.0-beta.2/tmux-x86_64-linux",
      "sha256": "..."
    }
  }
}
```

The installer downloads from these exact URLs, which may point to a different tag than the alias (e.g., alias `channel-beta` → manifest URLs point to `v0.2.0-beta.2`).

## Consequences

- Works correctly with alias releases (manifest URLs point to versioned assets).
- Works correctly with nightly date-based tags (manifest URLs use the nightly tag).
- Single code path for all channels.
- Slightly more complex shell parsing (but `awk`/`sed` on known JSON structure is reliable).

## Related

- DECISION-0127 (alias releases and manifest URLs)
- `packages/client/public/install.sh`
- `.github/scripts/make_manifest.py` (generates correct URLs from `--tag`)