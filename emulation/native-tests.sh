#!/usr/bin/env bash
set -euo pipefail
"$3" --check-metaschema schemas/native-case.schema.json
"$3" --schemafile schemas/native-case.schema.json emulation/fixtures/*.json
exec "$1" "$2" "$4"
