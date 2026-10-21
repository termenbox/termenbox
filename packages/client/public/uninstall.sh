#!/bin/sh
# Termenbox uninstaller. Served by the client at /uninstall.sh.
#
#   curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh                 shows what would be removed, changes nothing
#   (an install made for your own user without root is removed without sudo: curl ... | sh -s -- --yes)
#   curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh -s -- --yes     removes it
#
# Options (after --yes): --delete-home  also delete the home of the `termenbox` user, only if the installer created it
#                        --keep-state   keep the server's keys and owner (re-installing then keeps the same server)
#                        --force        the server binary is already gone, so the journaled changes cannot be undone: remove
#                                       only the leftover journal and state (check for a leftover service or unit files yourself)
# Never touched: your repositories, the home of the user your sessions run as (unless --delete-home and we created it),
# and tmux sessions already running (the service is built so that stopping it leaves them alive).
# This removes the server from this machine. To also drop it from your server list, use Remove server in the app.
set -eu

YES=""; KEEP_STATE=""; DELETE_HOME=""; FORCE=""
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    --keep-state) KEEP_STATE=1 ;;
    --delete-home) DELETE_HOME=1 ;;
    --force) FORCE=1 ;;
    *) printf 'termenbox: unknown option %s\n' "$a" >&2; exit 1 ;;
  esac
done

LEFTOVER=" and state"; [ -z "$KEEP_STATE" ] || LEFTOVER=" (the state is kept)"
say() { printf '%s\n' "$*"; }
die() { printf 'termenbox: %s\n' "$*" >&2; exit 1; }
# An install that needed no root (decision 0113) lives in the home of the user who made it and is removed by that user.
if [ "$(id -u)" -ne 0 ]; then
  UBIN="$HOME/.local/bin/termenbox-server"
  UJOURNAL="$HOME/.local/state/termenbox"
  USTATE="$HOME/.config/termenbox"
  if [ ! -x "$UBIN" ] && [ ! -d "$UJOURNAL" ]; then
    die "run this as root, or as the user the server was installed for: curl -fsSL <client>/uninstall.sh | sudo sh"
  fi
  say "Install for $(id -un), no root:"
  if [ -x "$UBIN" ] && [ -d "$UJOURNAL" ]; then
    "$UBIN" --journal-dir "$UJOURNAL" --revert --dry-run | sed 's/^/  /'
  fi
  [ -n "$KEEP_STATE" ] || say "  - the server's state   $USTATE (keys, owner)"
  [ ! -x "$UBIN" ] || say "  - Termenbox's hooks in Claude Code's settings, if you connected them"
  [ ! -x "$UBIN" ] || say "  - Termenbox's plugin in OpenCode's config, if you connected it"
  if [ -z "$YES" ]; then say ""; say "Nothing was changed. Run again with --yes to remove it."; exit 0; fi
  export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
  systemctl --user disable --now termenbox-update.timer termenbox-request.path termenbox.service >/dev/null 2>&1 || true
  # Before the state goes: the journal there says which settings file the hooks are in.
  if [ -x "$UBIN" ]; then
    TERMENBOX_CONFIG_DIR="$USTATE" "$UBIN" --disconnect-claude || say "Could not remove Termenbox's hooks from Claude Code's settings; remove the entries that run 'termenbox-server --hook claude' by hand."
    TERMENBOX_CONFIG_DIR="$USTATE" "$UBIN" --disconnect-opencode || say "Could not remove Termenbox's OpenCode plugin; remove the plugin file it wrote (usually ~/.config/opencode/plugins/termenbox.js) by hand."
  fi
  if [ -x "$UBIN" ] && [ -d "$UJOURNAL" ]; then
    "$UBIN" --journal-dir "$UJOURNAL" --revert || die "some changes could not be undone; see the lines above. The server's state was kept."
  elif [ -d "$UJOURNAL" ]; then
    [ -n "$FORCE" ] || die "$UBIN is missing, so the journaled changes in $UJOURNAL cannot be undone. Nothing was removed. If the service and its unit files are already gone, remove the leftovers with: curl -fsSL <client>/uninstall.sh | sh -s -- --yes --force"
    say "Removing only the leftover journal$LEFTOVER: the binary is gone, so nothing journaled is undone. Check $HOME/.config/systemd/user for termenbox*.service and termenbox-*.timer/path files."
  fi
  [ -n "$KEEP_STATE" ] || rm -rf "$USTATE"
  rm -rf "$UJOURNAL"
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  say "Removed. Sessions already running in tmux were not stopped (list them with: tmux -L termenbox ls, or plain tmux ls if this install kept your default tmux server, decision 0120; the tmux bundled with Termenbox, if it installed one, was removed with it, so that needs a tmux of your own)."
  exit 0
