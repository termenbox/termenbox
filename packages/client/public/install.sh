#!/bin/sh
# Termenbox installer. Served by the client at /install.sh.
#
#   curl -fsSL https://app.termenbox.dev/install.sh | sh -s -- <claim secret> <client origin> [mode]
#   curl -fsSL https://app.termenbox.dev/install.sh | sh -s -- - <client origin> [mode]      (shows a QR instead)
#
# The optional last word picks where sessions run (DECISION-0058, 0075):
#   (none)      an isolated Docker container (installs Docker if missing)
#   host-user   directly on this machine as a dedicated unprivileged user `termenbox`, as a systemd service
#   host-me     directly on this machine as the user who runs this command: sessions open on their behalf, in their
#               home, with their repositories, keys and agent logins. No user is created. No root is needed: it is a
#               user systemd service (decision 0113); what would need root is stated up front, not run.
#   host        directly on this machine as root, as a systemd service
# The host modes give the owner's browser a shell on the machine itself and need no Docker.
#
# It starts the server and hands it the claim secret. In host-me the server chooses its own UDP port from the
# reserved range (DECISION-0118); the other modes still pick one. It asks nothing and needs nothing afterwards: the
# server appears in Termenbox by itself. Safe to run again: a new secret re-claims the server (root on this host
# already allows that).
#
# Host modes: every change is recorded in /var/lib/termenbox-root before it is made and can be undone with
# `termenbox-server --revert`. The release is checked before anything is installed: its manifest must carry a valid
# signature by the release key below (checked with ssh-keygen), and the binary must match the signed SHA-256.
# The host modes also install a pinned, statically linked tmux next to the server (decision 0119), verified the same
# way, so no tmux has to be installed first and no package manager or root is needed for it. Docker keeps its own.
set -eu

SECRET="${1:--}"
ORIGIN="${2:-${TERMENBOX_CLIENT_URL:-https://app.termenbox.dev}}"
MODE="${3:-container}"
IMAGE="${TERMENBOX_IMAGE:-ghcr.io/termenbox/termenbox:latest}"
NAME="${TERMENBOX_CONTAINER:-termenbox}"
ROOT="${TERMENBOX_DATA:-/opt/termenbox}"

# The public keys that sign releases: `kid ssh-ed25519 key` lines, the same as packages/server/release-keys.txt.
# TERMENBOX_RELEASE_KEYS and TERMENBOX_RELEASE_URL exist for the installer's own tests; on a real install they are unset.
RELEASE_KEYS="${TERMENBOX_RELEASE_KEYS:-r1 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDNH6+yZgMpZl+tW8C/D+9Ga0bN39vXzIIJMKmHB6auf}"
RELEASE_URL="${TERMENBOX_RELEASE_URL:-https://github.com/termenbox/termenbox/releases/latest/download}"

say() { printf '%s\n' "$*"; }
die() { printf 'termenbox: %s\n' "$*" >&2; exit 1; }

case "$ORIGIN" in
  https://*|http://localhost*|http://127.0.0.1*) ;;
  *) die "the client address must start with https://" ;;
esac
ORIGIN="${ORIGIN%/}"
case "$MODE" in
  container|host|host-user|host-me) ;;
  *) die "unknown mode \"$MODE\" (use host-me, host-user, host, or leave it out)" ;;
esac
if [ "$SECRET" != "-" ]; then
  case "$SECRET" in
    *[!A-Za-z0-9_-]*) die "the claim secret has unexpected characters" ;;
  esac
  [ "${#SECRET}" -eq 43 ] || die "the claim secret has the wrong length"
fi

# host-me needs no root (decision 0113): it runs as the user who ran this command, as a user service. The old route (a system
# service) applies only to a machine whose system service already runs as this same user, from before 0113: refreshing it
# needs root. Any other user gets an install of their own, whatever else runs on the machine, so one machine can have a
# system install and a user install for each of its users.
ME="${SUDO_USER:-$(id -un)}"
LEGACY_SYSTEM=""
if [ "$MODE" = host-me ] && [ -f /etc/systemd/system/termenbox.service ] \
   && [ "$(sed -n 's/^User=//p' /etc/systemd/system/termenbox.service 2>/dev/null)" = "$ME" ]; then
  LEGACY_SYSTEM=1
fi
if [ -n "$LEGACY_SYSTEM" ] && [ "$(id -u)" -ne 0 ]; then
  say "Termenbox is already installed here as a system service running as $ME (an earlier version of the installer made it)."
  say "Refreshing it needs sudo, so you will be asked for your password. To move to the install that needs no root instead,"
  say "remove this one (keeping the server's keys and owner), then run this same command again:"
  say "  curl -fsSL $ORIGIN/uninstall.sh | sudo sh -s -- --yes --keep-state"
fi
if [ "$MODE" = host-me ] && [ -z "$LEGACY_SYSTEM" ]; then
  [ "$(id -u)" -ne 0 ] || die "host-me runs sessions as the user who runs this command and needs no root. Run it as your own user, without sudo (or use host / host-user for a system service)."
