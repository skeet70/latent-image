# Checks a manifest whose paths carry ordered events against an earlier one whose paths carry
# separate stores and calls:
#   jq -n --slurpfile old OLD_MANIFEST --slurpfile new NEW_MANIFEST --slurpfile contract STAGE1_CONTRACT \
#     -f replacement/media_permit/settings-manifest-events-delta.jq
# Each new path's events are projected back to stores (the store events) and calls (the call
# targets), and both manifests' path lists are compared as sets. A property record may then differ
# only for an id the contract classifies as delegated. Fails with the refused ids; otherwise lists
# every changed id with its step-limit cuts before and after, its call targets, the fields its guards
# compare, and the unmodelled store sites its old record held and its new record does not; then the
# other top-level keys that differ.

def projected:
  walk(if type == "object" and has("events") and has("guards") then
         (. + {stores: [.events[] | .store // empty], calls: [.events[] | .call.target // empty]}) | del(.events)
       else . end);

def path_sets: walk(if type == "object" and has("paths") then .paths |= sort else . end);

def unmodelled_sites: [.. | objects | select(.kind == "unmodelled" and has("instruction")) | .instruction] | unique;

def step_limit_cuts: [.. | objects | select(.kind == "cut" and .reason == "step-limit")] | length;

def call_targets: [.. | objects | select(has("call")) | .call.target] | unique;

def guard_fields: [.. | objects | select(has("subject")) | .subject | select(.kind == "field") | .location | "\(.base)+\(.offset)"] | unique;

($old[0] | path_sets) as $o
| ($new[0] | projected | path_sets) as $n
| ([$contract[0].classification.properties[] | select(.class == "delegated") | .property]) as $delegated
| if ($o.properties | map(.property)) != ($n.properties | map(.property)) then error("the manifests list different property ids") else . end
| [range(0; $n.properties | length) as $i
    | select($o.properties[$i] != $n.properties[$i])
    | {property: $n.properties[$i].property,
       permitted: ($n.properties[$i].property | IN($delegated[])),
       step_limit_cuts: {old: ($o.properties[$i] | step_limit_cuts), new: ($n.properties[$i] | step_limit_cuts)},
       call_targets: ($new[0].properties[$i] | call_targets),
       guard_fields: ($new[0].properties[$i] | guard_fields),
       sites_now_modelled: (($o.properties[$i] | unmodelled_sites) - ($n.properties[$i] | unmodelled_sites))}] as $changed
| {
    delegated: ($delegated | length),
    changed: [$changed[] | del(.permitted)],
    changed_count: ($changed | length),
    refused: [$changed[] | select(.permitted | not) | .property],
    other_keys_changed: [($o | keys[]) as $k | select($k != "properties" and $o[$k] != $n[$k]) | $k]
  }
| if (.refused | length) > 0 then error("changed records of ids that are not delegated: " + (.refused | join(", "))) else .pass = true end
