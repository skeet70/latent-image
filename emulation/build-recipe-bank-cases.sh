#!/usr/bin/env bash
# build-recipe-bank-cases.sh: compile the recipe-bank kernel blob and the
# runtime model, then run the case generator. Run under `devenv shell --`.
set -euo pipefail
cd "$(dirname "$0")/.."

RB_NAME_RECORD="${RB_NAME_RECORD:-0x011ac900u}"
BUILD_DIR="${BUILD_DIR:-/tmp/recipe-bank-cases}"
OUT_DIR="${OUT_DIR:-private-data/analysis/x-m1-1.50/recipes/runtime/kernel-cases}"
RUNNER="${RUNNER:-bazel-bin/emulation/native-runner}"
GEN="${GEN:-bazel-bin/emulation/recipe-bank-cases}"

mkdir -p "$BUILD_DIR"

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREESTANDING=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
HARDEN=NIX_HARDENING_ENABLE=

# 1. bank blob. Boot and compiled-recipe hooks have separate linker layouts.
OBJS=()
for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c \
  menu/recipe_bank_hook.c menu/recipe_bank_storage.c \
  menu/recipe_bank_actions.c menu/recipe_bank_edit.c; do
  obj="$BUILD_DIR/$(basename "${src%.*}").o"
  case "$src" in
    *.c) env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
      -DRB_NAME_RECORD="$RB_NAME_RECORD" -c "$src" -o "$obj" ;;
    *.S) env $HARDEN clang "${ARM[@]}" -c "$src" -o "$obj" ;;
  esac
  OBJS+=("$obj")
done
LINK_REPORT="$BUILD_DIR/link-report.txt"
: > "$LINK_REPORT"
link_blob() {
  env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
    -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,menu/recipe_bank.ld \
    -Wl,--defsym=RB_BASE=0x011dcca0 "$@" -o "$BUILD_DIR/kernel.elf" 2>> "$LINK_REPORT"
}
link_blob "${OBJS[@]}"
llvm-objcopy -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel.bin"
llvm-objcopy -j .text -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel-text.bin"
llvm-nm --print-size --numeric-sort "$BUILD_DIR/kernel.elf" > "$BUILD_DIR/kernel.syms"
sha256sum "$BUILD_DIR/kernel.bin"

# 2. model: linked against the measured kernel symbols its drivers call
DEFSYMS=()
for sym in rb_load rb_recall rb_save_current rb_stage_load rb_stage_field rb_stage_cancel \
  rb_stage_commit rb_readback rb_media_epoch_bump rb_bank_interpose; do
  addr=$(awk -v s="$sym" '$4 == s {print "0x" $1; exit}' "$BUILD_DIR/kernel.syms")
  if [ -z "$addr" ]; then
    echo "kernel symbol missing: $sym" >&2
    exit 1
  fi
  DEFSYMS+=("-Wl,--defsym=$sym=$addr")
done
env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections \
  -c emulation/recipe-bank-model.c -o "$BUILD_DIR/model.o"
env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
  -Wl,--no-dynamic-linker -Wl,--build-id=none \
  -Wl,-T,emulation/recipe-bank-model.ld "${DEFSYMS[@]}" "$BUILD_DIR/model.o" \
  -o "$BUILD_DIR/model.elf"
llvm-objcopy -O binary "$BUILD_DIR/model.elf" "$BUILD_DIR/model.bin"
llvm-nm --print-size --numeric-sort "$BUILD_DIR/model.elf" > "$BUILD_DIR/model.syms"
for pair in getter:getter setter:setter lowclose:rbm_low_close read:rbm_read \
  write:rbm_write close:rbm_close open:rbm_open stat:rbm_stat \
  indexfree:rbm_index_free mapping:rbm_mapping_delete resolve:rbm_resolve \
  tagrelease:rbm_tag_release helpers:helpers; do
  sec="${pair%%:*}"
  name="${pair##*:}"
  llvm-objcopy -j ".text.$sec" -O binary "$BUILD_DIR/model.elf" \
    "$BUILD_DIR/model-$name.bin" 2>/dev/null || rm -f "$BUILD_DIR/model-$name.bin"
done
llvm-objcopy -j .data -O binary "$BUILD_DIR/model.elf" "$BUILD_DIR/model-data.bin"
# the helpers span is an output section, not one symbol: record it under the
# name the generator looks up
read -r HADDR HSIZE <<< "$(llvm-readelf -S "$BUILD_DIR/model.elf" | awk '/\.text\.helpers/ {for (i = 1; i <= NF; i++) if ($i == "PROGBITS") {print $(i + 1), $(i + 3); exit}}')"
printf '%s %s T helpers\n' "$HADDR" "$HSIZE" >> "$BUILD_DIR/model.syms"
sha256sum "$BUILD_DIR/model.bin"

# 3. generate and run the cases
mkdir -p "$OUT_DIR"
"$GEN" --runner "$RUNNER" --build "$BUILD_DIR" --out "$OUT_DIR" \
  --fields reports/custom-settings.fields.json \
  --id4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  --id5 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  --candidate "${CANDIDATE:-private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate}" \
  --host-fixtures private-data/analysis/x-m1-1.50/recipes/runtime/host-fixtures
