#!/usr/bin/env bash
# The stub camera leaves the phase-1 capture the derivation reads.
set -euo pipefail
binary="$1"
session="$2"
root="${TEST_TMPDIR:?}/marker0"

stub="$root/camera-stub"
mkdir -p "$root"
cat > "$stub" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
echo "$*" >> "$STUB_CALLS"
case "$1" in
  probe-ownership)
    mkdir -p "$2/windows"
    ;;
  probe-routes)
    case "$2" in
      fixed)
        mkdir -p "$3/windows"
        awk -v pair="$STUB_PAIR" 'BEGIN{split(pair, word, " "); for(i=0;i<64;i++){v=(i<2)?strtonum(word[i+1]):0; printf "%c%c%c%c", v%256, int(v/256)%256, int(v/65536)%256, int(v/16777216)%256}}' > "$3/windows/0096bd20.bin"
        printf '\x6c\x6d\x6e\x6f' > "$3/windows/00a0f86c.bin"
        ;;
      derived)
        test -f "$3"
        test "$(stat -c %a "$3")" = 600
        mkdir -p "$4"
        ;;
    esac
    ;;
esac
STUB
chmod +x "$stub"

cursorOnlyPlan='{"windows":[{"index":0,"address":"0x00a0f86c","length":4,"purpose":"allocation-search cursor cell"}]}'
pairPlan='{"windows":[{"index":0,"address":"0x00a0f86c","length":4,"purpose":"allocation-search cursor cell"},{"index":1,"address":"0x00a000c0","length":32,"purpose":"class descriptor +0x60 method fields"},{"index":2,"address":"0x00a001a0","length":40,"purpose":"class descriptor +0x140 method fields"},{"index":3,"address":"0x00a000a0","length":32,"purpose":"class descriptor +0x60 method fields"},{"index":4,"address":"0x00a00180","length":40,"purpose":"class descriptor +0x140 method fields"}]}'
dedupPlan='{"windows":[{"index":0,"address":"0x00a0f86c","length":4,"purpose":"allocation-search cursor cell"},{"index":1,"address":"0x00a000c0","length":32,"purpose":"class descriptor +0x60 method fields"},{"index":2,"address":"0x00a001a0","length":40,"purpose":"class descriptor +0x140 method fields"}]}'

run_case() {
  local runRoot="$1" pair="$2" expectedPlan="$3" expectedRequests="$4"
  mkdir -p "$runRoot"
  export STUB_PAIR="$pair"
  export STUB_CALLS="$runRoot/calls"
  CAMERA="$stub" bash "$session" "$runRoot"

  local expectedCalls=(
    "probe-ownership $runRoot/sessions/OUT1-probe-ownership"
    "probe-routes fixed $runRoot/sessions/OUT3a-probe-routes-phase1"
    "probe-routes derived $runRoot/read-plans/marker0-routes-phase2.json $runRoot/sessions/OUT3b-probe-routes-phase2"
    "probe-ownership $runRoot/sessions/OUT2-probe-ownership"
  )
  local calls
  mapfile -t calls < "$STUB_CALLS"
  if [[ ${#calls[@]} -ne ${#expectedCalls[@]} ]]; then
    echo "$pair: four camera commands expected, saw ${#calls[@]}: ${calls[*]}" >&2
    exit 1
  fi
  local index
  for index in "${!expectedCalls[@]}"; do
    if [[ ${calls[$index]} != "${expectedCalls[$index]}" ]]; then
      echo "$pair: step $((index+1)) is '${calls[$index]}', expected '${expectedCalls[$index]}'" >&2
      exit 1
    fi
  done

  local plan="$runRoot/read-plans/marker0-routes-phase2.json"
  if [[ $(<"$plan") != "$expectedPlan" ]]; then
    echo "$pair: derived plan differs: $(<"$plan")" >&2
    exit 1
  fi

  local fixedRehearsal derivedRehearsal
  fixedRehearsal=$("$binary" rehearse probe-routes fixed)
  if [[ $fixedRehearsal != *"rehearsed probe-routes fixed: 3 read requests"* ]]; then
    echo "fixed rehearsal did not complete three read requests" >&2
    exit 1
  fi
  derivedRehearsal=$("$binary" rehearse probe-routes derived "$plan")
  if [[ $derivedRehearsal != *"rehearsed probe-routes derived: $expectedRequests read requests"* ]]; then
    echo "$pair: derived rehearsal did not complete $expectedRequests read requests" >&2
    exit 1
  fi
}

run_case "$root/pair" "0x00a00060 0x00a00040" "$pairPlan" 5
run_case "$root/dedup" "0x00a00060 0x00a00060" "$dedupPlan" 3
run_case "$root/cursor-collision" "0x00a0f80c 0x00a0f80c" "$cursorOnlyPlan" 1

echo "marker0-schedule: five steps in order, revised derivation accepted for the pair, a repeated word and a cursor collision"
