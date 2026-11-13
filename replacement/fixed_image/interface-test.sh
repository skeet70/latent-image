#!/usr/bin/env bash
set -euo pipefail
checker=$1 allowlist=$2 elf=$3 body=$4
export TMPDIR="$TEST_TMPDIR"
"$checker" "$allowlist" "$elf" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/admitted.json"
printf '{"schema":2,"stock_calls":[],"stock_regions":[]}\n' > "$TEST_TMPDIR/empty.json"
if "$checker" "$TEST_TMPDIR/empty.json" "$elf" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/undeclared.json"; then
  echo 'undeclared retained continuation admitted' >&2
  exit 1
fi
if "$checker" "$allowlist" "$elf" "$body" xm1_fixed_test_entry 0 "$TEST_TMPDIR/short-stack.json"; then
  echo 'insufficient owned stack budget admitted' >&2
  exit 1
fi
{ printf '\000'; dd if="$body" bs=1 skip=1 status=none; } > "$TEST_TMPDIR/changed.bin"
if "$checker" "$allowlist" "$elf" "$TEST_TMPDIR/changed.bin" xm1_fixed_test_entry 48 "$TEST_TMPDIR/changed.json"; then
  echo 'body from a different link admitted' >&2
  exit 1
fi
