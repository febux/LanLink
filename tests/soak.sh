#!/usr/bin/env bash
# M7 soak test: long-running stability + peer churn + leak check.
#
# Runs a full control/data plane on one host:
#   - coordinator + relayd on the host
#   - SOAK_PEERS daemons, each in its own network namespace connected to the
#     host via veth + bridge, each with its own TAP inside the namespace
#   - continuous mesh pings between all live peers
#   - churn: random peer restarts (SIGTERM and SIGKILL) and link flaps
#   - per-daemon RSS + fd sampling for leak detection
#
# At the end it prints/publishes a markdown report and fails if:
#   - packet loss exceeds SOAK_MAX_LOSS_PCT
#   - any daemon's RSS grew more than SOAK_MAX_RSS_GROWTH_KB
#   - a SIGTERM'd daemon failed to exit within 10 s
#
# Usage:
#   sudo ./tests/soak.sh                          # 72 h (the M7 gate)
#   sudo SOAK_DURATION=300 ./tests/soak.sh        # 5-minute smoke
#
# Knobs (env):
#   SOAK_DURATION          seconds (default 259200 = 72 h)
#   SOAK_PEERS             peer count (default 6)
#   SOAK_CHURN_INTERVAL    seconds between churn events (default 45)
#   SOAK_SAMPLE_INTERVAL   seconds between ping/RSS samples (default 60)
#   SOAK_MAX_LOSS_PCT      ping-loss failure threshold (default 2)
#   SOAK_MAX_RSS_GROWTH_KB daemon RSS growth threshold in kB, default 131072)
#   SOAK_DIR               working dir (default /tmp/lanlink-soak.$$)
#   SOAK_REPORT            report path (default $SOAK_DIR/soak-report.md)
#
# All cross-loop state lives in $SOAK_DIR files (PIDs, counters, samples) so
# the background loops and the report pass agree regardless of subshells.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

SOAK_DURATION="${SOAK_DURATION:-259200}"
SOAK_PEERS="${SOAK_PEERS:-6}"
SOAK_CHURN_INTERVAL="${SOAK_CHURN_INTERVAL:-45}"
SOAK_SAMPLE_INTERVAL="${SOAK_SAMPLE_INTERVAL:-60}"
SOAK_MAX_LOSS_PCT="${SOAK_MAX_LOSS_PCT:-2}"
SOAK_MAX_RSS_GROWTH_KB="${SOAK_MAX_RSS_GROWTH_KB:-131072}"
SOAK_DIR="${SOAK_DIR:-/tmp/lanlink-soak.$$}"
SOAK_REPORT="${SOAK_REPORT:-${SOAK_DIR}/soak-report.md}"

DAEMON="${REPO_DIR}/target/release/lanlinkd"
CTL="${REPO_DIR}/target/release/lanlinkctl"
COORD="${REPO_DIR}/target/release/lanlink-coordinator"
RELAYD="${REPO_DIR}/target/release/lanlink-relayd"

BR="ll_soak_br"
BR_IP="10.203.0.1"
COORD_PORT="18080"
RELAY_PORT="13478"
PASSWORD="soak-password-$(date +%s)"
GRACE_AFTER_START=25   # skip pinging a peer for N s after it (re)starts

START_TS="$(date -Is)"
COORD_PID=""
RELAY_PID=""
LOOPS_PID=()

log()  { printf '[soak %s] %s\n' "$(date +%H:%M:%S)" "$*"; }
fail() { printf '[soak] FATAL: %s\n' "$*" >&2; exit 1; }

# ---- file-backed shared state ---------------------------------------------
counters_dir="${SOAK_DIR}/counters"
pid_of()        { cat "${SOAK_DIR}/peer${1}.pid" 2>/dev/null || true; }
started_at()    { cat "${SOAK_DIR}/peer${1}.started" 2>/dev/null || echo 0; }
bump()          { local f="${counters_dir}/$1"; echo $(( $(cat "$f" 2>/dev/null || echo 0) + 1 )) > "$f"; }
counter()       { cat "${counters_dir}/$1" 2>/dev/null || echo 0; }

