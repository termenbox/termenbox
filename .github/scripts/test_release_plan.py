#!/usr/bin/env python3
"""Tests for release_plan.py: a tag selects exactly one channel, and aliases only ever move forward (R68)."""
import unittest
from pathlib import Path
import importlib.util

spec = importlib.util.spec_from_file_location("release_plan", Path(__file__).with_name("release_plan.py"))
rp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rp)


class Tags(unittest.TestCase):
    def test_a_stable_tag_is_stable_and_feeds_latest_and_the_beta_alias(self):
        p = rp.from_tag("v0.2.0")
        self.assertEqual((p["channel"], p["version"], p["latest"], p["prerelease"]), ("stable", "0.2.0", True, False))
        self.assertEqual(p["docker_tags"], ["latest", "0.2", "0.2.0"])
        self.assertEqual(p["aliases"], ["channel-beta"])

    def test_beta_and_rc_tags_are_beta_prereleases(self):
        for tag in ("v0.2.0-rc.1", "v0.2.0-beta.3"):
            p = rp.from_tag(tag)
            self.assertEqual((p["channel"], p["version"], p["core"]), ("beta", tag[1:], "0.2.0"))
            self.assertEqual((p["prerelease"], p["latest"], p["docker_tags"]), (True, False, ["beta", tag[1:]]))

    def test_no_other_tag_reaches_a_release(self):
        for tag in ("v0.2.0-nightly", "v2026.10.22-nightly", "v0.2", "v0.2.0.1", "v01.2.3", "v1.2.3-alpha.1",
                    "v1.2.3-rc", "v1.2.3-rc.01", "v1.2.3-rc.1+x", "0.2.0", "v1.2.3 ", "latest"):
            self.assertEqual(rp.from_tag(tag)["channel"], "none", tag)

    def test_a_nightly_has_a_dated_tag_and_a_prerelease_version(self):
        p = rp.nightly("0.1.36", "20261022")
        self.assertEqual((p["tag"], p["version"]), ("v2026.10.22-nightly", "0.1.36-nightly.20261022"))
        self.assertEqual((p["docker_tags"], p["aliases"]), (["nightly", "0.1.36-nightly.20261022"], ["channel-nightly"]))
        self.assertEqual((p["prerelease"], p["latest"]), (True, False))


class Order(unittest.TestCase):
    def test_semver_precedence(self):
        ordered = ["0.1.36-nightly.20261022", "0.1.36-nightly.20261023", "0.1.36", "0.1.37-rc.1", "0.1.37-rc.2",
                   "0.1.37-rc.10", "0.1.37", "0.2.0", "1.0.0"]
        for lower, higher in zip(ordered, ordered[1:]):
            self.assertTrue(rp.newer(higher, lower), f"{higher} > {lower}")
            self.assertFalse(rp.newer(lower, higher), f"{lower} < {higher}")

    def test_an_equal_version_is_not_newer_and_build_metadata_is_ignored(self):
        self.assertFalse(rp.newer("0.2.0", "0.2.0"))
        self.assertFalse(rp.newer("0.2.0+b2", "0.2.0+b1"))

    def test_a_stable_release_replaces_the_rc_it_came_from(self):
        self.assertTrue(rp.newer("0.2.0", "0.2.0-rc.3"))
        self.assertFalse(rp.newer("0.2.0-rc.3", "0.2.0"))


if __name__ == "__main__":
    unittest.main()
