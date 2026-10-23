#!/bin/sh
# Points a rolling alias release (channel-beta, channel-nightly) at a published release (R68, decision 0132).
#   update_alias.sh ALIAS MANIFEST SIG
# The alias holds only manifest.json and its signature, a copy of the versioned release's; the assets stay under the
# real tag the manifest names. It moves only forward: a candidate that is not a newer SemVer than the alias's current
# manifest leaves it alone, so a late-finishing older run can never roll an alias back. Needs gh, jq and GH_TOKEN.
set -eu
alias="$1"; manifest="$2"; sig="$3"
here="$(dirname "$0")"
candidate="$(jq -r .version "$manifest")"

if gh release view "$alias" >/dev/null 2>&1; then
  tmp="$(mktemp -d)"
  if gh release download "$alias" -p manifest.json -D "$tmp"; then
    current="$(jq -r .version "$tmp/manifest.json")"
    if ! python3 "$here/release_plan.py" newer "$candidate" "$current"; then
      echo "$alias stays at $current ($candidate is not newer)"
      exit 0
    fi
    echo "$alias: $current -> $candidate"
  fi
  gh release upload "$alias" "$manifest" "$sig" --clobber
else
  echo "$alias: created at $candidate"
  # Never "latest": releases/latest/download is the stable channel's address.
  gh release create "$alias" --title "$alias" --prerelease --latest=false \
    --notes "Rolling pointer of the ${alias#channel-} channel: only the manifest and its signature, which name the real release. Do not download from here by hand." \
    "$manifest" "$sig"
fi
