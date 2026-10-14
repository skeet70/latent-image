#!/usr/bin/env bash
set -euo pipefail
checker="$1"
schema="$2"
shift 2
if [ "$#" -eq 0 ]; then
  echo "usage: bazel run //af-session:check-annotation -- ANNOTATION.json..." >&2
  exit 1
fi
exec "$checker" --schemafile "$schema" "$@"
