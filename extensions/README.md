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
