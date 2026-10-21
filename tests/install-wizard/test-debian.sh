#!/usr/bin/env bash
# Debian container test for install-wizard phase 2.
# Builds the image, starts a systemd container, then runs the wizard in
# coordinator, relay, and peer roles and verifies all reach a running state.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMAGE="lanlink-install-wizard:debian"
CONTAINER="lanlink-iw-debian"

cd "${REPO_DIR}"

log() { printf '[iw-test debian] %s\n' "$*"; }
die() { printf '[iw-test debian] ERROR: %s\n' "$*" >&2; exit 1; }

wait_active() {
  local unit="$1" max="${2:-30}"
  for _ in $(seq 1 "${max}"); do
    if docker exec "${CONTAINER}" systemctl is-active "${unit}" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  docker exec "${CONTAINER}" systemctl is-active "${unit}"
}

wait_port() {
  local host="$1" port="$2" max="${3:-30}"
  for _ in $(seq 1 "${max}"); do
    if docker exec "${CONTAINER}" bash -lc "timeout 1 bash -c 'exec 3<>/dev/tcp/${host}/${port}'" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  die "port ${host}:${port} did not become reachable"
}

docker build -f tests/install-wizard/Dockerfile.debian -t "${IMAGE}" .

# Remove any previous container.
docker rm -f "${CONTAINER}" 2>/dev/null || true

# Start systemd in the container, mounting the repo so the install script can
# find local release artifacts under dist/*/out/.
docker run -d --name "${CONTAINER}" --privileged --cgroupns=host \
  -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
  -v "${REPO_DIR}:/lanlink:ro" \
  "${IMAGE}" /lib/systemd/systemd

# Wait for systemd to become ready.
log "waiting for systemd"
for _ in $(seq 1 30); do
  if docker exec "${CONTAINER}" systemctl is-system-running 2>/dev/null | grep -qE 'running|degraded'; then
    break
  fi
  sleep 1
done

docker exec "${CONTAINER}" systemctl is-system-running || true

# Coordinator install.
log "installing coordinator"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=coordinator LANLINK_PKG=deb LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlink-coordinator.service
log "coordinator is active"

# Wait for the REST API to be reachable.
wait_port 127.0.0.1 8080

# Verify coordinator API is usable.
log "creating network via coordinator API"
docker exec "${CONTAINER}" bash -lc 'lanlinkctl coord create --name testnet --password testpass'
log "coordinator API accepted network creation"

# Relay install.
log "installing relay"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=relay LANLINK_PKG=deb LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlink-relayd.service
log "relay is active"

# Idempotent re-run of coordinator should complete without error.
log "re-running coordinator install for idempotency"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=coordinator LANLINK_PKG=deb LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlink-coordinator.service
log "coordinator idempotent re-run passed"

# Peer install.
log "installing peer"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=peer LANLINK_PKG=deb LANLINK_COORD_URL=http://127.0.0.1:8080 LANLINK_NETWORK=testnet LANLINK_PASSWORD=testpass LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlinkd@testnet.service 60
log "peer is active"

# Query peer status socket and assert an online snapshot.
log "checking peer online snapshot"
docker exec "${CONTAINER}" bash -lc '
  network_id="$(sed -n "s/^[[:space:]]*network_id[[:space:]]*=[[:space:]]*\([0-9][0-9]*\).*/\1/p" /etc/lanlink/testnet.toml | head -n1)"
  [ -n "${network_id}" ] || { echo "network_id not found in config"; exit 1; }
  socket="/run/lanlink/lanlinkd-${network_id}.sock"
  [ -S "${socket}" ] || { echo "status socket not present at ${socket}"; exit 1; }
  snapshot="$(printf '"'"'{"cmd":"snapshot"}'"'"' | socat - UNIX-CONNECT:"${socket}")"
  printf "%s\n" "${snapshot}" | grep -q "\"assigned_ip\"" || { echo "snapshot does not show assigned_ip: ${snapshot}"; exit 1; }
  echo "peer online snapshot verified"
'
log "peer online snapshot verified"

log "Debian matrix test passed"

log "testing uninstall"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=peer LANLINK_PKG=deb LANLINK_NETWORK=testnet LANLINK_YES=1 /lanlink/scripts/install.sh --uninstall' || true
docker exec "${CONTAINER}" bash -lc '[ ! -f /etc/lanlink/testnet.toml ] && [ ! -f /etc/lanlink/testnet.secret ]' || die "peer config not removed"
docker exec "${CONTAINER}" systemctl is-active lanlinkd@testnet.service 2>/dev/null && die "peer unit still active after uninstall" || true

docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=relay LANLINK_PKG=deb LANLINK_YES=1 /lanlink/scripts/install.sh --uninstall' || true
docker exec "${CONTAINER}" systemctl is-active lanlink-relayd.service 2>/dev/null && die "relay unit still active after uninstall" || true

docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=coordinator LANLINK_PKG=deb LANLINK_YES=1 /lanlink/scripts/install.sh --uninstall' || true
docker exec "${CONTAINER}" systemctl is-active lanlink-coordinator.service 2>/dev/null && die "coordinator unit still active after uninstall" || true

log "uninstall test passed"

# Cleanup.
docker rm -f "${CONTAINER}" 2>/dev/null || true