else
  # Root is needed for services, users and the firewall. Re-run this script under sudo; it was piped in, so fetch it again.
  # host-me needs to know who asked: the name is read before the switch to root, then sudo hands it on as SUDO_USER.
  if [ "$MODE" = "host-me" ] && [ "$(id -u)" -eq 0 ] && { [ -z "${SUDO_USER:-}" ] || [ "$SUDO_USER" = root ]; }; then
    die "host-me runs sessions as the user who runs this command, but this is root. Run it as your own user (it asks for sudo itself), or use host / host-user."
  fi
  if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null 2>&1 || die "run this as root (or install sudo)"
    exec sudo env TERMENBOX_IMAGE="$IMAGE" TERMENBOX_CONTAINER="$NAME" TERMENBOX_DATA="$ROOT" \
      TERMENBOX_SKIP_FIREWALL="${TERMENBOX_SKIP_FIREWALL:-}" TERMENBOX_DRY_RUN="${TERMENBOX_DRY_RUN:-}" \
      TERMENBOX_FORCE="${TERMENBOX_FORCE:-}" TERMENBOX_RELEASE_URL="${TERMENBOX_RELEASE_URL:-}" TERMENBOX_RELEASE_KEYS="${TERMENBOX_RELEASE_KEYS:-}" \
      sh -c "$(curl -fsSL "$ORIGIN/install.sh")" termenbox-install "$SECRET" "$ORIGIN" "$MODE"
  fi
fi

# Whether a local port is already taken, so the next listener picks a free one. `ss` (iproute2) is the normal
# source; on a machine without it (a minimal image) the kernel's own table is read instead, so two installs still
# get different ports. There the port column is hex, and for TCP only sockets in LISTEN (state 0A) count.
in_use() { # <tcp|udp> <port>
  _proto="$1"; _port="$2"
  if command -v ss >/dev/null 2>&1; then
    _ssflag="-ltn"; [ "$_proto" = udp ] && _ssflag="-lun"
    ss -H "$_ssflag" 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]$_port\$"
    return
  fi
  for _f in "/proc/net/$_proto" "/proc/net/${_proto}6"; do
    [ -r "$_f" ] || continue
    awk -v port="$_port" -v proto="$_proto" \
      'BEGIN { want = sprintf("%04X", port) }
       NR > 1 && (proto == "udp" || $4 == "0A") { n = split($2, a, ":"); if (toupper(a[n]) == want) found = 1 }
       END { exit !found }' "$_f" && return 0
  done
  return 1
}
tcp_in_use() { in_use tcp "$1"; }
udp_in_use() { in_use udp "$1"; }
random_port() { echo $((20000 + $(od -An -N2 -tu2 /dev/urandom | tr -d ' ') % 40000)); }
# pick_port <in_use function> <first candidate> <file holding the previous choice>
pick_port() {
  if [ -s "$3" ]; then cat "$3"; return; fi
  _p="$2"
  _tries=0
  while "$1" "$_p" && [ "$_tries" -lt 50 ]; do
    _p="$(random_port)"
    _tries=$((_tries + 1))
  done
  [ "$_tries" -lt 50 ] || die "could not find a free port"
  echo "$_p"
}

open_firewall() {
  [ -z "${TERMENBOX_SKIP_FIREWALL:-}" ] || return 0
  if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
    ufw allow "$1/udp" >/dev/null 2>&1 || true
  fi
  if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    firewall-cmd --permanent --add-port="$1/udp" >/dev/null 2>&1 && firewall-cmd --reload >/dev/null 2>&1 || true
  fi
}

# ---------------------------------------------------------------------------------------------------------
# Isolated container (Docker)
# ---------------------------------------------------------------------------------------------------------
install_container() {
  if ! command -v docker >/dev/null 2>&1; then
    say "Installing Docker..."
    curl -fsSL https://get.docker.com | sh >/dev/null 2>&1 || die "could not install Docker; install it and run this command again"
  fi
  systemctl enable --now docker >/dev/null 2>&1 || true
  docker info >/dev/null 2>&1 || die "Docker is installed but not running"

  mkdir -p "$ROOT/home"
  PORT="$(pick_port udp_in_use 4443 "$ROOT/udp_port")"
  [ -n "$PORT" ] || die "could not find a free UDP port"
  echo "$PORT" > "$ROOT/udp_port"
  open_firewall "$PORT"

  chown -R 1000:1000 "$ROOT/home"
  docker pull -q "$IMAGE" >/dev/null 2>&1 || docker image inspect "$IMAGE" >/dev/null 2>&1 || die "could not download $IMAGE"
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" --restart unless-stopped \
    -p "$PORT:$PORT/udp" \
    -e "TERMENBOX_UDP_PORT=$PORT" -e "TERMENBOX_CLIENT_URL=$ORIGIN" \
    -e "TERMENBOX_SERVER_NAME=$(hostname -s 2>/dev/null || hostname)" \
    -v "$ROOT/home:/home/node" \
    "$IMAGE" >/dev/null || die "could not start the server"

  # Wait for the server process, then hand over the claim.
  i=0
  until docker exec "$NAME" termenbox-server --help >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || die "the server did not start; see: docker logs $NAME"
    sleep 1
  done

  if [ "$SECRET" = "-" ]; then
    OUT="$(docker exec "$NAME" termenbox-server --register --qr)"
    case "$OUT" in
      *"is registered to"*) say "$OUT" ;;
      *)
        say "Scan this with Termenbox (Add Server > Scan a QR), or open the link:"
        say "$OUT"
        ;;
    esac
  else
    docker exec "$NAME" termenbox-server --claim "$SECRET" >/dev/null
    say "Termenbox server started. It will appear in the app within a minute."
  fi
}

