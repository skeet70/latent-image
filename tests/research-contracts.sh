#!/usr/bin/env bash
set -euo pipefail

fuji_af_schema_checker="$1"
fuji_af_jq="$2"

"$fuji_af_schema_checker" --check-metaschema schemas/*.schema.json

for fuji_af_pair in sources:sources camera:camera firmware:firmware memory-map:memory-map benchmark-protocol:benchmark-protocol; do
  fuji_af_schema_name="${fuji_af_pair%%:*}"
  fuji_af_instance_name="${fuji_af_pair#*:}"
  "$fuji_af_schema_checker" --schemafile "schemas/$fuji_af_schema_name.schema.json" "manifests/$fuji_af_instance_name.json"
  "$fuji_af_jq" '.schema_version = 2' "manifests/$fuji_af_instance_name.json" > "$TEST_TMPDIR/invalid.json"
  if "$fuji_af_schema_checker" --schemafile "schemas/$fuji_af_schema_name.schema.json" "$TEST_TMPDIR/invalid.json"; then
    echo 'Unknown schema version incorrectly accepted' >&2
    exit 1
  fi
done

"$fuji_af_jq" -s '.' docs/evidence.jsonl > "$TEST_TMPDIR/evidence.json"
"$fuji_af_jq" -n --arg schema "file://$PWD/schemas/evidence.schema.json" \
  '{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"array","items":{"$ref":$schema}}' \
  > "$TEST_TMPDIR/evidence-array.schema.json"
"$fuji_af_schema_checker" --schemafile "$TEST_TMPDIR/evidence-array.schema.json" "$TEST_TMPDIR/evidence.json"

"$fuji_af_jq" '.sources[0].revision = null' manifests/sources.json > "$TEST_TMPDIR/invalid.json"
if "$fuji_af_schema_checker" --schemafile schemas/sources.schema.json "$TEST_TMPDIR/invalid.json"; then
  echo 'Selected source without a commit incorrectly accepted' >&2
  exit 1
fi

# This public design draft must not claim a frozen or authorized camera session,
# discard failed attempts, or equate confirmation with acceptable-focus onset.
for fuji_af_mutation in \
  '.status = "frozen"' \
  '.authorization.camera_operations_authorized = true' \
  '.attempt_policy.all_attempts_retained = false' \
  '.timing.acceptable_focus_onset = "camera confirmation"'; do
  "$fuji_af_jq" "$fuji_af_mutation" manifests/benchmark-protocol.json > "$TEST_TMPDIR/invalid.json"
  if "$fuji_af_schema_checker" --schemafile schemas/benchmark-protocol.schema.json "$TEST_TMPDIR/invalid.json"; then
    echo "Invalid pilot policy incorrectly accepted: $fuji_af_mutation" >&2
    exit 1
  fi
done

# Cross-field identifiers and counts need semantic checks beyond JSON Schema.
fuji_af_protocol_consistency='
  .design as $design |
  {P01: "bright", P02: "indoor", P03: "dim", P04: "low-contrast", P05: "hunting"} as $scenarios |
  ($design.planned_attempts == (($design.combinations | length) * $design.repeats_per_combination)) and
  (($design.combinations | map(.id) | unique | length) == ($design.combinations | length)) and
  all($design.combinations[];
      .id == (.scene_id + "-" + .start) and .scenario == $scenarios[.scene_id]) and
  all($scenarios | keys[]; . as $scene |
      ($design.combinations | map(select(.scene_id == $scene) | .start) | sort) ==
      (if $scene == "P01" then ["far", "near", "near-focused"] else ["far", "near"] end))
'
"$fuji_af_jq" -e "$fuji_af_protocol_consistency" manifests/benchmark-protocol.json > /dev/null
for fuji_af_mutation in \
  '.design.combinations[0].start = "far"' \
  '.design.combinations[0].scenario = "indoor"'; do
  "$fuji_af_jq" "$fuji_af_mutation" manifests/benchmark-protocol.json > "$TEST_TMPDIR/invalid.json"
  if "$fuji_af_jq" -e "$fuji_af_protocol_consistency" "$TEST_TMPDIR/invalid.json" > /dev/null; then
    echo "Inconsistent pilot combination incorrectly accepted: $fuji_af_mutation" >&2
    exit 1
  fi
done
