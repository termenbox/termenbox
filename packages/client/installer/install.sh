#!/bin/sh
# Termenbox installer. Served by the client at /i, with that client's address as the default below.
#
#   curl -fsSL https://app.termenbox.dev/i | sh                      this machine, as the user who runs it
#   curl -fsSL https://app.termenbox.dev/i | sh -s -- <mode>         another place for sessions (see below)
#
# One command for every topology: the host makes its own one-time claim and every run ends with the same pairing block,
# a QR, the link it encodes and a short code (decisions 0126, 0127). Nothing secret is in the command.
#
# The optional word picks where sessions run (DECISION-0058, 0075):
#   container   an isolated Docker container (installs Docker if missing)
#   host-user   directly on this machine as a dedicated unprivileged user `termenbox`, as a systemd service
#   (none)      host-me
#   host-me     directly on this machine as the user who runs this command: sessions open on their behalf, in their
#               home, with their repositories, keys and agent logins. No user is created. No root is needed: it is a
#               user systemd service (decision 0113); what would need root is stated up front, not run.
#   host        directly on this machine as root, as a systemd service
# The host modes give the owner's browser a shell on the machine itself and need no Docker.
#
# Options, before or after the word (decision 0131): --channel stable|beta|nightly picks the release stream (default
# stable; the container uses that channel's image tag), --auto-update lets the server install updates by itself. The
# default is automatic, except on nightly, which only notifies. The choice is written to update.toml beside the journal;
# run the installer again with the option, or edit that file, to change it.
#   curl -fsSL https://beta.app.termenbox.dev/i | sh
# Each channel's app serves its own copy: it defaults to that channel and trusts only that channel's release keys
# (stable: r1; beta: r1 and b1, as the beta alias can hold a stable release; nightly: n1; decision 0134).
#
# It starts the server and prints the pairing block. The UDP port is picked once, from the reserved range 4443-4542
# (DECISION-0118), and then stays: the server uses that port or stops with a message, it never moves to another. To
# choose the port yourself, set TERMENBOX_UDP_PORT=<port> in front of `sh` (needed when the range has no free port;
# an interactive run asks then). The installer says which port it is and how to open it. It asks nothing otherwise and
# needs nothing afterwards. Safe to run again: on an owned server it prints a link to add it to another browser.
#
# For tests and automation, TERMENBOX_CLAIM=<43-character secret> hands the server that claim secret, and
# TERMENBOX_CLIENT_URL names a different client (a self-hosted client serves this file with its own address as the default).
#
# Host modes: every change is recorded in /var/lib/termenbox-root before it is made and can be undone with
# `termenbox-server --revert`. The release is checked before anything is installed: its manifest must carry a valid
# signature by the release key below (checked with ssh-keygen), and the binary must match the signed SHA-256.
# The host modes also install a pinned, statically linked tmux next to the server (decision 0119), verified the same
# way, so no tmux has to be installed first and no package manager or root is needed for it. Docker keeps its own.
set -eu

SECRET="${TERMENBOX_CLAIM:--}"
ORIGIN="${TERMENBOX_CLIENT_URL:-https://app.termenbox.dev}" # client-origin: the client build bakes its own address here
NAME="${TERMENBOX_CONTAINER:-termenbox}"
ROOT="${TERMENBOX_DATA:-/opt/termenbox}"

# The public keys that sign releases: `kid ssh-ed25519 key [channel=...]` lines, the same as packages/server/release-keys.txt.
# A key without `channel=` is a stable key; a key signs only the channel it is bound to (decision 0130).
# TERMENBOX_RELEASE_KEYS and TERMENBOX_RELEASE_URL exist for the installer's own tests and for forks; on a real install they are unset.
RELEASE_KEYS="${TERMENBOX_RELEASE_KEYS:-r1 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDNH6+yZgMpZl+tW8C/D+9Ga0bN39vXzIIJMKmHB6auf}" # release-keys: the client build bakes its channel's keys here
# The channel this copy of the installer defaults to: the stable app serves stable, the beta app beta, the nightly app nightly.
DEFAULT_CHANNEL="stable" # default-channel: the client build bakes its channel here

say() { printf '%s\n' "$*"; }
die() { printf 'termenbox: %s\n' "$*" >&2; exit 1; }

# Options (decision 0131), before or after the mode word:
#   --channel stable|beta|nightly   which release stream this server follows (default stable); also TERMENBOX_CHANNEL
#   --auto-update                   install updates by themselves; the default, except on nightly, which only notifies
#                                   (an unattended daily restart on a server that wanted to try nightly is a surprise)
CHANNEL="${TERMENBOX_CHANNEL:-}"
AUTO_UPDATE="${TERMENBOX_AUTO_UPDATE:-}"
_mode=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --channel) [ "$#" -ge 2 ] || die "--channel needs stable, beta or nightly"; CHANNEL="$2"; shift 2 ;;
    --channel=*) CHANNEL="${1#--channel=}"; shift ;;
    --auto-update) AUTO_UPDATE=1; shift ;;
    -*) die "unknown option \"$1\" (use --channel stable|beta|nightly, --auto-update)" ;;
    *) [ -z "$_mode" ] || die "one mode only (got \"$_mode\" and \"$1\")"; _mode="$1"; shift ;;
  esac
done
set -- ${_mode:+"$_mode"}
MODE="${1:-host-me}"
# Given on purpose (flag or environment) means: write it, also over an existing install. Otherwise the install keeps its own.
CHANNEL_GIVEN="$CHANNEL$AUTO_UPDATE"
CHANNEL="${CHANNEL:-$DEFAULT_CHANNEL}"
case "$CHANNEL" in
  stable|beta|nightly) ;;
  *) die "unknown channel \"$CHANNEL\" (use stable, beta or nightly)" ;;
