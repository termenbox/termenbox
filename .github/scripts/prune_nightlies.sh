#!/bin/sh
# Deletes nightly releases (and their tags) older than KEEP_DAYS, never the one channel-nightly points at (R68).
#   prune_nightlies.sh [KEEP_DAYS]      (default 14; DRY_RUN=1 only prints). Needs gh, jq and GH_TOKEN.
# If the alias cannot be read, nothing is deleted: without it there is no telling which release must stay.
set -eu
keep="${1:-14}"
tmp="$(mktemp -d)"
if ! gh release download channel-nightly -p manifest.json -D "$tmp" 2>/dev/null; then
  echo "channel-nightly has no manifest; deleting nothing"
  exit 0
fi
# the tag is the directory of the asset URL: .../releases/download/<tag>/<file>
target="$(jq -r '.binaries | to_entries[0].value.url | split("/") | .[-2]' "$tmp/manifest.json")"
[ -n "$target" ] && [ "$target" != null ] || { echo "cannot read the alias target; deleting nothing"; exit 0; }
cutoff="$(date -u -d "$keep days ago" +%Y%m%d)"
echo "alias target $target; deleting nightlies before $cutoff"
gh release list --limit 300 --json tagName -q '.[].tagName' | grep -E '^v[0-9]{4}\.[0-9]{2}\.[0-9]{2}-nightly$' | while read -r tag; do
  day="$(printf '%s' "$tag" | sed -E 's/^v([0-9]{4})\.([0-9]{2})\.([0-9]{2})-nightly$/\1\2\3/')"
  [ "$day" -lt "$cutoff" ] || continue
  if [ "$tag" = "$target" ]; then echo "keeping $tag (the alias points at it)"; continue; fi
  echo "deleting $tag"
  [ -n "${DRY_RUN:-}" ] || gh release delete "$tag" --cleanup-tag --yes
done
