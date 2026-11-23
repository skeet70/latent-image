# Exif writer interface (X-M1 1.50)

The block step 1 of the [capture save path](capture-save-path.md) writes: `metadata_write_exif_header` `0x011db098` in overlay 46 of the plain image (`plain-01100000.bin` sha256 `ee211bda…d2889d`, base `0x01100000`) and `metadata_finish_exif` `0x011db188`. Everything here is read from the A32 bytes; the x70view decomp (`55436fc85c`) served as a reading aid. The owned unit is [`replacement/exif_writer`](../replacement/exif_writer/).

## Inputs

`0x011db098(output, ifd0, exif, maker, [sp] gps, [sp+4] ifd1, [sp+8] format, [sp+12] option)` returns the bytes written. The callers are `0x011d27f0` and `0x011d43a8`; the JPEG path passes `image_work + 0x42c`, `+0x5d8`, `+0x91c`, `+0xf218`, `+0xf2b8` and the words at `+0xf2e8`, `+0xf2ec`.

| Input | Read |
| --- | --- |
| source blocks | at the offset each tag-table rule names (`rule+0x1c`); the filters and capacities read further fields, the maker block up to `+0xe8f8` and its frame records at `+0x1d3c + 832*i` |
| identity `0x014e9d98` | make (16), model (32), software (64) copied into the IFD0 block at `+0`, `+0x10`, `+0x30`; interop index text `+0x70`; maker serial default `+0x74` |
| `format` (`SETTINGS+4`) | 0 builds each directory from the source block (`0x011cf990`); 1 and 2 replay a saved parse (`0x011cfe54`, passes 0 and 1); any other value writes no directory body |
| `option` (`SETTINGS+12`) | 1 admits rules with flag `0x10` (the GPS pointer and the GPS IFD) and enables the GPS directory |
| saved parse | state word `0x014e9e50` (bank 1 `0x014e9e54`) and file word `0x014e9e58` (`0x014e9e4c`); per directory a 12-byte slot (count, value offset, type) per rule and a value area, at state `+0xffc`/`+0x10d4` (IFD0), `+0x1c4c`/`+0x1e5c` (Exif), `+0x250c`/`+0x2c44` (maker), `+0xa444`/`+0xa51c` (GPS), `+0xa6f8`/`+0xa758` (IFD1) |
| retained tables | rules of 32 bytes (tag, flags, type, maximum count, callback, two words, pass-0 and pass-1 masks, source offset): IFD0 `0x011d09dc` (18), Exif `0x011d041c` (44), maker `0x011d0fbc` (154), image record `0x011d23c4` (9), interop `0x011d099c` (2), GPS `0x011d0c1c` (18), IFD1 `0x011d0ebc` (8), MPF `0x011d0e5c` (3) and `0x011d2324` (5); type sizes `0x011c2cb8`; mode masks `0x011d230c`; saved-replay masks `0x011d2314`, `0x011d24e4`, `0x011d040c`; markers and texts at `0x011c2ae8..0x011c2b1c`, `0x011d0404` and `0x011d22fc` |

## Output

At `output`: SOI (2), APP1 marker and a zero length (4), `Exif\0\0`, the little-endian TIFF header with IFD0 at 8, then in order IFD0, Exif IFD, maker note (`FUJIFILM`, `0x0c`, then an IFD whose offsets count from the note), interoperability IFD (`R98` from the identity, or `R03` when ColorSpace is `0xffff`; version `0100`), GPS IFD when `option` is 1 and the GPS pointer rule armed it, and IFD1. Each directory is followed by its out-of-line values, odd lengths padded with a zero; values of four bytes or fewer sit in the entry, zero-filled. Pointer tags are callbacks that record their value slot in the writer state `0x014e9d50`, and the following directory patches it. A filtered IFD0 also stores its next-IFD slot at `ifd0+0x154` and zero at `ifd0+0x158`.

`0x011db188(total, thumbnail_length)` stores `total - 4` big-endian as the APP1 length and `thumbnail_length` in the IFD1 JPEGInterchangeFormatLength slot.

Side effects outside the output: writer state `0x014e9d50..0x014e9d98`, settings `0x014e9e3c..0x014e9e4a`, a trace of the last directory and value at `0x014e9ea0..0x014e9efc`, and the identity copy into the IFD0 block.

## Services

All are leaf routines that only touch the memory their arguments name: byte copy `0x0004357c` (loader A), `memcpy` `0x00722054`, `strcpy` `0x00721b04`, `strncpy` `0x00721b40`, `strlen` `0x00721f30` and `memset` `0x007221c8` (which calls `0x007221a0`). Rule callbacks are reached through the table words with `(source, entry slot, value cursor, layout, [sp] entry)`.

## Extents and entries

The closure of `0x011db098` and `0x011db188`, with the callbacks the tables name, is 50 functions in `[0x011c14f0,0x011c1d18)`, `[0x011c1e00,0x011c1e7c)`, `[0x011c2154,0x011c2208)`, `[0x011c2b50,0x011c2b80)`, `[0x011c2c40,0x011c2cb8)`, `[0x011c35f8,0x011c36f8)`, `[0x011cda48,0x011d0404)` and `[0x011db098,0x011db1ac)`. Code outside the extents reaches 33 of them: the two entries, the 16 callbacks through table words, and initialize, IFD0, Exif, maker, GPS and thumbnail-length writers from `0x011cb750..0x011cb7a0`; the saved-parser transfer, slot and value lookups, table select and count, type size, value copy and halfword swap from the overlay's parsers and RAF writers; the filtered-directory writer from the MPF writers at `0x011c1d78` and `0x011c1df4`. Each keeps its stock address as a four-byte veneer.

