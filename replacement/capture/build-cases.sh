#!/usr/bin/env bash
# Host-only capture develop-step cases. Run under `devenv shell --`.
set -euo pipefail
cd "$(dirname "$0")/../.."

BUILD_DIR="${BUILD_DIR:-private-data/analysis/x-m1-1.50/capture-build}"
OUT_DIR="${OUT_DIR:-private-data/analysis/x-m1-1.50/capture-cases}"
RUNNER="${RUNNER:-bazel-bin/emulation/native-runner}"
GEN="${GEN:-bazel-bin/replacement/capture/capture-qualification}"
ID5_ELF="${ID5_ELF:-private-data/analysis/x-m1-1.50/lz/id5.elf}"

[[ -x "$RUNNER" ]] || { printf 'runner is not executable: %s\n' "$RUNNER" >&2; exit 1; }
[[ -x "$GEN" ]] || { printf 'generator is not executable: %s\n' "$GEN" >&2; exit 1; }
[[ -f "$ID5_ELF" ]] || { printf 'stock image is missing: %s\n' "$ID5_ELF" >&2; exit 1; }

mkdir -p "$BUILD_DIR" "$OUT_DIR"
cp bazel-bin/replacement/capture/capture-candidate.bin "$BUILD_DIR/capture-candidate.bin"
cp bazel-bin/replacement/capture/capture-deps-high.bin "$BUILD_DIR/capture-deps-high.bin"
cp bazel-bin/replacement/capture/capture-deps-low.bin "$BUILD_DIR/capture-deps-low.bin"
cp bazel-bin/replacement/capture/capture-deps-low-faulty.bin "$BUILD_DIR/capture-deps-low-faulty.bin"

candidate_bytes=$(wc -c < "$BUILD_DIR/capture-candidate.bin")
printf '%s\n' "$candidate_bytes" > "$BUILD_DIR/capture-candidate.length"
truncate -s 32768 "$BUILD_DIR/capture-candidate.bin"
truncate -s 131072 "$BUILD_DIR/capture-deps-high.bin"
truncate -s 16384 "$BUILD_DIR/capture-deps-low.bin"
truncate -s 16384 "$BUILD_DIR/capture-deps-low-faulty.bin"

dd if="$ID5_ELF" of="$BUILD_DIR/stock-wait.bin" bs=1 skip=$((0x2ac720)) count=$((0x11c)) status=none
entry=$(od -An -tx4 -N4 "$BUILD_DIR/stock-wait.bin" | tr -d ' \n')
[[ "$entry" == "e92d4870" ]] || { printf 'stock wait entry differs: %s\n' "$entry" >&2; exit 1; }
dd if="$BUILD_DIR/stock-wait.bin" of="$BUILD_DIR/capture-deps-high.bin" bs=1 seek=$((0xc828)) conv=notrunc status=none

"$GEN" "$RUNNER" "$BUILD_DIR" "$OUT_DIR"
