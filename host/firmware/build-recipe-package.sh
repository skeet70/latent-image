#!/usr/bin/env bash
# Host-only executable recipe image composition. Run from the repository root under devenv.
set -euo pipefail

OUT="${1:-private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package}"
STOCK="${STOCK:-private-data/firmware/x-m1-1.50/FWUP0002.DAT}"
SPEC="${SPEC:-private-data/analysis/x-m1-1.50/recipes/bank-spec.json}"
D3_SPEC="${D3_SPEC:-private-data/analysis/x-m1-1.50/recipes/d3-compiled.json}"
ID4=private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
ID5=private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
ID2=private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin
case "$OUT" in
  private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/*) ;;
  *) printf 'output must stay under the host-only RC completion directory\n' >&2; exit 1 ;;
esac
mkdir -p "$OUT" "$OUT/kernel" "$OUT/boot" "$OUT/d3" "$OUT/image"
read -r stock_digest _ < <(sha256sum "$STOCK")
[[ "$stock_digest" == 43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490 ]] || {
  printf 'pinned stock DAT required\n' >&2; exit 1;
}

if [[ "${BUILD_TOOLS:-1}" == 1 ]]; then
  bazel build //host/firmware:build-recipe-bank //host/firmware:build-compiled-recipes \
    //host/firmware:apply-patch-set //host/firmware:rebuild-update //lz:lz-encode //lz:fflzd
fi
TOOLS="${TOOLS:-bazel-bin}"
ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
LINK=(-nostdlib -static -fuse-ld=lld -Wl,--no-dynamic-linker -Wl,--build-id=none)
objects=()
for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c \
  menu/recipe_bank_storage.c menu/recipe_bank_actions.c menu/recipe_bank_edit.c; do
  obj="$OUT/kernel/$(basename "${src%.*}").o"
  if [[ "$src" == *.c ]]; then
    env NIX_HARDENING_ENABLE= clang "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
      -DRB_NAME_RECORD=0x00beb784u -c "$src" -o "$obj"
  else
    env NIX_HARDENING_ENABLE= clang "${ARM[@]}" -c "$src" -o "$obj"
  fi
  objects+=("$obj")
done
env NIX_HARDENING_ENABLE= clang "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank.ld \
  -Wl,--defsym=RB_BASE=0x00be1f00 -Wl,--defsym=RB_CAPACITY=0x8300 \
  "${objects[@]}" -o "$OUT/kernel/kernel.elf"
llvm-objcopy -O binary "$OUT/kernel/kernel.elf" "$OUT/kernel/kernel.bin"
llvm-objcopy -j .text -O binary "$OUT/kernel/kernel.elf" "$OUT/kernel/kernel-text.bin"
llvm-nm --defined-only --print-size --numeric-sort "$OUT/kernel/kernel.elf" > "$OUT/kernel/kernel.syms"
llvm-objdump -d --triple=armv7-none-eabi "$OUT/kernel/kernel.elf" > "$OUT/kernel/kernel.disasm"

env NIX_HARDENING_ENABLE= clang "${ARM[@]}" -c menu/recipe_bank_boot.S -o "$OUT/boot/boot.o"
env NIX_HARDENING_ENABLE= clang "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank_boot.ld \
  -Wl,--defsym=RB_SRC_BASE=0x011ad800 -Wl,--defsym=RB_SRC_END=0x011b7940 \
  -Wl,--defsym=RB_SRC_LIMIT=0x000b7940 -Wl,--defsym=RB_STORE_ENTRY=0x011adca0 \
  "$OUT/boot/boot.o" -o "$OUT/boot/boot.elf"
llvm-nm --defined-only --print-size --numeric-sort "$OUT/boot/boot.elf" > "$OUT/boot/boot.syms"
for fragment in sum entry publish check store; do
  llvm-objcopy -j ".text.rb_boot_$fragment" -O binary "$OUT/boot/boot.elf" "$OUT/boot/$fragment.bin"
done
llvm-objdump -d --triple=armv7-none-eabi "$OUT/boot/boot.elf" > "$OUT/boot/boot.disasm"

env NIX_HARDENING_ENABLE= clang "${ARM[@]}" -c menu/recipe_compiled.S -o "$OUT/d3/hook.o"
env NIX_HARDENING_ENABLE= clang "${ARM[@]}" -c menu/recipe_compiled_layout.S -o "$OUT/d3/layout.o"
env NIX_HARDENING_ENABLE= clang "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_compiled.ld \
  -Wl,--defsym=RC_BASE=0x0061c3ac "$OUT/d3/hook.o" "$OUT/d3/layout.o" -o "$OUT/d3/hook.elf"
llvm-objcopy -O binary "$OUT/d3/hook.elf" "$OUT/d3/hook.bin"
"$TOOLS/host/firmware/build-compiled-recipes" "$D3_SPEC" "$ID4" "$ID5" "$ID2" "$OUT/d3/hook.bin" "$OUT/d3"
"$TOOLS/host/firmware/apply-patch-set" "$OUT/d3/patch-set-image4.json" "$ID4" "$OUT/d3/id4-patched.bin"
"$TOOLS/host/firmware/apply-patch-set" "$OUT/d3/patch-set-image5.json" "$ID5" "$OUT/d3/id5-patched.bin"
"$TOOLS/host/firmware/build-recipe-bank" --complete "$SPEC" "$OUT/image/bank-plan.json" \
  "$OUT/image" "$OUT/kernel" "$OUT/boot" "$OUT/d3" "$STOCK"

"$TOOLS/lz/lz-encode" 0x160000 "$OUT/image/id4-final.bin" "$OUT/image/record4.bin"
"$TOOLS/lz/lz-encode" 0x2e0000 "$OUT/image/id5-final.bin" "$OUT/image/record5.bin"
"$TOOLS/host/firmware/rebuild-update" replace-record4 --decoder "$TOOLS/lz/fflzd" \
  --load-address 0x0061c1fc "$STOCK" "$OUT/image/record4.bin" "$OUT/image/id4-final.bin" "$OUT/image/record4.DAT"
"$TOOLS/host/firmware/rebuild-update" replace-record5 --decoder "$TOOLS/lz/fflzd" \
  "$OUT/image/record4.DAT" "$OUT/image/record5.bin" "$OUT/image/id5-final.bin" "$OUT/image/record5.DAT"
"$TOOLS/host/firmware/rebuild-update" write-label "$OUT/image/record5.DAT" 'WARM STANDARD' "$OUT/image/d3-boot.DAT"
"$TOOLS/host/firmware/rebuild-update" replace-plain --flash-offset 0x64d800 \
  "$OUT/image/d3-boot.DAT" "$OUT/image/source-package.bin" "$OUT/recipe-complete.DAT"
read -r dat_digest _ < <(sha256sum "$OUT/recipe-complete.DAT")
sha256sum "$SPEC" "$D3_SPEC" host/firmware/BuildRecipeBank.hs host/firmware/BuildCompiledRecipes.hs \
  host/firmware/build-recipe-package.sh menu/recipe_bank.ld menu/recipe_bank_boot.S menu/recipe_bank_boot.ld \
  menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c menu/recipe_bank_storage.c \
  menu/recipe_bank_actions.c menu/recipe_bank_edit.c menu/recipe_bank_fields.inc menu/recipe_bank_ui.h \
  menu/recipe_bank_edit.h menu/recipe_bank_diagnostic.h "$OUT/kernel/kernel.elf" "$OUT/boot/boot.elf" \
  "$OUT/image/id4-final.bin" "$OUT/image/id5-final.bin" "$OUT/image/source-package.bin" \
  | jq -Rn '[inputs | capture("(?<sha256>[^ ]+)  (?<path>.*)")]' > "$OUT/input-manifest.json"
jq --arg firmware "$OUT/recipe-complete.DAT" --arg digest "$dat_digest" --arg stock "$stock_digest" \
  --slurpfile inputs "$OUT/input-manifest.json" \
  '. + {firmware: $firmware, firmware_sha256: $digest, stock_sha256: $stock, inputs: $inputs[0], installable: false}' \
  "$OUT/image/package-layout.json" > "$OUT/recipe-complete.json"
printf 'Host-only image: %s\nManifest: %s\n' "$OUT/recipe-complete.DAT" "$OUT/recipe-complete.json"