# Liveness that treats zombies as dead (reparenting differs per spawner).
proc_alive() {
    local pid="$1" state
    [ -n "$pid" ] || return 1
    state="$(cut -d' ' -f3 "/proc/${pid}/stat" 2>/dev/null)" || return 1
    [ -n "$state" ] && [ "$state" != "Z" ]
}
daemon_alive() { proc_alive "$(pid_of "$1")"; }

cleanup() {
    local rc=$?
    set +e
    log "cleaning up..."
    for f in "${SOAK_DIR}"/peer*.pid; do
        [ -e "$f" ] || continue
        pid="$(cat "$f")"
        proc_alive "$pid" && kill -TERM "$pid" 2>/dev/null
    done
    sleep 2
    for f in "${SOAK_DIR}"/peer*.pid; do
        [ -e "$f" ] || continue
        kill -KILL "$(cat "$f")" 2>/dev/null
    done
    proc_alive "${COORD_PID}" && kill -TERM "${COORD_PID}" 2>/dev/null
    proc_alive "${RELAY_PID}" && kill -TERM "${RELAY_PID}" 2>/dev/null
    for p in "${LOOPS_PID[@]:-}"; do kill "$p" 2>/dev/null; done
    for ns in ll_soak_*; do ip netns del "$ns" 2>/dev/null; done
    ip link del "$BR" 2>/dev/null
    exit "$rc"
}
trap cleanup EXIT INT TERM

if [ "$(id -u)" -ne 0 ]; then
    echo "This test must run as root (netns/TAP require CAP_NET_ADMIN)." >&2
    exit 1
fi

for bin in "$DAEMON" "$CTL" "$COORD" "$RELAYD"; do
    [ -x "$bin" ] || fail "missing $bin; run: cargo build --release -p lanlinkd -p lanlinkctl -p coordinator -p lanlink-relay"
done

mkdir -p "${SOAK_DIR}" "${counters_dir}"
: > "${SOAK_DIR}/pings.tsv"
: > "${SOAK_DIR}/rss.tsv"

# ---------------------------------------------------------------------------
# Topology: host bridge + one netns per peer
# ---------------------------------------------------------------------------
log "creating bridge ${BR} (${BR_IP}/24)..."
ip link add "$BR" type bridge
ip addr add "${BR_IP}/24" dev "$BR"
ip link set "$BR" up

for i in $(seq 1 "$SOAK_PEERS"); do
    ns="ll_soak_${i}"
    host_end="v_soak_${i}"
    ip netns add "$ns"
    ip link add "$host_end" type veth peer name eth0 netns "$ns"
    ip link set "$host_end" up
    ip link set "$host_end" master "$BR"
    ip -n "$ns" addr add "10.203.0.$((9 + i))/24" dev eth0
    ip -n "$ns" link set eth0 up
    ip -n "$ns" link set lo up
    ip -n "$ns" route add default via "${BR_IP}"
done

# ---------------------------------------------------------------------------
# Control plane on the host
# ---------------------------------------------------------------------------
log "starting coordinator + relayd..."
"${COORD}" --listen "0.0.0.0:${COORD_PORT}" \
    --db "${SOAK_DIR}/soak.db" \
    --relay-server "${BR_IP}:${RELAY_PORT}" \
    > "${SOAK_DIR}/coordinator.log" 2>&1 &
COORD_PID=$!
"${RELAYD}" --listen "0.0.0.0:${RELAY_PORT}" \
    > "${SOAK_DIR}/relayd.log" 2>&1 &
RELAY_PID=$!
sleep 1
proc_alive "$COORD_PID" || { tail -5 "${SOAK_DIR}/coordinator.log"; fail "coordinator failed to start"; }
proc_alive "$RELAY_PID"  || { tail -5 "${SOAK_DIR}/relayd.log";  fail "relayd failed to start"; }

