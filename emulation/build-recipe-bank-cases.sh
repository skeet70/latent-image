#!/usr/bin/env bash
# Host-only bounded recipe kernel/D3 cases. Run under `devenv shell --`.
# RB_MODE=volatile compiles the storage unit with RB_VOLATILE_BANK=1.
set -euo pipefail
cd "$(dirname "$0")/.."

RB_MODE="${RB_MODE:-sd}"
case "$RB_MODE" in
  sd|volatile) ;;
  *) echo "RB_MODE must be sd or volatile, not '$RB_MODE'" >&2; exit 2 ;;
esac
if [ "$RB_MODE" = volatile ]; then MODE_SUFFIX=-volatile; else MODE_SUFFIX=; fi
RB_FILM_ENCODING="${RB_FILM_ENCODING:-d3}"
FILM_DEFS=()
case "$RB_FILM_ENCODING" in
  d3) ;;
  classic-chrome) FILM_DEFS=(-DRB_CC_BANK=1); MODE_SUFFIX="$MODE_SUFFIX-cc" ;;
  *) echo "RB_FILM_ENCODING must be d3 or classic-chrome" >&2; exit 2 ;;
esac
RB_BASE="${RB_BASE:-0x00be1f00}"
RB_NAME_RECORD="${RB_NAME_RECORD:-0x00beb784u}"
BUILD_DIR="${BUILD_DIR:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/kernel-build$MODE_SUFFIX}"
OUT_DIR="${OUT_DIR:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/kernel-cases$MODE_SUFFIX}"
KERNEL_DIR="${KERNEL_DIR:-}"
if [ "$RB_MODE" = volatile ] && [ -n "$KERNEL_DIR" ]; then
  echo "RB_MODE=volatile builds its own kernel; KERNEL_DIR must be unset" >&2
  exit 2
fi
if [ "$RB_FILM_ENCODING" = classic-chrome ] && { [ "$RB_MODE" != sd ] || [ -n "$KERNEL_DIR" ]; }; then
  echo "Classic Chrome compatibility cases build their own SD kernel" >&2
  exit 2
fi
RUNNER="${RUNNER:-bazel-bin/emulation/native-runner}"
GEN="${GEN:-bazel-bin/emulation/recipe-bank-cases}"
INTEROP="${INTEROP:-bazel-bin/emulation/recipe-bank-interop}"
JOBS="${JOBS:-8}"
CANDIDATE="${CANDIDATE:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-20261024b/image}"
ID4="${ID4:-$CANDIDATE/id4-final.bin}"
ID4_BASE="${ID4_BASE:-0x0061c1fc}"
ID5="${ID5:-$CANDIDATE/id5-final.bin}"
FIELDS="${FIELDS:-reports/custom-settings.fields.json}"
mkdir -p "$BUILD_DIR" "$OUT_DIR"
if [ -z "${HOST_FIXTURES:-}" ]; then
  HOST_FIXTURES="$BUILD_DIR/host-fixtures"
  "$INTEROP" encode "$HOST_FIXTURES"
fi
ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREESTANDING=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
HARDEN=NIX_HARDENING_ENABLE=

if [ -n "$KERNEL_DIR" ]; then
  for file in kernel.elf kernel.bin kernel-text.bin kernel.syms; do
    if [ "$KERNEL_DIR" != "$BUILD_DIR" ]; then cp "$KERNEL_DIR/$file" "$BUILD_DIR/$file"; fi
  done
else
  OBJS=()
  for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c \
    menu/recipe_bank_storage.c menu/recipe_bank_actions.c menu/recipe_bank_edit.c; do
    obj="$BUILD_DIR/$(basename "${src%.*}").o"
    volatile_def=()
    if [ "$RB_MODE" = volatile ] && [ "$src" = menu/recipe_bank_storage.c ]; then
      volatile_def=(-DRB_VOLATILE_BANK=1)
    fi
    case "$src" in
      *.c) env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
        -DRB_NAME_RECORD="$RB_NAME_RECORD" "${volatile_def[@]}" "${FILM_DEFS[@]}" -c "$src" -o "$obj" ;;
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
llvm-readelf --symbols --wide "$BUILD_DIR/kernel.elf" > "$BUILD_DIR/kernel.symbols"
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
llvm-readelf --symbols --wide "$BUILD_DIR/model.elf" > "$BUILD_DIR/model.symbols"
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
CASE_ARGS=()
if [ -n "${CASE:-}" ]; then CASE_ARGS=(--case "$CASE"); fi
"$GEN" --runner "$RUNNER" --build "$BUILD_DIR" --out "$OUT_DIR" --jobs "$JOBS" \
  --rb-base "$RB_BASE" --fields "$FIELDS" --id4 "$ID4" --id4-base "$ID4_BASE" \
  --id5 "$ID5" --candidate "$CANDIDATE" --host-fixtures "$HOST_FIXTURES" \
  --bank-mode "$RB_MODE" --film-encoding "$RB_FILM_ENCODING" "${BOOT_ARGS[@]}" "${CASE_ARGS[@]}"
