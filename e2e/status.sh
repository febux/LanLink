#!/usr/bin/env bash
# E2E scenario: query each peer's status socket for NDJSON snapshot.
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_root
start_scenario "status"
log "status scenario stub is syntactically valid"
finish_scenario
