#!/usr/bin/env bash
# bazel run //emulation:fujimod-updater-f7 -- --recovery-ram RAM --sd-image CARD --installed-nand NAND --output DIR
# Runs fujimod-xm1-updater in one emulator slot from the workspace, with the run
# document, captured boot ROM and stock 1.50 DAT.
set -euo pipefail
slot=$(realpath "$1")
driver=$(realpath "$2")
run=$(realpath "$3")
shift 3
cd "${BUILD_WORKSPACE_DIRECTORY:?run through bazel run}"
exec "$slot" "$driver" --run "$run" \
  --boot-rom private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin \
  --firmware private-data/firmware/x-m1-1.50/FWUP0002.DAT "$@"