esac
if [ "$CHANNEL" = nightly ] && [ -z "$AUTO_UPDATE" ]; then UPDATE_MODE=notify; else UPDATE_MODE=auto; fi

# Where this channel's manifest lives. `latest` is the newest stable release; the others are rolling alias releases that
# hold only a manifest (decision 0132): every asset URL comes from the signed manifest, never from this address.
case "$CHANNEL" in
  stable) CHANNEL_URL=https://github.com/termenbox/termenbox/releases/latest/download; CHANNEL_TAG=latest ;;
  beta) CHANNEL_URL=https://github.com/termenbox/termenbox/releases/download/channel-beta; CHANNEL_TAG=beta ;;
  nightly) CHANNEL_URL=https://github.com/termenbox/termenbox/releases/download/channel-nightly; CHANNEL_TAG=nightly ;;
esac
RELEASE_URL="${TERMENBOX_RELEASE_URL:-$CHANNEL_URL}"
case "$RELEASE_URL" in
  *[!A-Za-z0-9._~:/@+=-]*) die "the release address has characters the installer does not handle" ;;
esac
IMAGE="${TERMENBOX_IMAGE:-ghcr.io/termenbox/termenbox:$CHANNEL_TAG}"

# The server saves its UDP port when it starts; until then --register can only print the bare claim link, so ask again
# for a few seconds until the output carries the signed bundle.
pairing_output() {
  _n=0
  while :; do
    _o="$("$@")" || _o=""
    case "$_o" in *"/#/pair/"*) break ;; esac
    _n=$((_n + 1))
    [ "$_n" -lt 15 ] || break
    sleep 1
  done
  printf '%s' "$_o"
}

# The block every run ends with (decision 0126): QR, link and code, or the re-add link of an owned server.
# $1 is what `termenbox-server --register --qr` printed; empty means the server is not ready to say.
show_pairing() {
  if [ -z "$1" ]; then
    [ "$SECRET" != "-" ] || die "the server did not print its pairing link; see its log"
    return 0
  fi
  say "$1"
}

case "$ORIGIN" in
  https://*|http://localhost*|http://127.0.0.1*) ;;
  *) die "the client address must start with https://" ;;
esac
ORIGIN="${ORIGIN%/}"
case "$MODE" in
  container|host|host-user|host-me) ;;
  *) die "unknown mode \"$MODE\" (use host-me, host-user, host, container, or leave it out)" ;;
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
      TERMENBOX_CHANNEL="${CHANNEL_GIVEN:+$CHANNEL}" TERMENBOX_AUTO_UPDATE="$AUTO_UPDATE" \
      TERMENBOX_SKIP_FIREWALL="${TERMENBOX_SKIP_FIREWALL:-}" TERMENBOX_DRY_RUN="${TERMENBOX_DRY_RUN:-}" \
      TERMENBOX_FORCE="${TERMENBOX_FORCE:-}" TERMENBOX_UDP_PORT="${TERMENBOX_UDP_PORT:-}" TERMENBOX_RELEASE_URL="${TERMENBOX_RELEASE_URL:-}" TERMENBOX_RELEASE_KEYS="${TERMENBOX_RELEASE_KEYS:-}" \
      TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL="$ORIGIN" \
      sh -c "$(curl -fsSL "$ORIGIN/i")" termenbox-install "$MODE"
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

UDP_FIRST=4443
UDP_LAST=4542

valid_port() { case "$1" in ""|*[!0-9]*) return 1 ;; esac; [ "$1" -ge 1024 ] && [ "$1" -le 65535 ]; }

# The first free UDP port of the reserved range, nothing when there is none.
free_udp_in_range() {
  _p=$UDP_FIRST
  while [ "$_p" -le "$UDP_LAST" ]; do
    udp_in_use "$_p" || { echo "$_p"; return; }
    _p=$((_p + 1))
  done
}

# No free port in the range: ask on the terminal when there is one, otherwise stop and say how to choose.
ask_udp_port() {
  {
    echo "termenbox: no UDP port is free in $UDP_FIRST-$UDP_LAST."
    echo "Free one, or choose a port yourself (it must then be opened in the firewall)."
  } >&2
  if ( : </dev/tty ) 2>/dev/null; then
    printf 'UDP port to use: ' >/dev/tty
    read -r _a </dev/tty || _a=""
    valid_port "$_a" || die "not a port number (1024-65535)"
    echo "$_a"
    return
  fi
  die "run this command again with TERMENBOX_UDP_PORT=<port> in front of sh"
}

# choose_udp_port <file holding the earlier choice>: the owner's TERMENBOX_UDP_PORT, else the earlier choice (kept as it
# is: the firewall rule and the server's record are for that port), else the first free port of the range.
choose_udp_port() {
  if [ -n "${TERMENBOX_UDP_PORT:-}" ]; then
    valid_port "$TERMENBOX_UDP_PORT" || die "TERMENBOX_UDP_PORT must be a port number from 1024 to 65535"
    echo "$TERMENBOX_UDP_PORT"
    return
  fi
  if [ -s "$1" ]; then
    _saved="$(cat "$1")"
    valid_port "$_saved" || die "$1 does not hold a port number. Delete it, or set TERMENBOX_UDP_PORT=<port> in front of sh"
    echo "$_saved"
    return
  fi
  _free="$(free_udp_in_range)"
  if [ -n "$_free" ]; then echo "$_free"; return; fi
  ask_udp_port
}

