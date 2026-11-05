#!/usr/bin/env bash
# reclaim-disk.sh [--apply] [--min-age-hours N] [--verbose]: list, or with --apply delete, what the disk rules in AGENTS.md let go.
#   - private-data run output (RUN_ROOTS and top-level NAME-PID-MICROS run directories) that no tracked file on
#     main, no line added by an unmerged branch, no file in a worktree's working tree and no current ignored
#     baseline names, with or without the private-data/ prefix;
#   - clean worktrees whose HEAD is merged into main, with their agent/ branch;
#   - merged agent/ branches without a worktree;
#   - Bazel output bases whose recorded workspace directory is gone.
# Candidates under analysis/ are deleted only after a y at the terminal, one prompt per analysis/A/B group.
# Anything holding a file modified within the last N hours (default 48) is kept, as are firmware, checkpoints,
# sessions, photos and every other private-data entry outside RUN_ROOTS.
# Directories in the worktree root that git does not know are listed, never deleted.
# Each run's output is logged under private-data/scratch/reclaim/.
set -euo pipefail
shopt -s nullglob dotglob

apply=0
verbose=0
min_age_hours=48
while (($#)); do
  case "$1" in
    --apply) apply=1 ;;
    --verbose) verbose=1 ;;
    --min-age-hours) min_age_hours="${2:?--min-age-hours needs a value}"; shift ;;
    *) printf 'usage: reclaim-disk.sh [--apply] [--min-age-hours N] [--verbose]\n' >&2; exit 1 ;;
  esac
  shift
done

main=main
root="$(git rev-parse --show-toplevel)"
[[ "$(git -C "$root" rev-parse --abbrev-ref HEAD)" == "$main" ]] || { printf 'run from the %s checkout\n' "$main" >&2; exit 1; }
log_dir="$root/private-data/scratch/reclaim"
mkdir -p "$log_dir"
exec > >(tee "$log_dir/$(date +%Y%m%dT%H%M%S)$( ((apply)) && echo -apply).log") 2>&1
worktrees="$(dirname "$root")/latent-image-worktrees"
bazel_cache="$HOME/.cache/bazel"
RUN_ROOTS=(analysis scratch scratch-volcases verify-candidate emulator-runtimes experiments flash-boot-checks checkpoint-warmup)
queue=()
declare -A analysis_groups=()
kept_recent=0

