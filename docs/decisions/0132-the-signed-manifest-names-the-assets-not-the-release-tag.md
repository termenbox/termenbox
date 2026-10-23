---
id: 0132-the-signed-manifest-names-the-assets-not-the-release-tag
title: The signed manifest names where each asset is; an address and a tag never do
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [release, updater, installer, security]
date: 2026-10-23
---

## Decision

- **Every asset URL comes from the verified manifest**: `binaries[arch].url`, `tmux[arch].url` and `tmux_notices[name].url`. The installer and the updater never build one from the address the manifest was fetched from (`TERMENBOX_RELEASE_URL`) or from a tag name.
- The manifest address (`release_url`) is only where `manifest.json` and `manifest.json.sig` are read. It is `releases/latest/download` for stable and the rolling alias releases `channel-beta` and `channel-nightly` for the others. An alias holds **only a manifest**; its assets sit under the real tag (`v0.2.0-rc.1`, `v2026.10.22-nightly`), which `make_manifest.py --tag` writes into the URLs.
- An asset URL is used only if it is on this project's GitHub releases (`https://github.com/termenbox/termenbox/releases/download/`) and has no `..`, and only after the signature has verified. The hash in the manifest still decides whether the downloaded bytes are accepted. The one exception is the installer's and updater's own tests: with a `file://` release address, assets must lie under that address.
- The installer reads the full SemVer from the manifest (`0.2.0-rc.1`), not `X.Y.Z`.

## Why

The updater built `release_url/termenbox-server-ARCH-linux`, so a rolling alias that holds only a manifest could not work, and the version had to match the tag. The signed manifest is the one thing already trusted, so it is the right place to say where files are. The download host and the tag stay unsigned input.

## Consequences

- Guardrail `manifest-url-resolution` carries the rule.
- R68 can publish `channel-beta` and `channel-nightly` as manifest-only releases.
