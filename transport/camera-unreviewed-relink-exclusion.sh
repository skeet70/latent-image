#!/usr/bin/env bash
# Argument: the genquery output of deps(//transport:camera).
set -euo pipefail
deps="$1"
grep -qx '//transport:camera-core' "$deps" || { echo "camera-core missing from the camera's dependencies" >&2; exit 1; }
grep -qx '//transport:ram-push-pins' "$deps" || { echo "the compiled RAM-push pins are missing from the camera's dependencies" >&2; exit 1; }
if grep -E '^//emulation:(ram-push-unreviewed|Xm1UnreviewedRelink\.hs)$' "$deps"; then
  echo "//transport:camera depends on the unreviewed emulator relink" >&2
  exit 1
fi
