#!/usr/bin/env python3
"""Tests for image_digests.py: the two linux images are found among attestations, anything less is refused."""
import unittest

from image_digests import digests_from_index

A, B, C = ("sha256:" + c * 64 for c in "abc")


def entry(arch, digest, os="linux"):
    return {"digest": digest, "platform": {"architecture": arch, "os": os}}


class ImageDigests(unittest.TestCase):
    def test_both_linux_images_are_found_and_attestations_are_skipped(self):
        index = {"manifests": [entry("amd64", A), entry("arm64", B), entry("unknown", C, os="unknown")]}
        self.assertEqual(digests_from_index(index), {"x86_64": A, "aarch64": B})

    def test_a_missing_architecture_is_refused(self):
        with self.assertRaises(ValueError):
            digests_from_index({"manifests": [entry("amd64", A)]})

    def test_a_malformed_digest_is_ignored_and_so_the_architecture_is_missing(self):
        with self.assertRaises(ValueError):
            digests_from_index({"manifests": [entry("amd64", A), entry("arm64", "sha256:xyz")]})

    def test_two_different_images_for_one_architecture_are_refused(self):
        with self.assertRaises(ValueError):
            digests_from_index({"manifests": [entry("amd64", A), entry("amd64", C), entry("arm64", B)]})


if __name__ == "__main__":
    unittest.main()
