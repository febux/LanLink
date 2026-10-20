#!/usr/bin/env bash
# E2E scenario: gateway peer routes a shared subnet to the physical LAN.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "gateway"
log "gateway scenario stub is syntactically valid"
finish_scenario
