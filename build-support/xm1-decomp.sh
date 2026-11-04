#!/usr/bin/env bash
# x70view's X-M1 1.50 decomp (reports/xm1-decomp.md), read from committed refs only.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: xm1-decomp rev [--rev REV]
       xm1-decomp lookup ADDR|NAME [--rev REV]
       xm1-decomp show ADDR|NAME [--rev REV]
       xm1-decomp coverage [--rev REV]
       xm1-decomp boundary [--rev REV] [SECTION=PATH:OFFSET:SIZE:BASE ...]
ADDR is 0x-prefixed hex or eight hex digits; an address inside a function selects that function.
REV defaults to decomp/xm1-coverage. Every command prints the commit it read.
Environment: XM1_DECOMP_REPO (x70view checkout), XM1_DECOMP_CACHE (index directory), XM1_DECOMP_GIT, XM1_DECOMP_INDEX.
EOF
  exit 2
}

git_bin=${XM1_DECOMP_GIT:-git}
indexer=${XM1_DECOMP_INDEX:-$PWD/build-support/xm1-decomp-index}

workspace() {
  if [[ -n ${BUILD_WORKSPACE_DIRECTORY:-} ]]; then
    printf '%s\n' "$BUILD_WORKSPACE_DIRECTORY"
  else
    "$git_bin" rev-parse --show-toplevel
  fi
}

# The main checkout's sibling, also from a worktree under ../latent-image-worktrees.
default_repo() {
  local common
  common=$("$git_bin" -C "$(workspace)" rev-parse --path-format=absolute --git-common-dir)
  printf '%s\n' "$(dirname "$(dirname "$common")")/x70view"
}