# Which cloud this is, from the metadata address that clouds answer on (link-local, no outside service). Best effort:
# a host without one prints nothing.
detect_provider() {
  _m=http://169.254.169.254
  if curl -fs -m 1 -H "Metadata-Flavor: Google" "$_m/computeMetadata/v1/instance/id" >/dev/null 2>&1; then echo gcp
  elif curl -fs -m 1 -H "Metadata: true" "$_m/metadata/instance?api-version=2021-02-01" >/dev/null 2>&1; then echo azure
  elif curl -fs -m 1 "$_m/metadata/v1/id" >/dev/null 2>&1; then echo digitalocean
  elif curl -fs -m 1 "$_m/hetzner/v1/metadata" >/dev/null 2>&1; then echo hetzner
  elif curl -fs -m 1 -H "Authorization: Bearer Oracle" "$_m/opc/v2/instance/" >/dev/null 2>&1; then echo oracle
  elif curl -fs -m 1 "$_m/v1.json" >/dev/null 2>&1; then echo vultr
  elif curl -fs -m 1 "$_m/latest/meta-data/instance-id" >/dev/null 2>&1 \
    || curl -fs -m 1 -X PUT -H "X-aws-ec2-metadata-token-ttl-seconds: 60" "$_m/latest/api/token" >/dev/null 2>&1; then echo aws
  fi
}

# What the owner must do outside the installer for UDP port <port> to be reachable. $1 = port, $2 = whether this run
# already opened the host firewall (root modes). Best effort: the real test is the app's reachability check.
port_report() {
  _port="$1"
  say ""
  say "Termenbox listens on UDP port $_port."
  if [ -z "${2:-}" ]; then
    _fw=""
    if command -v ufw >/dev/null 2>&1 && systemctl is-active --quiet ufw 2>/dev/null; then _fw=ufw
    elif command -v firewall-cmd >/dev/null 2>&1 && systemctl is-active --quiet firewalld 2>/dev/null; then _fw=firewalld; fi
    case "$_fw" in
      ufw) say "This machine's firewall (ufw) is active. If port $_port is not open yet, as root:  ufw allow $_port/udp" ;;
      firewalld) say "This machine's firewall (firewalld) is active. If port $_port is not open yet, as root:  firewall-cmd --permanent --add-port=$_port/udp && firewall-cmd --reload" ;;
      *) say "No active ufw or firewalld was detected here. With another firewall, allow inbound UDP $_port yourself." ;;
    esac
  fi
  case "$(detect_provider)" in
    aws) say "Your host looks like AWS EC2: also allow inbound UDP $_port in the instance's security group." ;;
    gcp) say "Your host looks like Google Cloud: also allow ingress udp:$_port with a VPC firewall rule (gcloud compute firewall-rules create termenbox-udp --allow=udp:$_port)." ;;
    azure) say "Your host looks like Azure: also add an inbound port rule for UDP $_port to the VM's network security group." ;;
    digitalocean) say "Your host looks like DigitalOcean: if a Cloud Firewall is attached to the droplet, add an inbound UDP $_port rule." ;;
    hetzner) say "Your host looks like Hetzner: if a Cloud Firewall is attached to the server, add an inbound UDP $_port rule." ;;
    oracle) say "Your host looks like Oracle Cloud: also add an ingress rule for UDP $_port to the subnet's security list or network security group." ;;
    vultr) say "Your host looks like Vultr: if a firewall group is attached, add an inbound UDP $_port rule." ;;
    *) say "If your hosting provider has a network firewall or security group, allow inbound UDP $_port there too." ;;
  esac
  # WSL2 sits behind Windows' own NAT: its UDP port is not reachable from other devices, and the address it advertises is
  # internal. Mirrored networking is the one setting that makes UDP work; netsh portproxy handles TCP only.
  if grep -qi microsoft /proc/version 2>/dev/null; then
    say ""
    say "This looks like WSL. Its network is behind Windows, so UDP $_port is not reachable from other devices as it is. The app falls back to a slower connection until you do this, once, on Windows:"
    say "  1. In %UserProfile%\\.wslconfig add:   [wsl2]  networkingMode=mirrored"
    say "  2. In PowerShell:   wsl --shutdown     (then start WSL again)"
    say "  3. In an admin PowerShell, allow it through Windows Firewall:   New-NetFirewallRule -DisplayName Termenbox -Direction Inbound -Protocol UDP -LocalPort $_port -Action Allow"
    say "  If WSL still blocks it:   Set-NetFirewallHyperVVMSetting -Name '{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}' -DefaultInboundAction Allow"
    say "  Or, without changing Windows: run   termenbox-server --setup-cloudflare   (a free Cloudflare account of yours; see \"Connecting a home or WSL server\" in the README) and the app punches through or uses your relay."
  fi
  say "Add Server tests the connection from your browser and shows what is blocking it."
}

# Said when the unit pins a port by hand (an older installer, or TERMENBOX_UDP_PORT): the owner's, kept as it is.
pinned_port_note() { # <unit file> <port>
  _pin="$(sed -n 's/^Environment=TERMENBOX_UDP_PORT=//p' "$1" 2>/dev/null)"
  valid_port "$_pin" || return 0
  if [ "$_pin" -ge "$UDP_FIRST" ] && [ "$_pin" -le "$UDP_LAST" ]; then
    say "(The port is pinned in $1.)"
  else
    say "Note: $1 pins port $_pin, outside $UDP_FIRST-$UDP_LAST, so the range rule does not cover it. Open $_pin, or remove that line and restart the service to move into the range."
  fi
}

# Writes the owner's port into a unit as Environment=TERMENBOX_UDP_PORT (replacing any earlier line) and restarts the
# service, so a re-run of the installer with TERMENBOX_UDP_PORT takes effect. apply_udp_pin <user|system> <unit file> <port>
apply_udp_pin() {
  _flag=""; [ "$1" = user ] && _flag="--user"
  valid_port "$3" || die "TERMENBOX_UDP_PORT must be a port number from 1024 to 65535"
  awk -v port="$3" '/^Environment=TERMENBOX_UDP_PORT=/ { next } { print } /^\[Service\]/ { print "Environment=TERMENBOX_UDP_PORT=" port }' "$2" > "$2.new" \
    && cat "$2.new" > "$2" && rm -f "$2.new" || die "could not write the port into $2"
  # shellcheck disable=SC2086
  systemctl $_flag daemon-reload && systemctl $_flag restart termenbox.service || die "could not restart the service; see: journalctl $_flag -u termenbox"
}

