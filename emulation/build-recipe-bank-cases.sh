#!/usr/bin/env bash
# Host-only bounded recipe kernel/D3 cases. Run under `devenv shell --`.
set -euo pipefail
cd "$(dirname "$0")/.."

RB_BASE="${RB_BASE:-0x00be1f00}"
RB_NAME_RECORD="${RB_NAME_RECORD:-0x00beb784u}"
BUILD_DIR="${BUILD_DIR:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/kernel-build}"
OUT_DIR="${OUT_DIR:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/kernel-cases}"
KERNEL_DIR="${KERNEL_DIR:-}"
RUNNER="${RUNNER:-bazel-bin/emulation/native-runner}"
GEN="${GEN:-bazel-bin/emulation/recipe-bank-cases}"
CANDIDATE="${CANDIDATE:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package/image}"
ID4="${ID4:-$CANDIDATE/id4-final.bin}"
ID4_BASE="${ID4_BASE:-0x0061c1fc}"
ID5="${ID5:-$CANDIDATE/id5-final.bin}"
HOST_FIXTURES="${HOST_FIXTURES:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/host-fixtures}"
FIELDS="${FIELDS:-reports/custom-settings.fields.json}"
mkdir -p "$BUILD_DIR" "$OUT_DIR"
ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREESTANDING=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
HARDEN=NIX_HARDENING_ENABLE=

if [ -n "$KERNEL_DIR" ]; then
  for file in kernel.bin kernel-text.bin kernel.syms; do
    if [ "$KERNEL_DIR" != "$BUILD_DIR" ]; then cp "$KERNEL_DIR/$file" "$BUILD_DIR/$file"; fi
  done
else
  OBJS=()
  for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c \
    menu/recipe_bank_storage.c menu/recipe_bank_actions.c menu/recipe_bank_edit.c; do
    obj="$BUILD_DIR/$(basename "${src%.*}").o"
    case "$src" in
      *.c) env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
        -DRB_NAME_RECORD="$RB_NAME_RECORD" -c "$src" -o "$obj" ;;
      *.S) env $HARDEN clang "${ARM[@]}" -c "$src" -o "$obj" ;;
    esac
    OBJS+=("$obj")
  done
  env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
    -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,menu/recipe_bank.ld \
    -Wl,--defsym=RB_BASE="$RB_BASE" "${OBJS[@]}" -o "$BUILD_DIR/kernel.elf"
  llvm-objcopy -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel.bin"
  llvm-objcopy -j .text -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel-text.bin"
  llvm-nm --print-size --numeric-sort "$BUILD_DIR/kernel.elf" > "$BUILD_DIR/kernel.syms"
fi
sha256sum "$BUILD_DIR/kernel.bin"

env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections \
  -c emulation/recipe-bank-model.c -o "$BUILD_DIR/model.o"
DEFSYMS=()
while read -r sym; do
  [ -z "$sym" ] && continue
  addr=$(awk -v s="$sym" '$4 == s {print "0x" $1; exit}' "$BUILD_DIR/kernel.syms")
  if [ -z "$addr" ]; then echo "kernel symbol missing: $sym" >&2; exit 1; fi
  DEFSYMS+=("-Wl,--defsym=$sym=$addr")
done < <(llvm-nm --undefined-only "$BUILD_DIR/model.o" | awk '{print $2}')
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
  llvm-objcopy -j ".text.${pair%%:*}" -O binary "$BUILD_DIR/model.elf" \
    "$BUILD_DIR/model-${pair##*:}.bin"
done
llvm-objcopy -j .data -O binary "$BUILD_DIR/model.elf" "$BUILD_DIR/model-data.bin"
read -r HADDR HSIZE <<< "$(llvm-readelf -S "$BUILD_DIR/model.elf" | awk '/\.text\.helpers/ {for (i = 1; i <= NF; i++) if ($i == "PROGBITS") {print $(i + 1), $(i + 3); exit}}')"
printf '%s %s T helpers\n' "$HADDR" "$HSIZE" >> "$BUILD_DIR/model.syms"
sha256sum "$BUILD_DIR/model.bin"

BOOT_ARGS=()
if [ -n "${BOOT_PACKAGE:-}" ]; then BOOT_ARGS=(--boot-package "$BOOT_PACKAGE"); fi
"$GEN" --runner "$RUNNER" --build "$BUILD_DIR" --out "$OUT_DIR" \
  --rb-base "$RB_BASE" --fields "$FIELDS" --id4 "$ID4" --id4-base "$ID4_BASE" \
  --id5 "$ID5" --candidate "$CANDIDATE" --host-fixtures "$HOST_FIXTURES" "${BOOT_ARGS[@]}"
