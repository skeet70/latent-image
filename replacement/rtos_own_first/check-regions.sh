#!/usr/bin/env bash
# Checks regions.json against the symbols of both links.
set -euo pipefail

jq=$1
regions=$2
declare -A symbols=([a]=$3 [b]=$4)

status=0
for domain in a b; do
  table=$("$jq" -Rn '[inputs | split(" ") | select(length >= 3) | {key: .[-1], value: ("0x" + (.[0] | sub("^0+(?=.)"; "")))}] | from_entries' < "${symbols[$domain]}")
  failures=$("$jq" -r --arg domain "$domain" --argjson sym "$table" '
    def at(name): $sym[name] // "missing";
    def check(what; actual; expected): if actual == expected then empty else "\($domain) \(what): \(actual) != \(expected)" end;
    .domains[$domain] as $d
    | ($d.regions[] | (.slot | ltrimstr("0x")) as $k
        | check("\(.slot) entry"; at(.symbol); .entry),
          check("\(.slot) start"; .start; .entry),
          check("\(.slot) end"; at("__slot_\($k)_end"); .end),
          (if (.frame_bytes | type) == "number" then empty else "\($domain) \(.slot) frame_bytes missing" end)),
      ($d.tail
        | check("tail entry"; at(.symbol); .address),
          check("tail end"; at("__slot_1618_end"); .end),
          check("tail body"; at(.body.symbol); .body.address),
          check("tail body start"; at("__slot_1618_body_start"); .body.address),
          (.body.host_slot as $host | check("tail body end"; ($d.regions[] | select(.slot == $host) | .end); .body.end))),
      (if ($d.regions | length) == 16 then empty else "\($domain) region count \($d.regions | length)" end)
  ' "$regions")
  if [[ -n $failures ]]; then
    echo "$failures" >&2
    status=1
  fi
done
exit $status
