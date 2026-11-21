#!/usr/bin/env bash
# One stock/owned pair of an IMAGE SIZE changed-selection route and its gate comparison
# (reports/owned-menu-engine-design.md#image-size-changed-selection).
#
#   run-value-change.sh ROUTE SCHEDULE RUN_DOCUMENT INPUTS TRAJECTORIES OUTPUT
#
# ROUTE is change-stall or change-complete; SCHEDULE labels RUN_DOCUMENT in the receipt. INPUTS holds
# firmware.DAT, adjustment.nand, engine.a and retired.json; TRAJECTORIES holds picker-260.stock.json
# and picker-260.owned.json. Run from the repository root under `run-output exec`, outside an
# emulator slot, after building //emulation:xm1-viewer and
# //replacement/modal_runtime:menu-value-change-check. Each role holds one capped emulator slot.
set -euo pipefail
(( $# == 6 )) || { sed -n '2,11p' "$0" >&2; exit 2; }
route=$1 schedule=$2 run_document=$3 inputs=$4 trajectories=$5 output=$6
case "$route" in change-stall|change-complete) ;; *) printf 'route must be change-stall or change-complete\n' >&2; exit 2 ;; esac
viewer=bazel-bin/emulation/xm1-viewer
checker=bazel-bin/replacement/modal_runtime/menu-value-change-check
correspondence=replacement/menu_value/callback-correspondence.json
mkdir -p "$output"
[[ -z $(ls -A "$output") ]] || { printf 'output directory is not empty: %s\n' "$output" >&2; exit 2; }

for role in stock owned; do
  jq --arg role "$role" --arg route "$route" -f emulation/menu-engine-pair-trajectories/value.jq \
    "$trajectories/picker-260.$role.json" > "$output/$route.$role.json"
done
sha256sum "$run_document" "$inputs"/{firmware.DAT,adjustment.nand,engine.a,retired.json} \
  "$output/$route".{stock,owned}.json "$viewer" "$checker" "$correspondence" > "$output/inputs.sha256"
jq -n --arg route "$route" --arg schedule "$schedule" --arg document "$run_document" --arg source "$(git rev-parse HEAD)" \
  '{route: $route, schedule: $schedule, run_document: $document, source: $source}' > "$output/scenario.json"

statuses=()
for role in stock owned; do
  extra=(--modal-lifetime-guard)
  [[ $role == owned ]] && extra=(--ram-push-modal-scoped --modal75-owned-archive "$inputs/engine.a"
    --modal75-retired "$inputs/retired.json" --menu-engine-owned --menu-value-owned)
  status=0
  env XM1_EMULATOR_SLOTS="${XM1_EMULATOR_SLOTS:-1}" XM1_EMULATOR_MEMORY_MAX=2G XM1_EMULATOR_SWAP_MAX=0 \
    XM1_RAM_PUSH_CHECKPOINT_ROOT=private-data/emulator-checkpoints/ram-push \
    devenv shell -- "$viewer" --workspace . --ram-push-e2e --cardless --fujimod --unreviewed-relink \
    --fujimod-run-document "$run_document" --firmware "$inputs/firmware.DAT" --installed-nand "$inputs/adjustment.nand" \
    --trajectories "$output/$route.$role.json" --output "$output/$role-runtime" --receipt-output "$output/$role" \
    "${extra[@]}" < /dev/null > "$output/$role-viewer.log" 2>&1 || status=$?
  statuses+=("$status")
  printf '%s %s exit=%s\n' "$route" "$role" "$status"
done

check=0
"$checker" --correspondence "$correspondence" --stock "$output/stock" --owned "$output/owned" \
  --output "$output/gate.json" > "$output/gate.log" 2>&1 || check=$?
jq -n --argjson stock "${statuses[0]}" --argjson owned "${statuses[1]}" --argjson gate "$check" \
  --slurpfile scenario "$output/scenario.json" \
  '$scenario[0] + {stock_exit: $stock, owned_exit: $owned, gate_exit: $gate}' > "$output/verdict.json"
printf '%s %s gate exit=%s\n' "$route" "$schedule" "$check"
exit "$check"
