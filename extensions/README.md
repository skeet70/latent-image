# Firmware extensions

`//extensions:build-extension` builds a patched X-M1 `FWUP0002.DAT` from an official DAT and an extension manifest. The [top-level README](../README.md#firmware-extensions) lists the published extensions and the install steps. Building runs on the host and performs no camera I/O.

```sh
devenv shell -- bazel run //extensions:build-extension -- \
  EXTENSION_JSON STOCK_DAT private-data/NEW_OUTPUT_DIRECTORY
```

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
| `plain-span` | `flash_offset`, `path`, `sha256` | `rebuild-update replace-plain --flash-offset` |

Steps run in container order: the image-4 patch set, the image-5 patch set, the label, then plain spans in increasing, non-overlapping flash order. Each image and the label can appear at most once. The patch set's `target.firmware_sha256` must equal the manifest's base. Image-5 patch sets cannot move the load address. The tool refuses any other segment or step kind, so an extension that needs a step not listed here fails instead of building a partial image. After the last step it runs `rebuild-update verify` on the result.

## Adding an extension

Add the manifest and its parts to this directory, or reference existing parts by relative path. Set `output_sha256` from an independently reviewed build. Add a row to the top-level README table once the extension has worked on a physical camera.
