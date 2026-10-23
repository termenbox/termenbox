---
id: 0133-each-channel-has-its-own-release-workflow-and-the-aliases-only-move-forward
title: Each channel has its own release workflow and environment, rolling aliases only move forward, and the image tags follow the channel
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [release, ci, security, docker]
date: 2026-10-23
---

## Decision

- **One build, three triggers.** `release-build.yaml` (reusable) builds, writes the manifest, signs it and publishes. `release-stable.yaml` (tags `vX.Y.Z`), `release-beta.yaml` (`vX.Y.Z-rc.N`, `vX.Y.Z-beta.N`) and `release-nightly.yaml` decide whether a release may happen and call it. Tag shapes are decided by `.github/scripts/release_plan.py` with exact patterns, never by the tag globs alone (a glob such as `v[0-9]*.[0-9]*.[0-9]*` also matches `v0.2.0-rc.1`).
- **A key per environment.** The manifest job runs in the GitHub Environment `stable`, `beta` or `nightly` and reads `RELEASE_SIGNING_KEY` there, so a channel's key exists only in a run of that channel. The signature must verify against the key `r1`, `b1` or `n1` of that channel in `release-keys.txt`; a release whose key is not listed there is refused, not published unsigned.
- **Stable and beta come only from `master`** and the tag's core must equal `Cargo.toml` (decision 0129). **Nightly** is built from the head of a green CI run on `develop`, never from a red or unchecked head, at most once per UTC day and only when the commit changed since the last nightly. Tag `vYYYY.MM.DD-nightly`, manifest `X.Y.Z-nightly.YYYYMMDD`. Fourteen days of nightlies are kept; the release `channel-nightly` points at is never deleted, and nothing is deleted when the alias cannot be read.
- **Aliases only move forward.** `channel-beta` and `channel-nightly` are prerelease, never "latest", and hold only `manifest.json` and its signature (decision 0132). An alias is replaced only by a newer SemVer; a stable release also moves `channel-beta` when it is newer than the last beta, because a beta server follows stable too.
- **Image tags follow the channel.** A stable tag pushes `latest`, `X.Y` and `X.Y.Z`; a beta tag `beta` and the exact version; a nightly `nightly` and the exact version. There is no `edge`, and a push to `master` no longer moves `latest`. The image is built with the release version, so it reports its channel.

## Why

One workflow and one key for every stream meant a nightly bug or leaked token could publish a stable release. Environments and bound keys limit what each trigger can sign; forward-only aliases mean a slow older run cannot roll a channel back; image tags that follow the channel let `--channel beta` mean the same on a container as on a host.

## Consequences

- `:latest` users get no update until the first stable tag after this change, because `latest` moved with every push to `master` before. Say so in that release's notes.
- Owner work: create the Environments `stable`, `beta` and `nightly` with their `RELEASE_SIGNING_KEY`, put the `b1` and `n1` public keys in `release-keys.txt` (until then only stable verifies, and a beta or nightly publish stops at the signing step), and approve the first release of each channel. A repository-level `RELEASE_SIGNING_KEY` still reaches every environment until it is removed, but it is the stable key and so cannot sign a beta or nightly manifest.
