#!/usr/bin/env bash
# Arch Linux container test for install-wizard phase 2 using the tarball fallback.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMAGE="lanlink-install-wizard:arch"
CONTAINER="lanlink-iw-arch"

cd "${REPO_DIR}"

log() { printf '[iw-test arch] %s\n' "$*"; }
die() { printf '[iw-test arch] ERROR: %s\n' "$*" >&2; exit 1; }

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
  local host="$1" port="$2" max="${3:-60}"
  for _ in $(seq 1 "${max}"); do
    if docker exec "${CONTAINER}" bash -lc "timeout 1 bash -c 'exec 3<>/dev/tcp/${host}/${port}'" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  die "port ${host}:${port} did not become reachable"
}

docker build -f tests/install-wizard/Dockerfile.arch -t "${IMAGE}" .

docker rm -f "${CONTAINER}" 2>/dev/null || true

docker run -d --name "${CONTAINER}" --privileged --cgroupns=host \
  -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
  -v "${REPO_DIR}:/lanlink:ro" \
  "${IMAGE}" /usr/lib/systemd/systemd

log "waiting for systemd"
for _ in $(seq 1 60); do
  if docker exec "${CONTAINER}" systemctl is-system-running 2>/dev/null | grep -qE 'running|degraded'; then
    break
  fi
  sleep 1
done

docker exec "${CONTAINER}" systemctl is-system-running || true

log "installing coordinator via tarball"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=coordinator LANLINK_PKG=tarball LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlink-coordinator.service
log "coordinator is active"

wait_port 127.0.0.1 8080

log "creating network via coordinator API"
docker exec "${CONTAINER}" bash -lc 'lanlinkctl coord create --name testnet --password testpass'
log "coordinator API accepted network creation"

log "installing relay via tarball"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=relay LANLINK_PKG=tarball LANLINK_YES=1 /lanlink/scripts/install.sh'
wait_active lanlink-relayd.service
log "relay is active"

log "installing peer via tarball"
docker exec "${CONTAINER}" bash -lc 'LANLINK_ROLE=peer LANLINK_PKG=tarball LANLINK_COORD_URL=http://127.0.0.1:8080 LANLINK_NETWORK=testnet LANLINK_PASSWORD=testpass LANLINK_YES=1 /lanlink/scripts/install.sh'
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

log "Arch matrix test passed"

docker rm -f "${CONTAINER}" 2>/dev/null || true
