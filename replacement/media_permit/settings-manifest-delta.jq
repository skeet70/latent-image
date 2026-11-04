# Checks a regenerated settings manifest against an earlier one whose locations carry
# indexArgument/indexStride instead of terms:
#   jq -n --slurpfile old OLD_MANIFEST --slurpfile new NEW_MANIFEST \
#     -f replacement/media_permit/settings-manifest-delta.jq
# After the old locations are rewritten to terms, every property record that differs must have held,
# in the old manifest, an `other` value or source or an unmodelled store or guard. Fails with the
# refused ids; otherwise prints every changed id and the other top-level keys that differ.

def terms_form:
  walk(if type == "object" and has("indexArgument") then
         (. + {terms: (if .indexArgument == null then [] else [{argument: .indexArgument, stride: .indexStride}] end)})
         | del(.indexArgument, .indexStride)
       else . end);

def held_unmodelled: [.. | objects | select(.kind == "other" or .kind == "unmodelled")] | length > 0;

($old[0] | terms_form) as $o
| $new[0] as $n
| if ($o.properties | map(.property)) != ($n.properties | map(.property)) then error("the manifests list different property ids") else . end
| [range(0; $n.properties | length) as $i
    | select($o.properties[$i] != $n.properties[$i])
    | {property: $n.properties[$i].property, permitted: ($o.properties[$i] | held_unmodelled)}] as $changed
| {
    changed: [$changed[].property],
    changed_count: ($changed | length),
    refused: [$changed[] | select(.permitted | not) | .property],
    other_keys_changed: [($o | keys[]) as $k | select($k != "properties" and $o[$k] != $n[$k]) | $k]
  }
| if (.refused | length) > 0 then error("changed records that held no unmodelled value: " + (.refused | join(", "))) else .pass = true end