log "creating network..."
create_out="$("${CTL}" coord create \
    --coordinator "http://${BR_IP}:${COORD_PORT}" \
    --name "soak-$(date +%s)" \
    --password "$PASSWORD")"
NETWORK_ID="$(printf '%s\n' "$create_out" | awk '{print $3; exit}')"
case "$NETWORK_ID" in ''|*[!0-9]*) fail "could not parse network id from: $create_out";; esac
log "network id: ${NETWORK_ID} (${create_out})"

# ---------------------------------------------------------------------------
# Daemons (one per netns)
# ---------------------------------------------------------------------------
start_peer() {
    local i=$1
    cat > "${SOAK_DIR}/peer${i}.toml" <<EOF
[network]
coordinator = "http://${BR_IP}:${COORD_PORT}"
network_id = ${NETWORK_ID}
password = "${PASSWORD}"
bind = "0.0.0.0:$((9100 + i))"
tap = "ll0"
mtu = 1400
# STUN is unreachable inside the lab on purpose: exercises the host-candidate
# direct path (peers share the bridge) and the relay fallback, without WAN.
stun_servers = ["${BR_IP}:9999"]
EOF
    RUST_LOG=info ip netns exec "ll_soak_${i}" "$DAEMON" run --config "${SOAK_DIR}/peer${i}.toml" \
        >> "${SOAK_DIR}/peer${i}.log" 2>&1 &
    echo $! > "${SOAK_DIR}/peer${i}.pid"
    echo "${SECONDS}" > "${SOAK_DIR}/peer${i}.started"
}

log "starting ${SOAK_PEERS} peers..."
for i in $(seq 1 "$SOAK_PEERS"); do start_peer "$i"; done

log "waiting for all peers to join (TAP addresses)..."
deadline=$((SECONDS + 120))
joined=0
while [ "$SECONDS" -lt "$deadline" ]; do
    joined=0
    for i in $(seq 1 "$SOAK_PEERS"); do
        ip -n "ll_soak_${i}" -4 addr show dev ll0 2>/dev/null | grep -q 'inet ' && joined=$((joined + 1))
    done
    [ "$joined" -eq "$SOAK_PEERS" ] && break
    sleep 2
done
[ "$joined" -eq "$SOAK_PEERS" ] || fail "not all peers joined within 120 s ($joined/$SOAK_PEERS; see ${SOAK_DIR}/peer*.log)"
log "all peers joined"

# Current virtual IP of peer i's TAP (empty when down).
peer_vip() {
    ip -n "ll_soak_${1}" -4 addr show dev ll0 2>/dev/null \
        | awk '/inet /{split($2,a,"/"); print a[1]; exit}'
}

