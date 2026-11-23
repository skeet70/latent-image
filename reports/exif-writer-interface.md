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

## Blocks to a complete owned version

- No carrier: main has no overlay-46 staging path, so the owned unit is qualified natively only. A staged image needs a carrier for the plain image's overlays and a reference census like the DCF one; the plain image holds odd-valued words `0x011d00e1`, `0x011d0100`, `0x011d010c`, `0x011d011b` and `0x011d011d` (at `0x01217a70..0x012324c8`, outside overlay 46) that fall inside `0x011cfe54` and need review as data.
- No emulated capture reaches the writer ([#139](https://github.com/skeet70/latent-image/issues/139)), so there is no full-system pair.
- The saved replay depends on a parse that the overlay's parsers leave behind; its inputs here are synthetic slots, not a parse of a real file.
- The MPF index and attribute writers (`0x011c1d48`, `0x011c1d88`) and the RAF writers that share the helpers stay stock and call the owned entries.
