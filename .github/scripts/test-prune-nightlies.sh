#!/bin/sh
# prune_nightlies.sh with a stub gh: old nightlies go, recent ones and the alias target stay (R68).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
day() { date -u -d "$1 days ago" +%Y.%m.%d; }
{ for n in 1 3 13 15 20 40; do echo "v$(day $n)-nightly"; done; echo v0.2.0; echo v0.1.9-rc.1; } > "$tmp/releases"
old="v$(day 40)-nightly"
# the alias points at a 40 days old nightly (nothing newer built since)
printf '{"binaries":{"x86_64":{"url":"https://github.com/termenbox/termenbox/releases/download/%s/termenbox-server-x86_64-linux"}}}' "$old" > "$tmp/alias.json"
cat > "$tmp/gh" <<STUB
#!/bin/sh
case "\$1 \$2" in
  "release download") while [ \$# -gt 0 ]; do [ "\$1" = -D ] && d="\$2"; shift; done; [ -f "$tmp/alias.json" ] && cp "$tmp/alias.json" "\$d/manifest.json" ;;
  "release list") cat "$tmp/releases" | sed 's/.*/{"tagName":"&"}/' | jq -r '.tagName' ;;
  "release delete") echo "\$3" >> "$tmp/deleted" ;;
esac
STUB
chmod +x "$tmp/gh"
PATH="$tmp:$PATH" sh "$here/prune_nightlies.sh" 14 > "$tmp/out"
deleted="$(sort "$tmp/deleted")"
expect="$(printf 'v%s-nightly\n' "$(day 15)" "$(day 20)" | sort)"
[ "$deleted" = "$expect" ] || { cat "$tmp/out"; echo "deleted: $deleted"; echo "FAIL: wrong set deleted"; exit 1; }
grep -q "keeping $old" "$tmp/out" || { echo "FAIL: the alias target was not protected"; exit 1; }
rm -f "$tmp/deleted" "$tmp/alias.json"
PATH="$tmp:$PATH" sh "$here/prune_nightlies.sh" 14 | grep -q "deleting nothing" || { echo "FAIL: pruned without an alias"; exit 1; }
[ ! -f "$tmp/deleted" ] || { echo "FAIL: deleted without an alias"; exit 1; }
echo ok