## Branch coverage

The native suite folds the stock role's per-function branch bitmaps over the 50 functions: every conditional branch direction is reached by a case or listed in `unreachableDirections` (`replacement/exif_writer/Replacement/ExifWriter/Cases.hs`) with the bytes that exclude it, and the suite fails on an unlisted gap or a listed direction a case reaches. The exclusions fall into five groups:

- Constant settings: initialize `0x011c14f0` is the only store to the mode and byte order at `0x014e9e3c`, and its callers pass mode 0 (`0x011db0c4`) or 3 (`0x011cb740`) and order 0, so the mode-above-3 returns, the IFD1 writer's mode-above-2 return and the byte-swap skip in `0x011c1e00` never run. The RAF writer's mode 3 always comes with directory format 0.
- Pointer arming: the MakerNote and GPS callbacks arm their directories only inside a directory body, so neither is armed with a format above 2, and format 2's pass-1 replay skips rule `0x8825` (pass-1 mask 0).
- Table contents: rules `0x1210`, `0x4106` and `0x4206` have flags 0 and are never selected for the filter. Tags `0x1440`, `0x1441`, `0x4107..0x417f` and `0x4209..0x427f` are not in the maker table. Every rule type is in 1..10. Every directory kind a caller passes has a table and a saved area.
- Re-tests: `0x011cdbe4` and `0x011ce21c` re-read the halfword at `source+0x20a` that the preceding branch found to be `0xffff`. `0x011d02b8` re-tests a mask that `0x011d0198` already found covering. `0x011cf1e0` is reached only by tag `0x4104`.
- Callback reach: callback `0x011cee4c` (rule `0x1431`, pass-0 mask 9) is never generated by a format-1 replay. The `0x4282` image-record callback never sees a zero record count, because both filters reject `0x4282` then and pass 1 skips it.

## Words that name 0x011cfe54

Eighteen aligned words in the plain image fall inside `0x011cfe54`, all outside overlay 46: `0x011d0100` at `0x01217a70`, `0x01217f84` and `0x01218a54`; `0x011d011b` at `0x0121914c`; `0x011d011d` at `0x012195b4..0x012195c0`, `0x012195c8`, `0x012195f0..0x012195f8`, `0x01219600` and `0x01219fa4..0x01219fac`; `0x011d010c` at `0x012195c4`; `0x011d00e1` at `0x012324c8`. They are data:

- Each lies inside a run of words whose two halfwords are both below `0x400`: `[0x0121797c,0x01217b50)`, `[0x01217cd8,0x01217f98)`, `[0x012188ac,0x01218ab4)`, `[0x01218f80,0x0121933c)`, `[0x0121955c,0x01219890)` and `[0x01232468,0x012324d8)`. These are halfword tables: the neighbouring words have the same shape (`0x01000100`, `0x011c011b`, `0x010c011e`).
- Each of the first five runs contains addresses held in the pointer arrays at `0x01215510..0x0121565c`, `0x01216fec..0x01217144` and `0x012186e0..0x0121883c`. The sixth follows the tables at `0x01232368`, `0x01232380` and `0x012323a8`, which code at `0x01230d1c..0x01231de4` addresses with `movw`/`movt`.
- None is a usable code address. `0x011d00e1`, `0x011d011b` and `0x011d011d` are odd, so a `bx` would enter Thumb state in A32 code. `0x011d0100` (`str r9, [r3, #0x20]`) and `0x011d010c` (`beq`) lie inside `0x011cfe54`, not at its entry, and neither follows a `bl`, so neither is a return address. `0x011cfe54` is reached only by direct `bl` from the sub-writers.

The staging census lists the four aligned words among its reviewed sites and the odd ones as words that are not word-aligned. Over all seven scanned images it also found nine branches that would enter the extents in Thumb state and 18 further sites it reviews as data: halfword tables and high-entropy runs outside every function range, three A32 `bl` inside functions that read as Thumb `blx`, and two image-3 font words (`replacement/exif_writer/plain-overlay-unit.json`).

## Staged image

[`plain-overlay-stage`](../replacement/README.md#plain-image-overlay-staging) stages the writer with the [save file machine](save-file-machine.md) into the plain image and the DAT; overlay 46 is resident from attach on the cardless menu, card playback and PROTECT routes. On both card routes the overlay's saved-Exif parser (`0x011c25ec`, `0x011c36f8`, `0x011c6e30`) runs while playback reads the card's JPEGs and enters six owned entries: `saved_directory_slots`, `saved_directory_values`, `type_size`, `select_table`, `table_count` and `transfer_saved_parser`. Their interface (declared arguments, results and output bytes) is equal to stock, with no fetch of retired stock code (E-OVERLAY46-STAGED-PAIR-150). The header and directory writers are not reached.

## Blocks to a complete owned version

- No emulated capture reaches the header and directory writers ([#139](https://github.com/skeet70/latent-image/issues/139)); full-system pairs cover only the six parser-shared entries.
- The saved replay depends on a parse that the overlay's parsers leave behind. The suite replays synthetic slots and two stock-written Exif blocks read back as TIFF by the suite, not by the overlay's own parser.
- The MPF index and attribute writers (`0x011c1d48`, `0x011c1d88`) and the RAF writers that share the helpers stay stock and call the owned entries.
