#!/bin/sh
# Captures what the real installer prints, for the course's install video (DECISION-0161, course step 3.1).
#   scripts/capture-installer-output.sh
# Runs https://app.termenbox.dev/i (or $CAPTURE_ORIGIN/i) as an ordinary user in a throwaway systemd container, under
# `script` so the bytes and their timing are kept, then writes packages/client/e2e/video/fixtures/installer.typescript and
# installer.timing. The pairing link, its short code and every address are replaced by fixed examples: the server that made
# them is deleted when this script ends, and nothing real is kept. Needs Docker and network access; review the diff before commit.
set -eu
cd "$(dirname "$0")/.."

IMAGE="${CAPTURE_IMAGE:-jrei/systemd-debian:12}"
ORIGIN="${CAPTURE_ORIGIN:-https://app.termenbox.dev}"
OUT=packages/client/e2e/video/fixtures
NAME="termenbox-capture-$$"
mkdir -p "$OUT"
NET="$NAME-net"
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true; docker network rm "$NET" >/dev/null 2>&1 || true' EXIT

# The machine lives in the documentation range 192.0.2.0/24 (RFC 5737) and the pairing step runs LAN-only, so the link and its QR
# carry only that example address: no real address of this host or its network gets into the fixture.
docker network create --subnet 192.0.2.0/24 "$NET" >/dev/null
docker run -d --name "$NAME" --hostname workstation --network "$NET" --ip 192.0.2.10 --privileged --cgroupns=host -v /sys/fs/cgroup:/sys/fs/cgroup:rw "$IMAGE" >/dev/null
sleep 8
docker exec "$NAME" sh -c 'apt-get update -qq && apt-get install -y -qq curl openssh-client ca-certificates iproute2 util-linux >/dev/null'
docker exec "$NAME" sh -c '
  set -eu
  useradd --create-home --shell /bin/bash alice
  uid="$(id -u alice)"
  systemctl start "user@$uid.service"
  i=0; while [ ! -S "/run/user/$uid/bus" ] && [ "$i" -lt 20 ]; do i=$((i + 1)); sleep 1; done
  [ -S "/run/user/$uid/bus" ]
  install -d -o alice /home/alice/cap'
uid="$(docker exec "$NAME" id -u alice)"

# The user types the command in the video; what is captured is only the output. Width 72 keeps the QR readable on a phone-sized replay.
docker exec -u alice -e HOME=/home/alice -e XDG_RUNTIME_DIR="/run/user/$uid" -e DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" \
  -e TERM=xterm-256color -e TERMENBOX_LAN_ONLY=1 "$NAME" \
  script -q -O /home/alice/cap/out --log-timing /home/alice/cap/timing \
    -c "stty cols 72 rows 30; curl -fsSL $ORIGIN/i | sh" >/dev/null

docker cp "$NAME:/home/alice/cap/out" "$OUT/installer.raw"
docker cp "$NAME:/home/alice/cap/timing" "$OUT/installer.timing.raw"

# Replace what is specific to this throwaway server. The pairing link is the only thing with a secret in it.
${CAPTURE_PYTHON:-python3} scripts/scrub-installer-capture.py "$OUT/installer.raw" "$OUT/installer.timing.raw" "$OUT/installer.typescript" "$OUT/installer.timing"
rm -f "$OUT/installer.raw" "$OUT/installer.timing.raw"
echo "wrote $OUT/installer.typescript and $OUT/installer.timing"
