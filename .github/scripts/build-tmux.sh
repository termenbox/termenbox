#!/bin/sh
# Builds the bundled tmux (decision 0119): static against musl, with libevent and ncurses built from pinned sources
# and the terminfo entries compiled into the binary. Builds for the machine it runs on (CI uses one native runner per
# architecture).
#   build-tmux.sh OUT_DIR
# OUT_DIR gets tmux, plus the licence texts: tmux-COPYING (tmux, ISC), tmux-libevent-LICENSE (BSD-3-Clause) and
# tmux-ncurses-COPYING (X11/MIT style). Needs musl-gcc, make, curl and tic (ncurses-bin) on the build machine.
# Moving any pin below is a reviewed change; a download that does not match its sha256 stops the build.
set -eu

TMUX_VERSION=3.8
TMUX_SHA256=e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e
TMUX_URL=https://github.com/tmux/tmux/releases/download/$TMUX_VERSION/tmux-$TMUX_VERSION.tar.gz
LIBEVENT_VERSION=2.1.13-stable
LIBEVENT_SHA256=f7e9383b8c0baa81b687e5b5eecc01beefaf1b19b64151d95ed61647fe7a315c
LIBEVENT_URL=https://github.com/libevent/libevent/releases/download/release-$LIBEVENT_VERSION/libevent-$LIBEVENT_VERSION.tar.gz
NCURSES_VERSION=6.6
NCURSES_SHA256=355b4cbbed880b0381a04c46617b7656e362585d52e9cf84a67e2009b749ff11
NCURSES_URL=https://ftp.gnu.org/gnu/ncurses/ncurses-$NCURSES_VERSION.tar.gz
# What the server sets TERM from for the panes (decision 0119).
FALLBACKS=xterm-256color,screen-256color,tmux-256color,xterm,linux

OUT="$1"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
JOBS="$(nproc 2>/dev/null || echo 2)"
export CC=musl-gcc
DEPS="$WORK/deps"

fetch() { # url sha256 name
  curl -fsSL --retry 3 -o "$WORK/$3.tar.gz" "$1"
  echo "$2  $WORK/$3.tar.gz" | sha256sum -c - >/dev/null || { echo "$3: sha256 does not match the pin"; exit 1; }
  mkdir "$WORK/$3"
  tar -xzf "$WORK/$3.tar.gz" -C "$WORK/$3" --strip-components=1
}
fetch "$LIBEVENT_URL" "$LIBEVENT_SHA256" libevent
fetch "$NCURSES_URL" "$NCURSES_SHA256" ncurses
fetch "$TMUX_URL" "$TMUX_SHA256" tmux

cd "$WORK/libevent"
./configure --prefix="$DEPS" --disable-shared --enable-static --disable-openssl --disable-samples \
  --disable-libevent-regress >/dev/null
make -j"$JOBS" >/dev/null
make install >/dev/null

# Terminfo compiled in as fallbacks, so the binary reads no database from the host.
cd "$WORK/ncurses"
./configure --prefix="$DEPS" --without-shared --without-debug --without-cxx --without-cxx-binding --without-ada \
  --without-manpages --without-tests --enable-widec --without-gpm --disable-database \
  --with-fallbacks="$FALLBACKS" --disable-stripping >/dev/null
make -j"$JOBS" >/dev/null
make install >/dev/null

cd "$WORK/tmux"
./configure --enable-static \
  LIBEVENT_CFLAGS="-I$DEPS/include" LIBEVENT_LIBS="-L$DEPS/lib -levent" \
  LIBNCURSES_CFLAGS="-I$DEPS/include -I$DEPS/include/ncursesw" LIBNCURSES_LIBS="-L$DEPS/lib -lncursesw" \
  CFLAGS="-Os" LDFLAGS="-static" >/dev/null
make -j"$JOBS" >/dev/null

strip -o "$OUT/tmux" "$WORK/tmux/tmux"
cp "$WORK/tmux/COPYING" "$OUT/tmux-COPYING"
cp "$WORK/libevent/LICENSE" "$OUT/tmux-libevent-LICENSE"
cp "$WORK/ncurses/COPYING" "$OUT/tmux-ncurses-COPYING"
echo "built tmux $TMUX_VERSION (libevent $LIBEVENT_VERSION, ncurses $NCURSES_VERSION) in $OUT"
