---
id: manifest-url-resolution
title: Asset URLs come from the verified manifest and stay on the project's releases
status: active
governed-by: 0132-the-signed-manifest-names-the-assets-not-the-release-tag
grounded-in: [0063, 0076, 0130]
derivation-note: Given 0132 (the signed manifest names the assets) and 0076 (the download host is not trusted), a URL built from a tag or from the manifest's own address would let an unsigned input choose what is downloaded.
tags: [security, supply-chain, updater, installer]
---

# Guardrail: Manifest URL resolution

## Rule

1. The installer and the updater **MUST** take every asset URL (server binary, tmux, licence notices) from the manifest after its signature has verified, and **MUST NOT** build one from `TERMENBOX_RELEASE_URL`, `release_url` or a tag name.
2. An asset URL **MUST** start with `https://github.com/termenbox/termenbox/releases/download/` and **MUST NOT** contain `..`, whatever the signature says. Only a `file://` release address, used by the installer's own tests, may hold assets, and then only under that address.
3. The downloaded bytes **MUST** match the SHA-256 in the same manifest before they are run or swapped in.
4. The manifest **MUST** be accepted only if its channel is one this server follows and the key that signed it is bound to that channel (decision 0130).
