#!/usr/bin/env python3
"""Resolves the per-architecture image digests of a release for the signed manifest (decision 0154).

usage: image_digests.py IMAGE_REF OUT [--wait SECONDS]

IMAGE_REF is the multi-architecture tag docker-build.yaml pushed for the release (ghcr.io/termenbox/termenbox:1.2.3).
OUT receives {"x86_64": "sha256:...", "aarch64": "sha256:..."}, the digests of the two linux images the tag lists. The
tag is pushed by a workflow that runs beside the release, so the image may not exist yet: --wait polls for it until
the time is up. Both architectures are required; a tag that lists only one is not written.
"""
import json
import re
import subprocess
import sys
import time

PLATFORMS = {"amd64": "x86_64", "arm64": "aarch64"}
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def digests_from_index(index):
    """The digest per architecture from an image index; attestation entries (platform unknown) are skipped."""
    found = {}
    for entry in index.get("manifests", []):
        platform = entry.get("platform", {})
        arch = PLATFORMS.get(platform.get("architecture"))
        digest = entry.get("digest", "")
        if platform.get("os") != "linux" or arch is None or not DIGEST.fullmatch(digest):
            continue
        if arch in found and found[arch] != digest:
            raise ValueError(f"the tag lists two different {arch} images")
        found[arch] = digest
    missing = sorted(set(PLATFORMS.values()) - set(found))
    if missing:
        raise ValueError(f"the tag has no linux image for {', '.join(missing)}")
    return found


def inspect(ref):
    run = subprocess.run(["docker", "buildx", "imagetools", "inspect", "--raw", ref], capture_output=True, text=True)
    if run.returncode != 0:
        raise ValueError(run.stderr.strip() or "inspect failed")
    return json.loads(run.stdout)


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    wait = 0
    if "--wait" in argv:
        wait = int(argv[argv.index("--wait") + 1])
        args = [a for a in args if a != argv[argv.index("--wait") + 1]]
    if len(args) != 2:
        sys.exit(__doc__)
    ref, out = args
    deadline = time.monotonic() + wait
    while True:
        try:
            digests = digests_from_index(inspect(ref))
            break
        except ValueError as e:
            if time.monotonic() >= deadline:
                sys.exit(f"{ref}: {e}")
            print(f"waiting for {ref}: {e}", flush=True)
            time.sleep(30)
    with open(out, "w") as f:
        json.dump(digests, f, sort_keys=True, indent=2)
        f.write("\n")
    print(f"wrote {out}: {digests}")


if __name__ == "__main__":
    main(sys.argv)
