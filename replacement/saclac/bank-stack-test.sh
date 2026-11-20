#!/usr/bin/env bash
# check-fixed-image over every entry `saclac-extents stack-entries` declares for the bank image.
set -euo pipefail
extents=$1 checker=$2 stager=$3 allowlist=$4 carriers=$5 elf=$6 bin=$7 symbols=$8 disasm=$9 sections=${10}
export TMPDIR="$TEST_TMPDIR"
"$stager" body "$elf" "$TEST_TMPDIR/body.bin" > /dev/null
entries=()
while read -r name budget; do entries+=("$name" "$budget"); done \
  < <("$extents" stack-entries "$carriers" "$elf" "$bin" "$symbols" "$disasm" "$sections")
[ "${#entries[@]}" -gt 0 ]
"$checker" "$allowlist" "$elf" "$TEST_TMPDIR/body.bin" "${entries[@]}" "${TEST_UNDECLARED_OUTPUTS_DIR:-$TEST_TMPDIR}/bank-stack.json"
