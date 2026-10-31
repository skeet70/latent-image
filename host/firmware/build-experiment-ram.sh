#!/usr/bin/env bash
# Host-only relink of experiments/NAME at BASE, run from the workspace root by
# //transport:camera and the emulator with Bazel-provided tool paths.
set -euo pipefail

BASE="${1:?usage: build-experiment-ram.sh BASE OUT_DIR}"
OUT="${2:?usage: build-experiment-ram.sh BASE OUT_DIR}"
BUILDER="${RAM_PUSH_BUILDER:?}"
CLANG="${RAM_PUSH_CLANG:?}"
LD_LLD="${RAM_PUSH_LD_LLD:?}"
OBJCOPY="${RAM_PUSH_OBJCOPY:?}"
NM="${RAM_PUSH_NM:?}"
OBJDUMP="${RAM_PUSH_OBJDUMP:?}"
RUSTC="${RUSTC:?}"
EXPERIMENT="${EXPERIMENT:?}"
STOCK_ID4="${STOCK_ID4:?}"
STOCK_ID5="${STOCK_ID5:?}"
STOCK_PLAIN="${STOCK_PLAIN:?}"

case "$OUT" in
  private-data/analysis/x-m1-1.50/experiments/ram-push/*) ;;
  *) printf 'experiment output must stay under its private RAM-push build directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }
[[ -f "$EXPERIMENT/hooks.json" ]] || { printf 'no experiment at %s: it needs a hooks.json\n' "$EXPERIMENT" >&2; exit 1; }

read -r count setup activation abi < <("$BUILDER" --abi)
"$BUILDER" --trampolines "$EXPERIMENT" "$STOCK_ID4" "$STOCK_ID5" "$STOCK_PLAIN" "$OUT/trampolines.S"

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm -mfloat-abi=soft)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access -fno-stack-protector)
objects=("$OUT/trampolines.o")
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c "$OUT/trampolines.S" -o "$OUT/trampolines.o"
shopt -s nullglob
for src in "$EXPERIMENT"/*.c "$EXPERIMENT"/*.S; do
  name="${src##*/}"
  obj="$OUT/experiment-${name%.*}.o"
  case "$src" in
    *.c) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra -Iexperiments/sdk \
      -ffunction-sections -fdata-sections -c "$src" -o "$obj" ;;
    *.S) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -Iexperiments/sdk -c "$src" -o "$obj" ;;
  esac
  objects+=("$obj")
done
if [[ -f "$EXPERIMENT/lib.rs" ]]; then
  "$RUSTC" --crate-name=experiment --edition=2021 --crate-type=staticlib -C opt-level=z \
    --target=armv7r-none-eabi -C panic=abort -C relocation-model=static -C target-cpu=cortex-r4f \
    -C codegen-units=1 -C debuginfo=0 "$EXPERIMENT/lib.rs" -o "$OUT/libexperiment.a"
  objects+=("$OUT/libexperiment.a")
fi

NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -DRAM_PUSH_PUBLICATION_COUNT="${count}u" -DRAM_PUSH_SETUP_COUNT="${setup}u" \
  -DRAM_PUSH_ACTIVATION_COUNT="${activation}u" -DRAM_PUSH_ABI_VERSION="${abi}u" \
  -ffunction-sections -fdata-sections -c menu/ram_push_installer.c -o "$OUT/ram_push_installer.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" \
  -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,experiments/sdk/experiment.ld \
  -Wl,-Map,"$OUT/kernel.map" -Wl,--defsym=RAM_PUSH_BASE="$BASE" \
  -Wl,--defsym=RAM_PUSH_PUBLICATION_COUNT="$count" \
  "${objects[@]}" "$OUT/ram_push_installer.o" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$NM" --defined-only --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"
"$BUILDER" "$BASE" "$OUT" "$OUT" "$EXPERIMENT" "$STOCK_ID4" "$STOCK_ID5" "$STOCK_PLAIN"
