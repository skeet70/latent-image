#!/usr/bin/env bash
set -euo pipefail
correct=$1
mutant=$2
receipt=${3:-${TEST_TMPDIR:-.}/full-export-control-result.json}
correct_log="$receipt.correct.log"
mutant_log="$receipt.mutant.log"
test -x "$correct"
test -x "$mutant"
set +e
"$correct" >"$correct_log" 2>&1
correct_exit=$?
"$mutant" >"$mutant_log" 2>&1
mutant_exit=$?
set -e
test "$correct_exit" -eq 0
if [ "$mutant_exit" -eq 0 ] || [ "$mutant_exit" -eq 126 ] || [ "$mutant_exit" -eq 127 ] || [ "$mutant_exit" -ge 128 ]; then
  exit 1
fi
printf '{"control":"full-export-tail-zero-invented","test":"whole-rust-test-suites","correct_exit":%d,"mutant_exit":%d,"correct_log":"%s","mutant_log":"%s"}\n' \
  "$correct_exit" "$mutant_exit" "$correct_log" "$mutant_log" >"$receipt"
