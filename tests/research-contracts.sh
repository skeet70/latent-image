#!/usr/bin/env bash
set -euo pipefail

fuji_af_schema_checker="$1"
fuji_af_jq="$2"
fuji_af_git="$3"
fuji_af_evidence_record="$4"

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

"$fuji_af_jq" -n --arg schema "file://$PWD/schemas/evidence.schema.json" \
  '{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"array","minItems":1,"items":{"$ref":$schema}}' \
  > "$TEST_TMPDIR/evidence-array.schema.json"
fuji_af_check_evidence_ledger() {
  "$fuji_af_jq" -s '.' "$1" > "$TEST_TMPDIR/evidence.json" &&
    "$fuji_af_schema_checker" --schemafile "$TEST_TMPDIR/evidence-array.schema.json" "$TEST_TMPDIR/evidence.json"
}
fuji_af_check_evidence_ledger docs/evidence.jsonl

for fuji_af_empty_ledger in '' $' \t\n\n'; do
  printf '%s' "$fuji_af_empty_ledger" > "$TEST_TMPDIR/empty-evidence.jsonl"
  if fuji_af_check_evidence_ledger "$TEST_TMPDIR/empty-evidence.jsonl"; then
    echo 'Record-less evidence ledger incorrectly accepted' >&2
    exit 1
  fi
done

fuji_af_duplicate_ids="$("$fuji_af_jq" -rs 'group_by(.id) | map(select(length > 1) | .[0].id) | .[]' docs/evidence.jsonl)"
if [[ -n "$fuji_af_duplicate_ids" ]]; then
  printf 'Duplicate evidence ids:\n%s\n' "$fuji_af_duplicate_ids" >&2
  exit 1
fi

# A record replaces an older one on the same topic rather than sitting beside it.
fuji_af_duplicate_topics="$("$fuji_af_jq" -rs '
  map(select(.topic != null and .topic != ""))
  | group_by(.topic)
  | map(select(length > 1) | "\(.[0].topic): \(map(.id) | join(", "))")
  | .[]' docs/evidence.jsonl)"
if [[ -n "$fuji_af_duplicate_topics" ]]; then
  printf 'Duplicate evidence topics (topic: ids):\n%s\n' "$fuji_af_duplicate_topics" >&2
  exit 1
fi

# Every E- token in a tracked file, the ledger included, names a current record. Tokens followed by a signed
# or decimal number are camera key names. E-D is the exposure-compensation dial; E-MODEL is a placeholder file name.
# The runfiles ledger links back to the checkout.
fuji_af_ledger="$(readlink -f docs/evidence.jsonl)"
"$fuji_af_evidence_record" check-available "${fuji_af_ledger%/docs/evidence.jsonl}" docs/evidence-recipes.json docs/evidence.jsonl replacement/README.md +RTS -M1G -RTS
"$fuji_af_git" -C "${fuji_af_ledger%/docs/evidence.jsonl}" grep -I -n -o -P \
  '(?<![A-Za-z0-9_-])E-[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*(?![A-Za-z0-9_]|[-+=.][0-9])' \
  > "$TEST_TMPDIR/citations" || [[ $? -eq 1 ]]
fuji_af_dangling_citations() {
  "$fuji_af_jq" -rs --rawfile citations "$1" '
    (map({key: .id, value: true}) | from_entries + {"E-D": true, "E-MODEL": true}) as $known
    | $citations | split("\n")[] | select(. != "" and ($known[split(":") | last] | not))' docs/evidence.jsonl
}
fuji_af_dangling="$(fuji_af_dangling_citations "$TEST_TMPDIR/citations")"
if [[ -n "$fuji_af_dangling" ]]; then
  printf 'Evidence ids cited but not in docs/evidence.jsonl (file:line:id):\n%s\n' "$fuji_af_dangling" >&2
  exit 1
fi
printf 'reports/example.md:1:%s-NOT-A-RECORD-150\n' E >> "$TEST_TMPDIR/citations"
if [[ -z "$(fuji_af_dangling_citations "$TEST_TMPDIR/citations")" ]]; then
  echo 'Citation of a missing evidence id incorrectly accepted' >&2
  exit 1
