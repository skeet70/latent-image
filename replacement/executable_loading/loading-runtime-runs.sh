#!/usr/bin/env bash
# Changed-path runs for the task60 loading owner: native harness, then the
# Root bank (saclac-image-loading-*) staged and observed from reset.
# Run from the worktree root under run-output exec:
#   loading-runtime-runs.sh CANONICAL_DAT CANONICAL_IMAGE4 CANONICAL_IMAGE5 BOOT_ROM
set -euo pipefail
: "${XM1_RUN_WORK:?}" "${XM1_RUN_RESULT:?}"
[ "$#" -eq 4 ] || { echo "usage: loading-runtime-runs.sh DAT IMAGE4 IMAGE5 BOOT_ROM" >&2; exit 2; }
export XM1_EMULATOR_SLOTS=1
DAT=$1 ID4=$2 ID5=$3 ROM=$(realpath "$4")
RUN=emulation/fujimod-xm1/data/xm1-150.json
B=bazel-bin
L=$B/replacement/executable_loading
S=$B/replacement/saclac

"$B/emulation/emulator-slot" --native "$L/runtime-native-qualify" "$B/emulation/native-runner" \
  "$L/runtime-native.bin" "$L/runtime-native.symbols" "$L/runtime-retention.bin" "$L/runtime-retention.symbols" \
  "$XM1_RUN_RESULT/native.json" --native-jobs 1

"$B/host/firmware/build-first-stage" "$DAT" "$B/replacement/first_stage/first-stage-fixed-image.bin" \
  "$B/replacement/first_stage/first-stage-fixed-image.symbols" "$XM1_RUN_WORK/first-stage.DAT" \
  --load-offset 0x6000 --entry 0x6000 \
  --expect-slot-sha256 b943e4e7e666bc5c17a87e1aab76ca0811171ed00de5b4d7350857f9aa832dce

stage() {
  local policy=$1 scope=$2 ticks=$3 out=$XM1_RUN_WORK/$1 image=$S/saclac-image-loading-$1
  mkdir -p "$out/artifact"
  "$S/stage-saclac" "$B/lz/lz-encode" "$B/host/firmware/rebuild-update" "$B/lz/fflzd" "$DAT" "$ID5" "$ID4" \
    "$image.elf" "$image.bin" "$image.symbols" "$image.disasm" "$image.sections" "$XM1_RUN_WORK" \
    "$out/image5.bin" "$out/entry.DAT" "$out/saclac-contract.json" "$out/artifact/runtime.bin" "$out/publications.json"
  cp "$image.symbols" "$out/artifact/runtime.symbols"
  "$B/replacement/fixed_image/stage-fixed-image" "$B/lz/lz-encode" "$B/host/firmware/rebuild-update" "$B/lz/fflzd" \
    "$XM1_RUN_WORK/first-stage.DAT" "$out/artifact/runtime.bin" "$image.symbols" "$out/publications.json" \
    "$B/replacement/first_stage/first-stage-fixed-image.symbols" "$out/candidate.DAT"
  "$L/runtime-artifact" contract "$B/lz/fflzd" "$DAT" "$XM1_RUN_WORK/first-stage.DAT" "$ID4" \
    "$out/artifact/runtime.bin" "$image.symbols" "$out/publications.json" "$out/candidate.DAT" \
    "$scope" "$ticks" "$out/artifact/runtime-contract.json"
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

stage fujimod fujimod-3d90035d 4294967296
stage retention synthetic-retention-control 65536
fail=0
observe fujimod || fail=1
observe retention || fail=1
exit "$fail"
