#!/usr/bin/env bash
# Checks regions.json against the symbols and disassembly of both links: JQ REGIONS SYMBOLS_A SYMBOLS_B DISASM_A DISASM_B.
set -euo pipefail

jq=$1
regions=$2
declare -A symbols=([a]=$3 [b]=$4)
declare -A disasm=([a]=$5 [b]=$6)

# Stock raise IRQ pushes r11 only.
raise_irq_frame=4

# Peak stack bytes below entry of each function: its pushes plus its deepest `bl` callee, or the
# frame of the function a `b` leaves to after restoring sp, whichever is larger.
frames() {
  "$jq" -Rs --argjson raise "$raise_irq_frame" '
    def branch: "(eq|ne|hs|lo|hi|ls|ge|lt|gt|le)?$";
    def target: [.rest | capture("<(?<t>[^>+]+)>").t][0];
    reduce split("\n")[] as $line ({cur: null, f: {}};
      [$line | capture("^[0-9a-f]+ <(?<n>[^>+]+)>:$").n][0] as $head
      | [$line | capture("^\\s*[0-9a-f]+:\\s+(?<op>[a-z.]+)\\s*(?<rest>.*)$")][0] as $i
      | if $head then .cur = $head | .f[$head] = {own: 0, calls: [], tail: null}
        elif .cur == null or $i == null then .
        elif ($i.op | test("^push")) and .f[.cur].own == 0 then
          .f[.cur].own = 4 * ($i.rest | capture("\\{(?<r>[^}]*)\\}").r | split(",") | length)
        elif ($i.op | test("^bl" + branch)) and ($i | target) then .f[.cur].calls += [$i | target]
        elif ($i.op | test("^b" + branch)) and ($i | target) then .f[.cur].tail = ($i | target)
        else . end)
    | .f as $f
    | def frame($g):
        if $g == "xm1_rtos_scheduler_stock_raise_irq" then $raise
        else $f[$g] as $e
          | ($e.own + ([0, ($e.calls[] | frame(.))] | max)) as $t
          | if $e.tail then [$t, frame($e.tail)] | max else $t end
        end;
      $f | with_entries(.value = frame(.key))
  ' < "$1"
}

status=0
for domain in a b; do
  table=$("$jq" -Rn '[inputs | split(" ") | select(length >= 3) | {key: .[-1], value: ("0x" + (.[0] | sub("^0+(?=.)"; "")))}] | from_entries' < "${symbols[$domain]}")
  walked=$(frames "${disasm[$domain]}")
  failures=$("$jq" -r --arg domain "$domain" --argjson sym "$table" --argjson frame "$walked" '
    def at(name): $sym[name] // "missing";
    def check(what; actual; expected): if actual == expected then empty else "\($domain) \(what): \(actual) != \(expected)" end;
    .domains[$domain] as $d
    | ($d.regions[]
        | check("\(.helper) entry"; at(.symbol); .entry),
          check("\(.helper) start"; .start; .entry),
          check("\(.helper) end"; at("__\(.helper)_end"); .end),
          check("\(.helper) frame_bytes"; $frame[.symbol] // "missing"; .frame_bytes)),
      check("stock raise IRQ"; at("xm1_rtos_scheduler_stock_raise_irq"); $d.stock_raise_irq),
      ($d.shims[]
        | .helper as $helper
        | .body.host_helper as $host
        | check("\($helper) branch"; at(.branch.symbol); ($d.regions[] | select(.helper == $helper) | .entry)),
          check("\($helper) body"; at(.body.symbol); .body.address),
          check("\($helper) body start"; at("__\($helper)_body_start"); .body.address),
          check("\($helper) body end"; ($d.regions[] | select(.helper == $host) | .end); .body.end),
          check("\($helper) body frame_bytes"; $frame[.body.symbol] // "missing"; .body.frame_bytes)),
      (if ($d.regions | length) == 10 then empty else "\($domain) region count \($d.regions | length)" end)
  ' "$regions")
  if [[ -n $failures ]]; then
    echo "$failures" >&2
    status=1
  fi
done
exit $status
