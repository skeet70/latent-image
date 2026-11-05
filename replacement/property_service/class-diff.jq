# Checks a table-gen classification against the stage-1 contract's, record by record (class, reason
# and field) for all 645 properties:
#   jq -n --slurpfile old STAGE1_CONTRACT --slurpfile new CLASSIFICATION --slurpfile survey SLOT_ARMS \
#     -f replacement/property_service/class-diff.jq
# STAGE1_CONTRACT is private-data/scratch/PropertyService/stage-3/contract.json and SLOT_ARMS
# private-data/scratch/PropertyService2/slot-arms.json. A record may change only from delegated to
# owned-slot-field for a surveyed slot arm, or keep delegated with another reason for 0x1e6 or a
# stage-1 exploration-incomplete:setter id. Fails with every other change; otherwise lists each change.

($old[0].classification.properties) as $o
| ($new[0].properties) as $n
| ($survey[0].properties | map(.property)) as $slots
| ([$o[] | select(.reason == "exploration-incomplete:setter") | .property] + ["0x1e6"]) as $reasonable
| if ($o | map(.property)) != ($n | map(.property)) or ($n | length) != 645 then error("the classifications list different property ids") else . end
| [range(0; 645) as $i | select($o[$i] != $n[$i]) | {property: $n[$i].property, old: $o[$i], new: $n[$i]}]
| map(. + {allowed: (
    (.old.class == "delegated" and .new.class == "owned-slot-field" and (.property | IN($slots[])))
    or (.old.class == "delegated" and .new.class == "delegated" and (.property | IN($reasonable[]))))})
| {
    to_slot_field: [.[] | select(.new.class == "owned-slot-field") | .property],
    reason_changes: [.[] | select(.new.class == "delegated") | {property, old: .old.reason, new: .new.reason}],
    refused: [.[] | select(.allowed | not) | {property, old: (.old | {class, reason}), new: (.new | {class, reason})}],
    surveyed_not_slot_field: ([$slots[]] - [.[] | select(.new.class == "owned-slot-field") | .property])
  }
| if (.refused | length) > 0 then error("disallowed class changes: " + (.refused | tostring)) else .pass = true end
