#!/usr/bin/env bash
# One-command development launcher for LanLink.
# Starts the coordinator, the daemon (via sudo), and the TUI in a single shell.
#
# Usage:
#   ./scripts/dev-start.sh
#   ./scripts/dev-start.sh --release
#
# Environment variables:
#   LL_DEV_NETWORK_NAME   network name to create/join (default: gaming-night)
#   LL_DEV_PASSWORD       network password (default: hunter2)
#   LL_DEV_COORD_PORT     coordinator port (default: 8080)
#   LL_DEV_STATUS_SOCKET  daemon status socket path (default: tmp/lanlink-dev-<name>.sock)

set -euo pipefail

cd "$(dirname "$0")/.."

PROFILE=debug
if [ "${1:-}" = "--release" ] || [ "${1:-}" = "-r" ]; then
  PROFILE=release
  shift || true
fi

NETWORK_NAME="${LL_DEV_NETWORK_NAME:-gaming-night}"
PASSWORD="${LL_DEV_PASSWORD:-hunter2}"
COORD_PORT="${LL_DEV_COORD_PORT:-8080}"
COORD_URL="http://localhost:${COORD_PORT}"

STATUS_SOCKET="${LL_DEV_STATUS_SOCKET:-tmp/lanlink-dev-${NETWORK_NAME}.sock}"
LOG_DIR="logs"
mkdir -p "$LOG_DIR" tmp

if [ "$PROFILE" = "release" ]; then
  CARGO_ARGS="--release"
  BIN_DIR="target/release"
else
  CARGO_ARGS=""
  BIN_DIR="target/debug"
fi

coordinator_log="$LOG_DIR/coordinator.log"
daemon_log="$LOG_DIR/daemon.log"

COORD_PID=""
DAEMON_PID=""

cleanup() {
  echo "[dev] shutting down..."
  if [ -n "$COORD_PID" ]; then
    if kill -0 "$COORD_PID" >/dev/null 2>&1; then
      kill -TERM -- -"$COORD_PID" >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "$DAEMON_PID" ]; then
    if ps -p "$DAEMON_PID" >/dev/null 2>&1; then
      kill -TERM "$DAEMON_PID" >/dev/null 2>&1 || sudo kill -TERM "$DAEMON_PID" >/dev/null 2>&1 || true
    fi
  fi
  wait >/dev/null 2>&1 || true
  # Make sure the terminal is usable again if the TUI left it in raw mode.
  stty sane 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Ensure sudo is authenticated up-front so daemon stderr can be logged instead of
# swallowing an interactive password prompt.
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo -v
fi

echo "[dev] building workspace ($PROFILE)..."
cargo build $CARGO_ARGS --workspace

echo "[dev] starting coordinator on $COORD_URL (log: $coordinator_log)"
setsid "$BIN_DIR/lanlink-coordinator" --db "lanlink.db" --listen "0.0.0.0:$COORD_PORT" >"$coordinator_log" 2>&1 &
COORD_PID=$!

# Wait for the coordinator to accept connections.
for _ in $(seq 1 100); do
  if (exec 3<>/dev/tcp/localhost/$COORD_PORT) 2>/dev/null; then
    break
  fi
  sleep 0.2
done
if ! (exec 3<>/dev/tcp/localhost/$COORD_PORT) 2>/dev/null; then
  echo "[dev] coordinator failed to start; see $coordinator_log"
  exit 1
fi

echo "[dev] starting daemon as root (log: $daemon_log)"
export LANLINK_PASSWORD="$PASSWORD"
sudo -E env "LANLINK_PASSWORD=$PASSWORD" "$BIN_DIR/lanlinkd" \
  up \
  --name "$NETWORK_NAME" \
  --coordinator "$COORD_URL" \
  --status-socket "$STATUS_SOCKET" \
  >"$daemon_log" 2>&1 &
DAEMON_PID=$!

# Wait for the daemon status socket to appear, then make it accessible to the
# unprivileged TUI in this dev setup.
for _ in $(seq 1 100); do
  if [ -S "$STATUS_SOCKET" ]; then
    break
  fi
  sleep 0.2
done
if [ ! -S "$STATUS_SOCKET" ]; then
  echo "[dev] daemon failed to start; see $daemon_log"
  exit 1
fi
# Allow the same user that launched this script to connect to the socket.
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo chown "$(id -u -n)" "$STATUS_SOCKET" >/dev/null 2>&1 || true
  chmod 0600 "$STATUS_SOCKET" >/dev/null 2>&1 || true
fi

echo "[dev] starting TUI (Ctrl-C or 'q' to quit)..."
"$BIN_DIR/lanlink-tui" --coordinator "$COORD_URL" --status-socket "$STATUS_SOCKET"
