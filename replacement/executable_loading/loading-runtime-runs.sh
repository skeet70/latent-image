#!/usr/bin/env bash
# Changed-path runs for the task60 loading owner: native harness, then the
# Root bank (saclac-image-loading-*) staged with the platform loaders it calls
# and observed from reset.
# Run from the worktree root under run-output exec:
#   loading-runtime-runs.sh CANONICAL_DAT CANONICAL_IMAGE4 CANONICAL_IMAGE5 BOOT_ROM STOCK_LOADER_A STOCK_LOADER_B
set -euo pipefail
: "${XM1_RUN_WORK:?}" "${XM1_RUN_RESULT:?}"
[ "$#" -eq 6 ] || { echo "usage: loading-runtime-runs.sh DAT IMAGE4 IMAGE5 BOOT_ROM STOCK_LOADER_A STOCK_LOADER_B" >&2; exit 2; }
export XM1_EMULATOR_SLOTS=2
DAT=$1 ID4=$2 ID5=$3 ROM=$(realpath "$4") STOCK_LOADER_A=$5 STOCK_LOADER_B=$6
RUN=emulation/fujimod-xm1/data/xm1-150.json
B=bazel-bin
L=$B/replacement/executable_loading
S=$B/replacement/saclac
P=$B/replacement/rtos_platform
CARRIERS=replacement/rtos_platform/carriers.json

"$B/emulation/emulator-slot" --native "$L/runtime-native-qualify" "$B/emulation/native-runner" \
  "$L/runtime-native.bin" "$L/runtime-native.symbols" "$L/runtime-retention.bin" "$L/runtime-retention.symbols" \
  "$XM1_RUN_RESULT/native.json" --native-jobs 1

"$B/host/firmware/build-first-stage" "$DAT" "$B/replacement/first_stage/first-stage-fixed-image.bin" \
  "$B/replacement/first_stage/first-stage-fixed-image.symbols" "$XM1_RUN_WORK/first-stage.DAT" \
  --load-offset 0x6000 --entry 0x6000 \
  --expect-slot-sha256 b943e4e7e666bc5c17a87e1aab76ca0811171ed00de5b4d7350857f9aa832dce

stage() {
  local policy=$1 scope=$2 timeout_ms=$3 out=$XM1_RUN_WORK/$1 image=$S/saclac-image-loading-$1
  mkdir -p "$out/artifact"
  "$S/stage-saclac" "$B/lz/lz-encode" "$B/host/firmware/rebuild-update" "$B/lz/fflzd" "$DAT" "$ID5" "$ID4" \
    "$image.elf" "$image.bin" "$image.symbols" "$image.disasm" "$image.sections" "$CARRIERS" "$XM1_RUN_WORK" \
    "$out/image5.bin" "$out/entry.DAT" "$out/saclac-contract.json" "$out/artifact/runtime.bin" "$out/publications.json"
  cp "$image.symbols" "$out/artifact/runtime.symbols"
  "$P/stage-platform" stage "$CARRIERS" \
    "$P/platform-a-loading-$1.bin" "$P/platform-a-loading-$1.symbols" "$P/platform-a-loading-$1.disasm" "$P/placement-a-loading-$1.json" \
    "$P/platform-b-loading-$1.bin" "$P/platform-b-loading-$1.symbols" "$P/platform-b-loading-$1.disasm" "$P/placement-b-loading-$1.json" \
    "$image.symbols" "$XM1_RUN_WORK/first-stage.DAT" "$STOCK_LOADER_A" "$STOCK_LOADER_B" "$out/platform"
  "$B/replacement/fixed_image/stage-fixed-image" "$B/lz/lz-encode" "$B/host/firmware/rebuild-update" "$B/lz/fflzd" \
    "$out/platform/candidate.DAT" "$out/artifact/runtime.bin" "$image.symbols" "$out/publications.json" \
    "$out/platform/bank-entries.json" "$CARRIERS" \
    "$B/replacement/first_stage/first-stage-fixed-image.symbols" "$out/candidate.DAT"
  "$L/runtime-artifact" contract "$B/lz/fflzd" "$DAT" "$out/platform/candidate.DAT" "$ID4" \
    "$out/artifact/runtime.bin" "$image.symbols" "$out/publications.json" "$out/candidate.DAT" \
    "$scope" "$timeout_ms" "$out/artifact/runtime-contract.json"
  cp "$out/platform/bank-entries.json" "$XM1_RUN_RESULT/$policy-bank-entries.json"
  for name in saclac-contract.json publications.json candidate.DAT.json; do cp "$out/$name" "$XM1_RUN_RESULT/$policy-$name"; done
  cp "$out/artifact/runtime-contract.json" "$XM1_RUN_RESULT/$policy-runtime-contract.json"
}

observe() {
  local policy=$1 out=$XM1_RUN_WORK/$1 status=0
  "$B/emulation/emulator-slot" "$PWD/$B/emulation/loading-runtime-observer" --run "$PWD/$RUN" \
    --update "$out/candidate.DAT" --boot-rom "$ROM" --artifact "$out/artifact" \
    --output "$out/observed" > "$XM1_RUN_RESULT/$policy-observer.log" 2>&1 || status=$?
  echo "$status" > "$XM1_RUN_RESULT/$policy-observer.status"
  if [ -f "$out/observed/receipt.json" ]; then cp "$out/observed/receipt.json" "$XM1_RUN_RESULT/$policy-receipt.json"; fi
  if [ -f "$out/observed/failure.json" ]; then cp "$out/observed/failure.json" "$XM1_RUN_RESULT/$policy-failure.json"; fi
  return "$status"
}

# Success links the Root startup caller policy (30000 ms); retention links its explicit 22 ms test input.
stage fujimod fujimod-3d90035d 30000
stage retention synthetic-retention-control 22
fail=0
observe fujimod || fail=1
observe retention || fail=1
exit "$fail"
