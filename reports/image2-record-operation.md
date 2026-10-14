# Rebuilding the image-2 record and writing a compiled recipe name (1.50)

Host-only work on the pinned official 1.50 container `FWUP0002.DAT` (SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`). No camera and no USB device is touched. The emitted container is **not installable**: nothing here has had the independent review of its bytes that a write-capable artefact needs before it could go near the camera.

Evidence labels: **byte-read** for a direct read of a pinned artifact, **inherited** for a named record reused without re-derivation, **static-inference** for a conclusion drawn from those bytes, **unproved** for a named missing fact. The run results are host runs over host files.

## 1. The image-2 geometry the container declares

Image 2 is the id-2 segment entry's raw copy. The flashloader command `0x4c02` reads ids 2 and 3 raw from flash to the entry's load address (inherited: `private-data/analysis/x-m1-1.50/loaders/NOTES.md`), so the entry carries no record header and no compressed stream, unlike ids 4 and 5 whose flash spans hold a 20-byte header and the stream.

| Reading | Value | Source |
| --- | --- | --- |
| Segment entry 2 | `{2, 0x00130000, 0x0024bac8, 0, 0, 0x00ca0000, 0x00260000, 0, 0}` at media `0x1400d0` | byte-read |
| Extent in the main range | media `[0xca0000, 0xeebac8)`, 2,407,112 bytes | byte-read |
| Extent in the container file | raw `[0xca025c, 0xeebd18)`, the byte complement of the media image | byte-read |
| Window slack | media `[0xeebac8, 0xf00000)` is `0x00` (raw file `0xff`) | byte-read |
| Loaded content | the per-string-ID layout table and the GVFF bitmaps (inherited, `filmsim/NOTES.md`) | inherited |

The tool carves the extent out of the main range as its own part, between the image-5 record and the trailer, and refuses a container whose stored entry-2 words do not place it there. A supplied replacement must be the stored size word's length: the extent's media offset is fixed by the table, and a length change would move every later media structure without moving the words that describe them.

## 2. The operation and its gates

`//host/firmware:rebuild-update` gains three commands; `host/FujiAF/UpdateContainer.hs` gains the extent part, the replacement funnel and the assembly gate, and `host/FujiAF/LabelRecord.hs` owns the label record's buffer geometry and text encoding.

| Command | Behavior |
| --- | --- |
| `rebuild-update extract-image2 STOCK OUT_IMAGE2` | writes the id-2 extent in media form (the loader's raw copy), the form `replace-image2` reads |
| `rebuild-update replace-image2 STOCK IMAGE2_RAW OUTPUT` | the rebuild: substitute the extent, emit through the gated emitter, then verify from the written file |
| `rebuild-update write-label STOCK TEXT OUTPUT` | writes one printable-ASCII name (at most 27 UTF-16 code units plus terminator) into the id-0x83d record's inline buffer and emits through the same path |

`replaceImage2` (library) refuses a replacement whose length is not the container's own stored size word, and stores its complement. The emitter then reads the segment words back from its own assembly and compares them against the supplied part (`assemblyChecks`, a byte comparison of every named part, so a supplied length or offset the stored words do not describe cannot be emitted; the record-4 and record-5 paths gain the same gate). The carried set (identity records, selector, range fill, both restore lists) is still compared against the source container, and the stock restore lists against the pinned pairs.

The emitted file is then verified rather than trusted:

- the three range checksum words are recomputed from the file's own bytes over the ranges the file's own header words describe;
- the extent the file carries is the supplied image, byte for byte, read back through the file's own segment words;
- every byte outside the rebuilt extent and the main checksum word equals the source container (the strongest form of the carried-parts proof: identity records, selector, range fill, image 3, the plain image, both records, the trailer and both restore lists);
- a file that fails any of these is removed before the failure is reported.

The extent lies inside the main checksummed range, so the rebuild recomputes the main checksum word over the rebuilt bytes; the read-back recomputes it over the emitted file and compares the two.

## 3. Stock reproduction proof

The operation run with the stock extent must reproduce the stock file byte for byte. It does.

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50/update/image2
bazel-bin/host/firmware/rebuild-update extract-image2 "$D" "$R/stock-image2.bin"
bazel-bin/host/firmware/rebuild-update replace-image2 "$D" "$R/stock-image2.bin" "$R/stock-repro.DAT"
sha256sum "$D" "$R/stock-repro.DAT"
```

| Artefact | Bytes | SHA-256 |
| --- | --- | --- |
| stock id-2 extent `stock-image2.bin` (media form) | 2,407,112 | `236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5` |
| `stock-repro.DAT` | 38,405,148 | `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490` (the pinned stock hash) |

The command reports `extent vs stock: 0 bytes differ`, `main checksum word: 0x8f0ab18f -> 0x8f0ab18f`, `carried parts: 0 bytes differ outside the image-2 extent and the main checksum word`, `stock extent: the emitted container is byte-identical to the source over 38405148 bytes`, and 41/41 contract fields on the emitted container. `round-trip` on the same input still reproduces all 38,405,148 bytes, and `verify-stock` reports 59/59 checks on the pinned container (58 contract and pinned-stock fields plus the new image-2 size relation).

## 4. The emitted name container

`write-label` was run with the name `RECIPE 01` (the owner chooses the final text; this one is a demonstrator):

```sh
bazel-bin/host/firmware/rebuild-update write-label "$D" "RECIPE 01" "$R/FWUP0002-name.DAT"
```

| Reading | Value |
| --- | --- |
| Output | `FWUP0002-name.DAT`, 38,405,148 bytes, SHA-256 `17119b218dbb1056bc257e648cd9ed173466a394a94d5f44d95b5f3229b4f5d6` |
| Record buffer | image offset `0x24b7c6` (RAM `0x0037b7c6`, image load `0x00130000`) |
| File range written | raw `[0xeeba22, 0xeeba5a)` (the 56-byte buffer, stored complemented) |
| Extent change | 49 bytes differ from stock, first at image offset `0x24b7c6` |
| Main checksum word | `0x8f0ab18f` -> `0x8f0acc09`, recomputed from the emitted file |
| Carried parts | 0 bytes differ outside the image-2 extent and the main checksum word |
| Part diff | only `image 2 extent`: 49 bytes changed |

The read-back decodes the emitted file rather than the tool's memory: the raw bytes at `[0xca025c, 0xeebd18)` complement back to the media image, whose buffer at `0x24b7c6` reads `52 00 45 00 43 00 49 00 50 00 45 00 20 00 30 00 31 00 00 00 00 00 ...`, that is `RECIPE 01` as UTF-16LE, a zero terminator, then the zero fill; the file stores their complement (`ad ff ba ff ...`). The record's five layout halfwords read back as `{20, 20, 30, 36, 48}`, the stock values, so the name inherits the ordinary menu item box and no layout edit rides with it. The 49 differing bytes are the name's 18 bytes, its 2-byte terminator unit and the 30 bytes of stock text the write clears; the bytes that match are one `0x00` inside that span (the stock first unit's high byte) and the six zero bytes at the buffer's end.

## 5. Tests

All host-only, in the checked-in `devenv`/Bazel environment:

| Target | Result |
| --- | --- |
| `//host/firmware:update-container-tests` | 202 contract checks pass (fixture now carries a real id-2 extent; new cases: extent bytes and geometry, identity rebuild reproduction, length refusal, content-change diff, emitter refusals for length and offset drift, the image-2 size relation check, and three parse refusals for an extent that is empty, starts before the image-5 record or overlaps the trailer) |
| `//host/firmware:label-record-tests` | 20 label-record checks pass (pinned buffer address, load-relative offset and both refusals, layout carry, UTF-16 write with terminator and zero fill, bytes outside the buffer unchanged, read-back, the 27-unit boundary, and refusals for a long, empty, non-ASCII or wrong-layout source) |
| `//host/firmware:container-build-tests` | 46 CLI checks pass (the stock extent reproduces the source byte for byte, a changed extent moves only the extent and the main checksum word against the emitted file, and five refusals write no output file) |
| `//tests:research-contracts` | passes with `--cache_test_results=no` |
| `//host/... //lz/... //tests:research-contracts` | all 7 test targets pass |

Both fixtures now describe entry 2 by its real extent, so the container tests exercise the geometry the pinned container carries rather than a placeholder.

## 6. What is not established, and the installable status

- The name resolves only through the id-0x83d record's inline buffer: the accessor `FUN_01179b0c` returns that record for 0x83d and nothing for 0x83e (inherited, E-LABEL-RESOLVE-150). Nothing in this tool changes a film-simulation array, a menu node or the renderer, so the name has no surface on stock firmware.
- The extent's consumers beyond the raw load are not re-derived here; the tool's claim is the container contract, not the content's meaning.
- The write is host-only and pipeline evidence: the emitted container is `installable: false` until an independent review of its bytes (the field diff, the extent diff and the checksum) accepts it, and the owner's decision is unchanged.

## 7. Reproduction

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50/update/image2

bazel-bin/host/firmware/rebuild-update verify-stock "$D"
bazel-bin/host/firmware/rebuild-update round-trip "$D" "$R/stock-roundtrip.DAT"
bazel-bin/host/firmware/rebuild-update extract-image2 "$D" "$R/stock-image2.bin"
bazel-bin/host/firmware/rebuild-update replace-image2 "$D" "$R/stock-image2.bin" "$R/stock-repro.DAT"
bazel-bin/host/firmware/rebuild-update write-label "$D" "RECIPE 01" "$R/FWUP0002-name.DAT"
bazel-bin/host/firmware/rebuild-update diff "$D" "$R/FWUP0002-name.DAT"

# the emitted record buffer, complemented (raw file bytes)
od -A x -t x1 -v -j $((0xeeba22)) -N $((0x42)) "$R/FWUP0002-name.DAT"

devenv shell -- bazel test //host/... //lz/... //tests:research-contracts --test_output=errors
```
