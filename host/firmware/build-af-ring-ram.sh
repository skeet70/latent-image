#!/usr/bin/env bash
set -euo pipefail
BASE="${1:?usage: build-af-ring-ram.sh BASE OUT_DIR}"
OUT="${2:?usage: build-af-ring-ram.sh BASE OUT_DIR}"
BUILDER="${RAM_PUSH_BUILDER:?}"
CLANG="${RAM_PUSH_CLANG:?}"
LD_LLD="${RAM_PUSH_LD_LLD:?}"
OBJCOPY="${RAM_PUSH_OBJCOPY:?}"
NM="${RAM_PUSH_NM:?}"
OBJDUMP="${RAM_PUSH_OBJDUMP:?}"
RC0_ID5="${RC0_ID5:?}"
case "$OUT" in
  private-data/analysis/x-m1-1.50/af-ring/ram-push/*) ;;
  *) printf 'AF output must stay under its private RAM-push build directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }
read -r count setup activation abi < <("$BUILDER" --abi)
ARM=(--target=armv7r-none-eabi -march=armv7-r -marm -mfloat-abi=soft)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access -fno-stack-protector)
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c menu/af_ring_hooks.S -o "$OUT/af_ring_hooks.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections -c menu/af_ring_payload.c -o "$OUT/af_ring_payload.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -DRAM_PUSH_PUBLICATION_COUNT="${count}u" -DRAM_PUSH_SETUP_COUNT="${setup}u" \
  -DRAM_PUSH_ACTIVATION_COUNT="${activation}u" -DRAM_PUSH_ABI_VERSION="${abi}u" \
  -ffunction-sections -fdata-sections -c menu/ram_push_installer.c -o "$OUT/ram_push_installer.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" \
  -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,menu/af_ring_ram.ld \
  -Wl,-Map,"$OUT/kernel.map" -Wl,--defsym=RAM_PUSH_BASE="$BASE" \
  -Wl,--defsym=RAM_PUSH_PUBLICATION_COUNT="$count" \
  "$OUT/af_ring_hooks.o" "$OUT/af_ring_payload.o" "$OUT/ram_push_installer.o" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$NM" --defined-only --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"
"$BUILDER" "$BASE" "$OUT" "$OUT" "$RC0_ID5"
