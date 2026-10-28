#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -ge 1 ]] || { echo "no compiled test harnesses were supplied" >&2; exit 1; }
for harness in "$@"; do
  "$harness" --test-threads 1
done
