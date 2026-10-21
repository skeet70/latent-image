#!/usr/bin/env bash
set -euo pipefail
binary="$1"
"$binary" --help
tmpdir="${TEST_TMPDIR:?}/camera-caller"
mkdir -m 700 "$tmpdir"
printf '{}\n' > "$tmpdir/unresolved.json"
chmod 600 "$tmpdir/unresolved.json"
printf '{"windows":[{"index":0,"address":"0x00a0f86c","length":4,"purpose":"allocation-search cursor cell"}]}\n' > "$tmpdir/routes-plan.json"
chmod 600 "$tmpdir/routes-plan.json"

for bad in "run-raw" "validate" "identify-live" "read-config" "rehearse" "rehearse validate"; do
  if "$binary" $bad; then exit 1; fi
done
if "$binary" identify "$tmpdir/one" "$tmpdir/two"; then exit 1; fi

if (cd "$tmpdir" && "$binary" identify relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" identify "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" end-setting; then exit 1; fi

if "$binary" read-config "$tmpdir/unresolved.json" "$tmpdir/read-session"; then exit 1; fi
test ! -e "$tmpdir/read-session"
if "$binary" rehearse read-config "$tmpdir/unresolved.json"; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-ram relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-ram "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-ram; then exit 1; fi

if (cd "$tmpdir" && "$binary" read-log relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" read-log "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" read-log; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-bootrom relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-bootrom "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-bootrom; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-area2 relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-area2 "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-area2; then exit 1; fi
if "$binary" probe-area2 "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" probe-area2 --verbose; then exit 1; fi
if "$binary" probe-area3; then exit 1; fi
if "$binary" rehearse probe-area2 extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-straps relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-straps "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-straps; then exit 1; fi
if "$binary" probe-straps "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" probe-straps --verbose; then exit 1; fi
if "$binary" probe-strap; then exit 1; fi
if "$binary" rehearse probe-straps extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-code relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-code "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-code; then exit 1; fi
if "$binary" probe-code "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" probe-code --verbose; then exit 1; fi
if "$binary" probe-codes; then exit 1; fi
if "$binary" rehearse probe-code extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-ownership relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-ownership "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-ownership; then exit 1; fi
if "$binary" probe-ownership "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" probe-ownership --verbose; then exit 1; fi
if "$binary" probe-ownerships; then exit 1; fi
if "$binary" rehearse probe-ownership extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" probe-routes fixed relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-routes fixed "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" probe-routes fixed; then exit 1; fi
if "$binary" probe-routes fixed "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" probe-routes fixed --verbose; then exit 1; fi
if "$binary" probe-routes; then exit 1; fi
if "$binary" probe-routes derive "$tmpdir/routes-plan.json" "$tmpdir"; then exit 1; fi
if "$binary" probe-routes derived; then exit 1; fi
if "$binary" probe-routes derived relative-plan.json "$tmpdir"; then exit 1; fi
if "$binary" probe-routes derived "$tmpdir/unresolved.json" "$tmpdir/plan-rejected-out"; then exit 1; fi
test ! -e "$tmpdir/plan-rejected-out"
if "$binary" probe-routes derived "$tmpdir/routes-plan.json" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" rehearse probe-routes; then exit 1; fi
if "$binary" rehearse probe-routes derive; then exit 1; fi
if "$binary" rehearse probe-routes fixed extra; then exit 1; fi
if "$binary" rehearse probe-routes derived "$tmpdir/unresolved.json"; then exit 1; fi

if "$binary" dump-ram; then exit 1; fi
if "$binary" dump-ram bogus; then exit 1; fi
if "$binary" dump-ram Loaders; then exit 1; fi
if "$binary" dump-ram loaders a b; then exit 1; fi
if "$binary" dump-ram loaders --verbose; then exit 1; fi
if (cd "$tmpdir" && "$binary" dump-ram loaders relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" dump-ram loaders "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" dump-ram loaders; then exit 1; fi
if "$binary" rehearse dump-ram; then exit 1; fi
if "$binary" rehearse dump-ram bogus; then exit 1; fi
if "$binary" rehearse dump-ram loaders extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" read-media relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" read-media "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" read-media; then exit 1; fi
if "$binary" read-media "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" read-media --verbose; then exit 1; fi
if "$binary" read-medias; then exit 1; fi
if "$binary" read-media loaders; then exit 1; fi
if "$binary" rehearse read-media extra; then exit 1; fi

if (cd "$tmpdir" && "$binary" arm-auto-act relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" arm-auto-act "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" arm-auto-act; then exit 1; fi
if "$binary" arm-auto-act "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" arm-auto-act --verbose; then exit 1; fi
if "$binary" arm-auto-acts; then exit 1; fi
if "$binary" arm-auto-act 0x80; then exit 1; fi
if "$binary" rehearse arm-auto-act extra; then exit 1; fi
if (cd "$tmpdir" && "$binary" arm-auto-act --wait-for-device relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" arm-auto-act --wait-for-device "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" arm-auto-act --wait-for-device; then exit 1; fi
if "$binary" arm-auto-act --wait-for-device "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" arm-auto-act --wait-for-device --verbose; then exit 1; fi
if "$binary" rehearse arm-auto-act --wait-for-device; then exit 1; fi

reference="$tmpdir/reference.bin"
dd if=/dev/zero of="$reference" bs=256 count=1 status=none
chmod 600 "$reference"
dd if=/dev/zero of="$tmpdir/short.bin" bs=255 count=1 status=none
chmod 600 "$tmpdir/short.bin"
dd if=/dev/zero of="$tmpdir/shared.bin" bs=256 count=1 status=none
chmod 640 "$tmpdir/shared.bin"
for op in commit-f7-enable commit-f7-restore; do
  if "$binary" "$op"; then exit 1; fi
  if (cd "$tmpdir" && "$binary" "$op" "$reference" relative-session); then exit 1; fi
  test ! -e "$tmpdir/relative-session"
  if "$binary" "$op" "$reference" "$tmpdir"; then exit 1; fi
  if env -u BUILD_WORKSPACE_DIRECTORY "$binary" "$op" "$reference"; then exit 1; fi
  if "$binary" "$op" "$reference" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
  for bad in reference.bin "$tmpdir/short.bin" "$tmpdir/shared.bin" "$tmpdir/missing.bin" "$tmpdir"; do
    if (cd "$tmpdir" && "$binary" "$op" "$bad" "$tmpdir/refused-out"); then exit 1; fi
    test ! -e "$tmpdir/refused-out"
    if "$binary" rehearse "$op" "$bad"; then exit 1; fi
  done
  if "$binary" rehearse "$op"; then exit 1; fi
  if "$binary" rehearse "$op" "$reference" extra; then exit 1; fi
done
if "$binary" commit-f7; then exit 1; fi
if "$binary" rehearse commit-f7 "$reference"; then exit 1; fi

ramplan="$tmpdir/read-ram-plan.json"
printf '{"windows":[{"index":0,"address":"0x400a0800","length":64,"purpose":"log store header"},{"index":1,"address":"0x400c0800","length":4,"purpose":"boot tick cell"},{"index":2,"address":"0x009571d0","length":16,"purpose":"menu frame and cursor"}]}\n' > "$ramplan"
chmod 600 "$ramplan"
printf '{"windows":[{"index":0,"address":"0x0096bd20","length":256,"purpose":"two-request window"}]}\n' > "$tmpdir/read-ram-long.json"
chmod 600 "$tmpdir/read-ram-long.json"
if "$binary" read-ram; then exit 1; fi
if (cd "$tmpdir" && "$binary" read-ram "$ramplan" relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" read-ram "$ramplan" "$tmpdir"; then exit 1; fi
if env -u BUILD_WORKSPACE_DIRECTORY "$binary" read-ram "$ramplan"; then exit 1; fi
if "$binary" read-ram "$ramplan" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" read-ram "$ramplan" --verbose; then exit 1; fi
for bad in relative-plan.json "$tmpdir/unresolved.json" "$tmpdir/read-ram-long.json" "$tmpdir/missing.json"; do
  if (cd "$tmpdir" && "$binary" read-ram "$bad" "$tmpdir/ram-refused-out"); then exit 1; fi
  test ! -e "$tmpdir/ram-refused-out"
  if "$binary" rehearse read-ram "$bad"; then exit 1; fi
done
for pacing in "--repeat 601 --interval-ms 100" "--repeat 0 --interval-ms 100" "--repeat 2 --interval-ms 99" "--repeat 2 --interval-ms 10001" "--repeat 2" "--interval-ms 100 --repeat 2" "--repeat 0x2 --interval-ms 100"; do
  if "$binary" read-ram "$ramplan" $pacing "$tmpdir/ram-refused-out"; then exit 1; fi
  test ! -e "$tmpdir/ram-refused-out"
  if "$binary" rehearse read-ram "$ramplan" $pacing; then exit 1; fi
done
if "$binary" rehearse read-ram; then exit 1; fi
if "$binary" rehearse read-ram "$ramplan" "$tmpdir/ram-rehearsal-out"; then exit 1; fi
test ! -e "$tmpdir/ram-rehearsal-out"
if "$binary" probe-sd-host "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if (cd "$tmpdir" && "$binary" probe-sd-host relative-session); then exit 1; fi
test ! -e "$tmpdir/relative-session"
if "$binary" probe-sd-host --verbose; then exit 1; fi
if "$binary" rehearse probe-sd-host extra; then exit 1; fi
if "$binary" probe-sd-host --repeat 509 --interval-ms 100 "$tmpdir/sd-refused-out"; then exit 1; fi
test ! -e "$tmpdir/sd-refused-out"
if "$binary" probe-sd-host --repeat 2 "$tmpdir/sd-refused-out"; then exit 1; fi
test ! -e "$tmpdir/sd-refused-out"
if "$binary" ram-alloc; then exit 1; fi
if "$binary" ram-alloc 0; then exit 1; fi
if "$binary" ram-push; then exit 1; fi
if "$binary" ram-free; then exit 1; fi
if "$binary" ram-unload; then exit 1; fi
if "$binary" ram-push "$tmpdir/unresolved.json" "$tmpdir/unresolved.json" "$tmpdir/ram-push-refused"; then exit 1; fi
test ! -e "$tmpdir/ram-push-refused"
if "$binary" ram-free "$tmpdir/unresolved.json" "$tmpdir/ram-free-refused"; then exit 1; fi
test ! -e "$tmpdir/ram-free-refused"
if "$binary" ram-unload "$tmpdir/unresolved.json" "$tmpdir/ram-unload-refused"; then exit 1; fi
test ! -e "$tmpdir/ram-unload-refused"
if (cd "$tmpdir" && "$binary" ram-push unresolved.json unresolved.json "$tmpdir/ram-push-relative"); then exit 1; fi
test ! -e "$tmpdir/ram-push-relative"
if (cd "$tmpdir" && "$binary" ram-free unresolved.json "$tmpdir/ram-free-relative"); then exit 1; fi
test ! -e "$tmpdir/ram-free-relative"
if (cd "$tmpdir" && "$binary" ram-unload unresolved.json "$tmpdir/ram-unload-relative"); then exit 1; fi
test ! -e "$tmpdir/ram-unload-relative"
if "$binary" ram-push "$tmpdir/unresolved.json" "$tmpdir/unresolved.json" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" ram-free "$tmpdir/unresolved.json" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" ram-unload "$tmpdir/unresolved.json" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" rehearse ram-alloc; then exit 1; fi
if "$binary" rehearse ram-push; then exit 1; fi
if "$binary" rehearse ram-free; then exit 1; fi
if "$binary" rehearse ram-unload; then exit 1; fi
if "$binary" rehearse ram-push "$tmpdir/unresolved.json"; then exit 1; fi
if "$binary" rehearse ram-free "$tmpdir/unresolved.json"; then exit 1; fi
if "$binary" rehearse ram-unload "$tmpdir/unresolved.json"; then exit 1; fi
if "$binary" ram-relink; then exit 1; fi
if "$binary" ram-relink "$tmpdir/unresolved.json"; then exit 1; fi
if "$binary" ram-relink "$tmpdir/unresolved.json" "$tmpdir/one" "$tmpdir/two"; then exit 1; fi
if "$binary" ram-relink --verbose "$tmpdir/ram-relink-refused"; then exit 1; fi
if "$binary" ram-relink "$tmpdir/unresolved.json" "$tmpdir/ram-relink-refused"; then exit 1; fi
test ! -e "$tmpdir/ram-relink-refused"
if (cd "$tmpdir" && "$binary" ram-relink unresolved.json "$tmpdir/ram-relink-relative"); then exit 1; fi
test ! -e "$tmpdir/ram-relink-relative"
if "$binary" ram-relink "$tmpdir/unresolved.json" relative-out; then exit 1; fi
if "$binary" rehearse ram-relink "$tmpdir/unresolved.json"; then exit 1; fi

# Rehearsals journal every frame with fsync; on a disk-backed temp directory
# that alone can exceed the real-time transfer budget.
rehearsals="$(mktemp -d /dev/shm/fuji-af-cli-rejections.XXXXXX)"
trap 'rm -rf "$rehearsals"' EXIT
export TMPDIR="$rehearsals"
"$binary" rehearse identify
"$binary" rehearse end-setting
"$binary" rehearse probe-ram
"$binary" rehearse read-log
"$binary" rehearse probe-bootrom
"$binary" rehearse probe-area2
"$binary" rehearse probe-straps
"$binary" rehearse probe-code
"$binary" rehearse probe-ownership
"$binary" rehearse probe-routes fixed
"$binary" rehearse probe-routes derived "$tmpdir/routes-plan.json"
"$binary" rehearse dump-ram loaders
"$binary" rehearse read-media
"$binary" rehearse arm-auto-act
"$binary" rehearse commit-f7-enable "$reference"
"$binary" rehearse commit-f7-restore "$reference"
"$binary" rehearse read-ram "$ramplan"
"$binary" rehearse read-ram "$ramplan" --repeat 3 --interval-ms 250
"$binary" rehearse probe-sd-host
"$binary" rehearse probe-sd-host --repeat 3 --interval-ms 100
if readelf -d "$binary" | sed -n "/NEEDED/p" | rg "libusb|libudev"; then exit 1; fi
