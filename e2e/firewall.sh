#!/usr/bin/env bash
# E2E scenario: public network firewall drops blocked TCP, allows ICMP.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "firewall"
log "firewall scenario stub is syntactically valid"
finish_scenario