# The server's update policy (decision 0131), next to the journal: root-owned on a system install, the user's own on a
# user install, never writable by the server. Written on a fresh install, and again when --channel or --auto-update is
# given to an existing one (the way to change channel later; it is a host-level act, not a web action). An existing
# maintenance_window is kept. Needs $TMP, $JOURNAL and the journal helpers.
write_update_config() {
  _win="$(sed -n 's/^\(maintenance_window *=.*\)$/\1/p' "$JOURNAL/update.toml" 2>/dev/null | head -n 1)"
  _grace="$(sed -n 's/^\(grace *=.*\)$/\1/p' "$JOURNAL/update.toml" 2>/dev/null | head -n 1)"
  _ontimeout="$(sed -n 's/^\(on_timeout *=.*\)$/\1/p' "$JOURNAL/update.toml" 2>/dev/null | head -n 1)"
  {
    echo "# How this server updates (decision 0131). Edit it on this machine; nothing in the app changes it."
    echo "# channel: stable | beta | nightly      mode: auto (install at the next check) | notify (say so, install on request) | hold"
    echo "# maintenance_window = \"03:00-04:00\"    local time; limits the automatic install"
    echo "# grace = \"12h\"  on_timeout = \"install\"   only with mode = \"notify\": the owner has this long to answer a new release;"
    echo "#                                          then it installs (install) or is skipped (skip, the default). A major version never installs."
    echo "[update]"
    echo "channel = \"$CHANNEL\""
    echo "mode = \"$UPDATE_MODE\""
    [ -z "$_win" ] || echo "$_win"
    [ -z "$_grace" ] || echo "$_grace"
    [ -z "$_ontimeout" ] || echo "$_ontimeout"
  } > "$TMP/update.toml"
  put_file "$TMP/update.toml" "$JOURNAL/update.toml" 0600
}

# Points the updater units at this channel's manifest (the helper reads TERMENBOX_RELEASE_URL). For units that exist.
set_release_env() { # <unit file>...
  for _u in "$@"; do
    [ -f "$_u" ] || continue
    awk -v v="$RELEASE_URL" '/^Environment=TERMENBOX_RELEASE_URL=/ { next } { print } /^\[Service\]/ { print "Environment=TERMENBOX_RELEASE_URL=" v }' "$_u" > "$_u.new" \
      && cat "$_u.new" > "$_u" && rm -f "$_u.new" || die "could not write the release address into $_u"
  done
}

# Older timer units inherited the system requests directory even in a user install. Migrate existing units as well
# as generating correct new ones. Preserve every other argument and back up/journal the replacement before writing.
migrate_helper_requests() { # <unit file>...
  for _helper_unit in "$@"; do
    [ -f "$_helper_unit" ] || continue
    _helper_next="$TMP/requests-$(basename "$_helper_unit")"
    awk -v requests="$STATE/requests" '
      /^ExecStart=.* --root-helper (update|requests)( |$)/ && !/ --requests-dir(=|[ \t])/ { $0 = $0 " --requests-dir " requests }
      { print }
    ' "$_helper_unit" > "$_helper_next" || die "could not prepare helper unit migration"
    if ! cmp -s "$_helper_unit" "$_helper_next"; then
      put_file "$_helper_next" "$_helper_unit" 0644
      _added=1
    fi
  done
}

# One line about the policy, for the summary of what will be installed.
update_policy_note() {
  case "$UPDATE_MODE" in
    notify) say "  - updates     channel $CHANNEL, notify only: a new release is installed when you ask for it (--auto-update installs by itself)" ;;
    *) say "  - updates     channel $CHANNEL, installed automatically (change with --channel, or edit update.toml in the journal directory)" ;;
  esac
}

