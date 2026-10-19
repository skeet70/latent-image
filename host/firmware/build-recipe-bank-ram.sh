#!/usr/bin/env bash
# Host-only base-relinked recipe-bank and installer build, run by
# `//transport:camera` from the workspace root with Bazel-provided tool paths.
set -euo pipefail

BASE="${1:?usage: build-recipe-bank-ram.sh BASE OUT_DIR}"
OUT="${2:?usage: build-recipe-bank-ram.sh BASE OUT_DIR}"
BUILDER="${RAM_PUSH_BUILDER:?}"
CLANG="${RAM_PUSH_CLANG:?}"
LD_LLD="${RAM_PUSH_LD_LLD:?}"
OBJCOPY="${RAM_PUSH_OBJCOPY:?}"
NM="${RAM_PUSH_NM:?}"
OBJDUMP="${RAM_PUSH_OBJDUMP:?}"
SPEC="${SPEC:?}"
RC0_ID4="${RC0_ID4:?}"
RC0_ID5="${RC0_ID5:?}"
RC0_PLAIN="${RC0_PLAIN:?}"

case "$OUT" in
  private-data/analysis/x-m1-1.50/ram-push/*) ;;
  *) printf 'output must stay under the host-only RAM-push directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }

base_value=$((BASE))
(( base_value >= 0 && base_value <= 0xffffffff - 0x1b6d8 )) || {
  printf 'base leaves the 32-bit RAM address space\n' >&2; exit 1;
}
(( (base_value & 7) == 0 )) || {
  printf 'base must be 8-byte aligned\n' >&2; exit 1;
}
name_record=$(((base_value & 0xffff0000) | 0xb784))
if (( name_record < base_value + 0x9800 )); then
  name_record=$((name_record + 0x10000))
fi
bank_end=$((name_record + 14 * 66))
printf -v rb_name_record '0x%08xu' "$name_record"
printf -v rb_bank_end '0x%08x' "$bank_end"

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
LINK=(-nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" -Wl,--no-dynamic-linker -Wl,--build-id=none)
objects=()
for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c \
  menu/recipe_bank_storage.c menu/recipe_bank_actions.c menu/recipe_bank_edit.c; do
  name="${src##*/}"
  obj="$OUT/${name%.*}.o"
  case "$src" in
    *.c) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
      -DRB_NAME_RECORD="$rb_name_record" -c "$src" -o "$obj" ;;
    *.S) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c "$src" -o "$obj" ;;
  esac
  objects+=("$obj")
done
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections -c menu/ram_push_installer.c \
  -o "$OUT/ram_push_installer.o"
objects+=("$OUT/ram_push_installer.o")
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank_ram.ld \
  -Wl,--defsym=RAM_PUSH_BASE="$BASE" -Wl,--defsym=RAM_PUSH_BANK_END="$rb_bank_end" \
  "${objects[@]}" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$NM" --defined-only --print-size --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"

NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c menu/recipe_bank_boot.S -o "$OUT/boot.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank_boot.ld \
  -Wl,--defsym=RB_SRC_BASE=0x011ad800 -Wl,--defsym=RB_SRC_END=0x011b7940 \
  -Wl,--defsym=RB_SRC_LIMIT=0x000b7940 -Wl,--defsym=RB_STORE_ENTRY=0x011adca0 \
  "$OUT/boot.o" -o "$OUT/boot.elf"
"$OBJCOPY" -j .text.rb_boot_store -O binary "$OUT/boot.elf" "$OUT/store.bin"

"$BUILDER" --ram-push "$BASE" "$SPEC" \
  "$OUT/bank-plan.json" "$OUT" "$OUT" "$RC0_ID4" "$RC0_ID5" "$RC0_PLAIN"
printf 'runtime: %s/runtime.bin\nmanifest: %s/ram-push-manifest.json\n' "$OUT" "$OUT"
