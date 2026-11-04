#!/usr/bin/env bash
# Host-only base-relinked recipe-bank and installer build, run by
# `//transport:camera` from the workspace root with Bazel-provided tool paths.
set -euo pipefail

builder_mode=(--ram-push)
allocation_size=0x1dc98
publication_count=223
activation_count=92
modal_link=()
engine_link=()
case "${RB_PUBLICATION_GROUPS:-}" in
  "") ;;
  modal75)
    builder_mode=(--ram-push-variant modal75)
    allocation_size=0x1dcb8
    publication_count=224
    activation_count=93
    modal_link=(-Wl,--undefined=rb_modal_entry)
    ;;
  lifecycle)
    builder_mode=(--ram-push-variant lifecycle)
    allocation_size=0x1dcd8
    publication_count=226
    activation_count=95
    ;;
  lifecycle,modal75)
    builder_mode=(--ram-push-variant lifecycle,modal75)
    allocation_size=0x1dcf8
    publication_count=227
    activation_count=96
    modal_link=(-Wl,--undefined=rb_modal_entry)
    ;;
  modal75,menu-engine)
    builder_mode=(--ram-push-variant modal75,menu-engine)
    allocation_size=0x2dcc0
    publication_count=225
    activation_count=94
    modal_link=(-Wl,--undefined=rb_modal_entry -Wl,--undefined=rb_menu_engine_entry)
    engine_link=(-Wl,-T,menu/recipe_bank_ram_engine.ld)
    ;;
  *) printf 'invalid RB_PUBLICATION_GROUPS selection\n' >&2; exit 1 ;;
esac

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
RB_RUST_ARCHIVE="${RB_RUST_ARCHIVE:?}"

case "$OUT" in
  private-data/analysis/x-m1-1.50/ram-push/*) ;;
  *) printf 'output must stay under the host-only RAM-push directory\n' >&2; exit 1 ;;
esac
[[ -d "$OUT" && ! -L "$OUT" ]] || { printf 'output directory must exist\n' >&2; exit 1; }
[[ -f "$RB_RUST_ARCHIVE" ]] || { printf 'service archive must be a regular file\n' >&2; exit 1; }

base_value=$((BASE))
(( base_value >= 0 && base_value <= 0xffffffff - allocation_size )) || {
  printf 'base leaves the 32-bit RAM address space\n' >&2; exit 1;
}
(( (base_value & 7) == 0 )) || {
  printf 'base must be 8-byte aligned\n' >&2; exit 1;
}
minimum_name=$((base_value + 0xbb00))
name_record=$(((minimum_name & 0xffff0000) | 0xb784))
if (( name_record < minimum_name )); then
  name_record=$((name_record + 0x10000))
fi
bank_end=$((name_record + 0x400))
installer_start=$(((name_record + 0x800 + 31) & 0xffffffe0))
printf -v rb_name_record '0x%08xu' "$name_record"
printf -v rb_bank_end '0x%08x' "$bank_end"
printf -v rb_installer_start '0x%08x' "$installer_start"
if (( ${#engine_link[@]} )); then
  engine_start=$((base_value + 0x1dcc0))
  printf -v rb_engine_start '0x%08x' "$engine_start"
  printf -v rb_engine_end '0x%08x' "$((engine_start + 0x10000))"
  engine_link+=(-Wl,--defsym=RAM_PUSH_MENU_ENGINE_START="$rb_engine_start" -Wl,--defsym=RAM_PUSH_MENU_ENGINE_END="$rb_engine_end")
fi

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREE=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
LINK=(-nostdlib -static -fuse-ld=lld "--ld-path=$LD_LLD" -Wl,--no-dynamic-linker -Wl,--build-id=none)
objects=()
lifecycle_sources=()
lifecycle_link=()
case "${RB_PUBLICATION_GROUPS:-}" in
  lifecycle|lifecycle,modal75)
    lifecycle_sources=(replacement/save/save_stock.S replacement/media_permit/rb_media_permit_stock.S)
    lifecycle_link=(-Wl,--undefined=xm1_save_facade -Wl,--undefined=xm1_save_completion
      -Wl,--undefined=xm1_save_release_completion -Wl,--undefined=rb_media_permit_apply) ;;
esac
for src in menu/recipe_bank_arm.S menu/recipe_bank_probe.c menu/recipe_bank_hook.c \
  menu/recipe_bank_storage.c menu/recipe_bank_actions.c menu/recipe_bank_edit.c \
  menu/recipe_bank_diagnostic.c replacement/service/rb_service_stock.S \
  replacement/capture/rb_capture_stock.S "${lifecycle_sources[@]}"; do
  name="${src##*/}"
  obj="$OUT/${name%.*}.o"
  case "$src" in
    *.c) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra -I. \
      -DRB_VOLATILE_BANK=1 -DRB_CC_BANK=1 -DRB_NAME_RECORD="$rb_name_record" -c "$src" -o "$obj" ;;
    *.S) NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c "$src" -o "$obj" ;;
  esac
  objects+=("$obj")
