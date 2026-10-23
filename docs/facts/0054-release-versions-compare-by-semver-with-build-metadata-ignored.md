---
id: 0054
title: Release versions compare by SemVer precedence, build metadata is ignored, and a prerelease sorts below its release
status: active
kind: observation
governed-by: 0130-releases-have-three-channels-each-with-its-own-signing-key
tags: [release, updater, semver]
date: 2026-10-23
last-verified: 2026-10-23
---

# Release versions compare by SemVer precedence

## What is true

`release.rs` parses versions with the `semver` crate and compares them after dropping build metadata (`+abc1234`), which the crate would otherwise order.

- `0.1.36-nightly.20261022 < 0.1.36-nightly.20261023 < 0.1.36 < 0.1.37`: numeric prerelease identifiers compare as numbers, and a prerelease is below its release.
- `0.1.36-beta.1 < 0.1.36-beta.2 < 0.1.36-rc.1`: alphanumeric identifiers compare as text (`beta` < `rc`), so rc follows beta.
- A running binary reports `<version>+<build sha>`; the sha never decides whether a release is newer.
- A running binary's channel comes from the first prerelease identifier: `nightly`, `beta` or `rc` (beta), anything else stable.

Covered by the tests in `packages/server/src/release.rs`.
