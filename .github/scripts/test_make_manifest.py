#!/usr/bin/env python3
"""Tests for make_manifest.py: URLs come from the tag, the version and channel from their own options (R66)."""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("make_manifest.py")
NOTICES = ("tmux-COPYING", "tmux-libevent-LICENSE", "tmux-ncurses-COPYING")


def build(*args):
    with tempfile.TemporaryDirectory() as tmp:
        assets = Path(tmp)
        for arch in ("x86_64", "aarch64"):
            (assets / f"termenbox-server-{arch}-linux").write_bytes(b"server " + arch.encode())
            (assets / f"tmux-{arch}-linux").write_bytes(b"tmux " + arch.encode())
        for name in NOTICES:
            (assets / name).write_text(name)
        out = assets / "manifest.json"
        run = subprocess.run([sys.executable, str(SCRIPT), *args[:1], str(assets), str(out), *args[1:]], capture_output=True, text=True)
        return run, (json.loads(out.read_text()) if run.returncode == 0 else None)


class MakeManifest(unittest.TestCase):
    def test_a_plain_release_is_stable_and_downloads_from_its_own_tag(self):
        run, m = build("1.2.3")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual((m["version"], m["channel"]), ("1.2.3", "stable"))
        self.assertIn("/v1.2.3/termenbox-server-x86_64-linux", m["binaries"]["x86_64"]["url"])

    def test_the_tag_names_the_download_and_the_version_stays_semver(self):
        run, m = build("0.1.36-nightly.20261022", "--channel", "nightly", "--tag", "v2026.10.22-nightly")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual((m["version"], m["channel"]), ("0.1.36-nightly.20261022", "nightly"))
        for entry in (*m["binaries"].values(), *m["tmux"].values(), *m["tmux_notices"].values()):
            self.assertIn("/releases/download/v2026.10.22-nightly/", entry["url"])

    def test_prereleases_validate_and_junk_is_refused(self):
        self.assertEqual(build("0.1.36-beta.2", "--channel", "beta")[0].returncode, 0)
        for bad in ("1.2", "v1.2.3", "01.2.3", "1.2.3-", "one"):
            self.assertNotEqual(build(bad)[0].returncode, 0, bad)
        self.assertNotEqual(build("1.2.3", "--channel", "gold")[0].returncode, 0)


if __name__ == "__main__":
    unittest.main()
