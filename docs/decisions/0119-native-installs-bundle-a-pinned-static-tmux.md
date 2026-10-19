---
id: 0119-native-installs-bundle-a-pinned-static-tmux
title: Native installs carry a pinned, statically linked tmux, so no install needs tmux from the distribution
status: active
date: 2026-10-19
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [deployment, server, security, build]
---

## Decision

Accepted 2026-10-19. This removes the "tmux must already be installed" limit of 0113.

- **What is bundled.** One tmux binary per architecture (x86_64 and aarch64), built static against musl, libevent and ncurses. The server's session backend (0054) needs tmux 3.2 or newer for control mode; the pin is above that floor.
- **Pinned version: tmux 3.8**, the latest release when this was written (tagged 2026-09-09; `tmux-3.8.tar.gz`, sha256 `e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e`, checked against the GitHub release asset). The build pins the tarball hash and the libevent and ncurses versions and hashes too; moving any of them is a reviewed change. A tmux or dependency bump is a patch release of termenbox.
- **Licence: ISC** (tmux's COPYING; libevent is BSD-3-Clause, ncurses is X11/MIT style). All permit redistribution in a MIT-licensed project. The release carries tmux's COPYING and the notices of the two libraries next to the binary, as their licences ask.
- **Build.** The `build` job in `.github/workflows/release.yaml` builds tmux on each native runner (x86_64 and `ubuntu-24.04-arm`) with `musl-gcc` and `--enable-static`, linking libevent and ncurses built from the pinned sources. The existing static gate applies unchanged (no `INTERP`, no `NEEDED`), and a further gate runs the binary with an empty `PATH` and no terminfo on the host.
- **Terminfo.** ncurses is built with its terminfo entries compiled into the binary (`--with-fallbacks` for `xterm-256color`, `screen-256color`, `tmux-256color`, `xterm`, `linux`), so the binary needs no terminfo database on the host. The server sets `TERM` for the panes from that set.
- **Signing and carriage.** `.github/scripts/make_manifest.py` lists `tmux-<arch>-linux` with its sha256 next to the server binary, so the one manifest signature (0076) covers both. The installer and the updater verify the tmux binary against the signed manifest exactly as they verify the server binary, and the Release publishes it as an asset.
- **Resolution order**, in `Runner` only (`packages/server/src/backend.rs`): `TERMENBOX_TMUX` (an explicit path, never overridden), then the bundled copy, then `tmux` on `PATH`. A bundled or `PATH` copy older than 3.2 is skipped with a logged reason; a user-set `TERMENBOX_TMUX` that is older fails loudly instead.
- **Placement.** `host-me` extracts it to `~/.local/bin` beside the server (0113); `host-user` and `host` place it where the run user can reach it, and host mode needs it reachable from the session's namespace. The updater replaces it together with the server and rolls both back together (0076). The change journal (0067) records it.
- **Docker installs keep the distribution tmux.** The gain is small there and the image already pins its packages.
- **Who owns security updates.** The termenbox maintainers. tmux has no network surface beyond its local socket, but a tmux or libevent or ncurses security release is handled as a termenbox release: the build's pins are bumped, the signed release goes out, and the daily signed update (0076) delivers it. A scheduled CI job checks the tmux, libevent and ncurses release feeds weekly and opens an issue when a pin is behind; this is free (`no-paid-services`). The distribution's package manager no longer patches a bundled tmux, which is the cost this decision accepts.

## Why

A native install on a machine with no tmux, or an older one, today stops with a printed command that needs root (0113), and an old tmux silently loses control mode and falls back to scanning. A pinned static binary removes the last root step from user installs, makes control mode the same everywhere, and fits the signed pipeline already in place. The cost is a few MB per architecture, a build step, and owning tmux's security updates, which the weekly check and the patch-release rule keep small.

Rejected: building tmux on the user's machine (needs a compiler and root for libraries, against 0113); a native session backend in place of tmux (R23 keeps it out unless tmux limits bite); a glibc dynamic build (breaks on hosts with a different libc, against the static server design).
