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
    | ($d.regions[]
        | check("\(.helper) entry"; at(.symbol); .entry),
          check("\(.helper) start"; .start; .entry),
          check("\(.helper) end"; at("__\(.helper)_end"); .end),
          (if (.frame_bytes | type) == "number" then empty else "\($domain) \(.helper) frame_bytes missing" end)),
      check("stock raise IRQ"; at("xm1_rtos_scheduler_stock_raise_irq"); $d.stock_raise_irq),
      ($d.shims[]
        | .helper as $helper
        | .body.host_helper as $host
        | check("\($helper) branch"; at(.branch.symbol); ($d.regions[] | select(.helper == $helper) | .entry)),
          check("\($helper) body"; at(.body.symbol); .body.address),
          check("\($helper) body start"; at("__\($helper)_body_start"); .body.address),
          check("\($helper) body end"; ($d.regions[] | select(.helper == $host) | .end); .body.end)),
      (if ($d.regions | length) == 10 then empty else "\($domain) region count \($d.regions | length)" end)
  ' "$regions")
  if [[ -n $failures ]]; then
    echo "$failures" >&2
    status=1
  fi
done
exit $status