# The service must be up before a claim link is shown for it. On failure the server's own message is printed.
# service_up <user|system> <unit file>
service_up() {
  _flag=""; [ "$1" = user ] && _flag="--user"
  _hp="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' "$2" 2>/dev/null | head -n 1)"
  _i=0
  while [ "$_i" -lt 20 ]; do
    if [ -n "$_hp" ]; then curl -fs "http://127.0.0.1:$_hp/health" >/dev/null 2>&1 && return 0
    else
      # No port to ask: active must hold for a moment, because a server that dies on a taken port is active for an instant first.
      if systemctl $_flag is-active --quiet termenbox.service; then
        sleep 3
        systemctl $_flag is-active --quiet termenbox.service && return 0
      fi
    fi
    _i=$((_i + 1))
    sleep 1
  done
  say "The Termenbox service is not running:" >&2
  # shellcheck disable=SC2086
  journalctl $_flag -u termenbox.service -n 8 --no-pager 2>/dev/null | cut -c1-300 >&2 || true
  die "the server did not start. If the log above says a UDP port is taken, free it or choose another with TERMENBOX_UDP_PORT=<port> in front of sh. Full log: journalctl $_flag -u termenbox"
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
  PORT="$(choose_udp_port "$ROOT/udp_port")"
  [ -n "$PORT" ] || die "could not find a free UDP port"
  echo "$PORT" > "$ROOT/udp_port"
  open_firewall "$PORT"

  chown -R 1000:1000 "$ROOT/home"
  docker pull -q "$IMAGE" >/dev/null 2>&1 || docker image inspect "$IMAGE" >/dev/null 2>&1 || die "could not download $IMAGE"
  SERVER_NAME="$(hostname -s 2>/dev/null || hostname)"
  # The arguments are recorded in container.json for the updater to replay (decision 0155), so a character that would
  # need escaping in JSON is refused rather than escaped.
  case "$PORT$ORIGIN$SERVER_NAME$ROOT$NAME" in
    *[\"\\]*) die "the port, address, server name, data directory and container name must not contain quotes or backslashes" ;;
  esac
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  # container_updater_prepare records container.json (journaled) before the container exists; it prints nothing and
  # never stops the install: without it the container simply has no automatic updates.
  container_updater_prepare
  docker run -d --name "$NAME" --restart unless-stopped \
    -p "$PORT:$PORT/udp" \
    -e "TERMENBOX_UDP_PORT=$PORT" -e "TERMENBOX_CLIENT_URL=$ORIGIN" \
    -e "TERMENBOX_SERVER_NAME=$SERVER_NAME" \
    -e "TERMENBOX_REQUESTS_DIR=/home/node/.config/termenbox/requests" \
    -v "$ROOT/home:/home/node" \
    "$IMAGE" >/dev/null || die "could not start the server"

  # Wait for the server process, then hand over the claim.
  i=0
  until docker exec "$NAME" termenbox-server --help >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || die "the server did not start; see: docker logs $NAME"
    sleep 1
  done

  # A claim handed in by a test or a script runs first; whatever it is, the same pairing block is printed after it (decision 0126).
  if [ "$SECRET" != "-" ]; then
    docker exec "$NAME" termenbox-server --claim "$SECRET" >/dev/null
    say "Termenbox server started with the claim you gave it."
  fi
  OUT="$(pairing_output docker exec "$NAME" termenbox-server --register --qr)"
  show_pairing "$OUT"
  container_updater_units
  _opened=1; [ -z "${TERMENBOX_SKIP_FIREWALL:-}" ] || _opened=""
  port_report "$PORT" "$_opened"
}

# The container updater (decision 0155): the host gets the same root helper and timers as a machine install, with a
# Docker-aware apply step. The helper is the server binary of the image, copied out of it; the container never gets
# the Docker socket. Everything here is best effort and says so: a container without it still runs, it just does not
# update by itself.
CONTAINER_UPDATER=""
container_updater_prepare() {
  CONTAINER_UPDATER=""
  [ "$(id -u)" -eq 0 ] || return 0
  _tmp="$(mktemp -d)"
  _cid="$(docker create "$IMAGE" 2>/dev/null)" || _cid=""
  if [ -n "$_cid" ]; then
    docker cp "$_cid:/usr/local/bin/termenbox-server" "$_tmp/termenbox-server" >/dev/null 2>&1 || true
    docker rm -f "$_cid" >/dev/null 2>&1 || true
  fi
  if [ ! -x "$_tmp/termenbox-server" ] && [ -f "$_tmp/termenbox-server" ]; then chmod 755 "$_tmp/termenbox-server"; fi
  if [ ! -x "$_tmp/termenbox-server" ] || ! "$_tmp/termenbox-server" --help >/dev/null 2>&1; then
    rm -rf "$_tmp"
    say "Note: automatic updates for this container were not set up (the host could not run the server binary of $IMAGE). Update it by running this command again."
    return 0
  fi
  TMP="$_tmp"; JBIN="$TMP/termenbox-server"
  # An image chosen by hand is never pulled by the timer (a pinned image is a choice, like mode = hold).
  _pinned=false
  [ "$IMAGE" = "ghcr.io/termenbox/termenbox:$CHANNEL_TAG" ] || _pinned=true
  _repo="${IMAGE%%@*}"
  case "${_repo##*/}" in *:*) _repo="${_repo%:*}" ;; esac
  printf '{"v":1,"name":"%s","image":"%s","args":["--restart","unless-stopped","-p","%s:%s/udp","-e","TERMENBOX_UDP_PORT=%s","-e","TERMENBOX_CLIENT_URL=%s","-e","TERMENBOX_SERVER_NAME=%s","-e","TERMENBOX_REQUESTS_DIR=/home/node/.config/termenbox/requests","-v","%s/home:/home/node"],"pinned":%s}\n' \
    "$NAME" "$_repo" "$PORT" "$PORT" "$PORT" "$ORIGIN" "$SERVER_NAME" "$ROOT" "$_pinned" > "$TMP/container.json"
  mkdir -p "$JOURNAL"
  chmod 700 "$JOURNAL"
  put_file "$TMP/container.json" "$JOURNAL/container.json" 0600 || { rm -rf "$_tmp"; return 0; }
  if [ -e "$BIN" ] && [ "$(sha256sum "$BIN" | cut -d' ' -f1)" = "$(sha256sum "$JBIN" | cut -d' ' -f1)" ]; then :; else
    put_file "$JBIN" "$BIN" 0755
  fi
  JBIN="$BIN"
  CONTAINER_UPDATER=1
}

