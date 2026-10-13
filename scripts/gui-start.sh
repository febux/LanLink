#!/usr/bin/env bash
# One-command development launcher for LanLink.
# Starts the coordinator, the daemon (via sudo), and either the TUI or the
# browser WebUI in a single shell.
#
# Usage:
#   ./scripts/gui-start.sh
#   ./scripts/gui-start.sh --mode tui
#   ./scripts/gui-start.sh --mode web
#   ./scripts/gui-start.sh --mode desktop
#   ./scripts/gui-start.sh --mode web --release
#   ./scripts/gui-start.sh --mode web --bind 0.0.0.0:8686 --no-browser
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
MODE=tui
BIND="${LL_DEV_WEB_BIND:-127.0.0.1:8686}"
OPEN_BROWSER=1
COORD_PORT="${LL_DEV_COORD_PORT:-8080}"

while [ $# -gt 0 ]; do
  case "$1" in
    --release|-r)
      PROFILE=release
      shift
      ;;
    --mode)
      MODE="$2"
      shift 2
      ;;
    --bind)
      BIND="$2"
      shift 2
      ;;
    --no-browser)
      OPEN_BROWSER=0
      shift
      ;;
    --coordinator)
      # Consumed for compatibility with lanlink-gui --dev; ignored here because
      # this script always starts its own coordinator.
      shift 2
      ;;
    --status-socket)
      LL_DEV_STATUS_SOCKET="$2"
      shift 2
      ;;
    --bin-dir)
      # Consumed for compatibility with lanlink-gui --dev; ignored because the
      # script builds the workspace itself.
      shift 2
      ;;
    *)
      echo "Usage: $0 [--release|-r] [--mode tui|web|desktop] [--bind ADDR] [--no-browser]"
      exit 1
      ;;
  esac
done

if [ "$MODE" != "tui" ] && [ "$MODE" != "web" ] && [ "$MODE" != "desktop" ]; then
  echo "Invalid mode: $MODE (expected 'tui', 'web', or 'desktop')"
  exit 1
fi

NETWORK_NAME="${LL_DEV_NETWORK_NAME:-gaming-night}"
PASSWORD="${LL_DEV_PASSWORD:-hunter2}"
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
frontend_log="$LOG_DIR/${MODE}.log"

COORD_PID=""
DAEMON_PID=""
FRONTEND_PID=""

cleanup() {
  echo "[gui] shutting down..."
  if [ -n "$FRONTEND_PID" ]; then
    if kill -0 "$FRONTEND_PID" >/dev/null 2>&1; then
      kill -TERM "$FRONTEND_PID" >/dev/null 2>&1 || true
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
  # Make sure the terminal is usable again if the TUI left it in raw mode.
  stty sane 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Ensure sudo is authenticated up-front so daemon stderr can be logged instead of
# swallowing an interactive password prompt.
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo -v
fi

echo "[gui] building workspace ($PROFILE)..."
cargo build $CARGO_ARGS --workspace

echo "[gui] starting coordinator on $COORD_URL (log: $coordinator_log)"
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
  echo "[gui] coordinator failed to start; see $coordinator_log"
  exit 1
fi

echo "[gui] starting daemon as root (log: $daemon_log)"
export LANLINK_PASSWORD="$PASSWORD"
sudo -E env "LANLINK_PASSWORD=$PASSWORD" "$BIN_DIR/lanlinkd" \
  up \
  --name "$NETWORK_NAME" \
  --coordinator "$COORD_URL" \
  --status-socket "$STATUS_SOCKET" \
  >"$daemon_log" 2>&1 &
DAEMON_PID=$!

# Wait for the daemon status socket to appear, then make it accessible to the
# unprivileged frontend in this dev setup.
for _ in $(seq 1 100); do
  if [ -S "$STATUS_SOCKET" ]; then
    break
  fi
  sleep 0.2
done
if [ ! -S "$STATUS_SOCKET" ]; then
  echo "[gui] daemon failed to start; see $daemon_log"
  exit 1
fi
if [ "${EUID:-$(id -u)}" -ne 0 ]; then
  sudo chown "$(id -u -n)" "$STATUS_SOCKET" >/dev/null 2>&1 || true
  chmod 0600 "$STATUS_SOCKET" >/dev/null 2>&1 || true
fi

case "$MODE" in
  tui)
    echo "[gui] starting TUI (Ctrl-C or 'q' to quit)..."
    "$BIN_DIR/lanlink-tui" --coordinator "$COORD_URL" --status-socket "$STATUS_SOCKET" >"$frontend_log" 2>&1 &
    FRONTEND_PID=$!
    ;;
  web)
    WEB_URL="http://${BIND/0.0.0.0/127.0.0.1}"
    echo "[gui] starting WebUI on $BIND (log: $frontend_log)"
    "$BIN_DIR/lanlink-web" \
      --coordinator "$COORD_URL" \
      --status-socket "$STATUS_SOCKET" \
      --bind "$BIND" \
      >"$frontend_log" 2>&1 &
    FRONTEND_PID=$!

    # Wait for the WebUI to accept connections.
    WEB_PORT="${BIND##*:}"
    for _ in $(seq 1 100); do
      if (exec 3<>/dev/tcp/localhost/$WEB_PORT) 2>/dev/null; then
        break
      fi
      sleep 0.2
    done
    if ! (exec 3<>/dev/tcp/localhost/$WEB_PORT) 2>/dev/null; then
      echo "[gui] WebUI failed to start; see $frontend_log"
      exit 1
    fi

    echo "[gui] WebUI running at $WEB_URL"
    if [ "$OPEN_BROWSER" = "1" ]; then
      if command -v xdg-open >/dev/null 2>&1; then
        xdg-open "$WEB_URL" >/dev/null 2>&1 || true
      elif command -v open >/dev/null 2>&1; then
        open "$WEB_URL" >/dev/null 2>&1 || true
      fi
    fi
    ;;
  desktop)
    echo "[gui] starting desktop app (close the window to stop)..."
    "$BIN_DIR/lanlink-desktop" \
      --coordinator "$COORD_URL" \
      --status-socket "$STATUS_SOCKET" \
      >"$frontend_log" 2>&1 &
    FRONTEND_PID=$!
    ;;
esac

# Keep the script in the foreground until the frontend exits or the user
# presses Ctrl-C, then clean up the whole stack.
wait "$FRONTEND_PID"
