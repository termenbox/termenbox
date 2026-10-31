#!/bin/sh
# The container install and its updater run for real (decisions 0155, 0075): a machine with systemd and a Docker
# daemon, a local registry, and three images (a working server, a newer working one, a newer one whose health probe
# fails), each named by a manifest signed with a throwaway key. CI job "container installer".
#   test-container-install.sh SERVER_BINARY WORK_DIR
# WORK_DIR must be the same path for this machine and for the Docker daemon, because the container's data directory
# is a bind mount the daemon resolves. Expects /install.sh, /uninstall.sh and the repository's .github/scripts at
# /scripts, with docker, curl, ssh-keygen, python3 and systemd available. Exits non-zero on the first wrong thing.
set -eu

BIN="$1"
WORK="$2"
REG=127.0.0.1:5127
REPO="$REG/termenbox-ci"
NAME=termenbox-ci
ROOT="$WORK/data"
JOURNAL=/var/lib/termenbox-root
REQ="$ROOT/home/.config/termenbox/requests"
STATE_DIR="$ROOT/home/.config/termenbox"

fail() { echo "FAIL: $*"; docker logs "$NAME" 2>&1 | tail -20; journalctl --no-pager -n 30 2>/dev/null | tail -30; exit 1; }
running_digest() { docker image inspect -f '{{json .RepoDigests}}' "$(docker inspect -f '{{.Image}}' "$NAME")" | tr ',' '\n' | sed -n "s|.*$REPO@\(sha256:[0-9a-f]*\).*|\1|p" | head -1; }
json() { python3 -c "import json,sys; d=json.load(open(sys.argv[1])); v=eval(sys.argv[2]); print(' '.join(map(str,v)) if isinstance(v,tuple) else v)" "$1" "$2"; }

mkdir -p "$WORK"
echo "== a registry and three images"
docker rm -f termenbox-ci-registry >/dev/null 2>&1 || true
docker run -d --name termenbox-ci-registry -p "127.0.0.1:5127:5000" registry:2 >/dev/null
i=0; until curl -fs "http://$REG/v2/" >/dev/null; do i=$((i + 1)); [ "$i" -lt 30 ] || fail "the registry did not start"; sleep 1; done

image() { # tag healthy(1|0): the server binary in a small image; the health probe is curl, which a bad image breaks
  _d="$WORK/image-$1"; mkdir -p "$_d"; cp "$BIN" "$_d/termenbox-server"
  [ "$2" = 1 ] || printf '#!/bin/sh\nexit 1\n' > "$_d/curl"
  [ "$2" = 1 ] || chmod 755 "$_d/curl"
  {
    echo "FROM debian:12-slim"
    echo "RUN apt-get update -qq && apt-get install -y -qq curl tmux git ca-certificates >/dev/null && rm -rf /var/lib/apt/lists/* && (id node >/dev/null 2>&1 || useradd -m -u 1000 node)"
    echo "COPY termenbox-server /usr/local/bin/termenbox-server"
    echo "LABEL ci.tag=$1"
    [ "$2" = 1 ] || echo "COPY curl /usr/local/bin/curl"
    echo "USER node"
    echo "WORKDIR /home/node"
    echo 'CMD ["termenbox-server"]'
  } > "$_d/Dockerfile"
  docker build -q -t "$REPO:$1" "$_d" >/dev/null
  docker push -q "$REPO:$1" >/dev/null
  docker image inspect -f '{{json .RepoDigests}}' "$REPO:$1" | tr ',' '\n' | sed -n "s|.*$REPO@\(sha256:[0-9a-f]*\).*|\1|p" | head -1
}
D1="$(image one 1)"; D2="$(image two 1)"; D3="$(image bad 0)"
[ -n "$D1" ] && [ -n "$D2" ] && [ -n "$D3" ] || fail "an image has no digest"
docker rmi -f "$REPO:one" "$REPO:two" "$REPO:bad" >/dev/null 2>&1 || true

echo "== signed releases that name the images (0.99.1 works, 0.99.2 is unhealthy)"
mkdir -p "$WORK/keys"
rm -f "$WORK/keys/key" "$WORK/keys/key.pub"
ssh-keygen -q -t ed25519 -N '' -C test -f "$WORK/keys/key"
KEYS="t1 $(cut -d' ' -f1,2 "$WORK/keys/key.pub")"
release() { # dir version digest
  mkdir -p "$1"; cp "$BIN" "$1/termenbox-server-x86_64-linux"; cp "$BIN" "$1/termenbox-server-aarch64-linux"
  printf '{"x86_64":"%s","aarch64":"%s"}' "$3" "$3" > "$1/image.json"
  python3 /scripts/make_manifest.py "$2" "$1" "$1/manifest.json" --no-tmux --base-url "file://$1" --image-digests "$1/image.json" >/dev/null
  rm -f "$1/manifest.json.sig"
  ssh-keygen -Y sign -n termenbox-release -f "$WORK/keys/key" "$1/manifest.json" >/dev/null 2>&1
}
release "$WORK/rel-good" 0.99.1 "$D2"
release "$WORK/rel-bad" 0.99.2 "$D3"
release "$WORK/rel-none" 0.99.3 "$D2"
python3 - "$WORK/rel-none/manifest.json" <<'PY'
import json, sys
p = sys.argv[1]; m = json.load(open(p)); m.pop("image"); json.dump(m, open(p, "w"))
PY
rm -f "$WORK/rel-none/manifest.json.sig"
ssh-keygen -Y sign -n termenbox-release -f "$WORK/keys/key" "$WORK/rel-none/manifest.json" >/dev/null 2>&1

