#!/usr/bin/env bash
# bazel run //emulation:record-medium-boot-check -- --output NEW_DIR [--record-nand NAND]
#   [--firmware DAT] [--adjustment-nand NAND]
# Fresh-boots fujimod-xm1-ram-push in one emulator slot to its attach point and
# writes the record-area reads below to NEW_DIR/reads.json. The defaults are the
# stock 1.50 DAT and the stored-f7 adjustment NAND.
set -euo pipefail

if [[ ${1:-} != --in-slot ]]; then
  slot=$(realpath "$1")
  self=$(realpath "$0")
  jq=$(realpath "$2") bridge=$(realpath "$3") run=$(realpath "$4")
  shift 4
  cd "${BUILD_WORKSPACE_DIRECTORY:?run through bazel run}"
  exec "$slot" "$self" --in-slot "$jq" "$bridge" "$run" "$@"
fi
jq=$2 bridge=$3 run=$4
shift 4
dat=private-data/firmware/x-m1-1.50/FWUP0002.DAT
adjustment=private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand
rom=private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin
output='' record=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --output) output=$2 ;;
    --record-nand) record=$2 ;;
    --firmware) dat=$2 ;;
    --adjustment-nand) adjustment=$2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift 2
done
if [[ -z $output || -e $output ]]; then
  echo "--output must name a new directory" >&2
  exit 2
fi
mkdir -p "$output"

arguments=(--run "$run" --boot-rom "$rom" --installed-dat "$dat" --adjustment-nand "$adjustment"
  --output "$output/bridge" --ram-push-checkpoints "$output/checkpoints" --fresh-boot)
if [[ -n $record ]]; then
  arguments+=(--record-nand "$record")
fi

# name address count; the map is its 16-byte header and 0x6c entries.
reads=(
  "map-pointer 0x00919a20 4"
  "map 0x0095dae0 232"
  "area2-rejected 0x0089f834 4"
  "wrappers 0x0089f8b0 64"
  "kind2-staging 0x008e1700 512"
)

coproc BRIDGE { "$bridge" "${arguments[@]}" 2>"$output/bridge.stderr"; }
bridge_pid=$BRIDGE_PID
exec {from_bridge}<&"${BRIDGE[0]}" {to_bridge}>&"${BRIDGE[1]}"
line=''
# Sets `line` to the bridge's next reply.
expect() {
  IFS= read -r line <&"$from_bridge"
  printf '%s\n' "$line" >>"$output/transcript.jsonl"
  if [[ $("$jq" -r '.ok' <<<"$line") != true ]]; then
    echo "bridge refused: $line" >&2
    exit 1
  fi
}
expect
start_options=$line
printf '{"command":"start","installed":null}\n' >&"$to_bridge"
expect
results='{}'
for entry in "${reads[@]}"; do
  read -r name address count <<<"$entry"
  printf '{"command":"read-memory","address":%d,"count":%d}\n' "$address" "$count" >&"$to_bridge"
  expect
  results=$("$jq" -c --arg name "$name" --arg address "$address" --argjson reply "$line" \
    '. + {($name): {address: $address, bytes: $reply.bytes}}' <<<"$results")
done
printf '{"command":"finish"}\n' >&"$to_bridge"
expect
wait "$bridge_pid"
"$jq" --argjson options "$start_options" '{record_nand: $options.record_nand, reads: .}' \
  <<<"$results" >"$output/reads.json"