# ---------------------------------------------------------------------------------------------------------
# This host (native, no Docker)
# ---------------------------------------------------------------------------------------------------------
BIN=/usr/local/bin/termenbox-server
UNIT_NAME=termenbox.service
UNIT=/etc/systemd/system/$UNIT_NAME
STATE=/var/lib/termenbox
JOURNAL=/var/lib/termenbox-root

# Hands the claim over (or prints the registration QR) as the user the service runs as.
native_claim() {
  _env="TERMENBOX_CONFIG_DIR=$STATE TERMENBOX_CLIENT_URL=$ORIGIN"
  # shellcheck disable=SC2086 # _env is a list of NAME=value words
  if [ "$RUN_USER" = root ] || [ "$RUN_USER" = "$(id -un)" ]; then _run="env $_env"; else _run="runuser -u $RUN_USER -- env $_env"; fi
  if [ "$SECRET" = "-" ]; then
    # shellcheck disable=SC2086
    OUT="$($_run "$BIN" --register --qr)"
    case "$OUT" in
      *"is registered to"*) say "$OUT" ;;
      *)
        say "Scan this with Termenbox (Add Server > Scan a QR), or open the link:"
        say "$OUT"
        ;;
    esac
  else
    # shellcheck disable=SC2086
    $_run "$BIN" --claim "$SECRET" >/dev/null
    say "Termenbox server started. It will appear in the app within a minute."
  fi
}

# journal KIND key=value...  records a change before it is made. $JBIN is a verified termenbox-server (the
# downloaded one during an install, the installed one afterwards); it does the writing.
journal() {
  _kind="$1"; shift
  _args=""
  for f in "$@"; do _args="$_args --journal-field $f"; done
  # shellcheck disable=SC2086
  "$JBIN" --journal-dir "$JOURNAL" --journal-record "$_kind" $_args >/dev/null || die "could not write the journal"
}

# The root-owned updater and request handler (decisions 0076, 0077). They run the installed binary as root, so the
# server itself can stay an unprivileged user. Idempotent: only what is missing is added, each unit journaled first.
install_helper_units() {
  if [ ! -d "$STATE/requests" ]; then
    journal dir_created "path=$STATE/requests"
    mkdir -p "$STATE/requests"
    chown "$RUN_USER" "$STATE/requests"
    chmod 700 "$STATE/requests"
  fi
  _added=""
  add_unit() { # name, content
    [ -f "/etc/systemd/system/$1" ] && return 0
    journal unit_installed "name=$1" "path=/etc/systemd/system/$1"
    printf '%s\n' "$2" > "/etc/systemd/system/$1"
    _added=1
  }
  add_unit termenbox-update.service "[Unit]
Description=Termenbox updater

[Service]
Type=oneshot
ExecStart=$BIN --root-helper update --journal-dir $JOURNAL"
  add_unit termenbox-update.timer "[Unit]
Description=Check for a new Termenbox release every day

[Timer]
OnCalendar=daily
RandomizedDelaySec=3600
Persistent=true

[Install]
WantedBy=timers.target"
  add_unit termenbox-request.service "[Unit]
Description=Termenbox root helper (requests from the server)

[Service]
Type=oneshot
ExecStart=$BIN --root-helper requests --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_unit termenbox-request.path "[Unit]
Description=Watch for requests from the Termenbox server

[Path]
PathExistsGlob=$STATE/requests/*.json

[Install]
WantedBy=multi-user.target"
  if [ -n "$_added" ]; then
    systemctl daemon-reload
    systemctl enable --now termenbox-update.timer termenbox-request.path >/dev/null 2>&1 || die "could not start the updater; see: systemctl status termenbox-update.timer"
  fi
}

# The release: a signed manifest first, then a binary that matches the hash in it. Sets VERSION, SHA and JBIN.
fetch_release() {
  say "Checking the latest release..."
  curl -fsSL --retry 3 -o "$TMP/manifest.json" "$RELEASE_URL/manifest.json" || die "could not download the release manifest"
  curl -fsSL --retry 3 -o "$TMP/manifest.json.sig" "$RELEASE_URL/manifest.json.sig" || die "could not download the release signature"
  printf '%s\n' "$RELEASE_KEYS" | awk 'NF >= 3 { print "termenbox-release " $2 " " $3 }' > "$TMP/allowed_signers"
  ssh-keygen -Y verify -f "$TMP/allowed_signers" -I termenbox-release -n termenbox-release \
    -s "$TMP/manifest.json.sig" < "$TMP/manifest.json" >/dev/null 2>&1 \
    || die "the release signature is not valid, so nothing was installed"
  grep -q '"freeze": *true' "$TMP/manifest.json" && die "new installs are paused by the maintainers (a signed freeze); try again later"
  VERSION="$(sed -n 's/.*"version": *"\([0-9.]*\)".*/\1/p' "$TMP/manifest.json" | head -1)"
  SHA="$(manifest_sha binaries "$ARCH")"
  case "$SHA" in
    *[!0-9a-f]*|"") die "the release has no $ARCH binary" ;;
  esac
  [ "${#SHA}" -eq 64 ] || die "the release has no valid $ARCH hash"
  curl -fsSL --retry 3 -o "$TMP/termenbox-server" "$RELEASE_URL/termenbox-server-$ARCH-linux" || die "could not download the server"
  echo "$SHA  $TMP/termenbox-server" | sha256sum -c - >/dev/null 2>&1 || die "the downloaded server does not match the signed hash, so nothing was installed"
  chmod 755 "$TMP/termenbox-server"
  "$TMP/termenbox-server" --help >/dev/null 2>&1 || die "the downloaded server does not run on this machine"
  JBIN="$TMP/termenbox-server"
  fetch_tmux
}

