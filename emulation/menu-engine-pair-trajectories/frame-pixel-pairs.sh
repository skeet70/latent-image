#!/usr/bin/env bash
# Usage: frame-pixel-pairs.sh STOCK_RUN_DIR OWNED_RUN_DIR RECORD_JSON OUT
# Runs frame-pixel-check on every capture label with a screenshots/LABEL.ram.bin in both run directories, with the
# record address of RECORD_JSON (the package's render-frame-record.json), and writes OUT/verdict.json. Per-label
# outputs go to OUT/LABEL/. FRAME_PIXEL_CHECK names the tool (default bazel-bin/replacement/render_commands/frame-pixel-check).
# Exit 0 when at least one label is compared and every compared label passes, 1 otherwise, 2 on usage or input errors.
set -uo pipefail

if [ "$#" -ne 4 ]; then
  printf 'usage: %s STOCK_RUN_DIR OWNED_RUN_DIR RECORD_JSON OUT\n' "$0" >&2
  exit 2
fi
stock=$1 owned=$2 record_json=$3 out=$4
tool=${FRAME_PIXEL_CHECK:-bazel-bin/replacement/render_commands/frame-pixel-check}
[ -x "$tool" ] || { printf 'frame-pixel-check is not executable: %s\n' "$tool" >&2; exit 2; }
for directory in "$stock/screenshots" "$owned/screenshots"; do
  [ -d "$directory" ] || { printf 'missing %s\n' "$directory" >&2; exit 2; }
done
address=$(jq -er '.address | select(type == "string" and test("^0x[0-9a-fA-F]{1,8}$"))' "$record_json") \
  || { printf '%s has no 0x-hex address\n' "$record_json" >&2; exit 2; }
mkdir -p "$out" || exit 2

labels() { for path in "$1"/screenshots/*.ram.bin; do [ -e "$path" ] && basename "$path" .ram.bin; done | LC_ALL=C sort; }
stock_labels=$(labels "$stock")
owned_labels=$(labels "$owned")
common=$(LC_ALL=C comm -12 <(printf '%s\n' "$stock_labels") <(printf '%s\n' "$owned_labels") | sed '/^$/d')
stock_only=$(LC_ALL=C comm -23 <(printf '%s\n' "$stock_labels") <(printf '%s\n' "$owned_labels") | sed '/^$/d')
owned_only=$(LC_ALL=C comm -13 <(printf '%s\n' "$stock_labels") <(printf '%s\n' "$owned_labels") | sed '/^$/d')

results=$out/results.jsonl
: > "$results"
while IFS= read -r label; do
  [ -n "$label" ] || continue
  "$tool" --stock "$stock/screenshots/$label.ram.bin" --owned "$owned/screenshots/$label.ram.bin" \
    --record "$address" --out "$out/$label" 2> "$out/$label.stderr"
  status=$?
  [ "$status" -eq 2 ] && { printf '%s: input error, see %s\n' "$label" "$out/$label.stderr" >&2; exit 2; }
  summary=$out/$label/summary.json
  if [ -f "$summary" ]; then
    jq -c --arg label "$label" --argjson exit "$status" '{label: $label, exit: $exit, passed: ($exit == 0 and .passed), summary: .}' "$summary" >> "$results" || exit 2
  else
    jq -nc --arg label "$label" --argjson exit "$status" '{label: $label, exit: $exit, passed: false, summary: null}' >> "$results" || exit 2
  fi
  printf '%s exit=%s\n' "$label" "$status"
done <<< "$common"

lines() { jq -R . <<< "$1" | jq -s 'map(select(. != ""))'; }
jq -s --arg stock "$stock" --arg owned "$owned" --arg record "$record_json" --arg address "$address" \
  --argjson stock_only "$(lines "$stock_only")" --argjson owned_only "$(lines "$owned_only")" \
  '{schema: 1, stock: $stock, owned: $owned, record: {path: $record, address: $address},
    labels: ., stock_only: $stock_only, owned_only: $owned_only,
    passed: ((length > 0) and all(.[]; .passed))}' "$results" > "$out/verdict.json" || exit 2
rm -f "$results"
jq -e '.passed' "$out/verdict.json" > /dev/null
