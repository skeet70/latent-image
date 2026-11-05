#!/usr/bin/env bash
set -euo pipefail
script="$(realpath "${1:?reclaim script}")"
git_bin="$(realpath "${2:?git binary}")"
tmp="$(mktemp -d "${TEST_TMPDIR:-/tmp}/reclaim-disk.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
export HOME="$tmp/home" USER=reclaim-test
export SIGNAL_LOG="$tmp/signals"
mkdir -p "$HOME/.cache/bazel" "$tmp/bin" "$tmp/latent-image"
ln -s "$git_bin" "$tmp/bin/git"
export PATH="$tmp/bin:$PATH"
# Observe signals without addressing a process outside the fixture.
kill() { printf '%s\n' "$*" >> "$SIGNAL_LOG"; }
export -f kill
: > "$SIGNAL_LOG"
cd "$tmp/latent-image"
git init -q -b main
git config user.name 'Reclaim test'
git config user.email 'reclaim@example.invalid'
printf 'private-data/\n' > .gitignore
git add .gitignore
git commit -qm fixture
mkdir -p private-data/verify-candidate/baselines/nested private-data/scratch/{prefixed,relative,uncited}
printf '{"receipt":"private-data/scratch/prefixed/result.json","other":"scratch/relative/result.json"}\n' > private-data/verify-candidate/baselines/nested/current.json
for name in prefixed relative uncited; do printf '{}\n' > "private-data/scratch/$name/result.json"; done
cache="$HOME/.cache/bazel"
make_base() {
  local name="$1" marker="$2" workspace="$3" pid="$4"
  mkdir -p "$(dirname "$cache/$name/$marker")" "$cache/$name/server"
  printf '%s\n' "$workspace" > "$cache/$name/$marker"
  printf '%s\n' "$pid" > "$cache/$name/server/server.pid.txt"
}
make_base latent-image-unrelated-suffix DO_NOT_BUILD_HERE "$PWD" 101
make_base latent-image-install-marker install/DO_NOT_BUILD_HERE "$PWD" 102
mkdir -p "$tmp/existing-workspace"
make_base _bazel_reclaim-test/existing DO_NOT_BUILD_HERE "$tmp/existing-workspace" 103
make_base latent-image-old DO_NOT_BUILD_HERE "$tmp/gone" 201
make_base _bazel_reclaim-test/old DO_NOT_BUILD_HERE "$tmp/gone" 202
make_base latent-image-recent DO_NOT_BUILD_HERE "$tmp/gone" 301
make_base _bazel_reclaim-test/recent DO_NOT_BUILD_HERE "$tmp/gone" 302
mkdir -p "$cache/latent-image-unmarked"
find private-data "$cache" -exec touch -h -d '3 days ago' {} +
touch "$cache/latent-image-recent/server/server.pid.txt" "$cache/_bazel_reclaim-test/recent/server/server.pid.txt"
fail() { printf '%s\n' "$*" >&2; exit 1; }
bash "$script" --min-age-hours 6 > "$tmp/dry.log"
[[ ! -s "$SIGNAL_LOG" ]] || fail 'dry run signalled a server'
[[ -d "$cache/latent-image-old" && -d private-data/scratch/uncited ]] || fail 'dry run deleted output'
bash "$script" --apply --min-age-hours 6 > "$tmp/apply.log"
for path in private-data/verify-candidate/baselines/nested/current.json private-data/scratch/prefixed/result.json private-data/scratch/relative/result.json; do
  [[ -f "$path" ]] || fail "lost baseline citation: $path"
done
[[ ! -e private-data/scratch/uncited ]] || fail 'uncited old output survived'
for name in latent-image-unrelated-suffix latent-image-install-marker _bazel_reclaim-test/existing latent-image-recent _bazel_reclaim-test/recent latent-image-unmarked; do
  [[ -d "$cache/$name" ]] || fail "lost protected base: $name"
done
for name in latent-image-old _bazel_reclaim-test/old; do
  [[ ! -e "$cache/$name" ]] || fail "old orphan base survived: $name"
done
mapfile -t signals < "$SIGNAL_LOG"
[[ "${signals[*]}" == '201 202' ]] || fail "unexpected server signals: ${signals[*]}"
