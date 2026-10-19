#!/bin/sh
# Builds signed test releases for the installer and updater tests, all signed by one throwaway key.
#   make-test-releases.sh STATIC_BINARY OUT_DIR
# OUT_DIR gets: release (the binary as the first install), release2 (9.0.1, a different binary that works),
# release3 (9.0.2, a binary that starts but never serves, so the update must roll back), release4 (9.0.3, frozen),
# key / key.pub and keys.txt (the public key in release-keys format). The update releases use versions far above
# the crate's own so they stay "newer" whichever version is being built.
set -eu
BIN="$1"
OUT="$2"
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$OUT"
ssh-keygen -q -t ed25519 -N '' -C test -f "$OUT/key"
echo "t1 $(cut -d' ' -f1,2 "$OUT/key.pub")" > "$OUT/keys.txt"

release() { # dir version [--freeze]  (binary at $dir/termenbox-server-x86_64-linux)
  dir="$1"; version="$2"; shift 2
  cp "$dir/termenbox-server-x86_64-linux" "$dir/termenbox-server-aarch64-linux"
  python3 "$HERE/make_manifest.py" "$version" "$dir" "$dir/manifest.json" --no-tmux "$@" >/dev/null
  ssh-keygen -Y sign -n termenbox-release -f "$OUT/key" "$dir/manifest.json" >/dev/null 2>&1
}

mkdir -p "$OUT/release" "$OUT/release2" "$OUT/release3" "$OUT/release4"
cp "$BIN" "$OUT/release/termenbox-server-x86_64-linux"
release "$OUT/release" 0.1.0

cp "$BIN" "$OUT/release2/termenbox-server-x86_64-linux"
printf '\n# 9.0.1\n' >> "$OUT/release2/termenbox-server-x86_64-linux"   # trailing bytes: a different file that still runs
release "$OUT/release2" 9.0.1

printf '#!/bin/sh\nif [ "$1" = "--help" ]; then exit 0; fi\nexec sleep 3600\n' > "$OUT/release3/termenbox-server-x86_64-linux"
chmod +x "$OUT/release3/termenbox-server-x86_64-linux"
release "$OUT/release3" 9.0.2

cp "$BIN" "$OUT/release4/termenbox-server-x86_64-linux"
printf '\n# 9.0.3\n' >> "$OUT/release4/termenbox-server-x86_64-linux"
release "$OUT/release4" 9.0.3 --freeze