# The timers and the request path, once the container runs. The requests directory is on the container's volume so the
# server can leave a request there; the helper runs on the host as root.
container_updater_units() {
  [ -n "$CONTAINER_UPDATER" ] || return 0
  STATE="$ROOT/home/.config/termenbox"
  RUN_USER=1000
  # The server makes this directory as the container's user; if it has not yet, it must not end up owned by root.
  mkdir -p "$STATE" && chown -R 1000:1000 "$ROOT/home/.config"
  install_helper_units
  rm -rf "$TMP"
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
  if [ "$SECRET" != "-" ]; then
    # shellcheck disable=SC2086
    $_run "$BIN" --claim "$SECRET" >/dev/null
    say "Termenbox server started with the claim you gave it."
  fi
  # shellcheck disable=SC2086
  OUT="$(pairing_output $_run "$BIN" --register --qr)"
  show_pairing "$OUT"
  # CLAIM_UNIT and CLAIM_OPENED say which unit this is and whether this run opened the host firewall (root modes).
  if [ -r "$STATE/udp_port" ]; then
    port_report "$(cat "$STATE/udp_port")" "${CLAIM_OPENED:-}"
    pinned_port_note "$CLAIM_UNIT"
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

# An update timer from before decision 0154 fires once a day, between 00:00 and 01:00, so it never reaches a
# maintenance window elsewhere and a fresh install waits for the next midnight. Replace it (backed up and journaled by
# put_file) with the boot-and-6-hourly cadence. Idempotent: a timer that already has OnBootSec is left alone.
# migrate_update_timer <timer file> <new content>
UPDATE_TIMER="[Unit]
Description=Check for a new Termenbox release

[Timer]
OnBootSec=2min
OnUnitActiveSec=6h
RandomizedDelaySec=15min
Persistent=true

[Install]
WantedBy=timers.target"

# The local tick (decision 0154): every five minutes, no network, acts only when a grace deadline has passed.
TICK_TIMER="[Unit]
Description=Act on a passed Termenbox update deadline (no network unless one has passed)

[Timer]
OnBootSec=5min
OnUnitActiveSec=5min
AccuracySec=1min

[Install]
WantedBy=timers.target"

migrate_update_timer() {
  [ -f "$1" ] || return 0
  grep -q '^OnBootSec=' "$1" && return 0
  printf '%s\n' "$2" > "$TMP/timer-$(basename "$1")"
  put_file "$TMP/timer-$(basename "$1")" "$1" 0644
  _added=1
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
  # The policy file and the channel's manifest address: on a fresh install, an install from before they existed, or
  # when the owner gave --channel / --auto-update.
  if [ ! -f "$JOURNAL/update.toml" ] || [ -n "$CHANNEL_GIVEN" ]; then
    write_update_config
    set_release_env /etc/systemd/system/termenbox-update.service /etc/systemd/system/termenbox-request.service
    _added=1
  fi
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
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
ExecStart=$BIN --root-helper update --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_unit termenbox-update.timer "$UPDATE_TIMER"
  add_unit termenbox-update-tick.service "[Unit]
Description=Termenbox update deadline tick

[Service]
Type=oneshot
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
ExecStart=$BIN --root-helper tick --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_unit termenbox-update-tick.timer "$TICK_TIMER"
  add_unit termenbox-request.service "[Unit]
Description=Termenbox root helper (requests from the server)

[Service]
Type=oneshot
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
ExecStart=$BIN --root-helper requests --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_unit termenbox-request.path "[Unit]
Description=Watch for requests from the Termenbox server

[Path]
PathExistsGlob=$STATE/requests/*.json

[Install]
WantedBy=multi-user.target"
  migrate_helper_requests /etc/systemd/system/termenbox-update.service /etc/systemd/system/termenbox-request.service
  migrate_update_timer /etc/systemd/system/termenbox-update.timer "$UPDATE_TIMER"
  if [ -n "$_added" ]; then
    systemctl daemon-reload
    systemctl enable --now termenbox-update.timer termenbox-update-tick.timer termenbox-request.path >/dev/null 2>&1 || die "could not start the updater; see: systemctl status termenbox-update.timer"
    systemctl restart termenbox-update.timer >/dev/null 2>&1 || true
    systemctl start --no-block termenbox-update.service >/dev/null 2>&1 || true # the first check now, not at midnight
  fi
}

# The release: a signed manifest first, then a binary that matches the hash in it. Sets VERSION, SHA and JBIN.
fetch_release() {
  say "Checking the latest release..."
  curl -fsSL --retry 3 -o "$TMP/manifest.json" "$RELEASE_URL/manifest.json" || die "could not download the release manifest"
  curl -fsSL --retry 3 -o "$TMP/manifest.json.sig" "$RELEASE_URL/manifest.json.sig" || die "could not download the release signature"
  # The manifest names its channel (none: stable). It is read before the check only to choose which keys may sign it;
  # a changed channel breaks the signature. A key signs only its own channel, and this server follows only its own
  # channel: stable follows stable, beta follows beta and stable (the beta alias can hold a stable release), nightly
  # follows nightly (decision 0130).
  MCHAN="$(sed -n 's/.*"channel": *"\([a-z]*\)".*/\1/p' "$TMP/manifest.json" | head -1)"
  MCHAN="${MCHAN:-stable}"
  case "$CHANNEL:$MCHAN" in
    stable:stable|beta:beta|beta:stable|nightly:nightly) ;;
    *) die "the latest release is a $MCHAN release, which the $CHANNEL channel does not follow, so nothing was installed" ;;
  esac
  printf '%s\n' "$RELEASE_KEYS" | awk -v ch="$MCHAN" \
    'NF >= 3 { c = "stable"; if ($4 ~ /^channel=/) c = substr($4, 9); if (c == ch) print "termenbox-release-" ch " " $2 " " $3 }' > "$TMP/allowed_signers"
  ssh-keygen -Y verify -f "$TMP/allowed_signers" -I "termenbox-release-$MCHAN" -n termenbox-release \
    -s "$TMP/manifest.json.sig" < "$TMP/manifest.json" >/dev/null 2>&1 \
    || die "the release signature is not valid for the $MCHAN channel, so nothing was installed$([ -s "$TMP/allowed_signers" ] || echo " (this installer trusts no $MCHAN key: each channel's app serves its own installer)")"
  grep -q '"freeze": *true' "$TMP/manifest.json" && die "new installs are paused by the maintainers (a signed freeze); try again later"
  # The full SemVer, prerelease included (0.2.0-rc.1, 0.1.36-nightly.20261022), not just X.Y.Z.
  VERSION="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$TMP/manifest.json" | head -1)"
  printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$' \
    || die "the release has no valid version, so nothing was installed"
  SHA="$(manifest_attr binaries "$ARCH" sha256)"
  case "$SHA" in
    *[!0-9a-f]*|"") die "the release has no $ARCH binary" ;;
  esac
  [ "${#SHA}" -eq 64 ] || die "the release has no valid $ARCH hash"
  # The signed manifest, not the address it came from, names where each file is (decision 0132).
  fetch_asset "$(manifest_attr binaries "$ARCH" url)" "$TMP/termenbox-server" "the server"
  echo "$SHA  $TMP/termenbox-server" | sha256sum -c - >/dev/null 2>&1 || die "the downloaded server does not match the signed hash, so nothing was installed"
  chmod 755 "$TMP/termenbox-server"
  "$TMP/termenbox-server" --help >/dev/null 2>&1 || die "the downloaded server does not run on this machine"
  JBIN="$TMP/termenbox-server"
  fetch_tmux
}

