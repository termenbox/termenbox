#!/usr/bin/env python3
"""Writes manifest.json for a release (decision 0076).

usage: make_manifest.py VERSION ASSET_DIR OUT [--freeze] [--notes TEXT] [--no-tmux]

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
TMUX_NOTICES = ("tmux-COPYING", "tmux-libevent-LICENSE", "tmux-ncurses-COPYING")


def entry(version, path):
    if not path.is_file():
        sys.exit(f"missing {path}")
    return {
        "url": f"{BASE}/v{version}/{path.name}",
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    freeze = "--freeze" in argv
    with_tmux = "--no-tmux" not in argv
    notes = ""
    if "--notes" in argv:
        notes = argv[argv.index("--notes") + 1]
        args = [a for a in args if a != notes]
    if len(args) != 3:
        sys.exit(__doc__)
    version, asset_dir, out = args[0], Path(args[1]), Path(args[2])
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        sys.exit(f"version must look like 1.2.3, got {version!r}")
    binaries = {arch: entry(version, asset_dir / f"termenbox-server-{arch}-linux") for arch in ARCHES}
    manifest = {"version": version, "freeze": freeze, "notes": notes, "binaries": binaries}
    if with_tmux:
        manifest["tmux"] = {arch: entry(version, asset_dir / f"tmux-{arch}-linux") for arch in ARCHES}
        manifest["tmux_notices"] = {name: entry(version, asset_dir / name) for name in TMUX_NOTICES}
    out.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
    print(f"wrote {out} for {version}")


if __name__ == "__main__":
    main(sys.argv)