fi

# Each qualified composed contract cites an established record that names the digest's first 8 hex digits and binds it
# at a composed-contract.json outside ctl-/restage- directories.
fuji_af_qualifications_cited() {
  "$fuji_af_jq" -n -e --slurpfile q "$1" --slurpfile ledger "$2" '
    ($ledger | map({key: .id, value: .}) | from_entries) as $r
    | $q[0] | .schema == 1 and keys == ["contracts", "schema"]
      and (.contracts | map(.contract) | length == (unique | length))
      and all(.contracts[]; keys == ["contract", "evidence"] and (.contract | test("^[0-9a-f]{64}$"))
        and (.contract as $c | $r[.evidence] | . != null and .status == "established"
          and (.claim | contains($c[0:8]))
          and any(.artifacts[]?; .sha256 == $c and (.path | test("/composed-contract\\.json$"))
            and (.path | test("/(ctl|restage)-") | not))))' > /dev/null
}
fuji_af_qualifications=replacement/startup_artifact/qualifications.json
if ! fuji_af_qualifications_cited "$fuji_af_qualifications" docs/evidence.jsonl; then
  echo "$fuji_af_qualifications cites a contract its record does not qualify" >&2
  exit 1
fi
for fuji_af_mutation in \
  '.contracts[0].evidence = ("E" + "-NOT-A-RECORD-150")' \
  '.contracts[0].evidence = "E-STARTUP-J3-COMPOSED-NATURAL-150"' \
  '.contracts[4].contract = "238369f76fc57fa4f76694fb5de5aa407c6b7c72aada96f499ebf5f45626cb1b"' \
  '.contracts += [.contracts[0]]'; do
  "$fuji_af_jq" "$fuji_af_mutation" "$fuji_af_qualifications" > "$TEST_TMPDIR/qualifications.json"
  if fuji_af_qualifications_cited "$TEST_TMPDIR/qualifications.json" docs/evidence.jsonl; then
    echo "Unqualified contract row incorrectly accepted: $fuji_af_mutation" >&2
    exit 1
  fi
done
for fuji_af_mutation in \
  'if .id == "E-STARTUP-J1-COMPOSED-NATURAL-150" then .status = "open" else . end' \
  'if .id == "E-STARTUP-J1-COMPOSED-NATURAL-150" then (.artifacts[] | select(.path | endswith("/j1/stage/composed-contract.json")) | .path) |= sub("/j1/"; "/ctl-j1/") else . end'; do
  "$fuji_af_jq" -c "$fuji_af_mutation" docs/evidence.jsonl > "$TEST_TMPDIR/evidence.jsonl"
  if fuji_af_qualifications_cited "$fuji_af_qualifications" "$TEST_TMPDIR/evidence.jsonl"; then
    echo "Qualification by an unqualifying record incorrectly accepted: $fuji_af_mutation" >&2
    exit 1
  fi
done

fuji_af_markers="$("$fuji_af_git" -C "${fuji_af_ledger%/docs/evidence.jsonl}" grep -I -n -E \
  '^(<<<<<<< |>>>>>>> |=======$)' || [[ $? -eq 1 ]])"
if [[ -n "$fuji_af_markers" ]]; then
  printf 'Merge conflict markers in tracked files (file:line):\n%s\n' "$(cut -d: -f1,2 <<< "$fuji_af_markers")" >&2
  exit 1
fi

"$fuji_af_jq" '.sources[0].revision = null' manifests/sources.json > "$TEST_TMPDIR/invalid.json"
if "$fuji_af_schema_checker" --schemafile schemas/sources.schema.json "$TEST_TMPDIR/invalid.json"; then
  echo 'Selected source without a commit incorrectly accepted' >&2
  exit 1
fi

"$fuji_af_jq" '(.artifacts[] | select(.model == "X-T1") | .matches_installed) = true' manifests/firmware.json > "$TEST_TMPDIR/invalid.json"
if "$fuji_af_schema_checker" --schemafile schemas/firmware.schema.json "$TEST_TMPDIR/invalid.json"; then
  echo 'Sibling firmware claimed as installed incorrectly accepted' >&2
  exit 1