echo "== install (the image is chosen by hand, so the timer would not pull it; apply still can)"
export TERMENBOX_RELEASE_KEYS="$KEYS" TERMENBOX_SKIP_FIREWALL=1
SECRET="$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 43)"
TERMENBOX_IMAGE="$REPO@$D1" TERMENBOX_CONTAINER="$NAME" TERMENBOX_DATA="$ROOT" TERMENBOX_RELEASE_URL="file://$WORK/rel-good" \
  TERMENBOX_CLAIM="$SECRET" TERMENBOX_CLIENT_URL=https://app.termenbox.dev sh /install.sh container
[ "$(docker inspect -f '{{.State.Running}}' "$NAME")" = true ] || fail "the container is not running"
[ "$(running_digest)" = "$D1" ] || fail "the container does not run the first image"
[ "$(stat -c '%U %a' "$JOURNAL/container.json")" = "root 600" ] || fail "container.json is not root-owned 0600"
[ "$(json "$JOURNAL/container.json" "d['v'], d['name'], d['image'], d['pinned']")" = "1 $NAME $REPO True" ] || fail "container.json says $(cat "$JOURNAL/container.json")"
[ -x /usr/local/bin/termenbox-server ] || fail "the host helper was not installed"
/usr/local/bin/termenbox-server --journal-dir "$JOURNAL" --journal-list | grep -q container.json || fail "the journal has no container.json entry"
for t in termenbox-update.timer termenbox-update-tick.timer termenbox-request.path; do
  systemctl is-enabled --quiet "$t" || fail "$t is not enabled"
done
[ "$(stat -c '%u %a' "$REQ")" = "1000 700" ] || fail "the requests directory is $(stat -c '%u %a' "$REQ")"
[ "$(stat -c '%u' "$ROOT/home/.config")" = 1000 ] || fail ".config on the volume is not the container user's"

helper() { TERMENBOX_RELEASE_URL="file://$1" /usr/local/bin/termenbox-server --root-helper "$2" --journal-dir "$JOURNAL" --requests-dir "$REQ"; }

echo "== a release without an image changes nothing"
helper "$WORK/rel-none" apply
[ "$(running_digest)" = "$D1" ] || fail "a release without an image changed the container"

echo "== check never recreates the container"
helper "$WORK/rel-good" check
[ "$(running_digest)" = "$D1" ] || fail "check changed the container"
[ "$(json "$STATE_DIR/update-state.json" "d['available']")" = 0.99.1 ] || fail "the check did not record the release"

echo "== apply: pull the signed digest, recreate, keep nothing of the old container"
helper "$WORK/rel-good" apply
[ "$(running_digest)" = "$D2" ] || fail "the container does not run the new image ($(running_digest))"
! docker inspect "$NAME-previous" >/dev/null 2>&1 || fail "the previous container was left behind"
[ "$(json "$JOURNAL/container.json" "d['digest']")" = "$D2" ] || fail "container.json does not record the new digest"
/usr/local/bin/termenbox-server --journal-dir "$JOURNAL" --journal-list | grep -q container_replaced || fail "the journal has no container_replaced entry"
[ "$(json "$STATE_DIR/update-state.json" "d['last']['result']")" = updated ] || fail "the state does not say updated"

echo "== a release whose image is unhealthy is rolled back and not retried"
set +e; helper "$WORK/rel-bad" apply; rc=$?; set -e
[ "$rc" = 2 ] || fail "an unhealthy image should exit 2, got $rc"
[ "$(running_digest)" = "$D2" ] || fail "the working container is not back"
[ "$(docker inspect -f '{{.State.Running}}' "$NAME")" = true ] || fail "the working container is not running"
! docker inspect "$NAME-previous" >/dev/null 2>&1 || fail "the previous container name was left behind"
[ "$(json "$STATE_DIR/update-state.json" "d['skipped']")" = 0.99.2 ] || fail "the failed release is not pinned"

echo "== the tick has nothing to do"
helper "$WORK/rel-good" tick

echo "== uninstall removes the container, the timers and what the installer added"
TERMENBOX_CONTAINER="$NAME" TERMENBOX_DATA="$ROOT" sh /uninstall.sh --yes
! docker inspect "$NAME" >/dev/null 2>&1 || fail "the container is still there"
for t in termenbox-update.timer termenbox-update-tick.timer; do
  ! systemctl is-enabled --quiet "$t" 2>/dev/null || fail "$t is still enabled"
done
[ ! -e /usr/local/bin/termenbox-server ] || fail "the host helper is still installed"
[ ! -e /etc/systemd/system/termenbox-update-tick.timer ] || fail "a unit file is still there"
docker rm -f termenbox-ci-registry >/dev/null 2>&1 || true
echo "container install: all checks passed"
