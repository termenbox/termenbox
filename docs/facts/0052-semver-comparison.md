# FACT-0052: Version comparison uses semver crate

**Date:** 2026-10-22
**Status:** accepted

## Summary

All version comparisons in the server (manifest vs current, alias updates, beta/stable/nightly ordering) use the `semver` crate (Cargo feature `serde` enabled). The previous `parse_version` function that extracted only `X.Y.Z` is removed.

## Details

- Manifest `version` field is a full SemVer string (e.g., `0.2.0-beta.2`, `0.1.36-nightly.20261022`).
- `semver::Version::parse` accepts these; prerelease identifiers are compared per SemVer spec (numeric identifiers compared numerically, alphabetic lexicographically; prerelease < release).
- Build metadata (after `+`) is parsed but **ignored in precedence** per SemVer. This is correct: `0.2.0+abc123` == `0.2.0+def456`.
- Channel ordering:
  - Stable: `X.Y.Z` (no prerelease)
  - Beta/RC: `X.Y.Z-beta.N`, `X.Y.Z-rc.N` (prerelease < release)
  - Nightly: `X.Y.Z-nightly.YYYYMMDD` (prerelease with date; later date > earlier date)
- Alias update compares `semver::Version` of candidate vs current alias manifest.
- Updater `is_newer_than` uses `semver::Version` comparison.

## Examples

| A | B | A > B? |
|---|---|---|
| `0.2.0` | `0.2.0-beta.2` | yes |
| `0.2.0-rc.1` | `0.2.0-beta.5` | yes |
| `0.2.0-beta.3` | `0.2.0-beta.2` | yes |
| `0.1.36-nightly.20261022` | `0.1.36-nightly.20261021` | yes |
| `0.1.37-nightly.20261020` | `0.1.36` | yes (core version wins) |
| `0.1.36+abc123` | `0.1.36+def456` | no (equal) |

## Migration

- Add `semver = { version = "1.0", features = ["serde"] }` to `packages/server/Cargo.toml`.
- Replace `parse_version` with `semver::Version::parse`.
- Update tests.

## Related

- DECISION-0125 (release channels)
- DECISION-0126 (compile-time release version)
- DECISION-0127 (alias releases)
- `packages/server/src/release.rs`, `packages/server/src/updater.rs`