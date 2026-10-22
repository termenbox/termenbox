# DECISION-0126: Compile-time release version injection

**Date:** 2026-10-22
**Status:** accepted

## Context

`crate::version()` currently returns `CARGO_PKG_VERSION + "+" + BUILD_SHA`. This is the Cargo core version (e.g., `0.1.36`) plus a build identifier. For beta/nightly channels, the manifest version differs from Cargo version (e.g., `0.2.0-beta.2`, `0.1.36-nightly.20261022`). The updater compares `manifest.version` against `crate::version()` using a core-only comparison (`X.Y.Z`), which breaks channel semantics: a beta server reports `0.2.0+sha` and never sees `0.2.0-beta.2` as newer.

## Decision

Inject the release version at compile time via `TERMENBOX_RELEASE_VERSION` environment variable:

```rust
const RELEASE_VERSION: Option<&str> = option_env!("TERMENBOX_RELEASE_VERSION");

pub fn version() -> String {
    let release = RELEASE_VERSION.unwrap_or(env!("CARGO_PKG_VERSION"));
    let build = std::env::var("TERMENBOX_BUILD").unwrap_or_else(|_| "dev".into());
    format!("{}+{}", release, build.chars().take(7).collect::<String>())
}
```

CI and Docker builds set `TERMENBOX_RELEASE_VERSION` per channel:
- stable: `0.2.0`
- beta: `0.2.0-beta.2`
- nightly: `0.1.36-nightly.20261022`

Local builds without the env var fall back to `CARGO_PKG_VERSION`.

The `parse_version` function in `release.rs` is replaced with `semver::Version` for full SemVer comparison (including prerelease and build metadata ignored per SemVer spec).

## Consequences

- **Positive:** Binary's self-reported version matches the channel; updater comparison works correctly; no root-owned state file needed; rollback restores correct version automatically.
- **Negative:** Stable cannot be a byte-for-byte promotion of a beta binary (version embedded differs). Acceptable; stable is rebuilt from the same commit with different `TERMENBOX_RELEASE_VERSION`.
- **Migration:** All channel workflows must pass the env var. Dockerfile needs `ARG TERMENBOX_RELEASE_VERSION` and `ENV TERMENBOX_RELEASE_VERSION=$TERMENBOX_RELEASE_VERSION` before `cargo build`.

## Related

- DECISION-0125 (release channels)
- R70 (implementation plan)
- `packages/server/src/main.rs`, `packages/server/src/release.rs`, `packages/server/src/updater.rs`