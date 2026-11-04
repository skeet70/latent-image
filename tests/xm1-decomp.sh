#!/usr/bin/env bash
set -euo pipefail

absolute() { if [[ $1 == /* ]]; then printf '%s\n' "$1"; else printf '%s\n' "$PWD/$1"; fi; }
helper=$(absolute "$1")
XM1_DECOMP_GIT=$(absolute "$2")
export XM1_DECOMP_GIT
export HOME="$TEST_TMPDIR"
export TMPDIR="$TEST_TMPDIR/tmp"
mkdir -p "$TMPDIR"
git() { "$XM1_DECOMP_GIT" -c user.name=test -c user.email=test@example.invalid "$@"; }

repo="$TEST_TMPDIR/x70view"
export XM1_DECOMP_REPO="$repo"
export XM1_DECOMP_CACHE="$TEST_TMPDIR/cache"
x="$repo/decomp/firmwares/xm1-150"
mkdir -p "$x/kernel0/src" "$x/app/ui" "$x/symbols/app" "$repo/decomp/include"

image="$TEST_TMPDIR/loader.bin"
head -c 288 /dev/zero > "$image"
zeros() { head -c "$1" /dev/zero | sha256sum | cut -d' ' -f1; }
zeros8=$(zeros 8)
zeros4=$(zeros 4)
stock=$(sha256sum < "$image" | cut -d' ' -f1)
wrong=$(printf '%064d' 0)
row() { printf '{"address": %d, "name": "%s", "section": "%s", "sha256": "%s", "size": %d, "stock_sha256": "%s", "unit": "%s"}' "$@"; }
{
  printf '{"firmware": "xm1-150", "schema": 1, "source_sha256": "00", "functions": [\n'
  row $((0x40100)) exact_fn kernel0 "$zeros8" 8 "$stock" kernel0; printf ',\n'
  row $((0x40108)) draft_fn kernel0 "$zeros8" 8 "$stock" kernel0; printf ',\n'
  row $((0x40110)) FUN_00040110 kernel0 "$wrong" 4 "$stock" kernel0; printf ',\n'
  row $((0x40114)) dead_fn kernel0 "$zeros4" 4 "$stock" kernel0; printf ',\n'
  row $((0x40118)) cond_fn kernel0 "$zeros4" 4 "$stock" kernel0; printf ',\n'
  row $((0x61c400)) app_fn stream-0016025c "$wrong" 4 "$wrong" main; printf ',\n'
  row $((0x61c404)) multi_fn stream-0016025c "$wrong" 4 "$wrong" main; printf ',\n'
  row $((0x61c408)) FUN_0061c408 stream-0016025c "$wrong" 4 "$wrong" main; printf ',\n'
  row $((0x61c40c)) other_fn stream-0016025c "$wrong" 4 "$wrong" main; printf '\n]}\n'
} > "$x/function-ranges.json"
printf '%s\n' '00040100 exact_fn' '00040108 draft_fn' '00040110 inline_fn' '00040114 dead_fn' '00040118 cond_fn' '0004011c old_fn' > "$x/symbols/kernel0.txt"
printf '%s\n' '0061c400 b:app_fn' '0061c404 multi_fn' '0061c408 FUN_0061c408' '0061c40c other_fn' > "$x/symbols/app/m01.txt"
cat > "$x/kernel0/src/inline.h" <<'EOF'
#ifndef INLINE_H
#define INLINE_H
#define CONFIG_RTOS
static inline int inline_fn(int x) { return x; }
#ifdef CONFIG_RTOS
int dead_fn(int x);
#else
static inline int dead_fn(int x) { return x + 1; }
#endif
#endif
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

#define SOME_LEVEL LEVEL_BASE
#if SOME_LEVEL > 2
int cond_fn(void) { return 3; }
#endif

#if 0
int old_fn(void) { return 0; }
#endif
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

/* helper */
int other_fn(void)
{
	return 3;
}

#define REJECT(address, value) \
 int FUN_##address(void) { return value; }

REJECT(0061c408, -12)
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
lines() { printf '%s\n' "$@"; }
at_head=$'commit\t'"$third"$'\tdecomp/xm1-coverage'
range() { printf 'range\t%s\t%s\t%s-%s\tsize %d\tsha256 %s\tname %s' "$@"; }

expect rev "$third" "$("$helper" rev)"
expect rev-override "$first" "$("$helper" rev --rev HEAD~2)"

a=decomp/firmwares/xm1-150/kernel0/src/a.c
b=decomp/firmwares/xm1-150/app/ui/b.c
draft_range=$(range kernel0 kernel0 0x00040108 0x00040110 8 "$zeros8" draft_fn)
draft_symbol=$'symbol\tkernel0\t0x00040108\tdraft_fn'
draft_first=$(lines $'commit\t'"$first"$'\tHEAD~2' "$draft_range" "$draft_symbol" \
  $'definition\t'"$a"$':13\tnonmatching\t0x00040108\tdraft_fn\tNONMATCHING at '"$a"$':12: 3 instructions: register allocation')
expect lookup-address "$draft_first" "$("$helper" lookup 0x40108 --rev HEAD~2)"
expect lookup-inside "$draft_first" "$("$helper" lookup 0004010c --rev HEAD~2)"
expect lookup-name "$draft_first" "$("$helper" lookup draft_fn --rev HEAD~2)"

# A marker above the prototype alone still makes the build treat the definition as NONMATCHING.
expect lookup-prototype-marker "$(lines $'commit\t'"$second"$'\tHEAD~1' "$draft_range" "$draft_symbol" \
  $'definition\t'"$a"$':12\tnonmatching\t0x00040108\tdraft_fn\tNONMATCHING at '"$a"$':3: prototype list')" "$("$helper" lookup draft_fn --rev HEAD~1)"
expect lookup-unmarked "$(lines "$at_head" "$draft_range" "$draft_symbol" \
  $'definition\t'"$a"$':11\texact\t0x00040108\tdraft_fn')" "$("$helper" lookup draft_fn)"

# The caller and the prototypes are not definitions.
expect lookup-callers "$(lines "$at_head" "$(range kernel0 kernel0 0x00040100 0x00040108 8 "$zeros8" exact_fn)" \
  $'symbol\tkernel0\t0x00040100\texact_fn' $'definition\t'"$a"$':6\texact\t0x00040100\texact_fn')" "$("$helper" lookup exact_fn)"
expect lookup-header "$(lines "$at_head" "$(range kernel0 kernel0 0x00040110 0x00040114 4 "$wrong" FUN_00040110)" \
  $'symbol\tkernel0\t0x00040110\tinline_fn' $'definition\tdecomp/firmwares/xm1-150/kernel0/src/inline.h:4\theader\t0x00040110\tinline_fn')" "$("$helper" lookup inline_fn)"
expect lookup-dead-branch "$(lines "$at_head" "$(range kernel0 kernel0 0x00040114 0x00040118 4 "$zeros4" dead_fn)" \
  $'symbol\tkernel0\t0x00040114\tdead_fn' $'definition\tdecomp/firmwares/xm1-150/kernel0/src/inline.h:8\texcluded\t0x00040114\tdead_fn')" "$("$helper" lookup dead_fn)"
expect lookup-conditional "$(lines "$at_head" "$(range kernel0 kernel0 0x00040118 0x0004011c 4 "$zeros4" cond_fn)" \
  $'symbol\tkernel0\t0x00040118\tcond_fn' $'definition\t'"$a"$':18\tconditional\t0x00040118\tcond_fn')" "$("$helper" lookup cond_fn)"
expect lookup-if-zero "$(lines "$at_head" $'symbol\tkernel0\t0x0004011c\told_fn' $'definition\t'"$a"$':22\texcluded\t0x0004011c\told_fn')" "$("$helper" lookup old_fn)"
expect lookup-stem "$(lines "$at_head" "$(range main stream-0016025c 0x0061c400 0x0061c404 4 "$wrong" app_fn)" \
  $'symbol\tapp\t0x0061c400\tb:app_fn' $'definition\t'"$b"$':6\texact\t0x0061c400\tapp_fn')" "$("$helper" lookup app_fn)"
# The build's pattern needs the name on a line directly after the marker's comment.
expect lookup-split-head "$(lines "$at_head" "$(range main stream-0016025c 0x0061c404 0x0061c408 4 "$wrong" multi_fn)" \
  $'symbol\tapp\t0x0061c404\tmulti_fn' $'definition\t'"$b"$':9\texact\t0x0061c404\tmulti_fn')" "$("$helper" lookup multi_fn)"
# Failing there, the marker's reason runs on to the next comment's end and marks the name after it.
expect lookup-extended-marker "$(lines "$at_head" "$(range main stream-0016025c 0x0061c40c 0x0061c410 4 "$wrong" other_fn)" \
  $'symbol\tapp\t0x0061c40c\tother_fn' \
  $'definition\t'"$b"$':16\tnonmatching\t0x0061c40c\tother_fn\tNONMATCHING at '"$b"$':8: split head */ static int multi_fn(void) {  return 2; }  /* helper')" "$("$helper" lookup other_fn)"
expect lookup-generated "$(lines "$at_head" "$(range main stream-0016025c 0x0061c408 0x0061c40c 4 "$wrong" FUN_0061c408)" \
  $'symbol\tapp\t0x0061c408\tFUN_0061c408' $'definition\t'"$b"$':24\texact\t0x0061c408\tFUN_0061c408')" "$("$helper" lookup 0x61c408)"
expect lookup-unplaced "$(lines "$at_head" $'definition\t'"$b"$':1\tunplaced\tno symbol\tlocal_helper')" "$("$helper" lookup local_helper)"
if "$helper" lookup no_such_function > /dev/null 2>&1; then fail "lookup of an unknown name succeeded"; fi

draft_body=$(lines 'int draft_fn(int a)' '{' $'\treturn inline_fn(a);' '}')
expect show "$(lines $'commit\t'"$first"$'\tHEAD~2' \
  "== $a:12-16 nonmatching 0x00040108 draft_fn" \
  "marker $a:12: NONMATCHING: 3 instructions: register allocation" \
  '/* NONMATCHING: 3 instructions: register allocation */' \
  "$draft_body")" "$("$helper" show 0x00040108 --rev HEAD~2)"
expect show-prototype-marker "$(lines $'commit\t'"$second"$'\tHEAD~1' \
  "== $a:12-15 nonmatching 0x00040108 draft_fn" \
  "marker $a:3: NONMATCHING: prototype list" \
  "$draft_body")" "$("$helper" show draft_fn --rev HEAD~1)"

coverage_head=$'unit/section\tfunctions\twith_c\texact\tnonmatching\tunverified\tdefinitions\texact_definitions\tnonmatching_definitions\theader_definitions\tconditional_definitions'
coverage_rows=$(lines \
  $'kernel0/kernel0\t5\t4\t1\t1\t2\t4\t1\t1\t1\t1' \
  $'main/stream-0016025c\t4\t4\t3\t1\t0\t4\t3\t1\t0\t0' \
  $'all\t9\t8\t4\t2\t2\t8\t4\t2\t1\t1' \
  $'outside function-ranges\tapp\tunplaced\t1')
expect coverage "$(lines $'commit\t'"$first"$'\tHEAD~2' "$coverage_head" "$coverage_rows" \
  $'nonmatching definitions without a marker directly above\t1' \
  $'exact definitions shown from above their head (a NONMATCHING comment or a straddled #if)\t1' \
  $'definitions in preprocessor-excluded code\t2')" "$("$helper" coverage --rev HEAD~2)"
expect coverage-prototype-marker "$(lines $'commit\t'"$second"$'\tHEAD~1' "$coverage_head" "$coverage_rows" \
  $'nonmatching definitions without a marker directly above\t2' \
  $'exact definitions shown from above their head (a NONMATCHING comment or a straddled #if)\t1' \
  $'definitions in preprocessor-excluded code\t2')" "$("$helper" coverage --rev HEAD~1)"

status=0
boundary=$("$helper" boundary "kernel0=$image:0:288:0x40000") || status=$?
expect boundary-status 1 "$status"
expect boundary "$(lines "$at_head" \
  "section kernel0 $image sha256 $stock matches stock_sha256" \
  'rows kernel0 5 matched 4 mismatched 1 unmapped 0' \
  'rows stream-0016025c 4 matched 0 mismatched 0 unmapped 4' \
  'total 9 matched 4 mismatched 1 unmapped 4' \
  "MISMATCH kernel0 0x00040110 4 FUN_00040110 ranges $wrong ours $zeros4" \
  'UNMAPPED stream-0016025c 0x0061c400 4 app_fn no image' \
  'UNMAPPED stream-0016025c 0x0061c404 4 multi_fn no image' \
  'UNMAPPED stream-0016025c 0x0061c408 4 FUN_0061c408 no image' \
  'UNMAPPED stream-0016025c 0x0061c40c 4 other_fn no image')" "$boundary"

# The extracted trees are removed, also when the command fails.
leftovers=$(find "$TMPDIR" -mindepth 1 | wc -l)
expect no-leftover-trees 0 "$leftovers"
