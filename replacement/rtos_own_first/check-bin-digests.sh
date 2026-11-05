#!/usr/bin/env bash
# Checks each linked .bin against its pinned sha256: SHA256SUM (EXPECTED FILE)...
set -euo pipefail

sha256sum=$1
shift

status=0
while (($# > 0)); do
  expected=$1
  file=$2
  shift 2
  actual=$("$sha256sum" "$file")
  actual=${actual%% *}
  if [[ $actual != "$expected" ]]; then
    echo "$file: $actual != $expected" >&2
    status=1
  fi
done
exit $status