# manifest_sha SECTION KEY  prints the SHA-256 of manifest.json[SECTION][KEY] (the manifest is written with one key per line).
manifest_sha() {
  awk -v sec="$1" -v key="$2" '
    function nm(l) { sub(/^[^"]*"/, "", l); sub(/".*/, "", l); return l }
    /[{][ \t]*$/ { depth++; if (depth == 2) s = nm($0); else if (depth == 3) k = nm($0); next }
    /^[ \t]*[}]/ { depth--; next }
    depth == 3 && s == sec && k == key && /"sha256":/ { v = $0; sub(/.*"sha256": *"/, "", v); sub(/".*/, "", v); print v; exit }' "$TMP/manifest.json"
}

# The bundled tmux and its licence notices (decision 0119), checked against the signed manifest like the server.
# Sets TMUX_SHA (empty for a release from before tmux was bundled) and TMUX_NOTICES (the notice names downloaded).
TMUX_NOTICE_NAMES="tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING"
fetch_tmux() {
  TMUX_SHA=""
  TMUX_NOTICES=""
  _t="$(manifest_sha tmux "$ARCH")"
  [ -n "$_t" ] || return 0
  [ "${#_t}" -eq 64 ] && [ -z "$(printf '%s' "$_t" | tr -d '0-9a-f')" ] || die "the release has no valid $ARCH tmux hash"
  curl -fsSL --retry 3 -o "$TMP/termenbox-tmux" "$RELEASE_URL/tmux-$ARCH-linux" || die "could not download tmux"
  echo "$_t  $TMP/termenbox-tmux" | sha256sum -c - >/dev/null 2>&1 || die "the downloaded tmux does not match the signed hash, so nothing was installed"
  chmod 755 "$TMP/termenbox-tmux"
  "$TMP/termenbox-tmux" -V 2>/dev/null | grep -q '^tmux ' || die "the downloaded tmux does not run on this machine"
  for _n in $TMUX_NOTICE_NAMES; do
    _h="$(manifest_sha tmux_notices "$_n")"
    [ -n "$_h" ] || continue
    curl -fsSL --retry 3 -o "$TMP/$_n" "$RELEASE_URL/$_n" || die "could not download $_n"
    echo "$_h  $TMP/$_n" | sha256sum -c - >/dev/null 2>&1 || die "the downloaded $_n does not match the signed hash, so nothing was installed"
    TMUX_NOTICES="$TMUX_NOTICES $_n"
  done
  TMUX_SHA="$_t"
}

# put_file SRC DEST MODE  journals the file (as created, or as replaced with a backup), then puts it in place.
put_file() {
  _sum="$(sha256sum "$1" | cut -d' ' -f1)"
  if [ -e "$2" ]; then
    mkdir -p "$JOURNAL/backups"
    _backup="$JOURNAL/backups/$(basename "$2").$(date +%s)"
    cp -p "$2" "$_backup"
    journal file_replaced "path=$2" "backup=$_backup" "sha256=$_sum"
  else
    journal file_created "path=$2" "sha256=$_sum"
  fi
  install -m "$3" "$1" "$2"
}

# The bundled tmux and its notices go beside the server, where the user the sessions run as can reach them. The server
# looks there first, before tmux on PATH. Nothing to do for a release without one.
install_tmux() {
  [ -n "$TMUX_SHA" ] || return 0
  _dir="$(dirname "$BIN")"
  put_file "$TMP/termenbox-tmux" "$_dir/termenbox-tmux" 0755
  for _n in $TMUX_NOTICES; do
    put_file "$TMP/$_n" "$_dir/termenbox-$_n" 0644
  done
}

# A release from before tmux was bundled needs the machine's own. Said before anything is changed.
need_system_tmux() {
  [ -z "$TMUX_SHA" ] || return 0
  command -v tmux >/dev/null 2>&1 && return 0
  [ "$1" = root ] || die "this release has no bundled tmux and none is installed (sessions are tmux sessions); installing one takes root. Install it, for example: sudo apt-get install -y tmux   (or dnf, pacman, apk), then run this command again. Nothing was changed."
}

install_native() {
  [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1 \
    || die "this machine does not run systemd, which the host install needs. Use the isolated container install instead (leave out host / host-user)."
  command -v curl >/dev/null 2>&1 || die "curl is needed"
  command -v sha256sum >/dev/null 2>&1 || die "sha256sum is needed"
  command -v ssh-keygen >/dev/null 2>&1 || die "ssh-keygen is needed to check the release signature (install openssh-client) and nothing was changed"
  case "$(uname -m)" in
    x86_64|amd64) ARCH=x86_64 ;;
    aarch64|arm64) ARCH=aarch64 ;;
    *) die "unsupported processor $(uname -m); the host install supports x86_64 and aarch64" ;;
  esac
  RUN_USER=root
  [ "$MODE" = "host-user" ] && RUN_USER=termenbox
  if [ "$MODE" = "host-me" ]; then
    RUN_USER="${SUDO_USER:-}"
    case "$RUN_USER" in
      ""|root|*[!A-Za-z0-9._-]*) die "host-me needs a regular user account, not \"$RUN_USER\"" ;;
    esac
    id "$RUN_USER" >/dev/null 2>&1 || die "could not tell which user ran this command"
    # The unit's WorkingDirectory=~ and the sessions use this home; refuse an account that has none.
    _home="$( (getent passwd "$RUN_USER" 2>/dev/null || cat /etc/passwd) | sed -n "s/^$RUN_USER:[^:]*:[^:]*:[^:]*:[^:]*:\\([^:]*\\):.*/\\1/p" | head -n 1)"
    [ -d "$_home" ] || die "$RUN_USER has no home directory ($_home)"
  fi

  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT

  # Already installed here: refresh the claim only. TERMENBOX_FORCE=1 reinstalls.
  if [ -x "$BIN" ] && [ -f "$UNIT" ] && [ -z "${TERMENBOX_FORCE:-}" ]; then
    say "Termenbox is already installed on this machine."
    systemctl is-active --quiet "$UNIT_NAME" || systemctl start "$UNIT_NAME"
    RUN_USER="$(sed -n 's/^User=//p' "$UNIT")"
    RUN_USER="${RUN_USER:-root}"
    [ "$RUN_USER" = root ] || id "$RUN_USER" >/dev/null 2>&1 \
      || die "the installed service runs as $RUN_USER, which no longer exists. Remove Termenbox (uninstall.sh --yes) and install again."
    [ "$MODE" != host-me ] || [ "$RUN_USER" = "${SUDO_USER:-}" ] \
      || say "Note: sessions already run as $RUN_USER and stay that way. To change that, remove Termenbox first (curl -fsSL <client>/uninstall.sh | sudo sh -s -- --yes) and install again."
    JBIN="$BIN"
    install_helper_units # installs from before the updater existed get it here
    native_claim
    return
  fi

  fetch_release
  need_system_tmux root

  # 2. What will change. Everything below is recorded in the journal first.
  PORT="$(pick_port udp_in_use 4443 "$JOURNAL/udp_port")"
  HTTP_PORT="$(pick_port tcp_in_use 8080 "$JOURNAL/http_port")"
  [ -n "$PORT" ] && [ -n "$HTTP_PORT" ] || die "could not find free ports"
  say ""
  say "Termenbox $VERSION will be installed on this machine (sessions run as $RUN_USER):"
  say "  - the server  $BIN"
  [ "$RUN_USER" = root ] || { id "$RUN_USER" >/dev/null 2>&1 || say "  - a new user  $RUN_USER"; }
  if [ -n "$TMUX_SHA" ]; then
    say "  - tmux        $(dirname "$BIN")/termenbox-tmux (a pinned static build, used before any tmux on PATH)"
  else
    command -v tmux >/dev/null 2>&1 || say "  - the package tmux (sessions are tmux sessions)"
  fi
  say "  - state       $STATE"
  say "  - a service   $UNIT_NAME (starts at boot), and a daily update check (root-owned, signed releases only)"
  say "  - UDP port    $PORT (opened in the firewall if one is active)"
  say "Every change is recorded in $JOURNAL; undo them with: termenbox-server --revert --dry-run, then --revert"
  say ""
  [ -z "${TERMENBOX_DRY_RUN:-}" ] || { say "(dry run: nothing was changed)"; return; }

  mkdir -p "$JOURNAL"
  chmod 700 "$JOURNAL"
  echo "$PORT" > "$JOURNAL/udp_port"
  echo "$HTTP_PORT" > "$JOURNAL/http_port"

  # 3. The changes, each recorded first.
  if [ -e "$BIN" ]; then
    mkdir -p "$JOURNAL/backups"
    BACKUP="$JOURNAL/backups/termenbox-server.$(date +%s)"
    cp -p "$BIN" "$BACKUP"
    journal file_replaced "path=$BIN" "backup=$BACKUP" "sha256=$SHA"
  else
    journal file_created "path=$BIN" "sha256=$SHA"
  fi
  install -m 0755 "$TMP/termenbox-server" "$BIN"
  install_tmux

  if [ "$RUN_USER" != root ] && ! id "$RUN_USER" >/dev/null 2>&1; then
    journal user_created "name=$RUN_USER" "home=/home/$RUN_USER"
    useradd --create-home --shell /bin/bash "$RUN_USER" >/dev/null 2>&1 \
      || adduser -D -s /bin/sh "$RUN_USER" >/dev/null 2>&1 \
      || die "could not create the user $RUN_USER"
  fi

  if [ -z "$TMUX_SHA" ] && ! command -v tmux >/dev/null 2>&1; then
    say "Installing tmux..."
    if command -v apt-get >/dev/null 2>&1; then
      journal package_installed manager=apt name=tmux
      DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tmux >/dev/null 2>&1
    elif command -v dnf >/dev/null 2>&1; then
      journal package_installed manager=dnf name=tmux
      dnf install -y -q tmux >/dev/null 2>&1
    elif command -v yum >/dev/null 2>&1; then
      journal package_installed manager=yum name=tmux
      yum install -y -q tmux >/dev/null 2>&1
    elif command -v pacman >/dev/null 2>&1; then
      journal package_installed manager=pacman name=tmux
      pacman -S --noconfirm --quiet tmux >/dev/null 2>&1
    fi
    command -v tmux >/dev/null 2>&1 || die "could not install tmux; install it and run this command again"
  fi

  journal dir_created "path=$STATE"
  mkdir -p "$STATE"
  chown "$RUN_USER" "$STATE"
  chmod 700 "$STATE"

  if [ -z "${TERMENBOX_SKIP_FIREWALL:-}" ]; then
    if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
      journal firewall_rule_added tool=ufw "rule=$PORT/udp"
    elif command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
      journal firewall_rule_added tool=firewalld "rule=$PORT/udp"
    fi
    open_firewall "$PORT"
  fi

  journal unit_installed "name=$UNIT_NAME" "path=$UNIT"
  {
    echo "[Unit]"
    echo "Description=Termenbox terminal server"
    echo "After=network-online.target"
    echo "Wants=network-online.target"
    echo ""
    echo "[Service]"
    [ "$RUN_USER" = root ] || echo "User=$RUN_USER"
    echo "WorkingDirectory=~"
    [ "$RUN_USER" != root ] || echo "Environment=HOME=/root"
    echo "Environment=TERMENBOX_RUN=native"
    echo "Environment=TERMENBOX_RUN_USER=$RUN_USER"
    echo "Environment=TERMENBOX_CONFIG_DIR=$STATE"
    echo "Environment=TERMENBOX_HOST=127.0.0.1"
    echo "Environment=TERMENBOX_PORT=$HTTP_PORT"
    echo "Environment=TERMENBOX_UDP_PORT=$PORT"
    echo "Environment=TERMENBOX_CLIENT_URL=$ORIGIN"
    echo "ExecStart=$BIN"
    echo "Restart=on-failure"
    echo "RestartSec=2"
    echo "# The tmux sessions the server starts must outlive a restart or an update of the server."
    echo "KillMode=process"
    echo ""
    echo "[Install]"
    echo "WantedBy=multi-user.target"
  } > "$UNIT"
  systemctl daemon-reload
  systemctl enable --now "$UNIT_NAME" >/dev/null 2>&1 || die "could not start the service; see: journalctl -u $UNIT_NAME"

  i=0
  until curl -fs "http://127.0.0.1:$HTTP_PORT/health" >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || die "the server did not start; see: journalctl -u $UNIT_NAME"
    sleep 1
  done

  install_helper_units
  native_claim
}

