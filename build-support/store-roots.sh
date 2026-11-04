#!/usr/bin/env bash
# store-roots.sh list|check|pin: GC roots for the Nix store paths the evidence ledger cites.
# Cited: every store path in docs/evidence.jsonl, and every store path whose bin/ or lib/ a ledger
# artifact names, derivations excepted. Artifacts over 1 GiB are not scanned.
# Roots are indirect roots at private-data/store-roots/<store basename>.
# list prints "rooted|unrooted|missing PATH"; check also exits 1 when any present path is unrooted;
# pin roots every unrooted present path. A missing path is gone from the store and needs a rebuild.
set -euo pipefail
mode="${1:-}"
case "$mode" in
  list | check | pin) ;;
  *) printf 'usage: store-roots.sh list|check|pin\n' >&2; exit 2 ;;
esac
root="$(git rev-parse --show-toplevel)"
roots="$(realpath "$root/private-data")/store-roots"
ledger="$root/docs/evidence.jsonl"
store='/nix/store/[a-z0-9]{32}-[^/"'"'"'`[:space:],;:)}\\]+'

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

grep -ohE "$store" "$ledger" > "$work/cited" || [[ $? -eq 1 ]]
jq -r '.artifacts[]?.path' "$ledger" | sort -u > "$work/artifacts"
while IFS= read -r artifact; do
  [[ "$artifact" == /* ]] && file="$artifact" || file="$root/$artifact"
  if [[ -f "$file" && "$(stat -Lc %s "$file")" -le 1073741824 ]]; then printf '%s\0' "$file"; fi
done < "$work/artifacts" > "$work/files"
# Each grep maps "no match" to 0, so any other xargs status is a read failure.
xargs -0 -r sh -c 'grep -ahoE "$0" "$@" || [ $? -eq 1 ]' "$store/(bin|lib)\b" < "$work/files" | sed -E 's#/(bin|lib)$##' >> "$work/cited"
sed -E 's/\.+$//' "$work/cited" | { grep -vE '\.drv$' || true; } | sort -u > "$work/paths"

find /nix/var/nix/gcroots/auto -maxdepth 1 -type l -exec readlink {} + > "$work/registered" 2>/dev/null || true

state() {
  local link="$roots/${1#/nix/store/}"
  if [[ ! -e "$1" ]]; then echo missing
  elif [[ "$(readlink "$link" 2>/dev/null)" == "$1" ]] && grep -qxF "$link" "$work/registered"; then echo rooted
  else echo unrooted
  fi
}

unrooted=0
while IFS= read -r path; do
  current="$(state "$path")"
  if [[ "$mode" == pin && "$current" == unrooted ]]; then
    mkdir -p "$roots"
    nix-store --add-root "$roots/${path#/nix/store/}" --realise "$path" > /dev/null
    current=rooted
  fi
  if [[ "$current" == unrooted ]]; then unrooted=$((unrooted + 1)); fi
  printf '%s %s\n' "$current" "$path"
done < "$work/paths"

if [[ "$mode" == check && "$unrooted" -ne 0 ]]; then
  printf '%d cited store paths have no root; run build-support/store-roots.sh pin\n' "$unrooted" >&2
  exit 1
fi
