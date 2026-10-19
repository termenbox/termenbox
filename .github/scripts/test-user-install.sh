#!/bin/sh
# Runs the installer for the current user (host-me, decision 0113) inside a machine that runs systemd, as a normal user
# without root, and checks what it did (CI job "installer", mode host-me). This script itself runs as root only to set the
# machine up (a user, that user's systemd manager) and to inspect the result.
# Expects: /install.sh, /uninstall.sh, /release (+ /release2, /release3, /release4), /keys.txt, curl, ssh-keygen, iproute2.
set -eu

ORIGIN=https://app.termenbox.dev
SECRET="$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 43)"
fail() { echo "FAIL: $*"; exit 1; }

echo "== the installer for the current user refuses to run as root"
if TERMENBOX_RELEASE_URL=file:///release TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)" sh /install.sh "$SECRET" "$ORIGIN" host-me >/tmp/root.txt 2>&1; then
  fail "host-me installed as root"
fi
grep -q "needs no root" /tmp/root.txt || { cat /tmp/root.txt; fail "the refusal does not say why"; }

useradd --create-home --shell /bin/bash alice
A_UID="$(id -u alice)"
# What a login over ssh gives alice: her own systemd manager. Lingering is left off, so the installer's own step runs.
systemctl start "user@$A_UID.service"
i=0; while [ ! -S "/run/user/$A_UID/bus" ] && [ $i -lt 20 ]; do i=$((i + 1)); sleep 1; done
[ -S "/run/user/$A_UID/bus" ] || fail "alice has no user manager in this machine"