# manifest_attr SECTION KEY ATTR  prints manifest.json[SECTION][KEY][ATTR], ATTR being sha256 or url (the manifest is
# written with one key per line).
manifest_attr() {
  awk -v sec="$1" -v key="$2" -v attr="$3" '
    function nm(l) { sub(/^[^"]*"/, "", l); sub(/".*/, "", l); return l }
    /[{][ \t]*$/ { depth++; if (depth == 2) s = nm($0); else if (depth == 3) k = nm($0); next }
    /^[ \t]*[}]/ { depth--; next }
    depth == 3 && s == sec && k == key && $0 ~ "\"" attr "\":" { v = $0; sub(".*\"" attr "\": *\"", "", v); sub(/".*/, "", v); print v; exit }' "$TMP/manifest.json"
}

# fetch_asset URL DEST WHAT  downloads a file the signed manifest names. Its address must be on this project's GitHub
# releases (or, in the installer's own tests with a file:// release address, under that address) and have no "..";
# the caller then checks the signed hash, so the download host is still not trusted.
fetch_asset() {
  case "$RELEASE_URL" in
    file://*) _prefix="${RELEASE_URL%/}/" ;;
    *) _prefix=https://github.com/termenbox/termenbox/releases/download/ ;;
  esac
  case "$1" in
    "") die "the release does not name where to download $3, so nothing was installed" ;;
    *..*) die "the release names a suspicious address for $3, so nothing was installed" ;;
    "$_prefix"*) ;;
    *) die "the release names an address for $3 that is not this project's releases, so nothing was installed" ;;
  esac
  curl -fsSL --retry 3 -o "$2" "$1" || die "could not download $3"
}

# The bundled tmux and its licence notices (decision 0119), checked against the signed manifest like the server.
# Sets TMUX_SHA (empty for a release from before tmux was bundled) and TMUX_NOTICES (the notice names downloaded).
TMUX_NOTICE_NAMES="tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING"
fetch_tmux() {
  TMUX_SHA=""
  TMUX_NOTICES=""
  _t="$(manifest_attr tmux "$ARCH" sha256)"
  [ -n "$_t" ] || return 0
  [ "${#_t}" -eq 64 ] && [ -z "$(printf '%s' "$_t" | tr -d '0-9a-f')" ] || die "the release has no valid $ARCH tmux hash"
  fetch_asset "$(manifest_attr tmux "$ARCH" url)" "$TMP/termenbox-tmux" "tmux"
  echo "$_t  $TMP/termenbox-tmux" | sha256sum -c - >/dev/null 2>&1 || die "the downloaded tmux does not match the signed hash, so nothing was installed"
  chmod 755 "$TMP/termenbox-tmux"
  "$TMP/termenbox-tmux" -V 2>/dev/null | grep -q '^tmux ' || die "the downloaded tmux does not run on this machine"
  for _n in $TMUX_NOTICE_NAMES; do
    _h="$(manifest_attr tmux_notices "$_n" sha256)"
    [ -n "$_h" ] || continue
    fetch_asset "$(manifest_attr tmux_notices "$_n" url)" "$TMP/$_n" "$_n"
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
    if [ -n "${TERMENBOX_UDP_PORT:-}" ]; then
      apply_udp_pin system "$UNIT" "$TERMENBOX_UDP_PORT"
      echo "$TERMENBOX_UDP_PORT" > "$JOURNAL/udp_port"
    fi
    service_up system "$UNIT"
    install_helper_units # installs from before the updater existed get it here
    CLAIM_UNIT="$UNIT"
    CLAIM_OPENED=""
    native_claim
    return
  fi

  fetch_release
  need_system_tmux root

  # 2. What will change. Everything below is recorded in the journal first.
  PORT="$(choose_udp_port "$JOURNAL/udp_port")"
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
  say "  - a service   $UNIT_NAME (starts at boot), and an update check at boot and every 6 hours (root-owned, signed releases only)"
  update_policy_note
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
    echo "# Ten starts in five minutes end in a failed state that systemctl and the journal show."
    echo "StartLimitIntervalSec=300"
    echo "StartLimitBurst=10"
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
    echo "# Always: a clean exit that nobody asked for is also a server that is gone. systemctl stop and disable still stop it."
    echo "Restart=always"
    echo "RestartSec=2"
    echo "# The tmux sessions the server starts must outlive a restart or an update of the server."
    echo "KillMode=process"
    echo ""
    echo "[Install]"
    echo "WantedBy=multi-user.target"
  } > "$UNIT"
  systemctl daemon-reload
  systemctl enable --now "$UNIT_NAME" >/dev/null 2>&1 || die "could not start the service; see: journalctl -u $UNIT_NAME"

  service_up system "$UNIT"

  install_helper_units
  CLAIM_UNIT="$UNIT"
  CLAIM_OPENED=""; [ -n "${TERMENBOX_SKIP_FIREWALL:-}" ] || CLAIM_OPENED=1
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
    [ -z "${TERMENBOX_UDP_PORT:-}" ] || apply_udp_pin user "$UNIT_DIR/$SERVICE" "$TERMENBOX_UDP_PORT"
    service_up user "$UNIT_DIR/$SERVICE"
    install_user_helper_units
    CLAIM_UNIT="$UNIT_DIR/$SERVICE"
    CLAIM_OPENED=""
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
  # The server picks its port from the range on its first start. The owner's own choice, or no free port in the range
  # (asked for here, before anything changes), is written into the unit instead.
  UDP_PIN=""
  if [ -n "${TERMENBOX_UDP_PORT:-}" ]; then
    UDP_PIN="$(choose_udp_port /nonexistent)"
  elif [ ! -s "$STATE/udp_port" ] && [ -z "$(free_udp_in_range)" ]; then
    UDP_PIN="$(ask_udp_port)"
  fi
  say ""
  say "Termenbox $VERSION will be installed for $ME, with no root (sessions run as $ME):"
  say "  - the server  $BIN"
  [ -z "$TMUX_SHA" ] || say "  - tmux        $(dirname "$BIN")/termenbox-tmux (a pinned static build, used before any tmux on PATH)"
  say "  - state       $STATE"
  say "  - a user service $SERVICE, and an update check at boot and every 6 hours (signed releases only), in $UNIT_DIR"
  update_policy_note
  if [ -n "$UDP_PIN" ]; then say "  - UDP port    $UDP_PIN (your choice)"; else say "  - UDP port    the server picks a free one from 4443-4542 on its first start and keeps it"; fi
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
# Ten starts in five minutes end in a failed state that systemctl and the journal show.
StartLimitIntervalSec=300
StartLimitBurst=10

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
${UDP_PIN:+Environment=TERMENBOX_UDP_PORT=$UDP_PIN}
ExecStart=$BIN
# Always: a clean exit that nobody asked for is also a server that is gone. systemctl stop and disable still stop it.
Restart=always
RestartSec=2
# The tmux sessions the server starts must outlive a restart or an update of the server.
KillMode=process

