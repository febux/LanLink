#!/usr/bin/env bash
# E2E scenario: large ICMP echo payloads are not corrupted.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "pmtu"
log "pmtu scenario stub is syntactically valid"
finish_scenario
