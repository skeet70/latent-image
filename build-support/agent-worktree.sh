#!/usr/bin/env bash
# agent-worktree.sh NAME [BASE]: one agent's worktree beside the checkout, BASE defaulting to HEAD.
set -euo pipefail
name="${1:?usage: agent-worktree.sh NAME [BASE]}"
base="${2:-HEAD}"
[[ "$name" =~ ^[A-Za-z0-9._-]+$ ]] || { printf 'agent name must be [A-Za-z0-9._-]+\n' >&2; exit 1; }
root="$(git rev-parse --show-toplevel)"
dir="$(dirname "$root")/latent-image-worktrees/$name"
[[ -e "$dir" ]] && { printf 'worktree already exists: %s\n' "$dir" >&2; exit 1; }
git -C "$root" worktree add -b "agent/$name" "$dir" "$base"
ln -s "$root/private-data" "$dir/private-data"
printf 'worktree=%s\nbranch=agent/%s\noutput_base=%s\n' "$dir" "$name" "$HOME/.cache/bazel/latent-image-$name"