done
if (( ${#engine_link[@]} )); then
  # The engine's sections, by their Rust paths, move to .menu_engine.* for
  # menu/recipe_bank_ram_engine.ld.
  engine_section='^\.(text|rodata)\.(rb_menu_engine_|.*(11menu_engine(6engine|5block|5state|4tree)|19menu_engine_adapter))'
  declare -A renamed=()
  renames=()
  while read -r _ section _; do
    if [[ $section =~ $engine_section && -z ${renamed[$section]:-} ]]; then
      renamed[$section]=1
      renames+=(--rename-section "$section=.menu_engine$section")
    fi
  done < <("$OBJDUMP" -h "$RB_RUST_ARCHIVE")
  "$OBJCOPY" "${renames[@]}" "$RB_RUST_ARCHIVE" "$OUT/rust-engine.a"
  objects+=("$OUT/rust-engine.a")
else
  objects+=("$RB_RUST_ARCHIVE")
fi
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${FREE[@]}" -Oz -Wall -Wextra \
  -DRAM_PUSH_PUBLICATION_COUNT="${publication_count}u" -DRAM_PUSH_SETUP_COUNT=131u \
  -DRAM_PUSH_ACTIVATION_COUNT="${activation_count}u" -DRAM_PUSH_ABI_VERSION=5u \
  -ffunction-sections -fdata-sections -c menu/ram_push_installer.c \
  -o "$OUT/ram_push_installer.o"
objects+=("$OUT/ram_push_installer.o")
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c menu/classic_chrome_ram.S -o "$OUT/classic_chrome_ram.o"
objects+=("$OUT/classic_chrome_ram.o")
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank_ram.ld \
  -Wl,--defsym=RAM_PUSH_BASE="$BASE" -Wl,--defsym=RAM_PUSH_BANK_END="$rb_bank_end" \
  -Wl,--defsym=RAM_PUSH_INSTALLER_START="$rb_installer_start" \
  -Wl,--defsym=RAM_PUSH_PUBLICATION_COUNT="$publication_count" "${modal_link[@]}" "${lifecycle_link[@]}" "${engine_link[@]}" \
  -Wl,menu/classic_chrome_ram_stock.ld -Wl,--emit-relocs \
  "${objects[@]}" -o "$OUT/kernel.elf"
"$OBJCOPY" -O binary "$OUT/kernel.elf" "$OUT/linked.bin"
"$OBJCOPY" -j .text -O binary "$OUT/kernel.elf" "$OUT/kernel-text.bin"
"$NM" --defined-only --print-size --numeric-sort "$OUT/kernel.elf" > "$OUT/kernel.syms"
"$OBJDUMP" -d --triple=armv7-none-eabi "$OUT/kernel.elf" > "$OUT/kernel.disasm"

NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" -c menu/recipe_bank_boot.S -o "$OUT/boot.o"
NIX_HARDENING_ENABLE= "$CLANG" "${ARM[@]}" "${LINK[@]}" -Wl,-T,menu/recipe_bank_boot.ld \
  -Wl,--defsym=RB_SRC_BASE=0x011ad800 \
  -Wl,--defsym=RB_SRC_LIMIT=0x000b7940 -Wl,--defsym=RB_VALIDATE_ENTRY=0x011ad820 \
  -Wl,--defsym=RB_COPY_ENTRY=0x011adcdc \
  -Wl,--defsym=RB_STORE_ENTRY=0x011adca0 \
  "$OUT/boot.o" -o "$OUT/boot.elf"
"$OBJCOPY" -j .text.rb_boot_store -O binary "$OUT/boot.elf" "$OUT/store.bin"

"$BUILDER" "${builder_mode[@]}" "$BASE" "$SPEC" \
  "$OUT/bank-plan.json" "$OUT" "$OUT" "$RC0_ID4" "$RC0_ID5" "$RC0_PLAIN"
printf 'runtime: %s/runtime.bin\nmanifest: %s/ram-push-manifest.json\n' "$OUT" "$OUT"
