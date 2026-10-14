#!/usr/bin/env bash
# Recipe-bank UI-behavior case build and run under `devenv shell --`; host-only, no camera.
set -euo pipefail
cd "$(dirname "$0")/.."

RB_NAME_RECORD="${RB_NAME_RECORD:-0x011ac900u}"
NAME_RECORD_CLEAN="${RB_NAME_RECORD%u}"
NAME_RECORD_HEX="${NAME_RECORD_CLEAN#0x}"
BUILD_DIR="${BUILD_DIR:-/tmp/recipe-bank-ui-cases}"
OUT_DIR="${OUT_DIR:-private-data/analysis/x-m1-1.50/recipes/runtime/ui-cases}"
RUNNER="${RUNNER:-bazel-bin/emulation/native-runner}"
GEN="${GEN:-bazel-bin/emulation/recipe-bank-ui-cases}"
BUILDER="${BUILDER:-bazel-bin/host/firmware/build-recipe-bank}"
SPEC="${SPEC:-private-data/analysis/x-m1-1.50/recipes/bank-spec.json}"
PLAN="${PLAN:-$BUILD_DIR/bank-plan.json}"
ID4="${ID4:-private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin}"

mkdir -p "$BUILD_DIR" "$BUILD_DIR/out"

ARM=(--target=armv7r-none-eabi -march=armv7-r -marm)
FREESTANDING=(-ffreestanding -nostdlib -fno-builtin -mno-unaligned-access)
HARDEN=NIX_HARDENING_ENABLE=

# 1. the bank blob
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
env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
  -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,menu/recipe_bank.ld \
  -Wl,--defsym=RB_BASE=0x011dcca0 "${OBJS[@]}" -o "$BUILD_DIR/kernel.elf"
llvm-objcopy -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel.bin"
llvm-objcopy -j .text -O binary "$BUILD_DIR/kernel.elf" "$BUILD_DIR/kernel-text.bin"
llvm-nm --print-size --numeric-sort "$BUILD_DIR/kernel.elf" > "$BUILD_DIR/kernel.syms"
sha256sum "$BUILD_DIR/kernel.bin"

# the hook object must carry the record base the plan and the cases use
llvm-objdump -d "$BUILD_DIR/recipe_bank_hook.o" > "$BUILD_DIR/hook.dis"
base=$((16#$NAME_RECORD_HEX))
hi=$(printf '%x' $((base >> 16)))
lo=$(printf '%x' $((base & 65535)))
if ! grep -qi "$(printf '%x' "$base")" "$BUILD_DIR/hook.dis" && ! (grep -q "#0x$lo" "$BUILD_DIR/hook.dis" && grep -q "#0x$hi" "$BUILD_DIR/hook.dis"); then
  echo "warning: the hook object does not show the record base $RB_NAME_RECORD" >&2
fi

# 2. the storage model, linked against the measured kernel symbols its drivers call
env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections \
  -c emulation/recipe-bank-model.c -o "$BUILD_DIR/model.o"
DEFSYMS=()
while read -r sym; do
  [ -z "$sym" ] && continue
  addr=$(awk -v s="$sym" '$4 == s {print "0x" $1; exit}' "$BUILD_DIR/kernel.syms")
  if [ -n "$addr" ]; then
    DEFSYMS+=("-Wl,--defsym=$sym=$addr")
  fi
done < <(llvm-nm --undefined-only "$BUILD_DIR/model.o" | awk '{print $2}')
env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
  -Wl,--no-dynamic-linker -Wl,--build-id=none \
  -Wl,-T,emulation/recipe-bank-model.ld "${DEFSYMS[@]}" "$BUILD_DIR/model.o" \
  -o "$BUILD_DIR/model.elf"
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
read -r HADDR HSIZE <<< "$(llvm-readelf -S "$BUILD_DIR/model.elf" | awk '/\.text\.helpers/ {for (i = 1; i <= NF; i++) if ($i == "PROGBITS") {print $(i + 1), $(i + 3); exit}}')"
printf '%s %s T helpers\n' "$HADDR" "$HSIZE" >> "$BUILD_DIR/model.syms"

# 3. the UI model
env $HARDEN clang "${ARM[@]}" "${FREESTANDING[@]}" -Oz -Wall -Wextra \
  -ffunction-sections -fdata-sections \
  -c emulation/recipe-bank-ui-model.c -o "$BUILD_DIR/ui-model.o"
env $HARDEN clang "${ARM[@]}" -nostdlib -static -fuse-ld=lld \
  -Wl,--build-id=none -Wl,-T,emulation/recipe-bank-ui-model.ld \
  "$BUILD_DIR/ui-model.o" -o "$BUILD_DIR/ui-model.elf"
llvm-nm --print-size --numeric-sort "$BUILD_DIR/ui-model.elf" > "$BUILD_DIR/ui-model.syms"
for pair in a:ui-a b:ui-b c:ui-c rpresenter:ui-p; do
  sec="${pair%%:*}"
  out="${pair##*:}"
  llvm-objcopy -j ".text.$sec" -O binary "$BUILD_DIR/ui-model.elf" "$BUILD_DIR/$out.bin"
done
sha256sum "$BUILD_DIR/ui-model.elf"

# 4. a fresh plan from the pinned spec
"$BUILDER" "$SPEC" "$PLAN" "$BUILD_DIR/out"

# 5. the cases
mkdir -p "$OUT_DIR"
"$GEN" --runner "$RUNNER" --build "$BUILD_DIR" --out "$OUT_DIR" --plan "$PLAN" \
  --name-record "$NAME_RECORD_CLEAN" --id4 "$ID4" \
  --fields reports/custom-settings.fields.json
