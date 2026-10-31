# Firmware extensions

`//extensions:build-extension` builds a patched X-M1 `FWUP0002.DAT` from an official DAT and an extension manifest. The [top-level README](../README.md#firmware-extensions) lists the published extensions and the install steps. Building runs on the host and performs no camera I/O.

```sh
devenv shell -- bazel run //extensions:build-extension -- \
  EXTENSION_JSON STOCK_DAT private-data/NEW_OUTPUT_DIRECTORY
```

`--unpinned`, placed before `EXTENSION_JSON`, makes a development build: every step and the final `rebuild-update verify` still run, but the result is not compared with `output_sha256`; the tool prints `UNPINNED: <path> SHA-256 <digest>` and still moves the result to `NEW_OUTPUT_DIRECTORY/FWUP0002.DAT`. The digest of an unpinned build must be reviewed and then pinned as `output_sha256` before the extension is published or installed.

## Manifest

```json
{
  "schema_version": 1,
  "id": "RC0",
  "title": "Unlock PRO Neg. Std and PRO Neg. Hi",
  "base": { "model": "X-M1", "firmware_version": "1.50", "sha256": "…" },
  "steps": [
    { "kind": "image-patch-set", "path": "../recovery/rc0-patch-set.json", "sha256": "…" }
  ],
  "output_sha256": "…"
}
```

Every field is required and unknown fields are refused. Part paths are relative to the manifest, and each part's SHA-256 is checked before use. The stock DAT must match `base.sha256` and its version words must match `base.firmware_version`. The built DAT must match `output_sha256`.

| Step kind | Fields | Rebuild step |
| --- | --- | --- |
| `image-patch-set` | `path`, `sha256` | Decodes the stock image named by the patch set's `image.segment_id`, applies it with `apply-patch-set`, re-encodes it with `lz-encode` into that segment's flash window, then runs `rebuild-update replace-record4` (passing `--load-address` when `output_base` differs from `decode_base`) or `replace-record5` |
| `image2-label` | `text` | `rebuild-update write-label` |
| `image2-record` | `id`, `text` or `text_utf16`, `layout`, `current_hex` | `rebuild-update write-record`: writes the whole 66-byte special label record `id` (`0x83c` to `0x83e`) with `text` (printable ASCII, 1 to 27 characters) or `text_utf16` (1 to 27 nonzero UTF-16 code units, four lowercase hex digits each, such as `f073` for an icon-font code point) and the five `layout` halfwords (integers 0 to 65535), refused unless the record currently holds `current_hex` (132 lowercase hex digits, media form) |
| `image2-patch-set` | `path`, `sha256` | `rebuild-update extract-image2` on the current DAT (segment 2's extent, media form), `apply-patch-set` on that extent, then `rebuild-update replace-image2` with the whole extent |
| `font-patch-set` | `path`, `sha256` | `rebuild-update extract-font` on the current DAT (segment 3's window, FONT_ICON.FDL and its loaded tail, media form), `apply-patch-set` on that window, then `rebuild-update replace-font` with the whole window |
| `plain-span` | `flash_offset`, `path`, `sha256` | `rebuild-update replace-plain --flash-offset` |
| `plain-patch-set` | `path`, `sha256` | `rebuild-update extract-plain` on the current DAT, `apply-patch-set` on that window, then `rebuild-update replace-plain` with the whole window |

Steps run in container order: the image-4 patch set, the image-5 patch set, the label, image-2 records by increasing `id`, at most one image-2 patch set, at most one font patch set, then either plain spans in increasing, non-overlapping flash order or one plain patch set. Each image and the label can appear at most once; a record cannot use `0x83d` when the manifest has a label, and a plain patch set cannot be combined with plain spans. The patch set's `target.firmware_sha256` must equal the manifest's base. Image-5 patch sets cannot move the load address. A plain patch set's image block must have `segment_id` 6, `decode_base` `0x01100000` (the RAM address of the first byte of the extract-plain window), no differing `output_base`, and a `decode_sha256` equal to the SHA-256 of the window extracted from the step's input DAT. An image-2 patch set's image block must have `segment_id` 2, `decode_base` `0x00130000` (segment 2's load address, the first byte of the extract-image2 extent), no differing `output_base`, and a `decode_sha256` equal to the SHA-256 of the extent extracted from the step's input DAT, so it sees the records written before it. A font patch set's image block must have `segment_id` 3, `decode_base` `0x003bc000` (segment 3's load address, the first byte of the extract-font window), no differing `output_base`, and a `decode_sha256` equal to the SHA-256 of the font window extracted from the step's input DAT; `apply-patch-set` checks every current word against that window. The tool refuses any other segment or step kind, so an extension that needs a step not listed here fails instead of building a partial image. After the last step it runs `rebuild-update verify` on the result.

## Adding an extension

Add the manifest and its parts to this directory, or reference existing parts by relative path. Set `output_sha256` from an independently reviewed build. Add a row to the top-level README table once the extension has worked on a physical camera.

## RAM payload inputs

`ram-inputs/` holds the parameter files the project's RAM payload relinkers read besides the decoded official firmware: `classic-chrome-candidate.json` (the fitted Classic Chrome parameters, [model report](../reports/classic-chrome-model.md)), `recipe-bank-spec.json` (the recipe bank's slots and placement) and `recipe-d3-compiled.json` (the D3 recipes). Their bytes equal the inputs the reviewed RAM payloads were pinned from. They are development inputs and appear in no published extension.

After the stock decodes of [step 1 of the experiment guide](../docs/experimenting-on-your-x-m1.md#1-set-up-once), these commands produce every input `ram-session recipe-bank|classic-chrome|classic-chrome-icon|usb-toggle` relinks from. Each checks its inputs against pinned hashes. The USB toggle needs only the stock image-5 decode; the others need RC0 installed on the camera.

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50
P=private-data/analysis/classic-chrome
C=private-data/analysis/classic-chrome-patch/candidate-build-1
mkdir -p "$R/update/prefix" "$R/filmsim" "$R/recipes/rc-complete-20261013" "$P" private-data/analysis/classic-chrome-patch
bazel build //host/firmware:apply-patch-set //host/firmware:rebuild-update
bazel-bin/host/firmware/apply-patch-set recovery/rc0-patch-set.json "$R/lz/id4-0061c400.bin" "$R/update/prefix/id4-patched.bin"
bazel-bin/host/firmware/rebuild-update extract-image2 "$D" "$R/filmsim/id2-00130000.bin"
# Classic Chrome and its icon variant
cp extensions/ram-inputs/classic-chrome-candidate.json "$P/candidate.json"
bazel run //host/firmware:build-classic-chrome -- "$P/candidate.json" "$C"
bazel-bin/host/firmware/apply-patch-set "$C/cc-image4.json" "$R/lz/id4-0061c400.bin" "$C/patched-id4.bin"
bazel-bin/host/firmware/apply-patch-set "$C/cc-image5.json" "$R/lz/id5-00bf1a00.bin" "$C/patched-id5.bin"
bazel-bin/host/firmware/apply-patch-set "$C/cc-plain.json" "$R/lz/plain-01100000.bin" "$C/patched-plain.bin"
bazel run //extensions:build-extension -- extensions/rc0.json "$D" private-data/analysis/classic-chrome-patch/rc0-regression
# the recipe bank
cp extensions/ram-inputs/recipe-bank-spec.json "$R/recipes/bank-spec.json"
cp extensions/ram-inputs/recipe-d3-compiled.json "$R/recipes/d3-compiled.json"
bazel run //host/firmware:build-recipe-package -- "$R/recipes/rc-complete-20261013/package-field-marker"
```
