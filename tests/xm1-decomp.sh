#!/usr/bin/env bash
set -euo pipefail

absolute() { if [[ $1 == /* ]]; then printf '%s\n' "$1"; else printf '%s\n' "$PWD/$1"; fi; }
helper=$(absolute "$1")
XM1_DECOMP_GIT=$(absolute "$2")
export XM1_DECOMP_GIT
export HOME="$TEST_TMPDIR"
git() { "$XM1_DECOMP_GIT" -c user.name=test -c user.email=test@example.invalid "$@"; }

repo="$TEST_TMPDIR/x70view"
export XM1_DECOMP_REPO="$repo"
export XM1_DECOMP_CACHE="$TEST_TMPDIR/cache"
x="$repo/decomp/firmwares/xm1-150"
mkdir -p "$x/kernel0/src" "$x/app/ui" "$x/symbols/app" "$repo/decomp/include"

image="$TEST_TMPDIR/loader.bin"
head -c 288 /dev/zero > "$image"
zeros8=$(head -c 8 /dev/zero | sha256sum | cut -d' ' -f1)
stock=$(sha256sum < "$image" | cut -d' ' -f1)
wrong=$(printf '%064d' 0)
cat > "$x/function-ranges.json" <<EOF
{"firmware": "xm1-150", "schema": 1, "source_sha256": "00", "functions": [
  {"address": 262400, "name": "exact_fn", "section": "kernel0", "sha256": "$zeros8", "size": 8, "stock_sha256": "$stock", "unit": "kernel0"},
  {"address": 262408, "name": "draft_fn", "section": "kernel0", "sha256": "$zeros8", "size": 8, "stock_sha256": "$stock", "unit": "kernel0"},
  {"address": 262416, "name": "FUN_00040110", "section": "kernel0", "sha256": "$wrong", "size": 4, "stock_sha256": "$stock", "unit": "kernel0"},
  {"address": 6407168, "name": "app_fn", "section": "stream-0016025c", "sha256": "$wrong", "size": 4, "stock_sha256": "$wrong", "unit": "main"},
  {"address": 6407172, "name": "multi_fn", "section": "stream-0016025c", "sha256": "$wrong", "size": 4, "stock_sha256": "$wrong", "unit": "main"}
]}
EOF
printf '00040100 exact_fn\n00040108 draft_fn\n00040110 inline_fn\n' > "$x/symbols/kernel0.txt"
printf '0061c400 b:app_fn\n0061c404 multi_fn\n' > "$x/symbols/app/m01.txt"
cat > "$x/kernel0/src/inline.h" <<'EOF'
static inline int inline_fn(int x) { return x; }
EOF
cat > "$x/kernel0/src/a.c" <<'EOF'
#include "inline.h"

/* NONMATCHING: prototype list */
int draft_fn(int a);
void exact_fn(void);

void exact_fn(void)
{
	draft_fn(1);
}

/* NONMATCHING: 3 instructions: register allocation */
int draft_fn(int a)
{
	return inline_fn(a);
}
EOF
cat > "$x/app/ui/b.c" <<'EOF'
static int local_helper(void)
{
	return 1;
}

int app_fn(void) { return local_helper(); }

/* NONMATCHING: split head */
static int
multi_fn(void)
{
	return 2;
}
EOF
git init -q -b decomp/xm1-coverage "$repo"
git -C "$repo" add -A
git -C "$repo" commit -q -m first
first=$(git -C "$repo" rev-parse HEAD)
drop_line() { awk -v n="$1" 'NR != n' "$x/kernel0/src/a.c" > "$x/kernel0/src/a.c.new" && mv "$x/kernel0/src/a.c.new" "$x/kernel0/src/a.c"; }
drop_line 12
git -C "$repo" commit -q -am second
second=$(git -C "$repo" rev-parse HEAD)
drop_line 3
git -C "$repo" commit -q -am third
third=$(git -C "$repo" rev-parse HEAD)

fail() { echo "FAIL: $*" >&2; exit 1; }
expect() {
  local name=$1 expected=$2 actual=$3
  [[ $actual == "$expected" ]] || fail "$name"$'\nexpected:\n'"$expected"$'\nactual:\n'"$actual"
}

expect rev "$third" "$("$helper" rev)"
expect rev-override "$first" "$("$helper" rev --rev HEAD~2)"

a=decomp/firmwares/xm1-150/kernel0/src/a.c
draft_range=$'range\tkernel0\tkernel0\t0x00040108-0x00040110\tsize 8\tsha256 '"$zeros8"$'\tname draft_fn'
draft_first=$(printf '%s\n' \
  $'commit\t'"$first"$'\tHEAD~2' \
  "$draft_range" \
  $'symbol\tkernel0\t0x00040108\tdraft_fn' \
  $'definition\t'"$a"$':13\tnonmatching\t0x00040108\tdraft_fn\tNONMATCHING at '"$a"$':12: 3 instructions: register allocation')
expect lookup-address "$draft_first" "$("$helper" lookup 0x40108 --rev HEAD~2)"
expect lookup-inside "$draft_first" "$("$helper" lookup 0004010c --rev HEAD~2)"
expect lookup-name "$draft_first" "$("$helper" lookup draft_fn --rev HEAD~2)"

# A marker above the prototype alone still makes the build treat the definition as NONMATCHING.
expect lookup-prototype-marker "$(printf '%s\n' \
  $'commit\t'"$second"$'\tHEAD~1' \
  "$draft_range" \
  $'symbol\tkernel0\t0x00040108\tdraft_fn' \
  $'definition\t'"$a"$':12\tnonmatching\t0x00040108\tdraft_fn\tNONMATCHING at '"$a"$':3: prototype list')" "$("$helper" lookup draft_fn --rev HEAD~1)"
expect lookup-unmarked "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  "$draft_range" \
  $'symbol\tkernel0\t0x00040108\tdraft_fn' \
  $'definition\t'"$a"$':11\texact\t0x00040108\tdraft_fn')" "$("$helper" lookup draft_fn)"

# The caller and the prototypes are not definitions.
expect lookup-callers "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  $'range\tkernel0\tkernel0\t0x00040100-0x00040108\tsize 8\tsha256 '"$zeros8"$'\tname exact_fn' \
  $'symbol\tkernel0\t0x00040100\texact_fn' \
  $'definition\t'"$a"$':6\texact\t0x00040100\texact_fn')" "$("$helper" lookup exact_fn)"
expect lookup-header "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  $'range\tkernel0\tkernel0\t0x00040110-0x00040114\tsize 4\tsha256 '"$wrong"$'\tname FUN_00040110' \
  $'symbol\tkernel0\t0x00040110\tinline_fn' \
  $'definition\tdecomp/firmwares/xm1-150/kernel0/src/inline.h:1\texact\t0x00040110\tinline_fn')" "$("$helper" lookup inline_fn)"
expect lookup-stem "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  $'range\tmain\tstream-0016025c\t0x0061c400-0x0061c404\tsize 4\tsha256 '"$wrong"$'\tname app_fn' \
  $'symbol\tapp\t0x0061c400\tb:app_fn' \
  $'definition\tdecomp/firmwares/xm1-150/app/ui/b.c:6\texact\t0x0061c400\tapp_fn')" "$("$helper" lookup app_fn)"
# The build's pattern needs the name on the line after the marker.
expect lookup-split-head "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  $'range\tmain\tstream-0016025c\t0x0061c404-0x0061c408\tsize 4\tsha256 '"$wrong"$'\tname multi_fn' \
  $'symbol\tapp\t0x0061c404\tmulti_fn' \
  $'definition\tdecomp/firmwares/xm1-150/app/ui/b.c:9\texact\t0x0061c404\tmulti_fn')" "$("$helper" lookup multi_fn)"
expect lookup-unplaced "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  $'definition\tdecomp/firmwares/xm1-150/app/ui/b.c:1\tunplaced\tno symbol\tlocal_helper')" "$("$helper" lookup local_helper)"
if "$helper" lookup no_such_function > /dev/null 2>&1; then fail "lookup of an unknown name succeeded"; fi

draft_body=$(printf '%s\n' 'int draft_fn(int a)' '{' $'\treturn inline_fn(a);' '}')
expect show "$(printf '%s\n' \
  $'commit\t'"$first"$'\tHEAD~2' \
  "== $a:12-16 nonmatching 0x00040108 draft_fn" \
  "marker $a:12: NONMATCHING: 3 instructions: register allocation" \
  '/* NONMATCHING: 3 instructions: register allocation */' \
  "$draft_body")" "$("$helper" show 0x00040108 --rev HEAD~2)"
expect show-prototype-marker "$(printf '%s\n' \
  $'commit\t'"$second"$'\tHEAD~1' \
  "== $a:12-15 nonmatching 0x00040108 draft_fn" \
  "marker $a:3: NONMATCHING: prototype list" \
  "$draft_body")" "$("$helper" show draft_fn --rev HEAD~1)"

coverage_head=$'unit/section\tfunctions\twith_c\texact\tnonmatching\tdefinitions\texact_definitions\tnonmatching_definitions'
expect coverage "$(printf '%s\n' \
  $'commit\t'"$first"$'\tHEAD~2' \
  "$coverage_head" \
  $'kernel0/kernel0\t3\t3\t2\t1\t3\t2\t1' \
  $'main/stream-0016025c\t2\t2\t2\t0\t2\t2\t0' \
  $'all\t5\t5\t4\t1\t5\t4\t1' \
  $'outside function-ranges\tapp\tunplaced\t1' \
  $'nonmatching definitions without a marker directly above\t0' \
  $'exact definitions under a marker the build does not apply\t1')" "$("$helper" coverage --rev HEAD~2)"
expect coverage-prototype-marker "$(printf '%s\n' \
  $'commit\t'"$second"$'\tHEAD~1' \
  "$coverage_head" \
  $'kernel0/kernel0\t3\t3\t2\t1\t3\t2\t1' \
  $'main/stream-0016025c\t2\t2\t2\t0\t2\t2\t0' \
  $'all\t5\t5\t4\t1\t5\t4\t1' \
  $'outside function-ranges\tapp\tunplaced\t1' \
  $'nonmatching definitions without a marker directly above\t1' \
  $'exact definitions under a marker the build does not apply\t1')" "$("$helper" coverage --rev HEAD~1)"

status=0
boundary=$("$helper" boundary "kernel0=$image:0:288:0x40000") || status=$?
expect boundary-status 1 "$status"
expect boundary "$(printf '%s\n' \
  $'commit\t'"$third"$'\tdecomp/xm1-coverage' \
  "section kernel0 $image sha256 $stock matches stock_sha256" \
  'rows kernel0 3 matched 2 mismatched 1 unmapped 0' \
  'rows stream-0016025c 2 matched 0 mismatched 0 unmapped 2' \
  'total 5 matched 2 mismatched 1 unmapped 2' \
  "MISMATCH kernel0 0x00040110 4 FUN_00040110 ranges $wrong ours $(head -c 4 /dev/zero | sha256sum | cut -d' ' -f1)" \
  'UNMAPPED stream-0016025c 0x0061c400 4 app_fn no image' \
  'UNMAPPED stream-0016025c 0x0061c404 4 multi_fn no image')" "$boundary"
