#!/bin/sh
# Runs the native installer inside a machine that runs systemd and checks what it did (CI job "installer").
#   test-native-install.sh host-user|host   (host-me needs no root and has its own test: test-user-install.sh)
# Expects: /install.sh (the installer), /release (a signed test release), /keys.txt (the test release key),
# curl, ssh-keygen and iproute2 installed. Exits non-zero on the first thing that is wrong.
set -eu

MODE="$1"
ORIGIN=https://app.termenbox.dev
SECRET="$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 43)"
RUN_USER=termenbox
[ "$MODE" = host ] && RUN_USER=root
# /release (first install), /release2 (9.0.1 works), /release3 (9.0.2 starts but never serves), /release4 (9.0.3 is frozen)
export TERMENBOX_RELEASE_URL=file:///release
TERMENBOX_RELEASE_KEYS="$(cat /keys.txt)"
export TERMENBOX_RELEASE_KEYS
export TERMENBOX_SKIP_FIREWALL=1

fail() { echo "FAIL: $*"; exit 1; }
snapshot() { ls -A /etc/systemd/system /usr/local/bin /var/lib /home 2>/dev/null | sort | md5sum; }

before="$(snapshot)"
! command -v tmux >/dev/null 2>&1 || fail "this test needs a machine with no tmux (the install must bring one, decision 0119)"

echo "== a dry run changes nothing"
TERMENBOX_DRY_RUN=1 TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL="$ORIGIN" sh /install.sh "$MODE"
[ "$(snapshot)" = "$before" ] || fail "the dry run changed the machine"

echo "== install"
TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL="$ORIGIN" sh /install.sh "$MODE"
systemctl is-active --quiet termenbox || fail "the service is not active"
port="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' /etc/systemd/system/termenbox.service)"
curl -fs "http://127.0.0.1:$port/health" >/dev/null || fail "the server does not answer on its health port"
pid="$(systemctl show -p MainPID --value termenbox)"
[ "$pid" -gt 0 ] || fail "the service has no main process"
[ "$(stat -c %U "/proc/$pid")" = "$RUN_USER" ] || fail "the server does not run as $RUN_USER"
[ "$MODE" != host-user ] || id termenbox >/dev/null || fail "the user termenbox was not created"
BUNDLED=/usr/local/bin/termenbox-tmux
[ -x "$BUNDLED" ] || fail "the bundled tmux was not installed (decision 0119)"
! command -v tmux >/dev/null 2>&1 || fail "a tmux package was installed; the bundled one is all there should be"
[ "$(sha256sum "$BUNDLED" | cut -d' ' -f1)" = "$(sha256sum /release/tmux-x86_64-linux | cut -d' ' -f1)" ] || fail "the bundled tmux is not the signed one"
for n in tmux-COPYING tmux-libevent-LICENSE tmux-ncurses-COPYING; do [ -f "/usr/local/bin/termenbox-$n" ] || fail "the notice $n is not beside the tmux"; done
runuser -u "$RUN_USER" -- "$BUNDLED" -V | grep -q '^tmux ' || fail "$RUN_USER cannot run the bundled tmux"
termenbox-server --journal-list | grep -q termenbox-tmux || fail "the journal has no tmux entry"
if ! grep -q "stub tmux" "$BUNDLED"; then
  # A real tmux: a session as the user the sessions run as, in control mode too, which is what the server speaks.
  tm() { runuser -u "$RUN_USER" -- env TERM=xterm-256color TMUX_TMPDIR=/tmp "$BUNDLED" -f /dev/null "$@"; }
  tm new-session -d -s probe -x 80 -y 24 'echo session-ok; exec sleep 30'
  sleep 1
  tm capture-pane -p -t probe | grep -q session-ok || fail "the bundled tmux did not run a session"
  printf 'list-sessions\n' | tm -C attach -t probe 2>&1 | grep -q '^%begin' || fail "control mode did not answer"
  tm kill-server
fi
grep -q '^KillMode=process' /etc/systemd/system/termenbox.service || fail "sessions would die with the service"
[ -f /var/lib/termenbox/server_key ] || fail "the server did not create its state"
termenbox-server --journal-list | grep -q unit_installed || fail "the journal has no unit entry"
termenbox-server --journal-list | grep -q file_created || fail "the journal has no binary entry"

echo "== installing again changes nothing"
entries="$(termenbox-server --journal-list | wc -l)"
TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL="$ORIGIN" sh /install.sh "$MODE"
[ "$(termenbox-server --journal-list | wc -l)" = "$entries" ] || fail "a second install wrote more journal entries"
systemctl is-active --quiet termenbox || fail "the service stopped"

echo "== the updater and the request handler are installed and running"
systemctl is-active --quiet termenbox-update.timer || fail "the update timer is not active"
systemctl is-active --quiet termenbox-request.path || fail "the request path unit is not active"
[ -d /var/lib/termenbox/requests ] || fail "the requests directory is missing"
[ "$(stat -c %U /var/lib/termenbox/requests)" = "$RUN_USER" ] || fail "the requests directory does not belong to $RUN_USER"
termenbox-server --journal-list | grep -q termenbox-update.timer || fail "the updater units are not in the journal"
sha() { sha256sum "$1" | cut -d' ' -f1; }
helper() { TERMENBOX_RELEASE_URL="file://$1" termenbox-server --root-helper update; }

