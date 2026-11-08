#!/usr/bin/env bash
# Host-only relink of an existing render bank archive at a chosen allocation base.
set -euo pipefail
umask 077

base=${1:?usage: relink.sh BASE ARCHIVE OUT_DIR}
archive=${2:?}
out=${3:?}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-RenderAdapter}
host_build=(host-build --output_base="$output_base")

case "$out" in
  private-data/*) ;;
  *) printf 'output must stay under the host-only private-data tree\n' >&2; exit 1 ;;
esac
[ -f "$archive" ] || { printf 'archive is missing: %s\n' "$archive" >&2; exit 1; }
mkdir -p "$(dirname "$out")"
[ -d "$out" ] && [ -z "$(ls -A "$out")" ] || { printf 'output directory must be new and empty: %s\n' "$out" >&2; exit 1; }

"${host_build[@]}" build --jobs=2 --noremote_accept_cached //host/firmware:build-recipe-bank //host/firmware:check-bank-allowlist \
  //transport:render-manifest-admission-tests @lab_xm1_rust//:bin/rustc @lab_clang//:bin/clang \
  @lab_lld//:bin/ld.lld @lab_llvm//:bin/llvm-objcopy @lab_llvm//:bin/llvm-nm @lab_llvm//:bin/llvm-objdump \
  @lab_bash//:bin/bash @lab_coreutils//:bin/sha256sum

external="$output_base/external/_main~lab_dependencies~"
clang_bin=$(realpath "${external}lab_clang/bin")
lld_bin=$(realpath "${external}lab_lld/bin")
llvm_bin=$(realpath "${external}lab_llvm/bin")
coreutils_bin=$(realpath "${external}lab_coreutils/bin")

variant_tag=modal75,menu-engine,render
RB_PUBLICATION_GROUPS="$variant_tag" \
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
checker_sha=$(sha256sum bazel-bin/host/firmware/check-bank-allowlist | cut -d' ' -f1)
manifest_sha=$(sha256sum "$out/ram-push-manifest.json" | cut -d' ' -f1)
runtime_sha=$(sha256sum "$out/runtime.bin" | cut -d' ' -f1)
elf_sha=$(sha256sum "$out/kernel.elf" | cut -d' ' -f1)
archive_sha=$(sha256sum "$archive" | cut -d' ' -f1)
input_sha=$(sha256sum "$out/bank-allowlist-input.json" | cut -d' ' -f1)
report_sha=$(sha256sum "$out/bank-allowlist-report.json" | cut -d' ' -f1)
jq -n --arg checker "$checker_sha" --arg variant "$variant_tag" \
  --arg manifest "$manifest_sha" --arg runtime "$runtime_sha" --arg elf "$elf_sha" \
  --arg archive "$archive_sha" --arg input "$input_sha" --arg report "$report_sha" \
  '{schema:1,analysis:"bank-allowlist-check-r1",checker_sha256:$checker,bank_variant:$variant,policy:"enforce",manifest_sha256:$manifest,runtime_sha256:$runtime,kernel_elf_sha256:$elf,archive_sha256:$archive,allowlist_input_sha256:$input,report_sha256:$report}' \
  > "$out/bank-allowlist-proof.json"
bazel-bin/transport/render-manifest-admission-tests "$PWD/$out/ram-push-manifest.json"
sha256sum "$out/runtime.bin" "$out/kernel.elf" "$out/ram-push-manifest.json"