fi

# This public design draft must not claim a frozen or authorized camera session,
# discard failed attempts, or equate confirmation with acceptable-focus onset.
for fuji_af_mutation in \
  '.status = "frozen"' \
  '.authorization.camera_operations_authorized = true' \
  '.attempt_policy.all_attempts_retained = false' \
  '.design.repeats_per_combination = 0' \
  '.design.repeats_per_combination = 100' \
  '.design.planned_attempts = 0' \
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
  '.design.planned_attempts += 1' \
  '.design.combinations[0].scenario = "indoor"'; do
  "$fuji_af_jq" "$fuji_af_mutation" manifests/benchmark-protocol.json > "$TEST_TMPDIR/invalid.json"
  if "$fuji_af_jq" -e "$fuji_af_protocol_consistency" "$TEST_TMPDIR/invalid.json" > /dev/null; then
    echo "Inconsistent pilot combination incorrectly accepted: $fuji_af_mutation" >&2
    exit 1
  fi
done

# New code cannot define its own output root: the literals live only in the admission module and in
# tests/output-root-allowlist.txt, which shrinks as writers move to run-output.
fuji_af_checkout="${fuji_af_ledger%/docs/evidence.jsonl}"
fuji_af_output_root_allowlist=tests/output-root-allowlist.txt
fuji_af_output_root_literals=(private-data/scratch private-data/verify-candidate/runs)

