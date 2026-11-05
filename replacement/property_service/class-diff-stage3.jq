# Checks a table-gen classification against the stage-2 contract's, record by record (class, reason
# and field) for all 645 properties:
#   jq -n --slurpfile old STAGE2_CONTRACT --slurpfile new CLASSIFICATION -f replacement/property_service/class-diff-stage3.jq
# STAGE2_CONTRACT is private-data/scratch/PropertyService2/stage-1/contract.json. A record may change
# only when the contract has it delegated: to an owned class, or to delegated with another reason.
# Fails with every other change; otherwise lists each change and the new summary.

($old[0].classification.properties) as $o
| ($new[0].properties) as $n
| if ($o | map(.property)) != ($n | map(.property)) or ($n | length) != 645 then error("the classifications list different property ids") else . end
| [range(0; 645) as $i | select($o[$i] != $n[$i]) | {property: $n[$i].property, old: $o[$i], new: $n[$i]}]
| map(. + {allowed: (.old.class == "delegated")})
| {
    to_owned: [.[] | select(.new.class != "delegated") | {property, class: .new.class, from: .old.reason}],
    reason_changes: [.[] | select(.new.class == "delegated") | {property, old: .old.reason, new: .new.reason}],
    refused: [.[] | select(.allowed | not) | {property, old: (.old | {class, reason}), new: (.new | {class, reason})}],
    summary: $new[0].summary
  }
| if (.refused | length) > 0 then error("disallowed class changes: " + (.refused | tostring)) else .pass = true end
