#!/bin/sh
# Compares the pins in build-tmux.sh with the newest upstream releases (decision 0119) and prints one tab-separated
# line per pin that is behind: NAME PINNED LATEST URL. Prints nothing when every pin is current; a feed that cannot be
# read stops the check with an error, so an outage is never mistaken for "up to date".
#   check-pins.sh [BUILD_SCRIPT]
# GITHUB_TOKEN, when set, is sent with the GitHub API calls to avoid anonymous rate limits.
set -eu

SCRIPT="${1:-$(dirname "$0")/build-tmux.sh}"
pin() { sed -n "s/^$1=//p" "$SCRIPT" | head -n 1; }
api() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL --retry 3 -H "Authorization: Bearer $GITHUB_TOKEN" "https://api.github.com/$1"
  else
    curl -fsSL --retry 3 "https://api.github.com/$1"
  fi
}
# latest NAME PINNED LATEST URL: report when LATEST sorts after PINNED.
report() {
  [ -n "$3" ] || { echo "$1: no release found in its feed" >&2; exit 1; }
  if [ "$2" != "$3" ] && [ "$(printf '%s\n%s\n' "$2" "$3" | sort -V | tail -n 1)" = "$3" ]; then
    printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4"
  fi
}

# tmux: the newest non-prerelease on GitHub.
latest="$(api repos/tmux/tmux/releases/latest | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
report tmux "$(pin TMUX_VERSION)" "$latest" https://github.com/tmux/tmux/releases

# libevent: only "-stable" releases count; the 2.2 line is tagged alpha and is not what the pin follows.
latest="$(api 'repos/libevent/libevent/releases?per_page=30' | sed -n 's/.*"tag_name": *"release-\([^"]*-stable\)".*/\1/p' | sort -V | tail -n 1)"
report libevent "$(pin LIBEVENT_VERSION)" "$latest" https://github.com/libevent/libevent/releases

# ncurses: the release tarballs in the GNU archive (patch levels are not published there).
latest="$(curl -fsSL --retry 3 https://ftp.gnu.org/gnu/ncurses/ | grep -o 'ncurses-[0-9][0-9.]*\.tar\.gz"' |
  sed 's/^ncurses-//; s/\.tar\.gz"$//' | sort -V | tail -n 1)"
report ncurses "$(pin NCURSES_VERSION)" "$latest" https://ftp.gnu.org/gnu/ncurses/