declare -A fuji_af_output_root_allowed=()
fuji_af_output_root_errors=()
while IFS= read -r fuji_af_allowlist_line; do
  fuji_af_allowlist_entry="${fuji_af_allowlist_line%%#*}"
  fuji_af_allowlist_entry="${fuji_af_allowlist_entry#"${fuji_af_allowlist_entry%%[![:space:]]*}"}"
  fuji_af_allowlist_entry="${fuji_af_allowlist_entry%"${fuji_af_allowlist_entry##*[![:space:]]}"}"
  [[ -n "$fuji_af_allowlist_entry" ]] || continue
  fuji_af_output_root_allowed["$fuji_af_allowlist_entry"]=1
  if [[ ! -f "$fuji_af_checkout/$fuji_af_allowlist_entry" ]]; then
    [[ "$fuji_af_allowlist_line" == *'# pending'* ]] || fuji_af_output_root_errors+=("$fuji_af_allowlist_entry: file does not exist")
    continue
  fi
  grep -F -q -e "${fuji_af_output_root_literals[0]}" -e "${fuji_af_output_root_literals[1]}" \
    "$fuji_af_checkout/$fuji_af_allowlist_entry" ||
    fuji_af_output_root_errors+=("$fuji_af_allowlist_entry: names no output root")
done < "$fuji_af_output_root_allowlist"

fuji_af_output_root_files="$("$fuji_af_git" -C "$fuji_af_checkout" grep -l -F \
  -e "${fuji_af_output_root_literals[0]}" -e "${fuji_af_output_root_literals[1]}" \
  -- '*.hs' '*.rs' '*.sh' '*.jq' '*.nix' '*.bzl' '*.bazel' 'BUILD' ':(glob)**/BUILD' \
  ':(exclude)tests/research-contracts.sh' || [[ $? -eq 1 ]])"
fuji_af_output_root_offenders=()
while IFS= read -r fuji_af_output_root_file; do
  [[ -n "$fuji_af_output_root_file" ]] || continue
  [[ -n "${fuji_af_output_root_allowed[$fuji_af_output_root_file]:-}" ]] ||
    fuji_af_output_root_offenders+=("$fuji_af_output_root_file: names an output root")
done <<< "$fuji_af_output_root_files"

fuji_af_output_root_failed=
if (( ${#fuji_af_output_root_offenders[@]} > 0 )); then
  printf 'Code files outside %s that name an output root:\n' "$fuji_af_output_root_allowlist" >&2
  printf '%s\n' "${fuji_af_output_root_offenders[@]}" >&2
  fuji_af_output_root_failed=1
fi
if (( ${#fuji_af_output_root_errors[@]} > 0 )); then
  printf 'Stale entries in %s:\n' "$fuji_af_output_root_allowlist" >&2
  printf '%s\n' "${fuji_af_output_root_errors[@]}" >&2
  fuji_af_output_root_failed=1
fi
[[ -z "$fuji_af_output_root_failed" ]] || exit 1

# Every firmware unit keeps a shim table in replacement/shims.md. A unit is a directory under replacement/
# whose BUILD.bazel declares a firmware artifact: a firmware toolchain rule, the ARM Rust compiler, or a
# module/sources filegroup linked into one. A unit that lands without a table fails here.
fuji_af_shim_register="$fuji_af_checkout/replacement/shims.md"
fuji_af_shim_units=()
while IFS= read -r fuji_af_unit_build; do
  fuji_af_unit_dir="${fuji_af_unit_build%/BUILD.bazel}"
  fuji_af_unit="${fuji_af_unit_dir#replacement/}"
  if [[ "$fuji_af_unit" == */* ]]; then continue; fi
  if ! "$fuji_af_git" -C "$fuji_af_checkout" ls-files "$fuji_af_unit_dir" | grep -qE '\.(rs|S)$'; then continue; fi
  if grep -qE 'xm1_rust_(elf|staticlib|rlib)\(|@lab_xm1_rust|name = "(module|sources)"' \
    "$fuji_af_checkout/$fuji_af_unit_build"; then fuji_af_shim_units+=("$fuji_af_unit"); fi
done < <("$fuji_af_git" -C "$fuji_af_checkout" ls-files 'replacement/*/BUILD.bazel')

fuji_af_shim_kinds='stock-ABI export|veneer or carrier at a stock address|trampoline into retained stock|callback registry for guest words|kernel interface slot|guest-memory layout view|retained stock seam in assembly|C-ABI bridge between two owned units'
fuji_af_shim_errors=()
if [[ ! -f "$fuji_af_shim_register" ]]; then
  fuji_af_shim_errors+=("replacement/shims.md: missing")
elif ! grep -qx '## Owned on both sides' "$fuji_af_shim_register"; then
  fuji_af_shim_errors+=("replacement/shims.md: missing the Owned on both sides section")
else
  for fuji_af_unit in "${fuji_af_shim_units[@]}"; do
    fuji_af_section="$(awk -v heading="## $fuji_af_unit" '$0 == heading { found = 1; next } /^## / { found = 0 } found' "$fuji_af_shim_register")"
    if [[ -z "$fuji_af_section" ]]; then
      fuji_af_shim_errors+=("replacement/shims.md: no table for $fuji_af_unit")
      continue
    fi
    if ! grep -qF '| Shim | Kind | Other side | Owned now | Removal condition | State |' <<< "$fuji_af_section"; then
      fuji_af_shim_errors+=("replacement/shims.md: $fuji_af_unit has no shim table header")
      continue
    fi
    while IFS= read -r fuji_af_row; do
      [[ "$fuji_af_row" == '| '* ]] || continue
      if [[ "$fuji_af_row" == *'---'* ]]; then continue; fi
      fuji_af_kind="$(awk -F'|' '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $3); print $3 }' <<< "$fuji_af_row")"
      if [[ "$fuji_af_kind" == 'Kind' ]]; then continue; fi
      fuji_af_state="$(awk -F'|' '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $7); print $7 }' <<< "$fuji_af_row")"
      grep -qxE "$fuji_af_shim_kinds" <<< "$fuji_af_kind" ||
        fuji_af_shim_errors+=("replacement/shims.md: $fuji_af_unit has unknown kind '$fuji_af_kind'")
      grep -qxE 'required|condition met|removed' <<< "$fuji_af_state" ||
        fuji_af_shim_errors+=("replacement/shims.md: $fuji_af_unit has unknown state '$fuji_af_state'")
    done <<< "$fuji_af_section"
  done
fi
if (( ${#fuji_af_shim_errors[@]} > 0 )); then
  printf 'Shim register errors:\n%s\n' "${fuji_af_shim_errors[@]}" >&2
  exit 1
fi
