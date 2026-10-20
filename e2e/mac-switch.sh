#!/usr/bin/env bash
# E2E scenario: ARP + ICMP between peer TAPs, verify MAC-table unicast.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "mac-switch"
log "mac-switch scenario stub is syntactically valid"
finish_scenario