[Install]
WantedBy=default.target
UNIT
  systemctl --user daemon-reload
  systemctl --user enable --now "$SERVICE" >/dev/null 2>&1 || die "could not start the service; see: journalctl --user -u termenbox"

  service_up user "$UNIT_DIR/$SERVICE"

  # Starting at boot, not only at login, needs lingering (announced in the summary above). Last, so a failed install
  # leaves it alone. The user may switch it on for themselves; it is journaled before it is switched on, so uninstall
  # undoes it, and it is reversible. Not journaled when we could not read the earlier state: it may not be ours.
  if [ "$LINGER" = off ] || [ "$LINGER" = unknown ]; then
    [ "$LINGER" = off ] && journal linger_enabled "user=$ME"
    loginctl enable-linger "$ME" >/dev/null 2>&1 \
      || say "Lingering could not be switched on: the server starts at login only. As root:  loginctl enable-linger $ME"
  fi

  install_user_helper_units
  CLAIM_UNIT="$UNIT_DIR/$SERVICE"
  CLAIM_OPENED=""
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
  if [ ! -f "$JOURNAL/update.toml" ] || [ -n "$CHANNEL_GIVEN" ]; then
    write_update_config
    set_release_env "$UNIT_DIR/termenbox-update.service" "$UNIT_DIR/termenbox-request.service"
    _added=1
  fi
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
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
Environment=TERMENBOX_BINARY=$BIN
ExecStart=$BIN --root-helper update --user-helper --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_user_unit termenbox-update.timer "$UPDATE_TIMER"
  add_user_unit termenbox-update-tick.service "[Unit]
Description=Termenbox update deadline tick

[Service]
Type=oneshot
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
Environment=TERMENBOX_BINARY=$BIN
ExecStart=$BIN --root-helper tick --user-helper --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_user_unit termenbox-update-tick.timer "$TICK_TIMER"
  add_user_unit termenbox-request.service "[Unit]
Description=Termenbox helper (requests from the server)

[Service]
Type=oneshot
Environment=TERMENBOX_RELEASE_URL=$RELEASE_URL
Environment=TERMENBOX_BINARY=$BIN
ExecStart=$BIN --root-helper requests --user-helper --journal-dir $JOURNAL --requests-dir $STATE/requests"
  add_user_unit termenbox-request.path "[Unit]
Description=Watch for requests from the Termenbox server

[Path]
PathExistsGlob=$STATE/requests/*.json

[Install]
WantedBy=default.target"
  migrate_helper_requests "$UNIT_DIR/termenbox-update.service" "$UNIT_DIR/termenbox-request.service"
  migrate_update_timer "$UNIT_DIR/termenbox-update.timer" "$UPDATE_TIMER"
  if [ -n "$_added" ]; then
    systemctl --user daemon-reload
    systemctl --user enable --now termenbox-update.timer termenbox-update-tick.timer termenbox-request.path >/dev/null 2>&1 || die "could not start the updater; see: systemctl --user status termenbox-update.timer"
    systemctl --user restart termenbox-update.timer >/dev/null 2>&1 || true
    systemctl --user start --no-block termenbox-update.service >/dev/null 2>&1 || true # the first check now, not at midnight
  fi
}

case "$MODE" in
  container) install_container ;;
  host-me) if [ -n "$LEGACY_SYSTEM" ]; then install_native; else install_user; fi ;;
  host|host-user) install_native ;;
esac
