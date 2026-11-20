#!/usr/bin/env bash
set -euo pipefail
checker=$1 allowlist=$2 elf=$3 body=$4 published_admitted=$5 published_word=$6 published_outside=$7
above_admitted=$8 above_word=$9 above_outside=${10}
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
if "$checker" "$allowlist" "$elf" "$body" xm1_fixed_test_entry 48 xm1_fixed_test_entry 0 "$TEST_TMPDIR/one-short.json"; then
  echo 'one entry over its owned stack budget admitted among several' >&2
  exit 1
fi
{ printf '\000'; dd if="$body" bs=1 skip=1 status=none; } > "$TEST_TMPDIR/changed.bin"
if "$checker" "$allowlist" "$elf" "$TEST_TMPDIR/changed.bin" xm1_fixed_test_entry 48 "$TEST_TMPDIR/changed.json"; then
  echo 'body from a different link admitted' >&2
  exit 1
fi
"$checker" "$allowlist" "$published_admitted" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/published.json"
if "$checker" "$allowlist" "$published_word" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/published-word.json"; then
  echo 'published section with a non-trap, non-branch word admitted' >&2
  exit 1
fi
if "$checker" "$allowlist" "$published_outside" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/published-outside.json"; then
  echo 'published branch outside fixed code admitted' >&2
  exit 1
fi
"$checker" "$allowlist" "$above_admitted" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/above.json"
if "$checker" "$allowlist" "$above_word" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/above-word.json"; then
  echo 'section published above the image with a non-trap, non-branch word admitted' >&2
  exit 1
fi
if "$checker" "$allowlist" "$above_outside" "$body" xm1_fixed_test_entry 48 "$TEST_TMPDIR/above-outside.json"; then
  echo 'branch published above the image to outside fixed code admitted' >&2
  exit 1
fi
