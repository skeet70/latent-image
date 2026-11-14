#!/usr/bin/env bash
set -euo pipefail
bin=$1 jq=$2 sha=$3
[[ $bin = /* ]] || bin="$PWD/$bin"
[[ $jq = /* ]] || jq="$PWD/$jq"
[[ $sha = /* ]] || sha="$PWD/$sha"
cd "$TEST_TMPDIR"
mkdir source boot
printf '{"extents":[[100,200]],"entries":[{"address":"0x64","label":"clock"}]}' > plan.json
printf '{"points":[{"stimulus":{"kind":"end"},"tick":10}]}' > schedule.json
plan_digest=$("$sha" plan.json); plan_digest=${plan_digest%% *}
schedule_digest=$("$sha" schedule.json); schedule_digest=${schedule_digest%% *}
# Synthetic schema4 observations; whitespace and CRLF are intentional.
printf ' {"site":"time-interface","address":"0x64","instructions":20,"opaque":"preserve"}\r\n' > boot/traffic.jsonl
cat boot/traffic.jsonl > source/traffic.jsonl
printf '{"site":"time-return","address":"0x65","instructions":21}\n' >> source/traffic.jsonl
printf ' {"kind":"call","opaque":"preserve"}\r\n' > boot/interface.jsonl
cat boot/interface.jsonl > source/interface.jsonl
printf '{"kind":"return"}\n' >> source/interface.jsonl
"$jq" -n --arg plan "$plan_digest" --arg schedule "$schedule_digest" '
  ("a" * 64) as $d |
  {schema:4,complete:true,error:null,scenario:"menu-routes",run_sha256:$d,update_sha256:$d,
   boot_rom_sha256:$d,observer_sha256:$d,interface_sha256:$plan,schedule_sha256:$d,checkpoint:null,
   preparation:{timing:{clock:"board-ticks",schedule_sha256:$schedule},origin:{board_tick:0,instructions:0},
     boundary:{board_tick:10,instructions:20},history_boundary:{event_count:1,pending_calls:[
       {key:{core:1,task:3,depth:2,mode:19},sp:4096,resume:8192,label:"delay",track:true,
        outputs:[12288,12292],block_output:16384,queue:17}]},raw_events:1},
   start:"reset",observation_start_instructions:0,
   stimuli:{clock:"board-ticks",origin:{board_tick:0,instructions:0},points:[{stimulus:{kind:"end"},tick:10,instructions:20}]},
   scans:[],events:2,raw_phase:"route",raw_phase_events:1,entries:{clock:1,"kernel-or-return":1},instructions:21,menu:null}' > source/summary.json
printf '{"fetch_receipts":[],"visited":{},"retired_body_fetches":{},"events":2,"pending":0}' > source/interface-summary.json
"$jq" --arg schedule "$schedule_digest" '.scenario="boot"|.preparation=null|.schedule_sha256=$schedule|.events=1|.raw_phase="boot"|.raw_phase_events=1|.entries={clock:1}|.instructions=20' source/summary.json > boot/summary.json
printf '{"fetch_receipts":[{"start":100,"end":200,"fetches":8}],"visited":{"100":8},"retired_body_fetches":{},"events":1,"pending":1}' > boot/interface-summary.json
"$bin" source plan.json schedule.json 20 output boot
cmp boot/traffic.jsonl output/traffic.jsonl
cmp boot/interface.jsonl output/interface.jsonl
"$jq" -e '.status=="equivalent"' output/equivalence.json > /dev/null
"$jq" -e '.history_boundary.pending_calls[0] | .track==true and .outputs==[12288,12292] and .queue==17 and .key.task==3' output/boundary.json > /dev/null
"$bin" source plan.json schedule.json 20 unproven
"$jq" -e '.fetch_receipts==[]' unproven/interface-summary.json > /dev/null
if "$bin" source plan.json schedule.json 19 wrong-boundary; then exit 1; fi
"$jq" '.fetch_receipts[0].end=104' boot/interface-summary.json > partial.json
cp partial.json boot/interface-summary.json
if "$bin" source plan.json schedule.json 20 partial boot; then exit 1; fi
"$jq" -e '.status=="mismatch"' partial/equivalence.json > /dev/null
"$jq" -e '.fetch_receipts==[]' partial/interface-summary.json > /dev/null
"$jq" '.fetch_receipts[0].end=200' boot/interface-summary.json > complete.json
cp complete.json boot/interface-summary.json
printf 'different\n' >> boot/interface.jsonl
if "$bin" source plan.json schedule.json 20 different boot; then exit 1; fi
"$jq" -e '.status=="mismatch"' different/equivalence.json > /dev/null
mkdir large-source large-boot
cp source/summary.json source/interface-summary.json large-source/
cp boot/summary.json large-boot/
"$jq" '.pending=1|.fetch_receipts[0].end=200' boot/interface-summary.json > large-boot/interface-summary.json
padding=$("$jq" -nr '"x"*70000')
printf ' { "site":"time-interface","address":"0x64","instructions":20,"opaque":"%s"}\r\n' "$padding" > large-boot/traffic.jsonl
cat large-boot/traffic.jsonl > large-source/traffic.jsonl
printf '{"site":"time-return","address":"0x65","instructions":21}\n' >> large-source/traffic.jsonl
cp source/interface.jsonl large-source/
cp output/interface.jsonl large-boot/
"$bin" large-source plan.json schedule.json 20 large-output large-boot
cmp large-boot/traffic.jsonl large-output/traffic.jsonl
