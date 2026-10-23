#!/usr/bin/env python3
"""Writes manifest.json for a release (decision 0076).

usage: make_manifest.py VERSION ASSET_DIR OUT [--channel stable|beta|nightly] [--tag TAG] [--freeze] [--notes TEXT] [--no-tmux]

VERSION is the manifest version, full SemVer with an optional prerelease (1.2.3, 1.2.3-beta.1,
1.2.3-nightly.20261022). --tag names the release the assets are downloaded from (default v<VERSION>), because a
nightly alias such as v2026.10.22-nightly is not the version. --channel (default stable) is signed into the manifest.

ASSET_DIR holds one file per architecture named termenbox-server-<arch>-linux (x86_64, aarch64). The manifest
lists each with its download URL under this project's GitHub releases and its SHA-256. CI signs the exact bytes
written here, so the output is deterministic (sorted keys, no timestamps).

It also holds the bundled tmux (decision 0119): tmux-<arch>-linux per architecture, listed under "tmux" the same
way, and the licence notices that travel with it (tmux-COPYING, tmux-libevent-LICENSE, tmux-ncurses-COPYING),
listed under "tmux_notices". They are required unless --no-tmux is given (the installer tests that stub a release).
"""
import hashlib
import json
import re
import sys
from pathlib import Path

BASE = "https://github.com/termenbox/termenbox/releases/download"
ARCHES = ("x86_64", "aarch64")
CHANNELS = ("stable", "beta", "nightly")
SEMVER = re.compile(
    r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(-((0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?"
)
TMUX_NOTICES = ("tmux-COPYING", "tmux-libevent-LICENSE", "tmux-ncurses-COPYING")


def entry(tag, path):
    if not path.is_file():
        sys.exit(f"missing {path}")
    return {
        "url": f"{BASE}/{tag}/{path.name}",
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    freeze = "--freeze" in argv
    with_tmux = "--no-tmux" not in argv

    def option(name, default):
        nonlocal args
        if name not in argv:
            return default
        value = argv[argv.index(name) + 1]
        args = [a for a in args if a != value]
        return value

    notes = option("--notes", "")
    channel = option("--channel", "stable")
    tag = option("--tag", None)
    if len(args) != 3:
        sys.exit(__doc__)
    version, asset_dir, out = args[0], Path(args[1]), Path(args[2])
    if not SEMVER.fullmatch(version):
        sys.exit(f"version must look like 1.2.3 or 1.2.3-beta.1, got {version!r}")
    if channel not in CHANNELS:
        sys.exit(f"channel must be one of {', '.join(CHANNELS)}, got {channel!r}")
    tag = tag or f"v{version}"
    binaries = {arch: entry(tag, asset_dir / f"termenbox-server-{arch}-linux") for arch in ARCHES}
    manifest = {"version": version, "channel": channel, "freeze": freeze, "notes": notes, "binaries": binaries}
    if with_tmux:
        manifest["tmux"] = {arch: entry(tag, asset_dir / f"tmux-{arch}-linux") for arch in ARCHES}
        manifest["tmux_notices"] = {name: entry(tag, asset_dir / name) for name in TMUX_NOTICES}
    out.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
    print(f"wrote {out} for {version} ({channel}, tag {tag})")


if __name__ == "__main__":
    main(sys.argv)