# ---------------------------------------------------------------------------
# Background loops (all state via files)
# ---------------------------------------------------------------------------
sample_loop() {
    while :; do
        local live_ips=() live_ids=() i vip
        for i in $(seq 1 "$SOAK_PEERS"); do
            if daemon_alive "$i"; then
                vip="$(peer_vip "$i")"
                if [ -n "$vip" ]; then
                    live_ids+=("$i"); live_ips+=("$vip")
                fi
            fi
        done
        # ping from every live peer to every other live, settled peer
        if [ "${#live_ids[@]}" -ge 2 ]; then
            local a=0
            for i in "${live_ids[@]}"; do
                local b=0
                for j in "${live_ids[@]}"; do
                    if [ "$i" -ne "$j" ]; then
                        # skip peers still settling after a (re)start
                        local age=$(( SECONDS - $(started_at "$j") ))
                        if [ "$age" -ge "$GRACE_AFTER_START" ]; then
                            local out recv
                            out="$(ip netns exec "ll_soak_${i}" ping -c 2 -W 3 -q "${live_ips[$b]}" 2>/dev/null || true)"
                            recv="$(printf '%s\n' "$out" | awk -F'[ ,]' '/packets transmitted/{for (k=1;k<=NF;k++) if ($k=="received") print $(k-1)}')"
                            echo -e "${SECONDS}\t${i}->${j}\t2\t${recv:-0}" >> "${SOAK_DIR}/pings.tsv"
                        fi
                    fi
                    b=$((b + 1))
                done
                a=$((a + 1))
            done
        fi
        for i in $(seq 1 "$SOAK_PEERS"); do
            local pid rss fds
            pid="$(pid_of "$i")"
            if proc_alive "$pid"; then
                rss="$(awk '/VmRSS/{print $2}' "/proc/${pid}/status" 2>/dev/null)"
                if [ -n "$rss" ]; then
                    fds="$(ls "/proc/${pid}/fd" 2>/dev/null | wc -l)"
                    echo -e "${SECONDS}\tpeer${i}\t${rss}\t${fds}" >> "${SOAK_DIR}/rss.tsv"
                fi
            fi
        done
        sleep "$SOAK_SAMPLE_INTERVAL"
    done
}
sample_loop &
LOOPS_PID+=($!)

churn_loop() {
    while :; do
        sleep "$SOAK_CHURN_INTERVAL"
        local i pid action
        i=$(( (RANDOM % SOAK_PEERS) + 1 ))
        pid="$(pid_of "$i")"
        proc_alive "$pid" || continue
        action=$(( RANDOM % 3 ))
        case "$action" in
            0)  # graceful restart (SIGTERM must exit within 10 s)
                log "churn: SIGTERM peer ${i}"
                kill -TERM "$pid" 2>/dev/null || true
                ok=0
                for _ in $(seq 1 100); do
                    proc_alive "$pid" || { ok=1; break; }
                    sleep 0.1
                done
                if [ "$ok" -ne 1 ]; then
                    log "WARN: peer ${i} did not exit within 10 s after SIGTERM"
                    bump term_hangs
                    kill -KILL "$pid" 2>/dev/null || true
                fi
                bump restarts_term
                start_peer "$i"
                ;;
            1)  # crash + restart (exercises crash-safe recovery)
                log "churn: SIGKILL peer ${i}"
                kill -KILL "$pid" 2>/dev/null || true
                bump restarts_kill
                start_peer "$i"
                ;;
            2)  # link flap
                log "churn: link flap peer ${i}"
                ip -n "ll_soak_${i}" link set eth0 down 2>/dev/null || true
                sleep 2
                ip -n "ll_soak_${i}" link set eth0 up 2>/dev/null || true
                bump link_flaps
                ;;
        esac
    done
}
churn_loop &
LOOPS_PID+=($!)

# ---------------------------------------------------------------------------
# Soak
# ---------------------------------------------------------------------------
log "soaking for ${SOAK_DURATION} s (until $(date -d "+${SOAK_DURATION} seconds" 2>/dev/null || date))"
end_at=$((SECONDS + SOAK_DURATION))
while [ "$SECONDS" -lt "$end_at" ]; do
    sleep 10
    proc_alive "$COORD_PID" || { log "FATAL: coordinator died during soak"; break; }
done

for p in "${LOOPS_PID[@]}"; do kill "$p" 2>/dev/null || true; done

# Final graceful shutdown of all peers (hang check one more time).
log "stopping peers..."
for f in "${SOAK_DIR}"/peer*.pid; do
    [ -e "$f" ] || continue
    pid="$(cat "$f")"
    proc_alive "$pid" && kill -TERM "$pid" 2>/dev/null
done
deadline=$((SECONDS + 15))
alive=1
while [ "$SECONDS" -lt "$deadline" ] && [ "$alive" -gt 0 ]; do
    alive=0
    for f in "${SOAK_DIR}"/peer*.pid; do
        [ -e "$f" ] || continue
        proc_alive "$(cat "$f")" && alive=$((alive + 1))
    done
    [ "$alive" -eq 0 ] || sleep 1
