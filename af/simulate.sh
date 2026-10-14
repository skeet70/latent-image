#!/usr/bin/env bash
set -euo pipefail
fuji_af_simulator="$1"
shift
exec "$fuji_af_simulator" "$@"
