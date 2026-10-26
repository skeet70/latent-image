#!/usr/bin/env bash
set -euo pipefail
umask 077
BASE="${1:?usage: build-usb-toggle-ram.sh BASE OUT_DIR}"
OUT="${2:?usage: build-usb-toggle-ram.sh BASE OUT_DIR}"
BUILDER="${RAM_PUSH_BUILDER:?}"
CLANG="${RAM_PUSH_CLANG:?}"
LD_LLD="${RAM_PUSH_LD_LLD:?}"
OBJCOPY="${RAM_PUSH_OBJCOPY:?}"
NM="${RAM_PUSH_NM:?}"
OBJDUMP="${RAM_PUSH_OBJDUMP:?}"
RC0_ID5="${RC0_ID5:?}"
STUB="${USB_TOGGLE_STUB:?}"
SYMBOLS="${USB_TOGGLE_STUB_SYMBOLS:?}"
case "$OUT" in
  private-data/analysis/usb-toggle/ram-push/*) ;;
  *) printf 'USB toggle output must stay under its private RAM-push build directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }
read -r count setup activation abi < <("$BUILDER" --abi)
[[ "$count $setup $activation $abi" == '23 22 1 6' ]] || { printf 'USB toggle installer ABI differs\n' >&2; exit 1; }
ARM=(--target=armv7r-none-eabi -march=armv7-r -marm -mfloat-abi=soft)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access -fno-stack-protector)
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -DRAM_PUSH_PUBLICATION_COUNT="${count}u" -DRAM_PUSH_SETUP_COUNT="${setup}u" \
  -DRAM_PUSH_ACTIVATION_COUNT="${activation}u" -DRAM_PUSH_ABI_VERSION="${abi}u" \
  -ffunction-sections -fdata-sections -c menu/ram_push_installer.c -o "$OUT/ram_push_installer.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" \
  -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,host/firmware/usb-toggle-ram.ld \
  -Wl,-Map,"$OUT/kernel.map" -Wl,--defsym=RAM_PUSH_BASE="$BASE" \
  -Wl,--defsym=RAM_PUSH_PUBLICATION_COUNT="$count" \
  "$OUT/ram_push_installer.o" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$NM" --defined-only --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"
"$BUILDER" "$BASE" "$OUT" "$OUT" "$RC0_ID5" "$STUB" "$SYMBOLS"
