# x70view selected-source recovery

Primary credit for this work belongs to jitl (justjake) and [x70view](https://github.com/justjake/x70view). On 2026-10-14 the author explicitly permitted redistribution of his x70view work provided he is credited. The permission does not supply an author-provided SPDX license, sublicensing authority or downstream modification terms, and it does not cover Fujifilm firmware, camera captures or third-party material.

This directory stores an exact source patch and a per-file integrity manifest for 220 files retained from x70view in `emulation/xm1-upstream/` and `build-support/`. Each manifest entry maps a retained public file to its exact original Git blob at one of two privately retained revisions:

- `40406daf52228ace99cb1a3e322f3193f96b0283`, root tree `91858607f056ba2790b129777b6602b0943970c9`
- `62876536052549cbcba1e9f6abc1437e773dba2f`, root tree `0c2a6de5506c3d008407ab1cb2558f4770ac5cdd`

The public baseline used for the retained files is `5d9283cd48515de4d189e157944c5cc76721b5e5`. The retained files in this repository are unchanged; this artifact is not applied to them.

## Contents

| File | Content |
| --- | --- |
| `x70view-selected-source.patch` | Exact source patch, 665994 bytes, SHA-256 `66d4cdc0b389a65cacd5460350e475773e466f3dbfac26f10eae8647c5de38e0`. Direction: retained public bytes to named original bytes. |
| `manifest.json` | Per-file integrity mapping for 220 retained paths, plus the 23 unmatched retained paths and both original revision identities. SHA-256 `0b6b9b83c65a5223ad458883bb38482ab942a05de80773afb7e4787d3cffb92f`. |

The 220 mapped files split into 60 adapted paths whose original bytes differ from the retained public bytes, and 160 paths whose retained public bytes equal their original blobs. The patch carries changed bytes for the 60 adapted paths and leaves the 160 unchanged paths byte-identical. The manifest also lists 23 retained paths with no recovered original identity; no origin is inferred for them and the patch does not touch them. Their public introduction lineage is recorded in the [third-party inventory](../README.md#unmatched-retained-path-lineage); that lineage does not establish an original identity at either named pin, so they remain unmatched.

The patch is a non-executed source artifact. It is a unified diff, not a tool or script. Store and transfer it as bytes; do not run it as a program.

## Scope and limits

The artifact contains exact selected-file source bytes and integrity metadata only. It is not an upstream archive or complete historical checkout. It stores no firmware or capture payload files and adds no new captured input bytes. Patch context retains only previously public scoped reset-release fixture checks and documented synthetic model constants. The approved sixteen-byte `captured_mcu_profile.py` fixture is unchanged and is not newly conveyed patch content. Runtime inputs, generated reports, images, binaries and third-party dependency implementations are not included. Source-byte recovery does not establish standalone historical build completeness or clear any combined artifact.

Applying the patch restores the 60 adapted retained paths to their original bytes. That removes later project adaptations from those files, including the public inline x70view credit header in `build-support/exr2-gcc.nix`. Apply it only in a disposable reconstruction tree; never apply it to a working repository or to the retained public files. The prominent credit in this README and the repository-level attribution are mandatory and must remain with the artifact and any reconstruction.

## Reconstruct in a disposable tree

Run these commands with `bash` from a checkout of the published revision that contains this directory. The commands use `git`, `jq` and standard shell tools. They are intentionally destructive to the disposable tree only. Do not run the patch in a repository or worktree you intend to keep.

```bash
set -euo pipefail

manifest=third_party/source-recovery/manifest.json
patch=third_party/source-recovery/x70view-selected-source.patch

test "$(sha256sum "$patch" | awk '{print $1}')" = \
  66d4cdc0b389a65cacd5460350e475773e466f3dbfac26f10eae8647c5de38e0

# Every mapped and unmatched retained path must still hold the recorded public bytes.
jq -r '[(.files[] | .public), (.unmatched_retained_paths[] | .public)] | .[] | [.blob, .path] | @tsv' "$manifest" |
while IFS=$'\t' read -r blob path; do
  test "$(git hash-object -- "$path")" = "$blob" || {
    printf 'public blob mismatch: %s\n' "$path" >&2
    exit 1
  }
done

scratch=$(mktemp -d)
git worktree add --detach "$scratch/tree" HEAD
cd "$scratch/tree"

git apply --whitespace=nowarn "$patch"

# All 220 mapped paths now hold their exact original blobs.
jq -r '.files[] | [.original.blob, .public.path] | @tsv' "$manifest" |
while IFS=$'\t' read -r blob path; do
  test "$(git hash-object -- "$path")" = "$blob" || {
    printf 'original blob mismatch: %s\n' "$path" >&2
    exit 1
  }
done

# The two restored source scripts have original mode 100644.
test ! -x emulation/xm1-upstream/tools/continue_rec_scanout.py
test ! -x emulation/xm1-upstream/tools/probe_setup_menus.py

git apply -R --whitespace=nowarn "$patch"

# Reverse application restores every mapped and unmatched public blob.
jq -r '[(.files[] | .public), (.unmatched_retained_paths[] | .public)] | .[] | [.blob, .path] | @tsv' "$manifest" |
while IFS=$'\t' read -r blob path; do
  test "$(git hash-object -- "$path")" = "$blob" || {
    printf 'restored public blob mismatch: %s\n' "$path" >&2
    exit 1
  }
done

test -x emulation/xm1-upstream/tools/continue_rec_scanout.py
test -x emulation/xm1-upstream/tools/probe_setup_menus.py
git diff --exit-code --quiet

cd - >/dev/null
git worktree remove "$scratch/tree"
rmdir "$scratch"
```

`git apply` reports the two mode changes for the source scripts. Stop on any mismatch; do not force application or repair a mismatched tree.