done
if [ "$alive" -ne 0 ]; then
    log "WARN: ${alive} peer(s) failed to exit within 15 s of SIGTERM"
    term_hangs_final=$(( alive + $(counter term_hangs) ))
    echo "$term_hangs_final" > "${counters_dir}/term_hangs"
fi

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
read -r PING_SENT PING_RECV <<< "$(awk -F'\t' '{s+=$3; r+=$4} END{print s+0, r+0}' "${SOAK_DIR}/pings.tsv")"
loss_pct=0
[ "$PING_SENT" -gt 0 ] && loss_pct=$(awk -v s="$PING_SENT" -v r="$PING_RECV" 'BEGIN{printf "%.2f", 100*(s-r)/s}')

max_growth=0
{
    echo "# LanLink soak report"
    echo
    echo "- started: ${START_TS}, finished: $(date -Is), duration: ${SOAK_DURATION} s"
    echo "- peers: ${SOAK_PEERS}, sample interval: ${SOAK_SAMPLE_INTERVAL} s, churn interval: ${SOAK_CHURN_INTERVAL} s"
    echo "- churn: $(counter restarts_term) graceful restarts, $(counter restarts_kill) crash restarts, $(counter link_flaps) link flaps"
    echo "- SIGTERM hangs: $(counter term_hangs)"
    echo "- pings: ${PING_SENT} sent, ${PING_RECV} received, ${loss_pct}% loss"
    echo
    echo "## Daemon memory (RSS kB)"
    echo
    echo "| peer | first | last | max | growth | fds(last) |"
    echo "|---|---|---|---|---|---|"
    for i in $(seq 1 "$SOAK_PEERS"); do
        first="$(awk -F'\t' -v p="peer${i}" '$2==p && !seen[$2]++ {print $3}' "${SOAK_DIR}/rss.tsv" | head -1)"
        last="$(awk -F'\t' -v p="peer${i}" '$2==p {l=$3; fd=$4} END{print l}' "${SOAK_DIR}/rss.tsv")"
        last_fd="$(awk -F'\t' -v p="peer${i}" '$2==p {fd=$4} END{print fd}' "${SOAK_DIR}/rss.tsv")"
        maxr="$(awk -F'\t' -v p="peer${i}" '$2==p {if($3>m) m=$3} END{print m}' "${SOAK_DIR}/rss.tsv")"
        if [ -n "${first:-}" ] && [ -n "${last:-}" ]; then
            growth=$(( last > first ? last - first : 0 ))
            [ "$growth" -gt "$max_growth" ] && max_growth=$growth
            echo "| peer${i} | ${first} | ${last} | ${maxr} | ${growth} | ${last_fd} |"
        else
            echo "| peer${i} | (no samples) | | | | |"
        fi
    done
    echo
} > "$SOAK_REPORT"

verdict=PASS
if [ "$(awk -v a="$loss_pct" -v b="$SOAK_MAX_LOSS_PCT" 'BEGIN{print (a>b)?1:0}')" = "1" ]; then verdict=FAIL; fi
if [ "$max_growth" -gt "$SOAK_MAX_RSS_GROWTH_KB" ]; then verdict=FAIL; fi
if [ "$(counter term_hangs)" -gt 0 ]; then verdict=FAIL; fi

{
    echo "## Verdict: ${verdict}"
    echo
    echo "Thresholds: loss <= ${SOAK_MAX_LOSS_PCT}% (got ${loss_pct}%), RSS growth <= ${SOAK_MAX_RSS_GROWTH_KB} kB (max ${max_growth} kB), SIGTERM hangs = 0 (got $(counter term_hangs))."
} >> "$SOAK_REPORT"

cat "$SOAK_REPORT"
log "report: ${SOAK_REPORT}"
log "logs: ${SOAK_DIR}/peer*.log"

[ "$verdict" = "PASS" ]
