#!/usr/bin/env bash
set -euo pipefail
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
D=emulation/menu-engine-pair-trajectories
S=private-data/scratch/ControllerStrategy
out=${1:-$S/render-pairs/draw-route-probe.stock.json}
receipt=$S/inc5/fn-auto-open/menu-observe.json
picker=$(jq -L "$D" --arg role stock -f "$D/picker.jq" emulation/modal75-picker-trajectories/picker-15.json)
oracle=$(jq -n -L "$D" --arg role stock --arg name fn-auto-open --argjson sample '[]' \
  --slurpfile config emulation/menu-oracle-fn-auto-open.json \
  --arg receipt_source "$receipt" --slurpfile receipt "$receipt" -f "$D/fn-oracle.jq")
mkdir -p "$(dirname "$out")"
jq -n --argjson picker "$picker" --argjson oracle "$oracle" -f "$D/draw-route-probe.jq" > "$out"
printf '%s\n' "$out"