repo=${XM1_DECOMP_REPO:-}
cache=${XM1_DECOMP_CACHE:-}
rev=decomp/xm1-coverage
command=${1:-}
[[ -n $command ]] || usage
shift
args=()
while (($#)); do
  case $1 in
    --rev) [[ $# -ge 2 ]] || usage; rev=$2; shift 2 ;;
    *) args+=("$1"); shift ;;
  esac
done
[[ -n $repo ]] || repo=$(default_repo)
[[ -n $cache ]] || cache=$(workspace)/private-data/scratch/xm1-decomp/index

commit=$("$git_bin" -C "$repo" rev-parse --verify --quiet "$rev^{commit}") || {
  echo "xm1-decomp: $rev is not a commit in $repo" >&2
  exit 1
}

say_commit() { printf 'commit\t%s\t%s\n' "$commit" "$rev"; }

extract() {
  local tree paths=(decomp/firmwares/xm1-150)
  tree=$(mktemp -d)
  if [[ -n $("$git_bin" -C "$repo" ls-tree "$commit" decomp/include) ]]; then
    paths+=(decomp/include)
  fi
  "$git_bin" -C "$repo" archive "$commit" "${paths[@]}" | tar -x -C "$tree"
  printf '%s\n' "$tree"
}

index_file() {
  local file tree
  file=$cache/$commit-$(sha256sum < "$indexer" | cut -c1-12).tsv
  if [[ ! -s $file ]]; then
    mkdir -p "$cache"
    tree=$(extract)
    "$indexer" index "$tree/decomp" > "$file.partial" || { rm -rf "$tree" "$file.partial"; exit 1; }
    rm -rf "$tree"
    mv "$file.partial" "$file"
  fi
  printf '%s\n' "$file"
}

# Index rows naming the target: range rows, then sym and def rows of each selected address.
select_rows() {
  local file=$1 target=$2 kind value=
  if [[ $target =~ ^0[xX][0-9a-fA-F]+$ || $target =~ ^[0-9a-fA-F]{8}$ ]]; then
    kind=address
    value=$((16#${target#0[xX]}))
  else
    kind=name
  fi
  awk -F '\t' -v OFS='\t' -v kind="$kind" -v target="$target" -v value="$value" '
    NR == FNR {
      if (kind == "address") {
        if ($1 == "range" && $5 + 0 == value) exact[$5] = $4
        else if ($1 == "range" && $5 + 0 < value && value < $5 + $6) inside[$5] = $4
      } else {
        if ($1 == "range" && $8 == target) exact[$5] = $4
        else if ($1 == "sym" && ($4 == target || substr($4, length($4) - length(target)) == ":" target)) bysym[$3] = 1
        else if ($1 == "def" && $7 == target && $6 != "-") exact[$6] = $5
      }
      next
    }
    FNR == 1 {
      for (a in exact) { chosen[a] = 1; chosenhex[exact[a]] = 1; found++ }
      if (kind == "address" && !found) for (a in inside) { chosen[a] = 1; chosenhex[inside[a]] = 1; found++ }
      if (kind == "address" && !found) { chosen[value] = 1; chosenhex[sprintf("0x%08x", value)] = 1 }
    }
    $1 == "range" && (($5 in chosen) || ($4 in bysym)) { print; next }
    $1 == "sym" && (($3 in chosenhex) || ($3 in bysym)) { print; next }
    $1 == "def" && $6 != "-" && (($6 in chosen) || ($5 in bysym)) { print; next }
    $1 == "def" && kind == "name" && $7 == target && $6 == "-" { print; next }
  ' "$file" "$file"
}

describe() {
  awk -F '\t' '
    $1 == "range" { printf "range\t%s\t%s\t%s-0x%08x\tsize %d\tsha256 %s\tname %s\n", $2, $3, $4, $5 + $6, $6, $7, $8 }
    $1 == "sym" { printf "symbol\t%s\t%s\t%s\n", $2, $3, $4 }
    $1 == "def" {
      where = ($6 == "-") ? "no symbol" : $5
      printf "definition\t%s:%s\t%s\t%s\t%s%s\n", $8, $10, $12, where, $7, ($13 == "-") ? "" : "\tNONMATCHING at " $13 ": " $14
    }'
}

case $command in
  rev)
    ((${#args[@]} == 0)) || usage
    printf '%s\n' "$commit"
    ;;
  lookup)
    ((${#args[@]} == 1)) || usage
    rows=$(select_rows "$(index_file)" "${args[0]}")
    say_commit
    if [[ -z $rows ]]; then
      echo "xm1-decomp: no function, symbol or definition matches ${args[0]}" >&2
      exit 1
    fi
    describe <<< "$rows"
    ;;
  show)
    ((${#args[@]} == 1)) || usage
    rows=$(select_rows "$(index_file)" "${args[0]}")
    say_commit
    defs=$(awk -F '\t' '$1 == "def"' <<< "$rows")
    if [[ -z $defs ]]; then
      echo "xm1-decomp: no C definition matches ${args[0]}" >&2
      exit 1
    fi
    while IFS=$'\t' read -r _ _ _ _ address _ name file first _ end status marker reason; do
      printf '== %s:%s-%s %s %s %s\n' "$file" "$first" "$end" "$status" "$address" "$name"
      [[ $marker == - ]] || printf 'marker %s: NONMATCHING: %s\n' "$marker" "$reason"
      "$git_bin" -C "$repo" show "$commit:$file" | awk -v a="$first" -v b="$end" 'NR >= a && NR <= b'
    done <<< "$defs"
    ;;
  coverage)
    ((${#args[@]} == 0)) || usage
    file=$(index_file)
    say_commit
    # Columns 9 and 10 differ when a NONMATCHING comment sits directly above the definition.
    awk -F '\t' -v OFS='\t' '
      $1 == "range" { key = $2 "/" $3; functions[key]++; all++ }
      $1 == "def" && $3 != "-" {
        key = $3 "/" $4; fn = $3 SUBSEP $6
        defs[key]++; total++
        if ($12 == "exact") { exactdefs[key]++; totalexact++ } else { nmdefs[key]++; totalnm++; marked[fn] = 1 }
        if (!(fn in withc)) { withc[fn] = key; count[key]++; totalc++ }
        if ($12 == "nonmatching" && $9 == $10) away++
        if ($12 == "exact" && $9 != $10) unapplied++
      }
      $1 == "def" && $3 == "-" { other[$2 "\t" $12]++ }
      END {
        print "unit/section", "functions", "with_c", "exact", "nonmatching", "definitions", "exact_definitions", "nonmatching_definitions"
        n = 0
        for (key in functions) keys[++n] = key
        for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (keys[j] < keys[i]) { t = keys[i]; keys[i] = keys[j]; keys[j] = t }
        for (fn in marked) { nm[withc[fn]]++; totalmarked++ }
        for (i = 1; i <= n; i++) {
          key = keys[i]
          print key, functions[key], count[key] + 0, count[key] - nm[key], nm[key] + 0, defs[key] + 0, exactdefs[key] + 0, nmdefs[key] + 0
        }
        print "all", all, totalc + 0, totalc - totalmarked, totalmarked + 0, total + 0, totalexact + 0, totalnm + 0
        m = 0
        for (k in other) rest[++m] = k
        for (i = 1; i <= m; i++) for (j = i + 1; j <= m; j++) if (rest[j] < rest[i]) { t = rest[i]; rest[i] = rest[j]; rest[j] = t }
        for (i = 1; i <= m; i++) print "outside function-ranges", rest[i], other[rest[i]]
        print "nonmatching definitions without a marker directly above", away + 0
        print "exact definitions under a marker the build does not apply", unapplied + 0
      }' "$file"
    ;;
  boundary)
    images=("${args[@]}")
    if ((${#images[@]} == 0)); then
      r=$(workspace)/private-data/analysis/x-m1-1.50
      size() { stat -L -c %s "$1"; }
      # Overlay extents inside the plain image: payload slices copied to 0x01100000, 0x011c0000 and 0x011e0000.
      images=(
        "stream-0016025c=$r/lz/id4-0061c400.bin:0:$(size "$r/lz/id4-0061c400.bin"):0x0061c400"
        "stream-002c025c=$r/lz/id5-00bf1a00.bin:0:$(size "$r/lz/id5-00bf1a00.bin"):0x00bf1a00"
        "overlay44=$r/lz/plain-01100000.bin:0:0xac5a0:0x01100000"
        "overlay46=$r/lz/plain-01100000.bin:0xc0000:0x1cca0:0x011c0000"
        "overlay47=$r/lz/plain-01100000.bin:0xe0000:0x26a5e0:0x011e0000"
        "kernel0=$r/decode/loader-40000.bin:0:$(size "$r/decode/loader-40000.bin"):0x00040000"
        "kernel1=$r/decode/loader-d0000.bin:0:$(size "$r/decode/loader-d0000.bin"):0x000d0000"
      )
    fi
    say_commit
    tree=$(extract)
    status=0
    "$indexer" boundary "$tree/decomp" "${images[@]}" || status=$?
    rm -rf "$tree"
    exit "$status"
    ;;
  *) usage ;;
esac
