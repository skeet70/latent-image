#!/usr/bin/env bash
set -euo pipefail
umask 077

base=${1:?usage: build.sh BASE OUT_DIR MENU_TABLES BANK_PLAN MODAL75_TABLE}
out=${2:?}
tables=${3:?}
plan=${4:?}
modal75=${5:?}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-RenderAdapter}
bazel=(host-build --output_base="$output_base")
"${bazel[@]}" build --jobs=2 //host/firmware:build-recipe-bank //host/firmware:check-bank-allowlist \
  //transport:render-manifest-admission-tests \
  //replacement/modal_runtime:menu-engine-tree @lab_xm1_rust//:bin/rustc \
  @lab_clang//:bin/clang @lab_lld//:bin/ld.lld @lab_llvm//:bin/llvm-objcopy \
  @lab_llvm//:bin/llvm-nm @lab_llvm//:bin/llvm-objdump @lab_bash//:bin/bash @lab_coreutils//:bin/sha256sum
case "$(realpath -m -- "$out")" in
  "$(realpath -m -- private-data/scratch)"/*|"$(realpath -m -- private-data/analysis/x-m1-1.50/ram-push)"/*) ;;
  *) printf 'output must stay under a named private-data/scratch directory or the host-only RAM-push directory\n' >&2; exit 1 ;;
esac
mkdir -p "$(dirname "$out")"
mkdir "$out"
bazel-bin/replacement/modal_runtime/menu-engine-tree --tables "$tables" --package-plan "$plan" \
  --tree-out "$out/menu-tree.rs" --records-out "$out/menu-records.rs" --labels-out "$out/menu-labels.rs"
external="$output_base/external/_main~lab_dependencies~"
clang_bin=$(realpath "${external}lab_clang/bin")
lld_bin=$(realpath "${external}lab_lld/bin")
llvm_bin=$(realpath "${external}lab_llvm/bin")
coreutils_bin=$(realpath "${external}lab_coreutils/bin")
RB_MODAL75_TABLE="$PWD/$modal75" RB_MENU_ENGINE_TREE="$PWD/$out/menu-tree.rs" RB_MENU_ENGINE_RECORDS="$PWD/$out/menu-records.rs" \
  "${external}lab_xm1_rust/bin/rustc" --crate-name=bank --edition=2021 --crate-type=staticlib -C opt-level=z \
  --target=armv7r-none-eabi -C panic=abort -C relocation-model=static -C target-cpu=cortex-r4f -C codegen-units=1 \
  -C debuginfo=0 -C llvm-args=-min-jump-table-entries=4096 --cfg rb_modal_scoped --cfg rb_modal75_owned \
  --cfg rb_menu_engine_owned --cfg rb_render_owned replacement/bank/lib.rs -o "$out/bank-modal75-engine-render.a"
RB_PUBLICATION_GROUPS=modal75,menu-engine,render \
  RAM_PUSH_BUILDER="$PWD/bazel-bin/host/firmware/build-recipe-bank" \
  RAM_PUSH_CLANG="$clang_bin/clang" RAM_PUSH_LD_LLD="$lld_bin/ld.lld" \
  RAM_PUSH_OBJCOPY="$llvm_bin/llvm-objcopy" RAM_PUSH_NM="$llvm_bin/llvm-nm" \
  RAM_PUSH_OBJDUMP="$llvm_bin/llvm-objdump" PATH="$coreutils_bin" \
  RB_RUST_ARCHIVE="$out/bank-modal75-engine-render.a" \
  RB_RUST_BUILD_FACTS="${external}lab_xm1_rust/share/xm1-rust/build-facts.txt" \
  SPEC=private-data/analysis/x-m1-1.50/recipes/bank-spec.json \
  RC0_ID4=private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin \
  RC0_ID5=private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  RC0_PLAIN=private-data/analysis/x-m1-1.50/lz/plain-01100000.bin \
  "${external}lab_bash/bin/bash" host/firmware/build-recipe-bank-ram.sh "$base" "$out"
bazel-bin/host/firmware/check-bank-allowlist "$out/bank-allowlist-input.json" "$out/kernel.elf" --report "$out/bank-allowlist-report.json"
bazel-bin/transport/render-manifest-admission-tests "$PWD/$out/ram-push-manifest.json"
sha256sum "$out/runtime.bin" "$out/kernel.elf" "$out/ram-push-manifest.json"
