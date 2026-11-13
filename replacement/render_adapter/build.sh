#!/usr/bin/env bash
# Host-only build of the modal75,menu-engine,render bank image and its relink.
# The image is one Rust link unit over the subsystem rlibs; the menu tables,
# package plan and controller-75 table are declared build inputs of
# //replacement/bank/tables.
set -euo pipefail
umask 077

base=${1:?usage: build.sh BASE OUT_DIR}
out=${2:?}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-RenderAdapter}
bazel=(host-build --output_base="$output_base")
"${bazel[@]}" build --jobs=2 //host/firmware:build-recipe-bank //host/firmware:check-bank-allowlist \
  //transport:render-manifest-admission-tests \
  //replacement/bank:bank-modal75-engine-render \
  @lab_clang//:bin/clang @lab_lld//:bin/ld.lld @lab_llvm//:bin/llvm-objcopy \
  @lab_llvm//:bin/llvm-nm @lab_llvm//:bin/llvm-objdump @lab_bash//:bin/bash @lab_coreutils//:bin/sha256sum
case "$(realpath -m -- "$out")" in
  "$(realpath -m -- private-data/scratch)"/*|"$(realpath -m -- private-data/analysis/x-m1-1.50/ram-push)"/*) ;;
  *) printf 'output must stay under a named private-data/scratch directory or the host-only RAM-push directory\n' >&2; exit 1 ;;
esac
mkdir -p "$(dirname "$out")"
mkdir "$out"
archive="$PWD/bazel-bin/replacement/bank/bank-modal75-engine-render.a"
external="$output_base/external/_main~lab_dependencies~"
clang_bin=$(realpath "${external}lab_clang/bin")
lld_bin=$(realpath "${external}lab_lld/bin")
llvm_bin=$(realpath "${external}lab_llvm/bin")
coreutils_bin=$(realpath "${external}lab_coreutils/bin")
RB_PUBLICATION_GROUPS=modal75,menu-engine,render \
  RAM_PUSH_BUILDER="$PWD/bazel-bin/host/firmware/build-recipe-bank" \
  RAM_PUSH_CLANG="$clang_bin/clang" RAM_PUSH_LD_LLD="$lld_bin/ld.lld" \
  RAM_PUSH_OBJCOPY="$llvm_bin/llvm-objcopy" RAM_PUSH_NM="$llvm_bin/llvm-nm" \
  RAM_PUSH_OBJDUMP="$llvm_bin/llvm-objdump" PATH="$coreutils_bin" \
  RB_RUST_ARCHIVE="$archive" \
  RB_RUST_BUILD_FACTS="${external}lab_xm1_rust/share/xm1-rust/build-facts.txt" \
  SPEC=private-data/analysis/x-m1-1.50/recipes/bank-spec.json \
  RC0_ID4=private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin \
  RC0_ID5=private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  RC0_PLAIN=private-data/analysis/x-m1-1.50/lz/plain-01100000.bin \
  "${external}lab_bash/bin/bash" host/firmware/build-recipe-bank-ram.sh "$base" "$out"
bazel-bin/host/firmware/check-bank-allowlist "$out/bank-allowlist-input.json" "$out/kernel.elf" --report "$out/bank-allowlist-report.json"
bazel-bin/transport/render-manifest-admission-tests "$PWD/$out/ram-push-manifest.json"
sha256sum "$out/runtime.bin" "$out/kernel.elf" "$out/ram-push-manifest.json"