say() { printf '%s\n' "$*"; }
human() { numfmt --to=iec --from-unit=1024 "$1"; }
size_kb() { (($#)) || { echo 0; return; }; printf '%s\0' "$@" | du -sck --files0-from=- 2>/dev/null | tail -n 1 | cut -f1; }
declare -A fresh=()
while IFS= read -r -d '' f; do
  while [[ "$f" == "$root/private-data/"* && -z "${fresh[$f]:-}" ]]; do fresh["$f"]=1; f="${f%/*}"; done
done < <(find "$root/private-data/" -newermt "-${min_age_hours} hours" -print0 2>/dev/null)
recent() {
  if [[ "$1" == "$root/private-data/"* ]]; then [[ -n "${fresh[$1]:-}" ]]
  else [[ -n "$(find "$1" -newermt "-${min_age_hours} hours" -print -quit 2>/dev/null)" ]]; fi
}

drop() {
  local rel group
  if recent "$1"; then
    kept_recent=$((kept_recent + 1))
    ((verbose)) && say "keep   (modified <${min_age_hours}h) $1"
    return 0
  fi
  rel="${1#"$root/private-data/"}"
  if [[ "$rel" == analysis/* ]]; then
    group="$(cut -d/ -f1-3 <<< "$rel")"
    say "confirm $1  ($2)"
    analysis_groups["$group"]+="$1"$'\n'
    return 0
  fi
  say "delete $1  ($2)"
  queue+=("$1")
}

# Citations include current ignored baselines in each workspace.
declare -A cited=() under=()
add_citation() {
  local c="${1%%[.,:;)]}" p
  c="${c%/}"
  [[ -n "$c" && "$c" != . ]] || return 0
  cited["$c"]=1
  p="$c"
  while [[ "$p" == */* ]]; do p="${p%/*}"; under["$p"]=1; done
}
roots_alt="$(IFS='|'; echo "${RUN_ROOTS[*]}")"
pattern="(private-data/[A-Za-z0-9._/+@=-]+|(^|[^A-Za-z0-9._-])($roots_alt)/[A-Za-z0-9._/+@=-]+)"
add_citation scratch/reclaim
add_citation verify-candidate/baselines
while IFS= read -r c; do c="${c#[^A-Za-z0-9._-]}"; add_citation "${c#private-data/}"; done < <(
  {
    git -C "$root" grep --text -ohE "$pattern" "$main" || true
    git -C "$root" for-each-ref --format='%(refname:short)' refs/heads | while read -r b; do
      git -C "$root" merge-base --is-ancestor "$b" "$main" && continue
      git -C "$root" diff "$main...$b" 2>/dev/null | grep -aE '^\+' | grep -aohE "$pattern" || true
    done
    git -C "$root" worktree list --porcelain | sed -n 's/^worktree //p' | while IFS= read -r w; do
      [[ -d "$w" ]] && git -C "$w" grep --text --untracked -ohE "$pattern" 2>/dev/null || true
      if [[ -d "$w/private-data/verify-candidate/baselines" ]]; then
        find "$w/private-data/verify-candidate/baselines" -type f -exec grep -aohE "$pattern" {} + 2>/dev/null || true
      fi
    done
  } | sort -u)
say "citations: ${#cited[@]}"

# A cited directory with citations below it is a namespace: its own files stay, its subdirectories are pruned.
prune() {
  local rel="$1" child
  if [[ -n "${under[$rel]:-}" && -d "$root/private-data/$rel" && ! -L "$root/private-data/$rel" ]]; then
    for child in "$root/private-data/$rel"/*; do
      [[ -n "${cited[$rel]:-}" && ( ! -d "$child" || -L "$child" ) ]] && continue
      prune "$rel/${child##*/}"
    done
    return
  fi
  [[ -n "${cited[$rel]:-}" ]] && return
  drop "$root/private-data/$rel" "uncited run output"
}

say "== private-data"
for r in "${RUN_ROOTS[@]}"; do [[ -e "$root/private-data/$r" ]] && prune "$r"; done
for entry in "$root/private-data"/*; do
  name="${entry##*/}"
  [[ "$name" =~ -[0-9]+-[0-9]{16}$ ]] && prune "$name"
done

say "== worktrees"
removed_branches=()
while IFS=$'\t' read -r dir head branch; do
  [[ "$dir" == "$root" ]] && continue
  [[ -d "$dir" ]] || continue
  why=""
  if ! git -C "$root" merge-base --is-ancestor "$head" "$main" 2>/dev/null; then
    why="unmerged"
  elif [[ -n "$(git -C "$dir" status --porcelain 2>/dev/null)" ]]; then
    why="uncommitted changes"
  elif [[ -n "$(find "$dir" -path "$dir/private-data" -prune -o -newermt "-${min_age_hours} hours" -print -quit 2>/dev/null)" ]]; then
    why="modified <${min_age_hours}h"
  fi
  if [[ -n "$why" ]]; then
    [[ "$why" != unmerged ]] && say "keep   ($why) $dir"
    continue
  fi
  say "delete $dir  (worktree merged into $main${branch:+, branch $branch})"
  queue+=("$dir")
  [[ "$branch" == agent/* ]] && removed_branches+=("$branch")
done < <(git -C "$root" worktree list --porcelain | awk '
  /^worktree / { if (d != "") print d "\t" h "\t" b; d = substr($0, 10); h = ""; b = "" }
  /^HEAD / { h = $2 }
  /^branch / { b = substr($2, 12) }
  END { if (d != "") print d "\t" h "\t" b }')

worktree_list="$(git -C "$root" worktree list --porcelain)"
while read -r b; do
  [[ $'\n'"$worktree_list"$'\n' == *$'\n'"branch refs/heads/$b"$'\n'* ]] && continue
  say "delete branch $b (merged into $main)"
  removed_branches+=("$b")
done < <(git -C "$root" for-each-ref --merged "$main" --format='%(refname:short)' 'refs/heads/agent/')

orphans() {
  local d
  for d in "$1"/*/; do
    d="${d%/}"
    [[ -e "$d/.git" ]] && continue
    if [[ $'\n'"$worktree_list" == *$'\n'"worktree $d/"* ]]; then orphans "$d"; continue; fi
    say "orphan $d  (not a git worktree; inspect and delete by hand)"
  done
}
orphans "$worktrees"

say "== bazel output bases"
bazel_deletions=()
for base in "$bazel_cache"/latent-image-*/ "$bazel_cache"/_bazel_"$USER"/*/; do
  base="${base%/}"
  workspace=""
  keep=0
  for marker in "$base/DO_NOT_BUILD_HERE" "$base/execroot/DO_NOT_BUILD_HERE" "$base/install/DO_NOT_BUILD_HERE"; do
    [[ -f "$marker" ]] || continue
    workspace="$(< "$marker")"
    if [[ -d "$workspace" ]]; then keep=1; break; fi
  done
  ((keep)) && continue
  # Unmarked bases have no recorded workspace to establish that it is gone.
  [[ -n "$workspace" ]] || continue
  before="${#queue[@]}"
  drop "$base" "workspace $workspace is gone"
  ((${#queue[@]} > before)) && bazel_deletions+=("$base")
done

if ((${#analysis_groups[@]})); then
  say "== analysis (confirmation required)"
  while IFS= read -r group; do
    mapfile -t paths < <(printf '%s' "${analysis_groups[$group]}")
    kb="$(size_kb "${paths[@]}")"
    say "$group: ${#paths[@]} uncited paths, $(human "$kb")"
    ((apply)) || continue
    if [[ ! -r /dev/tty ]]; then say "  skipped: no terminal to confirm"; continue; fi
    printf '  delete them? [y/N] '
    read -r answer < /dev/tty || answer=""
    if [[ "$answer" == [yY] ]]; then queue+=("${paths[@]}"); say "  confirmed"; else say "  kept"; fi
  done < <(printf '%s\n' "${!analysis_groups[@]}" | sort)
fi

((kept_recent)) && say "kept $kept_recent paths modified within ${min_age_hours}h (--verbose lists them)"
total="$(size_kb "${queue[@]}")"
if ((apply)); then
  for base in "${bazel_deletions[@]}"; do
    [[ -f "$base/server/server.pid.txt" ]] && kill "$(< "$base/server/server.pid.txt")" 2>/dev/null || true
  done
  for p in "${queue[@]}"; do printf '%s\0' "${p%/*}"; done | xargs -0 -r chmod u+w 2>/dev/null || true
  printf '%s\0' "${queue[@]}" | xargs -0 -r chmod -R u+w 2>/dev/null || true
  printf '%s\0' "${queue[@]}" | xargs -0 -r rm -rf --
  git -C "$root" worktree prune
  for b in "${removed_branches[@]}"; do git -C "$root" branch -q -d "$b" || say "kept branch $b"; done
fi
say "$( ((apply)) && echo freed || echo "reclaimable without analysis/") $(human "${total:-0}")"