echo "== a frozen release is not installed"
before_sha="$(sha /usr/local/bin/termenbox-server)"
helper /release4 | grep -q "paused" || fail "a frozen release was not reported"
[ "$(sha /usr/local/bin/termenbox-server)" = "$before_sha" ] || fail "a frozen release changed the binary"

echo "== a newer release is installed, the old binary is kept, the service comes back"
helper /release2 | grep -q "updated .* -> 9.0.1" || fail "the update did not report success"
[ "$(sha /usr/local/bin/termenbox-server)" = "$(sha /release2/termenbox-server-x86_64-linux)" ] || fail "the new binary is not in place"
[ -f /usr/local/bin/termenbox-server.prev ] || fail "the previous binary was not kept"
[ "$(sha "$BUNDLED")" = "$(sha /release2/tmux-x86_64-linux)" ] || fail "the update did not carry the new tmux"
systemctl is-active --quiet termenbox || fail "the service is not active after the update"
# (applying the same release twice is covered by the unit tests: the test binaries all report the crate's own version)

echo "== a release that does not come up is rolled back"
good_sha="$(sha /usr/local/bin/termenbox-server)"
good_tmux="$(sha "$BUNDLED")"
set +e
helper /release3 > /tmp/rollback.txt 2>&1
code=$?
set -e
[ "$code" = 2 ] || { cat /tmp/rollback.txt; fail "a failed update did not exit with 2 (got $code)"; }
grep -q "rolled back" /tmp/rollback.txt || fail "the rollback was not reported"
[ "$(sha /usr/local/bin/termenbox-server)" = "$good_sha" ] || fail "the old binary was not restored"
[ "$(sha "$BUNDLED")" = "$good_tmux" ] || fail "the tmux was not put back with the server"
systemctl is-active --quiet termenbox || fail "the service is not active after the rollback"
port="$(sed -n 's/^Environment=TERMENBOX_PORT=//p' /etc/systemd/system/termenbox.service)"
for i in 1 2 3 4 5 6 7 8 9 10; do curl -fs "http://127.0.0.1:$port/health" >/dev/null && break; sleep 1; done
curl -fs "http://127.0.0.1:$port/health" >/dev/null || fail "the restored server does not answer"

echo "== a request from the server reaches the root helper, and junk does not"
requests=/var/lib/termenbox/requests
printf '{"action":"update-now"}' > $requests/r1.json
printf '{"action":"rm","path":"/"}' > $requests/r2.json
ln -s /etc/hostname $requests/r3.json
chown -h "$RUN_USER" $requests/*.json
i=0
while [ -n "$(ls -A $requests)" ] && [ $i -lt 30 ]; do i=$((i + 1)); sleep 1; done
[ -z "$(ls -A $requests)" ] || fail "the helper did not consume the requests"
[ -f /etc/hostname ] || fail "a link in the requests directory damaged its target"
# The helper takes the request files first and reports when it has finished, so the line can lag the files: wait for it.
i=0
until journalctl -u termenbox-request --no-pager | grep -q "up to date\|updated"; do
  i=$((i + 1))
  [ $i -lt 30 ] || { journalctl -u termenbox-request --no-pager | tail; fail "the helper did not act on the update request"; }
  sleep 1
done

echo "== a changed release is refused"
cp /release/manifest.json /tmp/manifest.good
echo " " >> /release/manifest.json 2>/dev/null || true
if TERMENBOX_FORCE=1 TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL="$ORIGIN" sh /install.sh "$MODE" >/tmp/refused.txt 2>&1; then
  cp /tmp/manifest.good /release/manifest.json 2>/dev/null || true
  fail "a release with a broken signature was installed"
fi
grep -q "signature is not valid" /tmp/refused.txt || { cat /tmp/refused.txt; fail "the refusal does not say why"; }
cp /tmp/manifest.good /release/manifest.json 2>/dev/null || true

echo "== uninstall.sh shows the plan first and changes nothing"
before_un="$(snapshot)"
sh /uninstall.sh | grep -q "Nothing was changed" || fail "the uninstall preview did not say it changed nothing"
[ "$(snapshot)" = "$before_un" ] || fail "the uninstall preview changed the machine"
systemctl is-active --quiet termenbox || fail "the preview stopped the service"

echo "== uninstall.sh --yes restores the machine"
sh /uninstall.sh --yes
[ ! -e /etc/systemd/system/termenbox.service ] || fail "the unit is still there"
[ ! -e /usr/local/bin/termenbox-server ] || fail "the binary is still there"
[ ! -e "$BUNDLED" ] || fail "the bundled tmux is still there"
ls /usr/local/bin/termenbox-tmux-* >/dev/null 2>&1 && fail "a tmux notice is still there"
[ ! -e /var/lib/termenbox ] || fail "the state directory is still there"
[ "$MODE" != host-user ] || ! id termenbox >/dev/null 2>&1 || fail "the user is still there"
[ "$MODE" != host-user ] || [ -d /home/termenbox ] || fail "the created user's home was deleted without --delete-home"
echo "OK: $MODE"