as_alice() {
  runuser -u alice -- env HOME=/home/alice XDG_RUNTIME_DIR="/run/user/$A_UID" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$A_UID/bus" \
    TERMENBOX_RELEASE_URL="${REL:-file:///release}" TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)" "$@"
}
BIN=/home/alice/.local/bin/termenbox-server
STATE=/home/alice/.config/termenbox
JOURNAL=/home/alice/.local/state/termenbox
UNITS=/home/alice/.config/systemd/user
usr() { as_alice systemctl --user "$@"; }
snapshot() { (cd /home/alice && find . -path ./.cache -prune -o -print | sort | md5sum); ls -A /etc/systemd/system /usr/local/bin | sort | md5sum; }

echo "== this machine has no tmux of its own (the install must bring one, decision 0119)"
if command -v tmux >/dev/null 2>&1; then fail "this test needs a machine with no tmux"; fi
BUNDLED=/home/alice/.local/bin/termenbox-tmux

echo "== a dry run changes nothing"
before="$(snapshot)"
as_alice env TERMENBOX_DRY_RUN=1 sh /install.sh "$SECRET" "$ORIGIN" host-me
[ "$(snapshot)" = "$before" ] || fail "the dry run changed the machine"

echo "== install, as alice, with no root"
before_system="$(ls -A /etc/systemd/system /usr/local/bin /var/lib | sort | md5sum)"
as_alice sh /install.sh "$SECRET" "$ORIGIN" host-me | tee /tmp/install.txt
usr is-active --quiet termenbox || fail "the user service is not active"
port="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' "$UNITS/termenbox.service")"
curl -fs "http://127.0.0.1:$port/health" >/dev/null || fail "the server does not answer on its health port"
pid="$(usr show -p MainPID --value termenbox)"
[ "$pid" -gt 0 ] || fail "the service has no main process"
[ "$(stat -c %U "/proc/$pid")" = alice ] || fail "the server does not run as alice"
! grep -q '^User=' "$UNITS/termenbox.service" || fail "the unit names a user; a user unit must not"
grep -q '^KillMode=process' "$UNITS/termenbox.service" || fail "sessions would die with the service"
grep -q '^WantedBy=default.target' "$UNITS/termenbox.service" || fail "the unit is not wanted by the user's default target"
[ -f "$STATE/server_key" ] || fail "the server did not create its state"
[ "$(ls -A /etc/systemd/system /usr/local/bin /var/lib | sort | md5sum)" = "$before_system" ] || fail "something outside alice's home was changed"
as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | grep -q unit_installed || fail "the journal has no unit entry"
as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | grep -q file_created || fail "the journal has no binary entry"
echo "== the bundled tmux is in place, verified, owned by alice, journaled, and the server runs on it"
[ -x "$BUNDLED" ] || fail "the bundled tmux is not in alice's ~/.local/bin"
[ "$(stat -c %U "$BUNDLED")" = alice ] || fail "the bundled tmux does not belong to alice"
[ "$(sha256sum "$BUNDLED" | cut -d' ' -f1)" = "$(sha256sum /release/tmux-x86_64-linux | cut -d' ' -f1)" ] || fail "the bundled tmux is not the signed one"
for n in tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING; do
  [ -f "/home/alice/.local/bin/termenbox-$n" ] || fail "the notice $n is not beside the tmux"
done
as_alice "$BUNDLED" -V | grep -q '^tmux ' || fail "the bundled tmux does not run"
as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | grep -q termenbox-tmux || fail "the journal has no tmux entry"
! command -v tmux >/dev/null 2>&1 || fail "a tmux was installed on the machine; the bundled one is all there should be"
# The server refuses to start without a tmux (startup self-check), and the machine has none on PATH: it is up, so it found the bundled one.
usr is-active --quiet termenbox || fail "the server is not running without a tmux on PATH"
if ! grep -q "stub tmux" "$BUNDLED"; then
  # A real tmux: a session that runs a command, and control mode, which is what the server speaks.
  tm() { as_alice env TERM=xterm-256color TMUX_TMPDIR=/tmp/alice-tmux "$BUNDLED" -f /dev/null "$@"; }
  as_alice mkdir -p /tmp/alice-tmux
  tm new-session -d -s probe -x 80 -y 24 'echo session-ok; exec sleep 30'
  sleep 1
  tm capture-pane -p -t probe | grep -q session-ok || fail "the bundled tmux did not run a session"
  printf 'list-sessions\n' | tm -C attach -t probe 2>&1 | grep -q '^%begin' || fail "control mode did not answer"
  tm kill-server
fi
# Starting at boot needs lingering: either alice could switch it on for herself, or the installer said what root would have to run.
if [ "$(loginctl show-user alice -p Linger --value)" = yes ]; then
  as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | grep -q linger_enabled || fail "lingering was enabled but is not in the journal"
else
  grep -q "loginctl enable-linger alice" /tmp/install.txt || fail "no lingering and no instruction how to get it"
fi
grep -q "What needs root" /tmp/install.txt || fail "the installer did not say first what needs root"
! grep -q '^Environment=TERMENBOX_UDP_PORT' "$UNITS/termenbox.service" || fail "the unit pins a UDP port; the server picks its own"
alice_port="$(cat "$STATE/udp_port")"
{ [ "$alice_port" -ge 4443 ] && [ "$alice_port" -le 4542 ]; } || fail "the server's UDP port $alice_port is outside 4443-4542"

echo "== installing again changes nothing"
entries="$(as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | wc -l)"
# An install from before the server picked its own UDP port left this file behind; it must do no harm.
as_alice sh -c "echo 4443 > '$JOURNAL/udp_port'"
as_alice sh /install.sh "$SECRET" "$ORIGIN" host-me
[ "$(as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | wc -l)" = "$entries" ] || fail "a second install wrote more journal entries"
usr is-active --quiet termenbox || fail "the service stopped"

echo "== the updater and the request handler are user units and running"
usr is-active --quiet termenbox-update.timer || fail "the update timer is not active"
usr is-active --quiet termenbox-request.path || fail "the request path unit is not active"
[ -d "$STATE/requests" ] || fail "the requests directory is missing"
[ "$(stat -c %U "$STATE/requests")" = alice ] || fail "the requests directory does not belong to alice"
as_alice "$BIN" --journal-dir "$JOURNAL" --journal-list | grep -q termenbox-update.timer || fail "the updater units are not in the journal"
sha() { sha256sum "$1" | cut -d' ' -f1; }
helper() { REL="file://$1" as_alice env TERMENBOX_BINARY="$BIN" "$BIN" --root-helper update --user-helper --journal-dir "$JOURNAL"; }

echo "== the user helper refuses to run as root, and a root helper refuses a plain user"
if TERMENBOX_RELEASE_URL=file:///release4 TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)" "$BIN" --root-helper update --user-helper --journal-dir "$JOURNAL" >/tmp/rootuser.txt 2>&1; then fail "the user helper ran as root"; fi
grep -q "must not run as root" /tmp/rootuser.txt || { cat /tmp/rootuser.txt; fail "the refusal does not say why"; }
if as_alice "$BIN" --root-helper update --journal-dir "$JOURNAL" >/tmp/plain.txt 2>&1; then fail "the root helper ran as a plain user"; fi

echo "== a frozen release is not installed"
before_sha="$(sha "$BIN")"
helper /release4 | grep -q "paused" || fail "a frozen release was not reported"
[ "$(sha "$BIN")" = "$before_sha" ] || fail "a frozen release changed the binary"

echo "== a newer release is installed by alice's own updater, the old binary is kept, the service comes back"
helper /release2 | grep -q "updated .* -> 9.0.1" || fail "the update did not report success"
[ "$(sha "$BIN")" = "$(sha /release2/termenbox-server-x86_64-linux)" ] || fail "the new binary is not in place"
[ -f "$BIN.prev" ] || fail "the previous binary was not kept"
[ "$(sha "$BUNDLED")" = "$(sha /release2/tmux-x86_64-linux)" ] || fail "the update did not carry the new tmux"
usr is-active --quiet termenbox || fail "the service is not active after the update"

echo "== a release that does not come up is rolled back"
good_sha="$(sha "$BIN")"
good_tmux="$(sha "$BUNDLED")"
# release3 carries the first tmux again, so a rollback that ignored tmux would leave it swapped.
set +e
helper /release3 > /tmp/rollback.txt 2>&1
code=$?
set -e
[ "$code" = 2 ] || { cat /tmp/rollback.txt; fail "a failed update did not exit with 2 (got $code)"; }
grep -q "rolled back" /tmp/rollback.txt || fail "the rollback was not reported"
[ "$(sha "$BIN")" = "$good_sha" ] || fail "the old binary was not restored"
[ "$(sha "$BUNDLED")" = "$good_tmux" ] || fail "the tmux was not put back with the server"
usr is-active --quiet termenbox || fail "the service is not active after the rollback"

echo "== a request from the server reaches the user helper, and junk does not"
requests="$STATE/requests"
printf '{"action":"update-now"}' > "$requests/r1.json"
printf '{"action":"rm","path":"/"}' > "$requests/r2.json"
ln -s /etc/hostname "$requests/r3.json"
chown -h alice "$requests"/*.json
i=0
while [ -n "$(ls -A "$requests")" ] && [ $i -lt 30 ]; do i=$((i + 1)); sleep 1; done
[ -z "$(ls -A "$requests")" ] || fail "the helper did not consume the requests"
[ -f /etc/hostname ] || fail "a link in the requests directory damaged its target"

echo "== a second user and a system install on the same machine (decisions 0115, 0118)"
# bob is not a sudoer, and a sudo that records every call stands in front of the real one: bob's install must never reach it.
useradd --create-home --shell /bin/bash bob
! id -nG bob | tr ' ' '\n' | grep -qxE 'sudo|wheel|admin' || fail "bob is a sudoer; the test needs a user without root"
B_UID="$(id -u bob)"
systemctl start "user@$B_UID.service"
i=0; while [ ! -S "/run/user/$B_UID/bus" ] && [ $i -lt 20 ]; do i=$((i + 1)); sleep 1; done
[ -S "/run/user/$B_UID/bus" ] || fail "bob has no user manager in this machine"
mkdir -p /tmp/nosudo
printf '#!/bin/sh\necho "$@" >> /tmp/sudo-called\nexit 1\n' > /tmp/nosudo/sudo
chmod 755 /tmp/nosudo /tmp/nosudo/sudo
rm -f /tmp/sudo-called
as_bob() {
  runuser -u bob -- env HOME=/home/bob XDG_RUNTIME_DIR="/run/user/$B_UID" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$B_UID/bus" \
    PATH="/tmp/nosudo:$PATH" TERMENBOX_RELEASE_URL=file:///release TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)" "$@"
}
bob_sc() { as_bob systemctl --user "$@"; }
B_STATE=/home/bob/.config/termenbox
SECRET2="$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 43)"
# The system install first (host-user): one per machine, with its own user, state and unit.
TERMENBOX_RELEASE_URL=file:///release TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)" TERMENBOX_SKIP_FIREWALL=1 sh /install.sh "$SECRET" "$ORIGIN" host-user >/dev/null
systemctl is-active --quiet termenbox || fail "the system service is not active"
sys_port="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' /etc/systemd/system/termenbox.service)"
sys_udp="$(sed -n 's/^Environment=TERMENBOX_UDP_PORT=//p' /etc/systemd/system/termenbox.service)"
as_bob sh /install.sh "$SECRET2" "$ORIGIN" host-me | tee /tmp/bob.txt
[ ! -e /tmp/sudo-called ] || fail "bob's install called sudo: $(cat /tmp/sudo-called)"
grep -q "4443-4542" /tmp/bob.txt || { cat /tmp/bob.txt; fail "bob's install did not print the UDP range note"; }
! grep -q "Reusing the server" /tmp/bob.txt || fail "bob's fresh install took over someone else's state"
bob_sc is-active --quiet termenbox || fail "bob's own service is not active next to the others"
usr is-active --quiet termenbox || fail "bob's install stopped alice's service"
systemctl is-active --quiet termenbox || fail "bob's install stopped the system service"

echo "== the three servers have their own ports, identities and state"
bob_port="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' /home/bob/.config/systemd/user/termenbox.service)"
bob_udp="$(cat "$B_STATE/udp_port")"
alice_udp="$(cat "$STATE/udp_port")"
{ [ "$bob_udp" -ge 4443 ] && [ "$bob_udp" -le 4542 ]; } || fail "bob's UDP port $bob_udp is outside 4443-4542"
{ [ "$alice_udp" -ge 4443 ] && [ "$alice_udp" -le 4542 ]; } || fail "alice's UDP port $alice_udp is outside 4443-4542"
[ "$alice_udp" != "$bob_udp" ] || fail "alice and bob took the same UDP port $alice_udp"
[ -z "$sys_udp" ] || { [ "$sys_udp" != "$alice_udp" ] && [ "$sys_udp" != "$bob_udp" ]; } || fail "the system server shares a UDP port with a user server"
alice_http="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' "$UNITS/termenbox.service")"
{ [ "$alice_http" != "$bob_port" ] && [ "$alice_http" != "$sys_port" ] && [ "$bob_port" != "$sys_port" ]; } || fail "two servers took the same health port"
for p in "$sys_port" "$alice_http" "$bob_port"; do curl -fs "http://127.0.0.1:$p/health" >/dev/null || fail "no server answers on port $p"; done
bob_pid="$(bob_sc show -p MainPID --value termenbox)"
{ [ "$bob_pid" -gt 0 ] && [ "$(stat -c %U "/proc/$bob_pid")" = bob ]; } || fail "bob's server does not run as bob"
a_sum="$(sha256sum "$STATE/server_key" | cut -d' ' -f1)"
b_sum="$(sha256sum "$B_STATE/server_key" | cut -d' ' -f1)"
s_sum="$(sha256sum /var/lib/termenbox/server_key | cut -d' ' -f1)"
{ [ "$a_sum" != "$b_sum" ] && [ "$a_sum" != "$s_sum" ] && [ "$b_sum" != "$s_sum" ]; } || fail "two servers share an identity"
{ [ "$(stat -c %U "$B_STATE")" = bob ] && [ "$(stat -c %U "$STATE")" = alice ]; } || fail "a state directory is not owned by its user"
if as_bob cat "$STATE/server_key" >/dev/null 2>&1; then fail "bob can read alice's server key"; fi
if as_alice cat "$B_STATE/server_key" >/dev/null 2>&1; then fail "alice can read bob's server key"; fi

echo "== stopping one leaves the others running"
usr stop termenbox
bob_sc is-active --quiet termenbox || fail "stopping alice's server stopped bob's"
systemctl is-active --quiet termenbox || fail "stopping alice's server stopped the system service"
curl -fs "http://127.0.0.1:$bob_port/health" >/dev/null || fail "bob's server stopped answering"
usr start termenbox
i=0; until curl -fs "http://127.0.0.1:$alice_http/health" >/dev/null 2>&1 || [ $i -ge 20 ]; do i=$((i + 1)); sleep 1; done
curl -fs "http://127.0.0.1:$alice_http/health" >/dev/null || fail "alice's server did not come back"
bob_sc stop termenbox
usr is-active --quiet termenbox || fail "stopping bob's server stopped alice's"
systemctl is-active --quiet termenbox || fail "stopping bob's server stopped the system service"
bob_sc start termenbox
i=0; until curl -fs "http://127.0.0.1:$bob_port/health" >/dev/null 2>&1 || [ $i -ge 20 ]; do i=$((i + 1)); sleep 1; done
curl -fs "http://127.0.0.1:$bob_port/health" >/dev/null || fail "bob's server did not come back"

echo "== uninstall.sh as alice shows the plan first and changes nothing"
before="$(snapshot)"
as_alice sh /uninstall.sh | grep -q "Nothing was changed" || fail "the uninstall preview did not say it changed nothing"
[ "$(snapshot)" = "$before" ] || fail "the uninstall preview changed the machine"
usr is-active --quiet termenbox || fail "the preview stopped the service"

echo "== uninstall.sh --yes, as alice, removes it all"
as_alice sh /uninstall.sh --yes
[ ! -e "$UNITS/termenbox.service" ] || fail "the unit is still there"
[ ! -e "$BIN" ] || fail "the binary is still there"
[ ! -e "$BUNDLED" ] || fail "the bundled tmux is still there"
ls /home/alice/.local/bin/termenbox-tmux-* >/dev/null 2>&1 && fail "a tmux notice is still there"
[ ! -e "$STATE" ] || fail "the state directory is still there"
[ ! -e "$JOURNAL" ] || fail "the journal is still there"
[ -d /home/alice ] || fail "alice's home was touched"
[ "$(loginctl show-user alice -p Linger --value)" != yes ] || fail "lingering the installer enabled was left on"
bob_sc is-active --quiet termenbox || fail "uninstalling alice's server stopped bob's"
systemctl is-active --quiet termenbox || fail "uninstalling alice's server stopped the system service"
[ -f "$B_STATE/server_key" ] && [ -f /var/lib/termenbox/server_key ] || fail "uninstalling alice's server removed another server's state"
curl -fs "http://127.0.0.1:$bob_port/health" >/dev/null || fail "bob's server stopped answering after alice's uninstall"

# bob and the system install are still here from the section above.

echo "== a system service that runs as the same user keeps the old route, and says why before asking for sudo"
sed -i 's/^User=.*/User=bob/' /etc/systemd/system/termenbox.service
if as_bob sh /install.sh "$SECRET2" "$ORIGIN" host-me >/tmp/legacy.txt 2>&1; then fail "the old route ran without sudo"; fi
grep -q "already installed here as a system service running as bob" /tmp/legacy.txt || { cat /tmp/legacy.txt; fail "no explanation before the sudo step"; }
grep -q "uninstall.sh | sudo sh -s -- --yes --keep-state" /tmp/legacy.txt || fail "the way to the no-root install is not given"
sed -i 's/^User=.*/User=termenbox/' /etc/systemd/system/termenbox.service

echo "== moving to the no-root install keeps the same server"
as_bob sh /uninstall.sh --yes >/dev/null
sh /uninstall.sh --yes --keep-state >/dev/null
[ -f /var/lib/termenbox/server_key ] || fail "--keep-state did not keep the state"
chown -R bob:bob /var/lib/termenbox     # what an earlier host-me install left behind: state owned by the user
old_key="$(sha256sum /var/lib/termenbox/server_key | cut -d' ' -f1)"
as_bob sh /install.sh "$SECRET2" "$ORIGIN" host-me | tee /tmp/move.txt
grep -q "Reusing the server" /tmp/move.txt || fail "the old keys were not reused"
[ "$(sha256sum /home/bob/.config/termenbox/server_key | cut -d' ' -f1)" = "$old_key" ] || fail "the new install has other keys"
as_bob systemctl --user is-active --quiet termenbox || fail "bob's service is not active after the move"
as_bob sh /uninstall.sh --yes >/dev/null
echo "OK: host-me"
