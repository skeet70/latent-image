#!/usr/bin/env bash
set -euo pipefail
jq=$1
sha256sum=$2
D=emulation/menu-engine-pair-trajectories
fixture=$D/default-sha256.json
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
legacy_output() {
  "$jq" 'walk(if type == "array" and length == 5 and (.[4] | type) == "object" and .[4].completion? == "presented-frame" then if (.[4] | keys) == ["completion"] then .[0:4] else .[4] |= del(.completion) end else . end)' "$tmp/output" > "$tmp/legacy"
  "$jq" -e 'all(.before_unload[] | select(.phase == "close-menu"); .steps[-1][4].completion == "presented-frame")' "$tmp/output" > /dev/null
}
while IFS=$'\t' read -r role name expected; do
  "$jq" -L "$D" --arg role "$role" -f "$D/picker.jq" "emulation/modal75-picker-trajectories/$name.json" > "$tmp/output"
  legacy_output
  read -r actual _ < <("$sha256sum" "$tmp/legacy")
  test "$actual" = "$expected"
  "$jq" -L "$D" --arg role "$role" --argjson render false -f "$D/picker.jq" "emulation/modal75-picker-trajectories/$name.json" > "$tmp/explicit"
  cmp "$tmp/output" "$tmp/explicit"
done < <("$jq" -r '.outputs[] | @tsv' "$fixture")
graph=$("$jq" -c '.oracle_graph' "$fixture")
config=$("$jq" -c '.oracle_config' "$fixture")
while IFS=$'\t' read -r role kind expected; do
  landing=null
  sample=null
  case "$kind" in landing|both) landing='[1,5,2]' ;; esac
  case "$kind" in sample|both) sample='[["ok",1,5,2],["back",1,5,2]]' ;; esac
  "$jq" -n -L "$D" --arg role "$role" --arg name synthetic --argjson graph "$graph" --argjson config "$config" \
    --argjson landing "$landing" --argjson sample "$sample" -f "$D/oracle.jq" > "$tmp/output"
  legacy_output
  read -r actual _ < <("$sha256sum" "$tmp/legacy")
  test "$actual" = "$expected"
done < <("$jq" -r '.oracles[] | @tsv' "$fixture")
for role in stock owned; do
  "$jq" -L "$D" --arg role "$role" --argjson render true --argjson render_banks '[1,3]' -f "$D/picker.jq" emulation/modal75-picker-trajectories/picker-15.json > "$tmp/render"
  "$jq" -e --arg role "$role" '
    (.before_unload[0].phase == "render-baseline") and
    ([.trace_sites[] | select(.[1] == "render-tree-entry") | .[2].reads[0]] == [{sp:0,bytes:28}]) and
    ([.trace_sites[] | select(.[1] == "render-tree-return") | .[2].reads[0]] == [{sp:196,bytes:68}]) and
    ([.trace_sites[] | select(.[1] == "render-picker-return") | .[2].reads[0]] == [{sp:156,bytes:68}]) and
    ([.trace_sites[] | select(.[1] == "render-picker-fn-return") | .[2].reads[0]] == [{sp:76,bytes:68}]) and
    ([.trace_sites[] | select(.[1] == "render-tree-return") | .[2].reads | length] == [20]) and
    ([.trace_sites[] | select(.[1] == "render-pane-entry") | [.[0],.[2].reads[2]]] == [["011662a0",["015e64d8",40]]]) and
    ([.trace_sites[] | select(.[1] == "render-pane-return") | [.[0],.[2].reads[0]]] == [["01158184",["015e650c",68]]]) and
    ([.trace_sites[] | select(.[1] == "render-pane-stock") | .[0]] == ["011662a4"]) and
    ([.trace_sites[] | select(.[1] | endswith("-adapter"))] | length == (if $role == "owned" then 3 else 0 end)) and
    all(.trace_sites[] | .[2].reads[]?; (if type == "object" then .bytes else .[1] end) <= 4096)
  ' "$tmp/render" > /dev/null
  if "$jq" -L "$D" --arg role "$role" --argjson render true --argjson render_banks '[19]' -f "$D/picker.jq" emulation/modal75-picker-trajectories/picker-15.json > /dev/null 2>&1; then exit 1; fi