# ---------------------------------------------------------------------------------------------------------
# This host, as you, without root (decision 0113): a user systemd service
# ---------------------------------------------------------------------------------------------------------
# Everything lives in the home of the user who ran the command and is recorded in a journal there. What would need
# root (lingering when it is denied, a firewall rule) is stated up front, never run.
install_user() {
  ME="$(id -un)"
  RUN_USER="$ME"
  case "${HOME:-}" in /*) ;; *) die "HOME is not set" ;; esac
  case "$HOME" in *[!A-Za-z0-9._/-]*) die "the home directory path has characters the installer does not handle: $HOME" ;; esac
  BIN="$HOME/.local/bin/termenbox-server"
  STATE="$HOME/.config/termenbox"
  JOURNAL="$HOME/.local/state/termenbox"
  UNIT_DIR="$HOME/.config/systemd/user"
  command -v curl >/dev/null 2>&1 || die "curl is needed"
  command -v sha256sum >/dev/null 2>&1 || die "sha256sum is needed"
  command -v ssh-keygen >/dev/null 2>&1 || die "ssh-keygen is needed to check the release signature (install openssh-client) and nothing was changed"
  case "$(uname -m)" in
    x86_64|amd64) ARCH=x86_64 ;;
    aarch64|arm64) ARCH=aarch64 ;;
    *) die "unsupported processor $(uname -m); the host install supports x86_64 and aarch64" ;;
  esac
  # A user service needs the user's own systemd. Over ssh it is there; after `su` or `sudo -u` it may need these two.
  export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
  command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1 \
    || die "no systemd user manager runs for $ME in this session (log in again, for example over ssh), or this machine has no systemd. Use the isolated container install instead, or host-user, which uses a system service."

  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT

  SERVICE=termenbox.service
  # Already installed here: refresh the claim only. TERMENBOX_FORCE=1 reinstalls.
  if [ -x "$BIN" ] && [ -f "$UNIT_DIR/$SERVICE" ] && [ -z "${TERMENBOX_FORCE:-}" ]; then
    say "Termenbox is already installed for $ME."
    systemctl --user is-active --quiet "$SERVICE" || systemctl --user start "$SERVICE"
    JBIN="$BIN"
    install_user_helper_units
    native_claim
    return
  fi

  # What will need root, said before anything is downloaded or changed (decision 0118, 0113). The firewall is
  # detected without root (`systemctl is-active`, because reading its rules needs root). Lingering we may switch on
  # ourselves; it is announced here and switched on below (after the dry-run guard), once a verified binary can
  # journal it first.
  FIREWALL=""
  if command -v ufw >/dev/null 2>&1 && systemctl is-active --quiet ufw 2>/dev/null; then
    FIREWALL=ufw
  elif command -v firewall-cmd >/dev/null 2>&1 && systemctl is-active --quiet firewalld 2>/dev/null; then
    FIREWALL=firewalld
  fi
  # No answer (no login session for this user, e.g. after su) is not "off": we then do not claim a state we cannot see.
  LINGER_WAS="$(loginctl show-user "$ME" -p Linger --value 2>/dev/null)" || LINGER_WAS=unknown
  LINGER=off
  say ""
  say "What needs root (this installer never runs sudo):"
  case "$FIREWALL" in
    ufw) say "  - the firewall ufw is active. Open the WebRTC range once, as root:  ufw allow 4443:4542/udp" ;;
    firewalld) say "  - the firewall firewalld is active. Open the WebRTC range once, as root:  firewall-cmd --permanent --add-port=4443-4542/udp && firewall-cmd --reload" ;;
    *) say "  - no active ufw or firewalld was detected. With another firewall, open 4443-4542/udp yourself" ;;
  esac
  if [ -n "${TERMENBOX_SKIP_LINGER:-}" ]; then
    LINGER=skipped
    say "  - starting at boot is skipped (TERMENBOX_SKIP_LINGER); later, as root:  loginctl enable-linger $ME"
  elif [ "$LINGER_WAS" = yes ]; then
    LINGER=on
    say "  - sessions start at boot: lingering is on"
  elif [ "$LINGER_WAS" = unknown ]; then
    LINGER=unknown
    say "  - sessions start at boot: lingering could not be read; it will be tried, but not undone on uninstall. If it fails, as root:  loginctl enable-linger $ME"
  else
    say "  - sessions start at boot: switching lingering on for $ME (reversible); if that is not allowed, as root:  loginctl enable-linger $ME"
  fi

  fetch_release
  need_system_tmux user
  HTTP_PORT="$(pick_port tcp_in_use 8080 "$JOURNAL/http_port")"
  [ -n "$HTTP_PORT" ] || die "could not find a free port"
  say ""
  say "Termenbox $VERSION will be installed for $ME, with no root (sessions run as $ME):"
  say "  - the server  $BIN"
  [ -z "$TMUX_SHA" ] || say "  - tmux        $(dirname "$BIN")/termenbox-tmux (a pinned static build, used before any tmux on PATH)"
  say "  - state       $STATE"
  say "  - a user service $SERVICE, and a daily update check (signed releases only), in $UNIT_DIR"
  say "  - UDP port    the server picks a free one from 4443-4542 and remembers it"
  say "Every change is recorded in $JOURNAL; undo them with: termenbox-server --journal-dir $JOURNAL --revert --dry-run, then --revert"
  say ""
  [ -z "${TERMENBOX_DRY_RUN:-}" ] || { say "(dry run: nothing was changed)"; return; }

  mkdir -p "$JOURNAL" "$UNIT_DIR"
  chmod 700 "$JOURNAL"
  echo "$HTTP_PORT" > "$JOURNAL/http_port"

  mkdir -p "$(dirname "$BIN")"
  if [ -e "$BIN" ]; then
    mkdir -p "$JOURNAL/backups"
    BACKUP="$JOURNAL/backups/termenbox-server.$(date +%s)"
    cp -p "$BIN" "$BACKUP"
    journal file_replaced "path=$BIN" "backup=$BACKUP" "sha256=$SHA"
  else
    journal file_created "path=$BIN" "sha256=$SHA"
  fi
  install -m 0755 "$TMP/termenbox-server" "$BIN"
  install_tmux

  if [ ! -d "$STATE" ]; then
    journal dir_created "path=$STATE"
    mkdir -p "$STATE"
    chmod 700 "$STATE"
    # Coming from the system install of an earlier version (removed with --keep-state): the same keys and owner make it
    # the same server in the app. Only a copy, and only when nothing runs from that directory any more.
    OLD_STATE=/var/lib/termenbox
    if [ ! -f /etc/systemd/system/termenbox.service ] && [ -O "$OLD_STATE" ] && [ -r "$OLD_STATE/server_key" ]; then
      say "Reusing the server's keys and owner from $OLD_STATE (a copy; the old directory is left for you to delete)."
      for f in "$OLD_STATE"/*; do
        [ "$(basename "$f")" = requests ] || cp -a "$f" "$STATE/"
      done
    fi
  fi

  journal unit_installed "name=$SERVICE" "path=$UNIT_DIR/$SERVICE"
  cat > "$UNIT_DIR/$SERVICE" <<UNIT
[Unit]
Description=Termenbox terminal server
After=network-online.target

[Service]
WorkingDirectory=$HOME
Environment=TERMENBOX_RUN=native
Environment=TERMENBOX_RUN_USER=$ME
Environment=TERMENBOX_CONFIG_DIR=$STATE
Environment=TERMENBOX_JOURNAL_DIR=$JOURNAL
Environment=TERMENBOX_REQUESTS_DIR=$STATE/requests
Environment=TERMENBOX_HOST=127.0.0.1
Environment=TERMENBOX_PORT=$HTTP_PORT
Environment=TERMENBOX_CLIENT_URL=$ORIGIN
ExecStart=$BIN
Restart=on-failure
RestartSec=2
# The tmux sessions the server starts must outlive a restart or an update of the server.
KillMode=process

[Install]
WantedBy=default.target
UNIT
  systemctl --user daemon-reload
  systemctl --user enable --now "$SERVICE" >/dev/null 2>&1 || die "could not start the service; see: journalctl --user -u termenbox"

  i=0
  until curl -fs "http://127.0.0.1:$HTTP_PORT/health" >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || die "the server did not start; see: journalctl --user -u termenbox"
    sleep 1
  done

  # Starting at boot, not only at login, needs lingering (announced in the summary above). Last, so a failed install
  # leaves it alone. The user may switch it on for themselves; it is journaled before it is switched on, so uninstall
  # undoes it, and it is reversible. Not journaled when we could not read the earlier state: it may not be ours.
  if [ "$LINGER" = off ] || [ "$LINGER" = unknown ]; then
    [ "$LINGER" = off ] && journal linger_enabled "user=$ME"
    loginctl enable-linger "$ME" >/dev/null 2>&1 \
      || say "Lingering could not be switched on: the server starts at login only. As root:  loginctl enable-linger $ME"
  fi

  install_user_helper_units
  native_claim
}

# The updater timer and the request handler as user units (decisions 0076, 0077, 0113). They run the installed binary as
# you, never as root. Idempotent: only what is missing is added, each unit journaled first.
install_user_helper_units() {
  if [ ! -d "$STATE/requests" ]; then
    journal dir_created "path=$STATE/requests"
    mkdir -p "$STATE/requests"
    chmod 700 "$STATE/requests"
  fi
  _added=""
  add_user_unit() { # name, content
    [ -f "$UNIT_DIR/$1" ] && return 0
    journal unit_installed "name=$1" "path=$UNIT_DIR/$1"
    printf '%s\n' "$2" > "$UNIT_DIR/$1"
    _added=1
  }
  add_user_unit termenbox-update.service "[Unit]
Description=Termenbox updater

[Service]
Type=oneshot
Environment=TERMENBOX_BINARY=$BIN
ExecStart=$BIN --root-helper update --user-helper --journal-dir $JOURNAL"
  add_user_unit termenbox-update.timer "[Unit]
Description=Check for a new Termenbox release every day

[Timer]
OnCalendar=daily
RandomizedDelaySec=3600
Persistent=true

[Install]
WantedBy=timers.target"
  add_user_unit termenbox-request.service "[Unit]
Description=Termenbox helper (requests from the server)

[Service]
Type=oneshot
Environment=TERMENBOX_BINARY=$BIN
ExecStart=$BIN --root-helper requests --user-helper --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_user_unit termenbox-request.path "[Unit]
Description=Watch for requests from the Termenbox server

[Path]
PathExistsGlob=$STATE/requests/*.json

[Install]
WantedBy=default.target"
  if [ -n "$_added" ]; then
    systemctl --user daemon-reload
    systemctl --user enable --now termenbox-update.timer termenbox-request.path >/dev/null 2>&1 || die "could not start the updater; see: systemctl --user status termenbox-update.timer"
  fi
}

case "$MODE" in
  container) install_container ;;
  host-me) if [ -n "$LEGACY_SYSTEM" ]; then install_native; else install_user; fi ;;
  host|host-user) install_native ;;
esac
