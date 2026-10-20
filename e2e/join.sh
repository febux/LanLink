#!/usr/bin/env bash
# E2E scenario: two peers join a coordinator-created network and get distinct IPs.
set -euo pipefail

# shellcheck source=e2e/lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "join"
# Full implementation added in group 4.
log "join scenario stub is syntactically valid"
finish_scenario
