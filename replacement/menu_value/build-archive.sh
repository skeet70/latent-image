#!/usr/bin/env bash
# Host-only build of the modal75, menu-engine and menu-value bank archive from
# the generated modal-75 table, composed menu tree and value table.
set -euo pipefail
umask 077

out=${1:?usage: build-archive.sh OUT_DIR MODAL75_TABLE MENU_TABLES BANK_PLAN VALUE_TABLE}
modal75=${2:?}
tables=${3:?}
plan=${4:?}
value=${5:?}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-MenuValuePackage}
bazel=(host-build --output_base="$output_base")

"${bazel[@]}" build --jobs=2 //replacement/modal_runtime:menu-engine-tree \
  //replacement/modal_runtime:menu-value-table @lab_xm1_rust//:bin/rustc

mkdir -p "$(dirname "$out")"
mkdir "$out"
bazel-bin/replacement/modal_runtime/menu-engine-tree --tables "$tables" --package-plan "$plan" \
  --tree-out "$out/menu-tree.rs" --records-out "$out/menu-records.rs" --labels-out "$out/menu-labels.rs"
external="$output_base/external/_main~lab_dependencies~"
RB_MODAL75_TABLE="$PWD/$modal75" RB_MENU_ENGINE_TREE="$PWD/$out/menu-tree.rs" \
  RB_MENU_ENGINE_RECORDS="$PWD/$out/menu-records.rs" RB_MENU_VALUE_TABLE="$PWD/$value" \
  "${external}lab_xm1_rust/bin/rustc" --crate-name=bank --edition=2021 --crate-type=staticlib -C opt-level=z \
  --target=armv7r-none-eabi -C panic=abort -C relocation-model=static -C target-cpu=cortex-r4f -C codegen-units=1 \
  -C debuginfo=0 -C target-feature=+strict-align -C llvm-args=-min-jump-table-entries=4096 \
  --cfg rb_modal_scoped --cfg rb_modal75_owned --cfg rb_menu_engine_owned --cfg rb_menu_value_owned \
  replacement/bank/lib.rs -o "$out/bank-modal75-value.a"
sha256sum "$out/bank-modal75-value.a"
