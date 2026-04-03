#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" ]; then
  export WORKSHOP_DATA_DIR="$1"
fi

exec ibazel run //packages/workshop_ui:dev
