#!/usr/bin/env bash
# The marker-0 read session of reports/marker0-read-plan.md as executable
# steps. Run it from the repository root inside the plan's one jig entry.
#
# Usage: transport/marker0-read-session.sh [PRIVATE_DATA_ROOT]
#
# A re-run needs the previous OUT directories moved aside. CAMERA substitutes
# one command for the devenv default.
set -euo pipefail

root=${1:-$PWD/private-data}
[[ $root = /* ]] || root=$PWD/$root
sessions=$root/sessions
plans=$root/read-plans
phaseOne=$sessions/OUT3a-probe-routes-phase1
capture=$phaseOne/windows/0096bd20.bin
plan=$plans/marker0-routes-phase2.json

if [[ -n ${CAMERA:-} ]]; then camera=("$CAMERA"); else camera=(devenv shell -- bazel run //transport:camera --); fi

mkdir -p "$sessions" "$plans"

echo "step 1/5: probe-ownership -> OUT1-probe-ownership"
"${camera[@]}" probe-ownership "$sessions/OUT1-probe-ownership"

echo "step 2/5: probe-routes fixed -> OUT3a-probe-routes-phase1"
"${camera[@]}" probe-routes fixed "$phaseOne"

echo "step 3/5: derive the phase-2 window list from $capture"
[[ -f $capture ]] || { echo "missing phase-1 capture: $capture" >&2; exit 1; }
# The reader requires an owned, group-unreadable plan file, and truncation
# keeps a predecessor's mode, so the path is recreated under umask 077.
umask 077
rm -f "$plan"
od -A n -t x4 -j 0 -N 8 -v "$capture" | \
  awk 'BEGIN{n=0; printf "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"allocation-search cursor cell\"}"} {for(i=1;i<=NF;i++){p=strtonum("0x"$i); if(p!=0 && p!=0x00a0f86c && p+0x60!=0x00a0f86c && p+0x140!=0x00a0f86c && !seen[p]++){printf ",{\"index\":%d,\"address\":\"0x%08x\",\"length\":32,\"purpose\":\"class descriptor +0x60 method fields\"},{\"index\":%d,\"address\":\"0x%08x\",\"length\":40,\"purpose\":\"class descriptor +0x140 method fields\"}", 2*n+1, p+0x60, 2*n+2, p+0x140; n++}}} END{printf "]}"}' \
  > "$plan"
descriptors=$(od -A n -t x4 -j 0 -N 8 -v "$capture" | awk '{for(i=1;i<=NF;i++){p=strtonum("0x"$i); if(p!=0 && p!=0x00a0f86c && p+0x60!=0x00a0f86c && p+0x140!=0x00a0f86c && !seen[p]++) n++}} END{print n+0}')
echo "distinct class descriptor words: $descriptors; phase-2 read requests: $((1+2*descriptors)); phase-2 transfers: $((24+8*descriptors))"

echo "step 4/5: probe-routes derived -> OUT3b-probe-routes-phase2"
"${camera[@]}" probe-routes derived "$plan" "$sessions/OUT3b-probe-routes-phase2"

echo "step 5/5: probe-ownership -> OUT2-probe-ownership"
"${camera[@]}" probe-ownership "$sessions/OUT2-probe-ownership"

echo "session complete: report the four output directories, the exit used and whether the settings survived"
