#!/bin/sh
# Builds signed test releases for the installer and updater tests, all signed by one throwaway key.
#   make-test-releases.sh STATIC_BINARY OUT_DIR [TMUX_DIR]
# TMUX_DIR is what .github/scripts/build-tmux.sh writes (tmux plus the three licence texts). Every release carries
# that tmux as the bundled one (decision 0119). Without it a stub that only answers `tmux -V` stands in, which is
# enough to test the install and the update but not a session.
# OUT_DIR gets: release (the binary as the first install), release2 (0.99.1, a different binary that works),
# release3 (0.99.2, a binary that starts but never serves, so the update must roll back), release4 (0.99.3, frozen),
# key / key.pub and keys.txt (the public key in release-keys format). The update releases use versions above
# the crate's own but in the same major (0.99.x), because the updater holds a major bump back for the owner.
set -eu
BIN="$1"
OUT="$2"
TMUX_DIR="${3:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$OUT"
ssh-keygen -q -t ed25519 -N '' -C test -f "$OUT/key"
echo "t1 $(cut -d' ' -f1,2 "$OUT/key.pub")" > "$OUT/keys.txt"

# The tmux every release starts from, and its notices.
mkdir -p "$OUT/tmux"
if [ -n "$TMUX_DIR" ]; then
  cp "$TMUX_DIR/tmux" "$OUT/tmux/tmux"
  for n in tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING; do cp "$TMUX_DIR/$n" "$OUT/tmux/$n"; done
else
  printf '#!/bin/sh\nif [ "$1" = "-V" ]; then echo "tmux 3.8"; exit 0; fi\necho "stub tmux: not a real tmux" >&2\nexit 1\n' > "$OUT/tmux/tmux"
  for n in tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING; do echo "test notice $n" > "$OUT/tmux/$n"; done
fi
chmod 755 "$OUT/tmux/tmux"

release() { # dir version [--freeze]  (binary at $dir/termenbox-server-x86_64-linux, tmux from $OUT/tmux unless the dir has its own)
  dir="$1"; version="$2"; shift 2
  cp "$dir/termenbox-server-x86_64-linux" "$dir/termenbox-server-aarch64-linux"
  [ -f "$dir/tmux-x86_64-linux" ] || cp "$OUT/tmux/tmux" "$dir/tmux-x86_64-linux"
  cp "$dir/tmux-x86_64-linux" "$dir/tmux-aarch64-linux"
  for n in tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING; do cp "$OUT/tmux/$n" "$dir/$n"; done
  # The assets are named by the signed manifest, so it must point at where the test machine mounts this directory.
  python3 "$HERE/make_manifest.py" "$version" "$dir" "$dir/manifest.json" --base-url "file:///$(basename "$dir")" "$@" >/dev/null
  ssh-keygen -Y sign -n termenbox-release -f "$OUT/key" "$dir/manifest.json" >/dev/null 2>&1
}

mkdir -p "$OUT/release" "$OUT/release2" "$OUT/release3" "$OUT/release4"
cp "$BIN" "$OUT/release/termenbox-server-x86_64-linux"
release "$OUT/release" 0.1.0

cp "$BIN" "$OUT/release2/termenbox-server-x86_64-linux"
printf '\n# 0.99.1\n' >> "$OUT/release2/termenbox-server-x86_64-linux"   # trailing bytes: a different file that still runs
cp "$OUT/tmux/tmux" "$OUT/release2/tmux-x86_64-linux"
printf '\n# 0.99.1\n' >> "$OUT/release2/tmux-x86_64-linux"                 # the same for tmux: the update must carry it too
release "$OUT/release2" 0.99.1

printf '#!/bin/sh\nif [ "$1" = "--help" ]; then exit 0; fi\nexec sleep 3600\n' > "$OUT/release3/termenbox-server-x86_64-linux"
chmod +x "$OUT/release3/termenbox-server-x86_64-linux"
release "$OUT/release3" 0.99.2

cp "$BIN" "$OUT/release4/termenbox-server-x86_64-linux"
printf '\n# 0.99.3\n' >> "$OUT/release4/termenbox-server-x86_64-linux"
release "$OUT/release4" 0.99.3 --freeze
