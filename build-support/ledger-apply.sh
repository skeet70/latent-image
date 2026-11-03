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

refuse() {
  printf 'ledger-apply: %s\n' "$@" >&2
  exit 1
}

[[ -s "$ledger" && "$(tail -c 1 "$ledger" | od -An -c | tr -d ' ')" == '\n' ]] || refuse "$ledger must end with a newline"
awk 'NF == 0 { exit 1 }' "$ledger" || refuse "$ledger has a blank line"
"$jq" -r .id "$ledger" > "$work/ids"
[[ "$(wc -l < "$work/ids")" -eq "$(wc -l < "$ledger")" ]] || refuse "$ledger must hold exactly one record per line"
"$jq" -s -e 'length == 1 and (.[0] | type) == "object"' "$transform" > /dev/null || refuse "$transform must hold one JSON object"

"$jq" -r --rawfile ids "$work/ids" '
  def bad_id: type != "string" or test("[\t\n]") or length == 0;
  ($ids | split("\n") | map(select(. != ""))) as $have
  | ($have | map({key: ., value: true}) | from_entries) as $known
  | [ ((keys - ["add", "remove", "replace"])[] | "unknown transform key: " + .),
      (("add", "remove", "replace") as $k | select(has($k) and (.[$k] | type) != "array") | $k + " must be an array"),
      ($have | group_by(.) | map(select(length > 1))[] | "duplicate id in ledger: " + .[0]),
      ($have[] | select(test("\t")) | "bad id in ledger: " + tojson)
    ] as $structural
  | if $structural != [] then $structural[] else
      ([.remove[]?] + [.replace[]? | .id?] + [.add[]? | .id?]) as $named
      | ( ((.replace[]?, .add[]?) | select(type != "object") | "replace and add entries must be records"),
          ($named[] | select(bad_id) | "bad id: " + tojson),
          ($named | map(select(bad_id | not)) | group_by(.) | map(select(length > 1))[] | "id named more than once in transform: " + .[0]),
          ((.remove[]?, (.replace[]? | .id?)) | select(bad_id | not) | select($known[.] | not) | "no such id: " + .),
          (.add[]? | .id? | select(bad_id | not) | select($known[.]) | "id already exists: " + .) )
    end' "$transform" > "$work/errors"
if [[ -s "$work/errors" ]]; then
  sed 's/^/ledger-apply: /' "$work/errors" >&2
  exit 1
fi

"$jq" -r '(.remove[]? | "R\t" + .), (.replace[]? | "P\t" + .id + "\t" + tojson)' "$transform" > "$work/ops"
paste "$work/ids" "$ledger" | awk -F'\t' '
  FILENAME == ARGV[1] { if ($1 == "R") gone[$2] = 1; else swap[$2] = substr($0, length($2) + 4); next }
  { id = $1; line = substr($0, length(id) + 2) }
  id in gone { next }
  id in swap { print swap[id]; next }
  { print line }' "$work/ops" - > "$work/out"
"$jq" -c '.add[]?' "$transform" >> "$work/out"
mv "$work/out" "$ledger"
