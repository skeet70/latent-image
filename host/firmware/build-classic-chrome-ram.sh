#!/usr/bin/env bash
# Host-only base-relinked Classic Chrome RAM-push build
# (reports/classic-chrome-patch.md section 8.2): menu/classic_chrome_ram.S and
# the generic installer, compiled against menu/ram_push_abi.h with this
# payload's publication counts and ABI version; no recipe-bank object. Run by
# FujiAF.RamPush from the workspace root with Bazel-provided tool paths.
set -euo pipefail

BASE="${1:?usage: build-classic-chrome-ram.sh BASE OUT_DIR}"
OUT="${2:?usage: build-classic-chrome-ram.sh BASE OUT_DIR}"
BUILDER="${RAM_PUSH_BUILDER:?}"
CLANG="${RAM_PUSH_CLANG:?}"
LD_LLD="${RAM_PUSH_LD_LLD:?}"
OBJCOPY="${RAM_PUSH_OBJCOPY:?}"
NM="${RAM_PUSH_NM:?}"
OBJDUMP="${RAM_PUSH_OBJDUMP:?}"
PARAMETERS="${CC_PARAMETERS:?}"
RC0_ID4="${RC0_ID4:?}"
RC0_ID5="${RC0_ID5:?}"
RC0_PLAIN="${RC0_PLAIN:?}"
RC0_ID2="${RC0_ID2:?}"
CC_ID4="${CC_ID4:?}"
CC_ID5="${CC_ID5:?}"
CC_PLAIN="${CC_PLAIN:?}"

case "$OUT" in
  private-data/analysis/classic-chrome-patch/ram-push/*) ;;
  *) printf 'output must stay under the host-only Classic Chrome RAM-push directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }

base_value=$((BASE))
(( base_value >= 0 && base_value <= 0xffff0000 )) || {
  printf 'base leaves the 32-bit RAM address space\n' >&2; exit 1;
}
(( (base_value & 7) == 0 )) || {
  printf 'base must be 8-byte aligned\n' >&2; exit 1;
}

read -r count setup activation abi < <("$BUILDER" --abi)

# Copies SOURCE to TARGET with each OLD line replaced by its NEW line; every
# OLD line must occur exactly once.
substitute() {
  local source="$1" target="$2"
  shift 2
  local -a olds=() news=() hits=()
  while (( $# )); do olds+=("$1"); news+=("$2"); hits+=(0); shift 2; done
  local line index replaced
  : > "$target"
  while IFS= read -r line || [[ -n "$line" ]]; do
    replaced=0
    for index in "${!olds[@]}"; do
      if [[ "$line" == "${olds[$index]}" ]]; then
        printf '%s\n' "${news[$index]}" >> "$target"
        hits[$index]=$(( hits[index] + 1 ))
        replaced=1
      fi
    done
    (( replaced )) || printf '%s\n' "$line" >> "$target"
  done < "$source"
  for index in "${!olds[@]}"; do
    (( hits[index] == 1 )) || {
      printf '%s: "%s" matched %d lines\n' "$source" "${olds[$index]}" "${hits[$index]}" >&2; exit 1;
    }
  done
}

installer="$OUT/installer"
mkdir "$installer"
cp menu/ram_push_installer.c "$installer/ram_push_installer.c"
substitute menu/ram_push_abi.h "$installer/ram_push_abi.h" \
  '#define RAM_PUSH_PUBLICATION_COUNT 250u' "#define RAM_PUSH_PUBLICATION_COUNT ${count}u" \
  '#define RAM_PUSH_SETUP_COUNT 167u' "#define RAM_PUSH_SETUP_COUNT ${setup}u" \
  '#define RAM_PUSH_ACTIVATION_COUNT 83u' "#define RAM_PUSH_ACTIVATION_COUNT ${activation}u" \
  '#define RAM_PUSH_ABI_VERSION 2u' "#define RAM_PUSH_ABI_VERSION ${abi}u"

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
LINK=(-nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" -Wl,--no-dynamic-linker -Wl,--build-id=none)
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c menu/classic_chrome_ram.S -o "$OUT/classic_chrome_ram.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections -c "$installer/ram_push_installer.c" \
  -o "$OUT/ram_push_installer.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/classic_chrome_ram_runtime.ld \
  -Wl,menu/classic_chrome_ram_stock.ld -Wl,-Map,"$OUT/kernel.map" \
  -Wl,--defsym=RAM_PUSH_BASE="$BASE" -Wl,--defsym=RAM_PUSH_PUBLICATION_COUNT="$count" \
  "$OUT/classic_chrome_ram.o" "$OUT/ram_push_installer.o" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$NM" --defined-only --print-size --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"

"$BUILDER" "$BASE" "$PARAMETERS" "$OUT" "$OUT" \
  "$RC0_ID4" "$RC0_ID5" "$RC0_PLAIN" "$RC0_ID2" "$CC_ID4" "$CC_ID5" "$CC_PLAIN"
printf 'runtime: %s/runtime.bin\nmanifest: %s/ram-push-manifest.json\nsymbols: %s/kernel.syms\n' "$OUT" "$OUT" "$OUT"
