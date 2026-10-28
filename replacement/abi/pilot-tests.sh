#!/usr/bin/env bash
set -euo pipefail

native_case=$1
runner=$2
pilot_bin=$3
pilot_elf=$4
pilot_symbols=$5
pilot_disasm=$6
defect_bin=$7
template=$8
jq=$9
readelf=${10}
sha256sum=${11}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
    printf 'pilot: %s\n' "$*" >&2
    exit 1
}

show() {
    printf -- '--- report ---\n' >&2
    cat "$1/report.json" >&2
    printf -- '--- stderr ---\n' >&2
    cat "$1/stderr.txt" >&2
}

run_case() {
    local bin=$1 dir=$2 rc
    mkdir -p "$dir"
    cp "$bin" "$dir/pilot.bin"
    local size sha
    size=$(stat -L -c %s "$dir/pilot.bin")
    sha=$("$sha256sum" "$dir/pilot.bin" | cut -d' ' -f1)
    sed -e "s/@SIZE@/$size/g" -e "s/@SHA@/$sha/g" "$template" > "$dir/pilot.json"
    set +e
    "$native_case" --runner "$runner" "$dir/pilot.json" > "$dir/report.json" 2> "$dir/stderr.txt"
    rc=$?
    set -e
    return "$rc"
}

[ "$(stat -L -c %s "$pilot_bin")" -eq 3840 ] || fail "flat image is not the declared 0xf00-byte span"
if ! awk '$4 == "xm1_abi_probe" { found = 1; if ($1 != "00001000") exit 1 } END { exit !found }' "$pilot_symbols"; then
    fail "symbol list does not place xm1_abi_probe at 0x1000"
fi

attributes=$("$readelf" -A "$pilot_elf")
if printf '%s\n' "$attributes" | grep -q 'ABI_VFP_args'; then
    fail "the pilot carries a VFP argument attribute"
fi
if ! printf '%s\n' "$attributes" | grep -q 'cortex-r4'; then
    fail "the pilot does not carry the cortex-r4 CPU attribute"
fi
if ! printf '%s\n' "$attributes" | grep -q 'ABI_align_preserved'; then
    fail "the pilot does not carry the 8-byte alignment attribute"
fi

for mnemonic in $(awk -F'\t' '/^[ ]*[0-9a-f]+:/ { print $2 }' "$pilot_disasm" | awk '{ print $1 }' | sort -u); do
    case "$mnemonic" in
        sdiv|udiv|mrc|mcr|ldrex|strex|ldrexd|strexd|dmb|dsb|isb|clrex|swp|swpb|svc|smc|eret|rfe|srs|v*)
            fail "linked artifact contains forbidden mnemonic $mnemonic"
            ;;
    esac
done

positive="$work/positive"
if ! run_case "$pilot_bin" "$positive"; then
    show "$positive"
    fail "the qualified artifact did not pass its native case"
fi
status=$("$jq" -r '.status' "$positive/report.json")
runner_status=$("$jq" -r '.runner.status' "$positive/report.json")
mismatches=$("$jq" -r '.mismatches | length' "$positive/report.json")
instructions=$("$jq" -r '.runner.instructions' "$positive/report.json")
[ "$status" = passed ] || { show "$positive"; fail "positive case status $status"; }
[ "$runner_status" = returned ] || { show "$positive"; fail "positive runner status $runner_status"; }
[ "$mismatches" = 0 ] || { show "$positive"; fail "positive case has $mismatches mismatches"; }
printf 'positive: status=%s runner=%s instructions=%s\n' "$status" "$runner_status" "$instructions"
printf 'positive case sha256: %s\n' "$("$jq" -r '.case_sha256' "$positive/report.json")"

defect="$work/defect"
if run_case "$defect_bin" "$defect"; then
    show "$defect"
    fail "the ABI defect artifact was accepted"
fi
status=$("$jq" -r '.status' "$defect/report.json")
runner_status=$("$jq" -r '.runner.status' "$defect/report.json")
mismatches=$("$jq" -r '.mismatches | length' "$defect/report.json")
details=$("$jq" -r '.mismatches | join("; ")' "$defect/report.json")
[ "$status" = failed ] || { show "$defect"; fail "defect case status $status"; }
[ "$runner_status" = returned ] || { show "$defect"; fail "defect runner status $runner_status"; }
[ "$mismatches" -gt 0 ] || { show "$defect"; fail "defect case has no mismatches"; }
case "$details" in
    *"register r0"*|*"memory results"*) ;;
    *) show "$defect"; fail "defect was not rejected at the ABI result: $details" ;;
esac
printf 'defect: status=%s runner=%s mismatches=%s\n' "$status" "$runner_status" "$details"
