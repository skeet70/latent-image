#!/usr/bin/env bash
# ledger-apply.sh LEDGER TRANSFORM: apply an id-keyed curation transform to an evidence ledger in place.
# TRANSFORM is one JSON object: {"remove": [ID...], "replace": [RECORD...], "add": [RECORD...]}.
# remove and replace ids must each name one existing record; add ids must be new; no id may appear twice.
# Replaced records keep their line, added records go at the end and untouched lines keep their bytes.
# JQ names the jq binary, defaulting to jq on PATH.
set -euo pipefail
ledger="${1:?usage: ledger-apply.sh LEDGER TRANSFORM}"
transform="${2:?usage: ledger-apply.sh LEDGER TRANSFORM}"
jq="${JQ:-jq}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

"$jq" -r .id "$ledger" > "$work/ids"
if [[ "$(wc -l < "$work/ids")" -ne "$(wc -l < "$ledger")" ]]; then
  printf 'ledger-apply: %s must hold exactly one record per line\n' "$ledger" >&2
  exit 1
fi

"$jq" -r --rawfile ids "$work/ids" '
  ($ids | split("\n") | map(select(. != ""))) as $have
  | ($have | map({key: ., value: true}) | from_entries) as $known
  | ([.remove[]?] + [.replace[]?.id] + [.add[]?.id]) as $named
  | [ ((keys - ["add", "remove", "replace"])[] | "unknown transform key: " + .),
      ($have | group_by(.) | map(select(length > 1))[] | "duplicate id in ledger: " + .[0]),
      ($named | group_by(.) | map(select(length > 1))[] | "id named more than once in transform: " + .[0]),
      ($named[] | select(type != "string") | "record without a string id"),
      ((.remove[]?, .replace[]?.id) | strings | select($known[.] | not) | "no such id: " + .),
      (.add[]?.id | strings | select($known[.]) | "id already exists: " + .) ]
  | .[]' "$transform" > "$work/errors"
if [[ -s "$work/errors" ]]; then
  sed 's/^/ledger-apply: /' "$work/errors" >&2
  exit 1
fi

"$jq" -r '(.remove[]? | "R\t" + .), (.replace[]? | "P\t" + .id + "\t" + tojson)' "$transform" > "$work/ops"
paste "$work/ids" "$ledger" | awk -F'\t' '
  NR == FNR { if ($1 == "R") gone[$2] = 1; else swap[$2] = substr($0, length($2) + 4); next }
  { id = $1; line = substr($0, length(id) + 2) }
  id in gone { next }
  id in swap { print swap[id]; next }
  { print line }' "$work/ops" - > "$work/out"
"$jq" -c '.add[]?' "$transform" >> "$work/out"
mv "$work/out" "$ledger"
