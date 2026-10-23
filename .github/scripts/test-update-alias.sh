#!/bin/sh
# update_alias.sh with a stub gh: an alias is created when missing, replaced by a newer version, never by an older one (R68).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
cat > "$tmp/gh" <<STUB
#!/bin/sh
echo "\$1 \$2" >> "$tmp/calls"
case "\$1 \$2" in
  "release view") [ -f "$tmp/alias.json" ] ;;
  "release download") while [ \$# -gt 0 ]; do [ "\$1" = -D ] && d="\$2"; shift; done; cp "$tmp/alias.json" "\$d/manifest.json" ;;
esac
STUB
chmod +x "$tmp/gh"
manifest() { printf '{"version":"%s"}' "$1" > "$tmp/manifest.json"; : > "$tmp/manifest.json.sig"; }
run() { : > "$tmp/calls"; PATH="$tmp:$PATH" sh "$here/update_alias.sh" channel-beta "$tmp/manifest.json" "$tmp/manifest.json.sig" > /dev/null; }

manifest 0.2.0-rc.1; rm -f "$tmp/alias.json"; run
grep -q "release create" "$tmp/calls" || { echo "FAIL: a missing alias was not created"; exit 1; }

printf '{"version":"0.2.0-rc.1"}' > "$tmp/alias.json"
manifest 0.2.0-rc.2; run
grep -q "release upload" "$tmp/calls" || { echo "FAIL: a newer beta did not replace the alias"; exit 1; }
manifest 0.2.0; run
grep -q "release upload" "$tmp/calls" || { echo "FAIL: the stable release did not replace its rc"; exit 1; }

printf '{"version":"0.2.0"}' > "$tmp/alias.json"
manifest 0.2.1-rc.1; run
grep -q "release upload" "$tmp/calls" || { echo "FAIL: the next rc is newer than the last stable"; exit 1; }
printf '{"version":"0.2.1-rc.1"}' > "$tmp/alias.json"
manifest 0.2.0; run
! grep -q "release upload\|release create" "$tmp/calls" || { echo "FAIL: an older stable rolled the alias back"; exit 1; }
manifest 0.2.1-rc.1; run
! grep -q "release upload" "$tmp/calls" || { echo "FAIL: the same version was uploaded again"; exit 1; }
echo ok