fi

NAME="${TERMENBOX_CONTAINER:-termenbox}"
ROOT="${TERMENBOX_DATA:-/opt/termenbox}"
case "$ROOT" in /|/opt|/home|/root|/usr|/var|/etc|"") die "refusing to use $ROOT as the data directory" ;; esac
BIN=/usr/local/bin/termenbox-server
STATE=/var/lib/termenbox
JOURNAL=/var/lib/termenbox-root
UNIT_NAME=termenbox

found=""

# Isolated container install.
if command -v docker >/dev/null 2>&1 && docker inspect "$NAME" >/dev/null 2>&1; then
  found=1
  say "Isolated container install:"
  say "  - the container   $NAME"
  say "  - its data        $ROOT (the sessions' home, keys and owner; repositories and agent logins inside it are deleted too)"
  if [ -n "$YES" ]; then
    docker rm -f "$NAME" >/dev/null
    if [ -n "$KEEP_STATE" ]; then say "Removed the container (kept $ROOT)."; else rm -rf "$ROOT"; say "Removed the container and $ROOT."; fi
  fi
fi

# Native host install.
if [ -x "$BIN" ] || [ -d "$JOURNAL" ]; then
  found=1
  say "Host install:"
  if [ -x "$BIN" ] && [ -d "$JOURNAL" ]; then
    "$BIN" --journal-dir "$JOURNAL" --revert --dry-run | sed 's/^/  /'
  fi
  [ -n "$KEEP_STATE" ] || say "  - the server's state   $STATE (keys, owner)"
  [ ! -x "$BIN" ] || say "  - Termenbox's hooks in Claude Code's settings, if you connected them"
  [ ! -x "$BIN" ] || say "  - Termenbox's plugin in OpenCode's config, if you connected it"
  if [ -n "$YES" ]; then
    systemctl disable --now termenbox-update.timer termenbox-request.path "$UNIT_NAME" >/dev/null 2>&1 || true
    if [ -x "$BIN" ]; then
      TERMENBOX_CONFIG_DIR="$STATE" "$BIN" --disconnect-claude || say "Could not remove Termenbox's hooks from Claude Code's settings; remove the entries that run 'termenbox-server --hook claude' by hand."
      TERMENBOX_CONFIG_DIR="$STATE" "$BIN" --disconnect-opencode || say "Could not remove Termenbox's OpenCode plugin; remove the plugin file it wrote (usually ~/.config/opencode/plugins/termenbox.js) by hand."
    fi
    if [ -x "$BIN" ] && [ -d "$JOURNAL" ]; then
      "$BIN" --journal-dir "$JOURNAL" --revert ${DELETE_HOME:+--delete-home} || die "some changes could not be undone; see the lines above. The server's state was kept."
    elif [ -d "$JOURNAL" ]; then
      [ -n "$FORCE" ] || die "$BIN is missing, so the journaled changes in $JOURNAL cannot be undone. Nothing was removed. If the service and its unit files are already gone, remove the leftovers (the journal and, unless --keep-state, $STATE) with: curl -fsSL <client>/uninstall.sh | sudo sh -s -- --yes --force"
      say "Removing only the leftover journal$LEFTOVER: the binary is gone, so nothing journaled is undone. Check /etc/systemd/system for termenbox*.service, termenbox-*.timer and termenbox-*.path files."
    fi
    [ -n "$KEEP_STATE" ] || rm -rf "$STATE"
    systemctl daemon-reload >/dev/null 2>&1 || true
    say "Removed. Sessions already running in tmux were not stopped (list them with: tmux -L termenbox ls, or plain tmux ls if this install kept the default tmux server, as their user, decision 0120; the tmux bundled with Termenbox, if it installed one, was removed with it, so that needs a tmux of your own)."
  fi
fi

[ -n "$found" ] || { say "Termenbox is not installed on this machine."; exit 0; }
[ -n "$YES" ] || { say ""; say "Nothing was changed. Run again with --yes to remove it."; }
