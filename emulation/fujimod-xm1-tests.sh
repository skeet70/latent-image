#!/usr/bin/env bash
set -euo pipefail
stamp="${1:?stamp path}"
runtime="${2:?runtime path}"
[[ -s "$stamp" ]] || { echo "the checked runtime build left no test stamp" >&2; exit 1; }
grep -q 'cargo test' "$stamp" || { echo "the stamp does not record the test phase" >&2; exit 1; }
[[ -x "$runtime" ]] || { echo "the runtime binary is missing" >&2; exit 1; }
