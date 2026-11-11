#!/usr/bin/env bash
# Host-only build of the standalone value adapter image and its always-decline control.
set -euo pipefail
umask 077

out=${1:?usage: build-image.sh OUT_DIR VALUE_TABLE [decline]}
value=${2:?}
mode=${3:-normal}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-MenuValuePackage}
bazel=(host-build --output_base="$output_base")

"${bazel[@]}" build --jobs=2 //replacement/modal_runtime:menu-value-table \
  @lab_xm1_rust//:bin/rustc @lab_clang//:bin/clang @lab_lld//:bin/ld.lld \
  @lab_llvm//:bin/llvm-objcopy @lab_llvm//:bin/llvm-nm @lab_llvm//:bin/llvm-objdump

mkdir -p "$(dirname "$out")"
mkdir "$out"
external="$output_base/external/_main~lab_dependencies~"
clang_bin=$(realpath "${external}lab_clang/bin")
lld_bin=$(realpath "${external}lab_lld/bin")
llvm_bin=$(realpath "${external}lab_llvm/bin")
decline=()
case "$mode" in
  normal) crate=menu_value_image ;;
  decline) crate=menu_value_image_decline; decline=(--cfg rb_menu_value_always_decline) ;;
  *) printf 'mode must be normal or decline\n' >&2; exit 1 ;;
esac
RB_MENU_VALUE_TABLE="$PWD/$value" "${external}lab_xm1_rust/bin/rustc" --crate-name="$crate" --edition=2021 \
  --crate-type=staticlib -C opt-level=z --target=armv7r-none-eabi -C panic=abort -C relocation-model=static \
  -C target-cpu=cortex-r4f -C codegen-units=1 -C debuginfo=0 -C target-feature=+strict-align -C jump-tables=no \
  "${decline[@]}" replacement/menu_value/adapter/image.rs -o "$out/lib$crate.a"
NIX_HARDENING_ENABLE= "$clang_bin/clang" --target=armv7r-none-eabi -march=armv7-r -marm -mfloat-abi=soft \
  -nostdlib -static -fuse-ld=lld --ld-path="$lld_bin/ld.lld" -Wl,--no-dynamic-linker -Wl,--build-id=none \
  -Wl,-T,replacement/menu_value/adapter/image.ld -Wl,-e,rb_menu_value_size_arm \
  "$out/lib$crate.a" -o "$out/$crate.elf"
"$llvm_bin/llvm-objcopy" -O binary "$out/$crate.elf" "$out/$crate.bin"
"$llvm_bin/llvm-nm" -S --defined-only --numeric-sort "$out/$crate.elf" > "$out/$crate.symbols"
"$llvm_bin/llvm-objdump" -d --no-show-raw-insn --triple=armv7r-none-eabi --mcpu=cortex-r4f "$out/$crate.elf" > "$out/$crate.disasm"
sha256sum "$out/$crate.bin"
