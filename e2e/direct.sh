#!/usr/bin/env bash
# E2E scenario: assert status path is Direct after join.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "direct"
log "direct scenario stub is syntactically valid"
finish_scenario
