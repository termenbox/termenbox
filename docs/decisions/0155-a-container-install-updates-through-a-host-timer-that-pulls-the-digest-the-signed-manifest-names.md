---
id: 0155-a-container-install-updates-through-a-host-timer-that-pulls-the-digest-the-signed-manifest-names
title: A container install updates through a host timer that pulls the digest the signed manifest names
status: active
accepted-by: Sergey Strebulaev <s@vivantel.dev>
date: 2026-10-31
track: product
tags: [release, updater, container, security]
amends: [0063-signed-releases-and-opt-in-updates]
related: [0076-native-releases-and-updater, 0077-root-actions-by-request-file, 0131-nightly-installs-notify-and-do-not-update-themselves-by-default, 0154-a-new-release-waits-a-grace-period-for-the-owner-then-installs-or-is-skipped]
---

## Decision

- **The updater runs on the host, never in the container.** The installer adds the same root helper and timers the machine install has (`termenbox-update.service`/`.timer`), with a Docker-aware apply step. The container gets no Docker socket and no new privilege (`unprivileged-server-never-holds-root`).
- **The signed manifest names the image.** `manifest.json` gains `image: { "x86_64": "sha256:...", "aarch64": "sha256:..." }` (digests of the per-architecture images in `ghcr.io/termenbox/termenbox`), written by `make_manifest.py` after the image is pushed. A manifest without it is not a container release. The helper pulls `ghcr.io/termenbox/termenbox@<digest>`, never a tag, so what runs is what the key signed (decision 0063's pinned digests extend to updates). Old servers ignore the unknown key (`serde(default)`).
- **Apply = recreate, health-check, roll back.** Pull the digest; if the running container already uses it, stop. Otherwise rename the running container to `<name>-previous`, create a new one from the recorded `docker run` arguments with the new digest, wait for `termenbox-server --help` and the health probe; on success remove the previous container and prune its image, on failure remove the new one and rename the old one back and start it. The state volume (`$ROOT/home`) is shared and untouched, so a rollback loses nothing (`updates-are-verified-and-reversible`).
- **Run arguments are recorded, not rebuilt.** The installer writes the exact arguments it used (port, env, volume, restart policy) to `$ROOT/container.json` in the journal directory, root-owned, before it creates the container (`host-changes-are-journaled`). The helper replays that file and refuses one it did not write (owner root, mode 0600, schema-versioned).
- **Policy, state and the app are shared with the machine install.** The same `update.toml`, `update-state.json`, request files, grace period and app card (decisions 0131, 0154). Only the apply step differs. `TERMENBOX_IMAGE` pins a tag by hand and then disables the timer's pull (a pinned image is a choice, as `hold`).
- **`docker` becomes a root dependency of the helper**, found on `PATH`; the helper calls the CLI with fixed arguments and no shell, takes the image reference only from the verified manifest, and the container name only from the journal.
- **Release order.** First a release whose manifest carries `image` and whose server ignores it; only then does the installer write the timer for a container, so no container pulls what no manifest names.

## Why

Decision 0063 says container auto-update is on, but a container has no updater, and the install command is the only way to update it. A host timer gives containers the same check, notice, grace period and rollback as the machine install without handing the container the Docker socket, which would be root on the host.

Not chosen: Watchtower or another third-party updater (an unsigned tag moves what runs, outside the release key); giving the container the Docker socket (a compromised server would own the host); updating by tag (a tag is mutable, a digest is what the manifest signs); an in-container updater that replaces the binary (the image is the unit of release, and the container has no systemd or root).

## Consequences

- Amends decision 0063 (container updates become real and digest-pinned); decisions 0076, 0077, 0131 and 0154 are reused unchanged.
- Release pipeline: `docker-build.yaml` must publish digests before `release-build.yaml` makes the manifest; the manifest tests and `--verify-release` learn `image`.
- Plan: U8 in `docs/plans/archive/server-update-flow.md`, split into U8a (manifest and pipeline), U8b (journal file and installer), U8c (helper apply and rollback), U8d (docs and an end-to-end test against a local registry).
