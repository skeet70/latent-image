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
if readelf -d "$binary" | sed -n "/NEEDED/p" | rg "libusb|libudev"; then exit 1; fi
