#!/usr/bin/env bash
# Host-only build of the modal75, menu-engine and menu-value bank archive.
# The archive is one Rust link unit over the subsystem rlibs; the modal-75,
# composed menu tree and value tables are declared build inputs of
# //replacement/bank/tables.
set -euo pipefail
umask 077

out=${1:?usage: build-archive.sh OUT_DIR}
output_base=${RB_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-MenuValuePackage}
bazel=(host-build --output_base="$output_base")

"${bazel[@]}" build --jobs=2 //replacement/bank:bank-modal75-engine-render-value

mkdir -p "$(dirname "$out")"
mkdir "$out"
cp bazel-bin/replacement/bank/bank-modal75-engine-render-value.a "$out/bank-modal75-value.a"
sha256sum "$out/bank-modal75-value.a"
