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


def build(*args, image=None):
    with tempfile.TemporaryDirectory() as tmp:
        assets = Path(tmp)
        if image is not None:
            (assets / "image.json").write_text(json.dumps(image))
            args = (*args, "--image-digests", str(assets / "image.json"))
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

    def test_a_base_url_replaces_the_download_address_for_test_releases(self):
        run, m = build("9.0.1", "--base-url", "file:///release2/")
        self.assertEqual(run.returncode, 0, run.stderr)
        for entry in (*m["binaries"].values(), *m["tmux"].values(), *m["tmux_notices"].values()):
            self.assertTrue(entry["url"].startswith("file:///release2/"), entry["url"])
        self.assertEqual(m["binaries"]["x86_64"]["url"], "file:///release2/termenbox-server-x86_64-linux")

    def test_prereleases_validate_and_junk_is_refused(self):
        self.assertEqual(build("0.1.36-beta.2", "--channel", "beta")[0].returncode, 0)
        for bad in ("1.2", "v1.2.3", "01.2.3", "1.2.3-", "one"):
            self.assertNotEqual(build(bad)[0].returncode, 0, bad)
        self.assertNotEqual(build("1.2.3", "--channel", "gold")[0].returncode, 0)

    def test_image_digests_are_signed_in_and_only_well_formed_ones(self):
        good = {"x86_64": "sha256:" + "a" * 64, "aarch64": "sha256:" + "b" * 64}
        run, m = build("1.2.3", image=good)
        self.assertEqual((run.returncode, m["image"]), (0, good))
        self.assertNotIn("image", build("1.2.3")[1])
        for bad in ({"x86_64": good["x86_64"]}, {**good, "aarch64": "latest"}, {**good, "riscv": good["x86_64"]}):
            self.assertNotEqual(build("1.2.3", image=bad)[0].returncode, 0, bad)


if __name__ == "__main__":
    unittest.main()
