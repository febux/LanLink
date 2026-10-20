#!/usr/bin/env bash
# E2E scenario: block direct UDP, keep relay path open, assert Relay path.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "relay"
log "relay scenario stub is syntactically valid"
finish_scenario
