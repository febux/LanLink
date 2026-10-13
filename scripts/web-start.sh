#!/usr/bin/env bash
# One-command development launcher for the LanLink WebUI.
# Starts the coordinator, the daemon (via sudo), and the WebUI in a single shell.
#
# Usage:
#   ./scripts/web-start.sh
#   ./scripts/web-start.sh --release
#   ./scripts/web-start.sh --bind 0.0.0.0:8686
#   ./scripts/web-start.sh --no-browser
#
# Environment variables:
#   LL_DEV_NETWORK_NAME   network name to create/join (default: gaming-night)
#   LL_DEV_PASSWORD       network password (default: hunter2)
#   LL_DEV_COORD_PORT     coordinator port (default: 8080)
#   LL_DEV_STATUS_SOCKET  daemon status socket path (default: tmp/lanlink-dev-<name>.sock)
#   LL_DEV_WEB_BIND       WebUI bind address (default: 127.0.0.1:8686)

set -euo pipefail

cd "$(dirname "$0")/.."

PROFILE=debug
BIND="${LL_DEV_WEB_BIND:-127.0.0.1:8686}"
OPEN_BROWSER=1

while [ $# -gt 0 ]; do
  case "$1" in
    --release|-r)
      PROFILE=release
      shift
      ;;
    --bind)
      BIND="$2"
      shift 2
      ;;
    --no-browser)
      OPEN_BROWSER=0
      shift
      ;;
    *)
      echo "Usage: $0 [--release|-r] [--bind ADDR] [--no-browser]"
      exit 1
      ;;
  esac
done

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
web_log="$LOG_DIR/web.log"

COORD_PID=""
DAEMON_PID=""
WEB_PID=""

cleanup() {
  echo "[web] shutting down..."
  if [ -n "$WEB_PID" ]; then
    if kill -0 "$WEB_PID" >/dev/null 2>&1; then
      kill -TERM "$WEB_PID" >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "$DAEMON_PID" ]; then
    if ps -p "$DAEMON_PID" >/dev/null 2>&1; then
      kill -TERM "$DAEMON_PID" >/dev/null 2>&1 || sudo kill -TERM "$DAEMON_PID" >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "$COORD_PID" ]; then
    if kill -0 "$COORD_PID" >/dev/null 2>&1; then
      kill -TERM -- -"$COORD_PID" >/dev/null 2>&1 || true
    fi
  fi
  wait >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

# Ensure sudo is authenticated up-front so daemon stderr can be logged instead of
# swallowing an interactive password prompt.
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo -v
fi

echo "[web] building workspace ($PROFILE)..."
cargo build $CARGO_ARGS --workspace

echo "[web] starting coordinator on $COORD_URL (log: $coordinator_log)"
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
  echo "[web] coordinator failed to start; see $coordinator_log"
  exit 1
fi

echo "[web] starting daemon as root (log: $daemon_log)"
export LANLINK_PASSWORD="$PASSWORD"
sudo -E env "LANLINK_PASSWORD=$PASSWORD" "$BIN_DIR/lanlinkd" \
  up \
  --name "$NETWORK_NAME" \
  --coordinator "$COORD_URL" \
  --status-socket "$STATUS_SOCKET" \
  >"$daemon_log" 2>&1 &
DAEMON_PID=$!

# Wait for the daemon status socket to appear, then make it accessible to the
# unprivileged WebUI in this dev setup.
for _ in $(seq 1 100); do
  if [ -S "$STATUS_SOCKET" ]; then
    break
  fi
  sleep 0.2
done
if [ ! -S "$STATUS_SOCKET" ]; then
  echo "[web] daemon failed to start; see $daemon_log"
  exit 1
fi
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo chown "$(id -u -n)" "$STATUS_SOCKET" >/dev/null 2>&1 || true
  chmod 0600 "$STATUS_SOCKET" >/dev/null 2>&1 || true
fi

WEB_URL="http://${BIND/0.0.0.0/127.0.0.1}"

echo "[web] starting WebUI on $BIND (log: $web_log)"
"$BIN_DIR/lanlink-web" \
  --coordinator "$COORD_URL" \
  --status-socket "$STATUS_SOCKET" \
  --bind "$BIND" \
  >"$web_log" 2>&1 &
WEB_PID=$!

# Wait for the WebUI to accept connections.
WEB_PORT="${BIND##*:}"
for _ in $(seq 1 100); do
  if (exec 3<>/dev/tcp/localhost/$WEB_PORT) 2>/dev/null; then
    break
  fi
  sleep 0.2
done
if ! (exec 3<>/dev/tcp/localhost/$WEB_PORT) 2>/dev/null; then
  echo "[web] WebUI failed to start; see $web_log"
  exit 1
fi

echo "[web] open $WEB_URL in your browser"

if [ "$OPEN_BROWSER" = "1" ]; then
  if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$WEB_URL" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then
    open "$WEB_URL" >/dev/null 2>&1 || true
  fi
fi

# Keep the script in the foreground until the user presses Ctrl-C, then cleanup.
echo "[web] press Ctrl-C to stop"
wait "$WEB_PID"