done
for role in stock owned; do
  for name in fn fn-down fn-auto-open; do
    case "$name" in
      fn) key=fn; landing='[99,321,17]'; path='[]' ;;
      fn-down) key=down; landing='[99,321,18]'; path='[["fn",99,321,17]]' ;;
      fn-auto-open) key=right; landing='[36,330,0]'; path='[["fn",99,321,17],["down",99,321,18],["down",99,321,20]]' ;;
    esac
    receipt=$("$jq" -nc --arg key "$key" --argjson landing "$landing" '[{key:$key,key_reply:{menu:{menu_id:$landing[0],setting_id:$landing[1],cursor:$landing[2]}}}]')
    "$jq" -n -L "$D" --arg role "$role" --arg name "$name" --arg receipt_source synthetic --argjson receipt "$receipt" \
      --argjson sample '[["ok",1,5,2],["back",1,5,2]]' --argjson render true --argjson render_banks '[1]' \
      --slurpfile config "emulation/menu-oracle-$name.json" -f "$D/fn-oracle.jq" > "$tmp/fn"
    "$jq" -e --arg key "$key" --argjson landing "$landing" --argjson path "$path" '
      ([.before_unload[] | select(.phase == "oracle-path") | .steps[]] == $path) and
      ([.before_unload[] | select(.phase == "oracle-key") | .steps[]] == [[$key] + $landing]) and
      (.before_unload[-1].phase == "close-menu" and .before_unload[-1].steps == [["back"] + $landing + [{completion:"presented-frame"}]]) and
      (.before_unload[0].phase == "render-baseline")
    ' "$tmp/fn" > /dev/null
  done
done
if "$jq" -n -L "$D" --arg role stock --arg name fn-instant-af --arg receipt_source synthetic --argjson receipt "$receipt" \
  --argjson sample '[]' --slurpfile config emulation/menu-oracle-fn-auto-open.json -f "$D/fn-oracle.jq" > /dev/null 2>&1; then exit 1; fi
for role in stock owned; do
  "$jq" -L "$D" --arg role "$role" --argjson render true -f "$D/picker.jq" emulation/modal75-picker-trajectories/picker-260.json > "$tmp/picker"
  "$jq" --arg role "$role" --arg route close-pop -f "$D/value.jq" "$tmp/picker" > "$tmp/value"
  "$jq" -e --arg role "$role" '
    ([.before_unload[] | [.phase, .steps]] == [
      ["render-baseline", []], ["package-menu-prefix", []],
      ["picker-route", [["ok",1,5,2],["down",1,5,3]]],
      ["value-open", [["ok",1,246,0]]], ["value-close", [["half-press",1,246,0]]],
      ["value-reopen", [["ok",1,5,3]]], ["other-route", [["down",1,5,4],["down",1,5,5]]],
      ["other-open", [["ok",1,260,0]]], ["other-move", [["down",1,260,1]]],
      ["other-cancel", [["back",1,5,5]]], ["other-reopen", [["ok",1,260,0]]],
      ["other-leave", [["back",1,5,5]]], ["close-menu", [["back",1,5,5,{completion:"presented-frame"}]]]]) and
    ([.trace_sites[] | select(.[0] == "00d51b58") | .[1]] == ["stock-value-size-arm"]) and
    ([.trace_sites[] | select(.[1] == "owned-value-apply")] | length == (if $role == "owned" then 1 else 0 end))
  ' "$tmp/value" > /dev/null
  "$jq" --arg role "$role" -f "$D/value.jq" "$tmp/picker" > "$tmp/sites"
  "$jq" -e --slurpfile picker "$tmp/picker" '.before_unload == $picker[0].before_unload and ([.trace_sites[] | select(.[0] == "00d51b58")] | length == 1)' "$tmp/sites" > /dev/null
  "$jq" -L "$D" --arg role "$role" -f "$D/picker.jq" emulation/modal75-picker-trajectories/picker-15.json > "$tmp/other"
  if "$jq" --arg role "$role" --arg route close-pop -f "$D/value.jq" "$tmp/other" > /dev/null 2>&1; then exit 1; fi
done
printf 'default trajectory bytes and opt-in render captures pass\n'
