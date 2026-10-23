---
id: 0130-releases-have-three-channels-each-with-its-own-signing-key
title: Releases have three channels, a manifest names its channel, and each channel has its own signing key
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [release, updater, security, ci]
date: 2026-10-23
---

## Decision

- **Channels.** Stable (tags `vX.Y.Z`, from `master`), beta (`vX.Y.Z-rc.N`, `vX.Y.Z-beta.N`, from `master`) and nightly (`vYYYY.MM.DD-nightly`, from `develop`, decision 0129).
- **The version is full SemVer.** `Cargo.toml` holds only the core `X.Y.Z`. The prerelease lives in the tag and in the signed manifest `version` (`0.1.36-nightly.20261022`). A build sets `TERMENBOX_RELEASE_VERSION` at compile time and the binary reports it; without it the crate version applies. Ordering is SemVer with build metadata ignored, so `0.1.36-nightly.20261022 < 0.1.36-nightly.20261023 < 0.1.36 < 0.1.37`.
- **A manifest names its channel**, signed with the rest, and `make_manifest.py` writes it. The download tag is separate from the version, because `v2026.10.22-nightly` is not a version.
- **One key per channel**: `r1` (stable), `b1` (beta), `n1` (nightly). A line in `release-keys.txt` may end in `channel=...`; without it the key is a stable key, so existing lists keep their meaning. A key signs only its own channel: a nightly key cannot sign a stable manifest.
- **Trust sets.** A stable binary follows stable only (key `r1`). A beta binary follows beta and stable (`b1`, `r1`), because the beta alias can point at a stable release. A nightly binary follows nightly only (`n1`). A binary knows its own channel from the prerelease label of its version.
- Old binaries know only `r1` and the old manifest shape, ignore the new field and reject the other keys, so they keep working on stable.

## Why

One signing key and one stream mean a leaked key signs every release and a prerelease cannot order correctly. Binding keys to channels limits what a leaked nightly key can do to what nightly hosts already run. Guardrail `releases-are-signed` and decision 0076 still hold; this narrows what each key may sign.

Not chosen: one key with a channel field only (the key could still sign any channel); a channel inferred from the tag (the tag is unsigned input to the manifest).

## Consequences

- The private keys for `b1` and `n1` are owner work (GitHub Environments `beta` and `nightly`). Until their public halves are in `release-keys.txt`, only stable releases verify.
- Steps R67 (updater policy), R68 (workflows and aliases) and R70 (client notice) build on this.
