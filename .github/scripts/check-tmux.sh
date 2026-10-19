#!/bin/sh
# The gate for the bundled tmux (decision 0119): the binary is fully static and runs on a bare host, with an empty
# PATH and no terminfo database, and drives a real session in control mode.
#   check-tmux.sh TMUX_BINARY
set -eu
BIN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"

if readelf -l "$BIN" | grep -q INTERP; then echo "tmux asks for a dynamic loader"; exit 1; fi
if readelf -d "$BIN" 2>/dev/null | grep -q NEEDED; then echo "tmux needs shared libraries"; exit 1; fi

HOME_DIR="$(mktemp -d)"
trap 'env -i PATH= TMUX_TMPDIR="$HOME_DIR" "$BIN" kill-server >/dev/null 2>&1 || true; rm -rf "$HOME_DIR"' EXIT
# An empty environment: no PATH, no terminfo variables, and a terminfo directory that does not exist.
tm() { env -i PATH= HOME="$HOME_DIR" TMUX_TMPDIR="$HOME_DIR" TERM=xterm-256color \
  TERMINFO=/nonexistent TERMINFO_DIRS=/nonexistent "$BIN" -f /dev/null "$@"; }

version="$(tm -V)"
echo "$version"
case "$version" in "tmux 3."[2-9]*|"tmux 3."[1-9][0-9]*|"tmux "[4-9]*) ;; *) echo "tmux is older than 3.2"; exit 1 ;; esac

tm new-session -d -s gate -x 80 -y 24 '/bin/sh -c "echo gate-ok; exec /bin/sleep 60"'
sleep 1
tm list-sessions | grep -q '^gate:' || { echo "no session after new-session"; exit 1; }
tm capture-pane -p -t gate | grep -q gate-ok || { echo "the pane did not run its command"; exit 1; }

# Control mode, which is what the server speaks (decision 0054).
out="$(printf 'list-sessions\n' | tm -C attach -t gate 2>&1 || true)"
echo "$out" | grep -q '^%begin' || { echo "control mode did not answer: $out"; exit 1; }

# A client on a terminal: the compiled-in terminfo has to be found for every TERM the server uses.
SCRIPT_BIN="$(command -v script || true)"
TIMEOUT_BIN="$(command -v timeout || true)"
[ -n "$SCRIPT_BIN" ] && [ -n "$TIMEOUT_BIN" ] || { echo "the gate needs script and timeout"; exit 1; }
{
  for term in xterm-256color screen-256color tmux-256color xterm linux; do
    res="$(env -i PATH= HOME="$HOME_DIR" TMUX_TMPDIR="$HOME_DIR" TERM="$term" TERMINFO=/nonexistent TERMINFO_DIRS=/nonexistent \
      "$TIMEOUT_BIN" 3 "$SCRIPT_BIN" -qec "$BIN -f /dev/null attach -t gate" /dev/null </dev/null 2>&1 || true)"
    if echo "$res" | grep -qi 'unsuitable terminal\|not a terminal'; then echo "TERM=$term: $res"; exit 1; fi
  done
}
echo "tmux gate passed"
