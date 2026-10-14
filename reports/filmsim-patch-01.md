# FILMSIM-PATCH-01: RC0 decode-prefix extension (1.50)

**Host-only prefix accepted by static inference.** The owner chose decode extension on 2026-10-09. The new values and labels occupy a 192-byte prefix at `0x0061c340` and `0x0061c380`, while all original image-4 bytes keep their RAM addresses and bss is unchanged. The rejected in-decode placements remain rejected. **Installed 2026-10-09** (E-FILMSIM-INSTALL-150): the owner's SD card install of the container built from this manifest ran, the body boots, and the relocated arrays are live in an ordinary menu, so the runtime writer absence is hardware-observed over that boot path. The manifest stays `installable:false` as a host-build state, and the write-trace residual stays named in section 5.3.
Evidence labels are **byte-read**, **static-inference**, **hardware-observed**, **emulated** and **unproved**. The inherited offer-list emulation is address-independent and is not ownership evidence.
The machine-readable form is [recovery/rc0-patch-set.json](../recovery/rc0-patch-set.json), preserved byte-for-byte from the accepted private build manifest, with the array bytes byte-identical to the accepted E-FILMSIM-EMUL-150 case input `private-data/analysis/x-m1-1.50/native-case/filmsim-offer/arrays-patched.bin` (SHA-256 `1f119230453bd78db681a2b2b931c60d5e74c0c0b5c296a56501a1538e3a87f2`).
Stock source addresses are relative to image 4 (`lz/id4-0061c400.bin`, base `0x0061c400`, SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`). Its input offset is `address - 0x0061c400`; the extended output offset is `address - 0x0061c340`.
The independent review verified all 68 words of the rejected placement against that decode, the ordering rule, matrix columns, surface coverage and exclusion of the 781 runtime-written bytes (byte-read, inherited review). Its ownership finding invalidates placement, not those contents.

## 1. Surface coverage: one relocation serves all three nodes

The shooting menu node `0x007823a4`, the Fn node `0x00785d74` and the type-9 node `0x0077d184` share the values array `0x0078319c` and the labels array `0x0078d4f8`, and a word census over every image finds six words naming those two arrays and no seventh: `0x007823c4`/`0x007823c8`, `0x00785d94`/`0x00785d98`, `0x0077d1a4`/`0x0077d1a8` (byte-read of every aligned word and every `movw`/`movt` pair of id4, id5, the plain image and both loader copies, and of every byte offset of id2, id3, both boot records, the EN language slot and the captured id4/id5 bss of two boots).
One relocation therefore serves all three surfaces: one new values array, one new labels array, and one pointer swap per node, six words in total.
Each node keeps its own pointer words at `+0x20`/`+0x24`, so the swaps are per node while the arrays are shared; three separate array pairs would buy nothing.
The stock arrays become unreachable only after all six pointer words have moved (static-inference from the reviewed six-word census).

| Surface | Node | RC0 disposition |
| --- | --- | --- |
| shooting menu FILM SIMULATION | `0x007823a4` | covered: pointer pair swapped to the relocated arrays |
| Fn-assigned submenu | `0x00785d74` | covered: same |
| unnamed text node | `0x0077d184` | covered: same |
| Q menu cell | `0x0078b000`, values `0x00782348`, labels `0x00789e2c` | unchanged: its rows name col2 `{0, 3, 1, 4, 5}` and its labels are glyph ids in the closed Q space of recipe-design P5, which RC0 does not enter |
| FILM 1-3 bracket children | `0x00786774`, `0x00786da8`, `0x007898d4` (properties 0x100-0x102) | unchanged: own arrays, and the matrix cells touch only keys `0xb`/`0xc`, which no bracket row carries |
| unlabelled nine-row list | `0x0078d2d0` | unchanged: its rows `{0, 3, 1, 7, 0xb, 9, 8, 0xa, 0xc}` resolve to col2 `{0, 3, 1, 4, 7, 9, 8, 0xa, 5}` (byte-read conversion table `0x0077ac30`), so no row of it touches matrix keys `0xb`/`0xc` and the new cells do not reach it |
| RAW-conversion list | `0x0078bd5c` (property 0x22b) | unchanged: own code domain |
| shooting-menu tile, Fn function list, display-customisation list | `0x0078ea98` label, `0x0078d03c`, `0x00789170` | unchanged: they name the item, not its values |
| My Menu, custom-recipe editor, C1-C7 switch | none on 1.50 | not applicable (recipe-design 14.2) |

One shared-matrix side effect comes with change A and is stated rather than hidden: a stored film value with col2 `0xb` or `0xc` stops being replaced by the mode default in the C-record compare `FUN_00d4bbc4` (static-inference, filmsim-offer-mechanism change A), which is the intended behaviour once such a value is selectable.
Every other consumer keeps stock inputs: the conversion table `0x0077ac30`, develop dispatch `FUN_00d78418`, the metadata encoder `FUN_00eaac74` and the probe code are untouched.

## 2. Accepted prefix write set

**Byte-read:** the manifest specifies 68 ordered word writes: 16 values words, 32 labels words, 14 matrix cells and six pointer words. The prefix adds 192 bytes to the decoded file; 82 prefix-template bytes and 32 preserved-body bytes differ, giving 114 changed bytes against the constructed zero-prefix-plus-stock template. Prefix current bytes are creation-template bytes, not bytes of the stock decode. They also equal the table preload at the corresponding RAM addresses. All table columns below show four little-endian bytes, not a displayed numerical word.
**Static-inference:** values and labels occupy `[0x0061c340,0x0061c380)` and `[0x0061c380,0x0061c400)`. The arrays complete before matrix changes and pointer swaps; each node receives its labels pointer before its values pointer. The container is rebuilt offline, so there is no intermediate live walker state. This manifest must not be used as a live-write input.

| Order | RAM address | Output offset | Current bytes | New bytes | Role |
| --- | --- | --- | --- | --- | --- |
| 1 | `0x0061c340` | `0x00000000` | `00000000` | `00000000` | values-array |
| 2 | `0x0061c344` | `0x00000004` | `00000000` | `00000000` | values-array |
| 3 | `0x0061c348` | `0x00000008` | `00000000` | `03000000` | values-array |
| 4 | `0x0061c34c` | `0x0000000c` | `00000000` | `00000000` | values-array |
| 5 | `0x0061c350` | `0x00000010` | `00000000` | `01000000` | values-array |
| 6 | `0x0061c354` | `0x00000014` | `00000000` | `00000000` | values-array |
| 7 | `0x0061c358` | `0x00000018` | `00000000` | `07000000` | values-array |
| 8 | `0x0061c35c` | `0x0000001c` | `00000000` | `00000000` | values-array |
| 9 | `0x0061c360` | `0x00000020` | `00000000` | `0c000000` | values-array |
| 10 | `0x0061c364` | `0x00000024` | `00000000` | `00000000` | values-array |
| 11 | `0x0061c368` | `0x00000028` | `00000000` | `05000000` | values-array |
| 12 | `0x0061c36c` | `0x0000002c` | `00000000` | `00000000` | values-array |
| 13 | `0x0061c370` | `0x00000030` | `00000000` | `06000000` | values-array |
| 14 | `0x0061c374` | `0x00000034` | `00000000` | `00000000` | values-array |
| 15 | `0x0061c378` | `0x00000038` | `00000000` | `feffff7f` | values-array |
| 16 | `0x0061c37c` | `0x0000003c` | `00000000` | `00000000` | values-array |
| 17 | `0x0061c380` | `0x00000040` | `00000000` | `8a040000` | labels-array |
| 18 | `0x0061c384` | `0x00000044` | `00000000` | `89040000` | labels-array |
| 19 | `0x0061c388` | `0x00000048` | `00000000` | `8c040000` | labels-array |
| 20 | `0x0061c38c` | `0x0000004c` | `00000000` | `8b040000` | labels-array |
| 21 | `0x0061c390` | `0x00000050` | `00000000` | `8e040000` | labels-array |
| 22 | `0x0061c394` | `0x00000054` | `00000000` | `8d040000` | labels-array |
| 23 | `0x0061c398` | `0x00000058` | `00000000` | `90040000` | labels-array |
| 24 | `0x0061c39c` | `0x0000005c` | `00000000` | `8f040000` | labels-array |
| 25 | `0x0061c3a0` | `0x00000060` | `00000000` | `92040000` | labels-array |
| 26 | `0x0061c3a4` | `0x00000064` | `00000000` | `91040000` | labels-array |
| 27 | `0x0061c3a8` | `0x00000068` | `00000000` | `94040000` | labels-array |
| 28 | `0x0061c3ac` | `0x0000006c` | `00000000` | `93040000` | labels-array |
| 29 | `0x0061c3b0` | `0x00000070` | `00000000` | `9f040000` | labels-array |
| 30 | `0x0061c3b4` | `0x00000074` | `00000000` | `9e040000` | labels-array |
| 31 | `0x0061c3b8` | `0x00000078` | `00000000` | `a2040000` | labels-array |
| 32 | `0x0061c3bc` | `0x0000007c` | `00000000` | `a0040000` | labels-array |
| 33 | `0x0061c3c0` | `0x00000080` | `00000000` | `b0040000` | labels-array |
| 34 | `0x0061c3c4` | `0x00000084` | `00000000` | `af040000` | labels-array |
| 35 | `0x0061c3c8` | `0x00000088` | `00000000` | `b2040000` | labels-array |
| 36 | `0x0061c3cc` | `0x0000008c` | `00000000` | `b1040000` | labels-array |
| 37 | `0x0061c3d0` | `0x00000090` | `00000000` | `9b040000` | labels-array |
| 38 | `0x0061c3d4` | `0x00000094` | `00000000` | `9a040000` | labels-array |
| 39 | `0x0061c3d8` | `0x00000098` | `00000000` | `99040000` | labels-array |
| 40 | `0x0061c3dc` | `0x0000009c` | `00000000` | `9c040000` | labels-array |
| 41 | `0x0061c3e0` | `0x000000a0` | `00000000` | `97040000` | labels-array |
| 42 | `0x0061c3e4` | `0x000000a4` | `00000000` | `96040000` | labels-array |
| 43 | `0x0061c3e8` | `0x000000a8` | `00000000` | `95040000` | labels-array |
| 44 | `0x0061c3ec` | `0x000000ac` | `00000000` | `98040000` | labels-array |
| 45 | `0x0061c3f0` | `0x000000b0` | `00000000` | `ffffffff` | labels-array |
| 46 | `0x0061c3f4` | `0x000000b4` | `00000000` | `ffffffff` | labels-array |
| 47 | `0x0061c3f8` | `0x000000b8` | `00000000` | `ffffffff` | labels-array |
| 48 | `0x0061c3fc` | `0x000000bc` | `00000000` | `ffffffff` | labels-array |
| 49 | `0x00795db4` | `0x00179a74` | `00000000` | `01000000` | matrix key 11, column 1 |
| 50 | `0x00795db8` | `0x00179a78` | `00000000` | `01000000` | matrix key 11, column 2 |
| 51 | `0x00795dc0` | `0x00179a80` | `00000000` | `01000000` | matrix key 11, column 4 |
| 52 | `0x00795dc4` | `0x00179a84` | `00000000` | `01000000` | matrix key 11, column 5 |
| 53 | `0x00795dc8` | `0x00179a88` | `00000000` | `01000000` | matrix key 11, column 6 |
| 54 | `0x00795dcc` | `0x00179a8c` | `00000000` | `01000000` | matrix key 11, column 7 |
| 55 | `0x00795e04` | `0x00179ac4` | `00000000` | `01000000` | matrix key 11, column 21 |
| 56 | `0x00795e10` | `0x00179ad0` | `00000000` | `01000000` | matrix key 12, column 1 |
| 57 | `0x00795e14` | `0x00179ad4` | `00000000` | `01000000` | matrix key 12, column 2 |
| 58 | `0x00795e1c` | `0x00179adc` | `00000000` | `01000000` | matrix key 12, column 4 |
| 59 | `0x00795e20` | `0x00179ae0` | `00000000` | `01000000` | matrix key 12, column 5 |
| 60 | `0x00795e24` | `0x00179ae4` | `00000000` | `01000000` | matrix key 12, column 6 |
| 61 | `0x00795e28` | `0x00179ae8` | `00000000` | `01000000` | matrix key 12, column 7 |
| 62 | `0x00795e60` | `0x00179b20` | `00000000` | `01000000` | matrix key 12, column 21 |
| 63 | `0x007823c8` | `0x00166088` | `f8d47800` | `80c36100` | labels-array pointer |
| 64 | `0x007823c4` | `0x00166084` | `9c317800` | `40c36100` | values-array pointer |
| 65 | `0x00785d98` | `0x00169a58` | `f8d47800` | `80c36100` | labels-array pointer |
| 66 | `0x00785d94` | `0x00169a54` | `9c317800` | `40c36100` | values-array pointer |
| 67 | `0x0077d1a8` | `0x00160e68` | `f8d47800` | `80c36100` | labels-array pointer |
| 68 | `0x0077d1a4` | `0x00160e64` | `9c317800` | `40c36100` | values-array pointer |


The matrix cells are word writes of `0x00000001` into the 22 zero cells of the key-`0xb` row at `0x00795dac` and key-`0xc` row at `0x00795e08` (byte-read; the leading key words are nonzero), cell `+4 + 4·column` into the row, column index taken from the mode list at `0x00792fac` (`1, 0, 0x1f, 0x20, 0x24, 0x23, 0x22, 0x21, 5, 0x18, 8, 0xb, 0xc, 0xd, 0x12, 0x13, 0x11, 0x10, 0x16, 0x15, 0x17, 0x2c`, terminator `0x2d`).
The covered columns 1, 2, 4, 5, 6, 7 and 21 are the ASTIA and Velvia pattern (modes 0, `0x1f`, `0x24`, `0x23`, `0x22`, `0x21`, `0x2c`), the seven-column option the owner has not overridden; the value `1` matches the stock ASTIA and Velvia rows byte for byte and matches the accepted emulation input.

### 2.1 New values at `0x0061c340` (orders 1-16)

Offsets below are relative to the values destination; the current creation-template bytes are zero.

```
00000000: 00000000 00000000 03000000 00000000   row 0, row 3
00000010: 01000000 00000000 07000000 00000000   row 1, row 7
00000020: 0c000000 00000000 05000000 00000000   row 0xc, row 5 (PRO Neg Std)
00000030: 06000000 00000000 feffff7f 00000000   row 6 (PRO Neg Hi), terminator
```

Each pair is `{conversion-table row, child}`; all children are 0 and the walk stops at `{0x7ffffffe, 0}` (byte-read structure, filmsim-offer-mechanism section 2).
Rows 5 and 6 are the stock table rows `{9, -, 0xb}` and `{0xa, -, 0xc}` of `0x0077ac30`, so no conversion-table byte changes.

### 2.2 New labels at `0x0061c380` (orders 17-48)

Offsets below are relative to the labels destination; the current creation-template bytes are zero. The displayed words are numerical ids; section 2 lists their little-endian write bytes.

```
00000000: 0000048a 00000489 0000048c 0000048b   PROVIA
00000010: 0000048e 0000048d 00000490 0000048f   Velvia
00000020: 00000492 00000491 00000494 00000493   ASTIA
00000030: 0000049f 0000049e 000004a2 000004a0   B&W
00000040: 000004b0 000004af 000004b2 000004b1   SEPIA
00000050: 0000049b 0000049a 00000499 0000049c   PRO Neg Std
00000060: 00000497 00000496 00000495 00000498   PRO Neg Hi
00000070: ffffffff ffffffff ffffffff ffffffff   terminator
```

Each entry is `{preview icon, list icon, help, name}` string ids, terminated by `-1`, parallel to the values array (byte-read structure).
The first five entries are byte-identical to the stock labels at `0x0078d4f8`, and the two appended quadruples reuse the existing ids `0x495`-`0x49c`, which sit below the `0x83c` class boundary, so RC0 adds no string id and no label record (byte-read, E-FILMSIM-OFFER-MAP-150).

## 3. What is untouched

The disjoint-relocation design leaves the stock values `[0x0078319c,0x007831cc)` and labels `[0x0078d4f8,0x0078d558)` unchanged. They could instead be reclaimed once unreachable, but their capacity is insufficient (section 4.3).
The conversion table `0x0077ac30` and its pointer words at `0x0081b09c + 4·property`, the descriptor list at `0x0079944c`, the matrix rows of every other key, `FUN_00d52684`, `FUN_0061d534`, `FUN_00631b74` and the whole commit path are not written.
No code word is written anywhere: the change is data only (static-inference from the write set above).

## 4. Descriptor-aware ownership result

**No accepted region survives.** Every previously named candidate is rejected, including both runs formerly given equal standing. The scanner also enumerates the whole image's fill runs rather than treating that earlier table as a complete census. This is an ownership rejection, not a proof that every byte of the decode has a known consumer.

### 4.1 The new test

Before choosing an address, enumerate every adjacent little-endian word pair at every byte offset of each shipped artifact. Interpret each pair as `{base,size}` and `{base,end}`, normalize bit-30 aliases of addresses, use 64-bit addition to reject wrapped size spans, and intersect the resulting half-open span with every candidate. A candidate is rejected if any confirmed consumer-owned span overlaps the intended write bytes, even when neither word names an address inside the candidate. A whole-resource descriptor can therefore reject a zero-filled interior without any direct reference to that interior (static-inference).

The bounded C helper is `private-data/analysis/x-m1-1.50/filmsim/patch-01/descriptor-coverage.c`. Its historical host run scans id4, id5, the plain image, both loaders, both boot records and EN and finds **399** maximal zero/`0xff` runs of at least 64 bytes in image 4 (byte-read). Its historical id2/id3 raw-offset and id2-polarity mistakes are corrected in the fresh prefix census, section 5.3. `descriptor-coverage.txt` retains the old census and consumer-confirmed registry pairs; numeric pairs outside those registries are not silently classified as descriptors.

A raw pair hit is not by itself a consumer proof: neighbouring pointers, packed samples, instruction bytes and aggregate load descriptors also match. Confirm field meaning and the consumer before calling a span owned. Conversely, absence of a raw pair is not acceptance: indirect or non-adjacent fields, arithmetic counts, `movw`/`movt`-formed descriptors, indexes from a named parent and runtime-built descriptors still require a consumer-side argument. No claim of complete descriptor discovery is made (unproved). The scanner deliberately admits spans beginning below or ending above image 4, not only bases inside a candidate.

### 4.2 Eliminations and corrected boundaries

| Candidate or family | Ownership disposition |
| --- | --- |
| `[0x007d28fc,0x007d338a)`, 2,702 bytes; old arrays `[0x007d2e00,0x007d2ec0)` | rejected: registry entry 20 at `0x007dcf50` holds `{0x007d2180,0x2400}`, span `[0x007d2180,0x007d4580)` (byte-read) |
| formerly named alternate `[0x007e2ddc,0x007e33b4)` | rejected: actual maximal run is `[0x007e2ddc,0x007e3404)`, 1,576 bytes; record 7 starts at `0x00810fa8`, with `{base,size}` fields at `0x00810fac`/`0x00810fb0` holding `{0x007e1e7c,0x19c0}`, span `[0x007e1e7c,0x007e383c)` (byte-read) |
| `[0x007d3afc,0x007d458c)` | rejected: the first descriptor covers all but the last 12 bytes; its remaining tail cannot hold even the 64-byte values array |
| late zero runs within `[0x007d4588,0x007d99c8)`, `[0x007d99d0,0x007da0d0)`, `[0x007dcf64,0x007e1e64)` | rejected: size-armed registry pairs at `0x00810fb4`, `0x007dcf38`, `0x00811014`; boundary-crossing tails are too small for either array (byte-read) |
| late zero runs within `[0x007e383c,0x007e683c)`, `[0x007e683c,0x007eaffc)`, `[0x0080270c,0x00803f8c)` | rejected: registry pairs at `0x00810fe4`, `0x00810ffc`, `0x00810fdc`; the adjacent parameter words are not a gap (byte-read, static-inference) |
| four blocks `[0x00804c0c,0x0080617e)`, `[0x00806dec,0x0080835a)`, `[0x00808fc8,0x0080a534)`, `[0x0080b1a2,0x0080c71c)` | rejected: `{0x00803f90,0x8780}` at `0x007dcf58` covers `[0x00803f90,0x0080c710)`; the last 12-byte tail is too small (byte-read), strengthening the earlier referenced-family rejection |
| `[0x0080eed8,0x0080ef2c)` and `[0x0080f330,0x0080f3d8)` | rejected: `{0x0080c718,0x2cc0}` at `0x0081100c` covers both (byte-read) |
| `[0x007d0c74,0x007d13f4)` | previous rejection retained: buffer base and `0x780` size are named at `DAT_0093b618`/`DAT_0093b620` (static-inference) |
| `[0x007cc8e5,0x007ccae0)` and `[0x007cbeb3,0x007cbfa0)` | previous rejection retained: indexed dispatch and source/count tables (static-inference) |
| `0x007a157a`-`0x007a1f04` cluster | previous rejection retained: literal-pool-named buffers at `0x00639b0c`-`0x00639b18` (byte-read, static-inference) |
| high `0xff` ramp runs, bad-block remap run, `[0x0089ca35,0x0089cda0)`, ordinary-settings default zeros | previous rejection retained: curve contents, boot-filled remap state, interior table pointers and settings defaults (byte-read, static-inference; remap capture hardware-observed) |
| other census runs, including the menu/matrix zeros and additional short data interiors | not accepted: the raw census is not an ownership argument; no established consumer exclusion or patch-owned capacity is supplied for them (unproved) |
| unloaded flash slack, plain-extent gaps, `[0x0089ec00,0x0089ec28)` | previous rejection retained: no RAM load, or only 40 bytes at the bss-clear boundary |

The descriptor field meaning is consumer-bound: `FUN_006ed0d8` reads the `0x007dceb0` registry's second word as a length when it is at least 3; `FUN_006ed150` does the same for the 16-byte records rooted at `0x00810f38`, loading the pointer at `+4` and length at `+8`. Both queue the pair through `FUN_006ecb64`; `FUN_006ec86c` passes it to `FUN_006ec660(ptr,len,1)`, then the driver path uses opcode `0x2e04` (static-inference, independent review and `filmsim/ghidra/all.c` lines 128885-129308). Whether a particular ordinary camera action transfers or writes these resources remains unproved and is not needed to reject their ownership.

The former claim of zero unaligned address-window hits is withdrawn: the independent review found byte-shifted instruction/parameter coincidences in id4, id5, plain and bss. The substantive aligned-literal and resolved-`movw`/`movt` results did not prove absence of a covering descriptor. The two captured boots show no changed bytes here (hardware-observed), not absence of a writer; value-identical clears and re-copies evade a byte diff. Neither observation rescues the rejected region.

There is no reserved in-decode residual slack. The old run continues to `0x007d338a`, so its post-array zero suffix would have been 1,226 bytes, not 1,214; it belongs to the same shipped resource. An ownership-accepted region is falsified by a covering consumer span, a parent/index path into it, an overlapping writer even if value-identical, a runtime-built reference, or a target-build change. No in-decode placement is accepted.

### 4.3 Space the patch itself frees: decisive capacity and ordering

The six-word census establishes that relocating all three nodes makes the stock values extent `[0x0078319c,0x007831cc)` (`0x30` bytes) and labels extent `[0x0078d4f8,0x0078d558)` (`0x60` bytes) unreachable (byte-read references, static-inference). They reclaim **`0x90` = 144 bytes**, in two disjoint extents. The seven-entry arrays with their terminators require **`0x40` + `0x80` = `0xc0` = 192 bytes**. The shortfall is **`0x30` = 48 bytes**. Values alone exceed their stock extent by `0x10`; labels exceed theirs by `0x20`. Even ignoring contiguity, the new arrays do not fit. Crossing either end overwrites the next property's array or node, not reclaimed space (byte-read). No overlapping-source placement is derived.

For a live word-write route, the old arrays are not free until every referencing node has stopped using them and any active walker is quiescent. Arrays-first is unsafe when the destination overlaps a still-referenced source. An overlapping-source design would need complete arrays in a distinct owned staging location, then all six node retargets, then source reclamation and complete final arrays, then a final labels-before-values retarget per node. Without such owned staging or a consumer-quiescence mechanism, word ordering alone cannot make reclamation safe (static-inference). An offline container rebuild has no runtime intermediate array state, but it still has the same capacity shortfall.

Direct reclamation of the existing two arrays is insufficient. The owner chose the prefix-extension shape assessed in section 5; the linked body remains at its original addresses.

## 5. Decode-prefix extension and container contract

The owner chose decode extension on 2026-10-09. **Static-inference:** prepend `0xc0` bytes and lower image 4's load from `0x0061c400` to `0x0061c340`. Every original byte moves forward `0xc0` in the file and remains at its original RAM address. This is an extended decoded object, not loading the unextended linked body `0xc0` lower. No original code address, absolute data pointer, entry point, bss address or later image load moves. Values use its first `0x40` bytes and labels its next `0x80`; no reclaimed stock extent is used.

### 5.1 Intended fields, recorded before encoding

These are the intended values, not an encoder-derived result. **Byte-read** labels apply to the stock column; **static-inference** labels apply to the intended column and invariants.

| Field | Stock | Intended prefix build |
| --- | --- | --- |
| segment 4 load, table media `0x1400d4` | `0x0061c400` | `0x0061c340` |
| record decoded size, word 0 | `0x00282800` | `0x002828c0` |
| segment 4 size, table media `0x1400d8` | `0x00282828` | `0x002828e8` |
| segment 4 bss base / length | `0x0089ec00` / `0x00352e00` | unchanged |
| bss end, next image 5 load | `0x00bf1a00` | unchanged |
| original 32-byte embedded header at RAM `0x0061c400` | `{0,61c400,282828,69cdf8,0,0,0,aaaaffff}` | byte-identical, now output offset `0xc0` |
| trailer words 5 / 6 | `0x0061c400` / `0x00282828` | `0x0061c340` / `0x002828e8` |
| trailer words 0 / 1 | `0x024a0000` / `0x024a0000` | unchanged |
| record word 1, total_len | includes the 20-byte record header | encoder-measured, compare stored word against emitted record byte count |
| record word 2, chunk count | `ceil(total_len / 0x4000)` | encoder-measured with corrected total_len semantics |
| record words 3 / 4 | feed page `0x4000`, mode `0` | unchanged |
| segment 4 flash base / reserved lengths | `0x20` / `0x160000` / `0x160000` | unchanged |
| main checksum, raw `0x20c` | stock complement sum | recomputed over the changed main payload |
| DAT extents, identity/version fields, selector, auxiliary bytes and restore lists | stock | carried byte-identical |

**Static-inference:** `load + decoded_size = 0x0089ec00` and `load + segment_size = 0x0089ec28` both remain fixed. The 40-byte decoded/segment-size difference is retained. The segment and trailer must agree on the new load and size; the embedded header continues to describe the linked body at its original address. Only the two table words, record header and stream, trailer words 5/6 and main checksum may differ. Record growth and its off-feed-grid remainder are not carried over from the former placement. The corrected record length must fit `0x160000`; flash headroom does not establish RAM ownership.

### 5.2 Loader and header consumers

**Static-inference, boot disassembly and pseudocode:** `FUN_00002250` consumes table-entry load, bss base/length, flash base and reserved flash length. It stages the compressed source at `0x42e40000`, clears the same bss interval, and passes the declared load to `FUN_00003d3c`. That hardware setup ORs the destination's bit-30 alias and advances by produced byte count; the mapped path does not round the load to `0x0061c400`. The `0x0061c340` destination is 64-byte aligned. The flash-read length and its 11-block transfer are unchanged.

**Byte-read and static-inference:** both boot records retain the fixed embedded-header access at `0x0061c40c`; the mapped entry transfer does not fetch an entry point from new segment-load-plus-12. The prefix therefore cannot replace the original header, and the rebuild gate must compare its preserved 32 bytes, not rewrite them. Application getter `FUN_00657114` walks nine-word table entries, but the aligned direct-branch census finds its application callers requesting ids 2, 3, 5, 6 and 7, not 4. `FUN_006570f4`, `FUN_00657184` and `FUN_006571a0` access the table header and plain-extent descriptors at `0x0061c000`. This census is not proof against every indirect call or computed consumer (unproved).

### 5.3 Prefix ownership and overwrite order

**Byte-read:** the table, segment records, terminator and plain-extent records fit below `0x0061c200`; the last plain-extent terminator is at `0x0061c1f8`. The candidate `[0x0061c340,0x0061c400)` lies above those retained objects. Its stock preload bytes are zero. Zero fill is not the ownership argument.

**Static-inference:** boot `FUN_00001dc4` copies the table block from media `0x140000` to `0x0061c000` by `FUN_0000218c(...,1)`, a `0x20000`-byte transfer covering the candidate and existing image body. The mapped blocking copy completes before the image-4 loader/decompression path is entered. Stock decompression already overwrites the copied block from `0x0061c400` upward; prefix decompression additionally overwrites `[0x0061c340,0x0061c400)` after that copy, not before it. Retained table walkers stop at their terminators below the prefix. No mapped parent/index path reads the table-block tail as live descriptor content.

**Byte-read and static-inference:** confirmed resource registries at `0x007dceb0` and `0x00810f38` do not cover the prefix. Language records rooted at `0x007c8db4` hold flash source, block count and RAM destination; three `0x20000` blocks loaded at `0x005bc000` end at `0x0061c000`, below the candidate. Treating adjacent `{destination,next flash source}` fields as `{base,size}` invents a covering span whose second field the consumer never uses as a size.

**Byte-read:** the fresh bounded C census scans adjacent little-endian pairs at every byte offset with 64-bit span sums and bit-30 address normalization, direct window literals, adjacent `movw`/`movt` formations and direct branch callers. Inputs include id4, id5, plain, both loaders, both boot records, EN, complemented id2 at DAT raw `0x00ca025c` and complemented id3 at `0x0106025c`, plus the actual table bytes. The earlier in-decode census's id2/id3 raw offsets and id2 polarity were wrong; its confirmed image-4 descriptor rejections still stand, but those historical input ranges cannot establish whole-artifact coverage. The fresh output is `filmsim/patch-01/prefix-census.txt`, generated by `extension-census.c`.

**Byte-read:** the two direct candidate-window matches are unaligned instruction fragments, not address literals: RAM `0x0082d5f2` reads `0x0061c352` across aligned A32 words `0xc3520000` / `0xda000061`; RAM `0x00f4ba7e` reads `0x0061c3a0` across `0xc3a00000` / `0xda000061`. No aligned direct window literal or adjacent resolved `movw`/`movt` pair names the prefix. Raw numeric span hits are not all semantically classified: the confirmed language neighbour-field hits are excluded by their consumers, these direct hits by instruction boundaries, and known instruction/sample/string candidates are not automatically descriptors. Remaining raw pairs are unclassified, not evidence of an exhaustive absence proof.

**Static-inference acceptance, unproved residual:** the prefix is accepted for this host build on retained-extent boundaries and the mapped copy-before-decompression order. A later reload of the table block after decompression would overwrite the new arrays and falsify this ownership argument; so would a newly discovered covering consumer, a parent/index access into the tail or a runtime-created reference/writer. The current mapping does not prove that no such writer exists. Two byte-equal boot captures cannot exclude value-identical writes. **Hardware-observed, 2026-10-09 (E-FILMSIM-INSTALL-150):** the installed prefix is live in an ordinary menu, so a later reload of the table block after decompression did not occur on this body's observed boot path and nothing overwrote the new arrays there. What remains open is a value-identical writer, which no menu observation can exclude; closing that would still need a complete host-emulated load-path memory-write trace through the relevant execution with no skipped writer, and corresponding consumer coverage. The manifest is host-only.

### 5.4 Four extension/reuse shapes considered

| Shape | Evidence and disposition |
| --- | --- |
| Grow at the old end, alter bss-clear base only | **Byte-read, static-inference: rejected.** `FUN_0061db3c` copies `0x1c2` bytes from `0x0096cc64` to `0x0089ec00` through `FUN_0061db04` and `FUN_00722054`, overwriting any 48-byte tail extension. Startup `FUN_0069cdf8` also conditionally clears `[0x0089ec00,0x00973780)` via a fixed-address `FUN_007221a0` call. Moving the table's bss-clear base does not retarget either writer. |
| Lower load and prepend bytes while retaining linked addresses | **Static-inference: accepted prefix contract above.** Lowering the load without prepending is rejected: it shifts the linked body and leaves absolute references and fixed entry access wrong. |
| Place arrays in bss while reducing the clear span | **Static-inference: not accepted.** Fixed tail writers reject the initial bss bytes; heap initialization reserves `[0x00a71a00,0x00bf1a00)` through cursor `0x00829a08` and allocator `FUN_0072b110`. No independently owned middle-bss extent is established. This is not a proof that every middle-bss byte has a known writer. |
| Split or compress/shared-label reuse of the stock arrays | **Byte-read, static-inference: insufficient without a new code/format design.** Values need 64 bytes, so fit only in the reclaimed 96-byte labels slot, leaving 32 there plus a separate 48-byte values slot. The 128-byte labels array fits neither remaining contiguous slot. `FUN_00bf9360` indexes 16-byte label quadruples and loads their four ids directly; `FUN_00bf93e0` walks 8-byte values records and supplies the matching index. Scattering records or sharing quadruples through an indirection changes the consumer format and is outside the data-only RC0 contract. |

The offline route has no live-write staging or quiescence requirement. Those hazards remain specific to a deferred live-write route and are not reasons to reject the persistent prefix design.

### 5.5 Prefix build run

**Host run, 2026-10-09 (byte-read outputs).** The chain ran on the pinned DAT and decode with the fixed encoder (`lz/FujiAF/Lz/Encode.hs`), the schema-3 manifest and `rebuild-update replace-record4 --load-address 0x0061c340`. Each command below reads and writes host files only.

```sh
bazel-bin/host/firmware/apply-patch-set \
  recovery/rc0-patch-set.json \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin
bazel-bin/lz/lz-encode 0x160000 \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin \
  private-data/analysis/x-m1-1.50/update/prefix/record4.bin
bazel-bin/host/firmware/rebuild-update replace-record4 --decoder bazel-bin/lz/fflzd --load-address 0x0061c340 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  private-data/analysis/x-m1-1.50/update/prefix/record4.bin \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin \
  private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT
bazel-bin/host/firmware/rebuild-update verify private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT
bazel-bin/host/firmware/rebuild-update diff \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT
```

| Artefact | Bytes | SHA-256 |
| --- | --- | --- |
| prefixed decode `update/prefix/id4-patched.bin` | 2,631,872 (`0x2828c0`) | `a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551` |
| record `update/prefix/record4.bin` | 1,336,834 (`0x146602`) | `360278d3132f1a467c85793a8ffa4f5e197096ed3b36eab966e198321ff6a248` |
| container `update/prefix/FWUP0002-prefix.DAT` | 38,405,148 (unchanged) | `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8` |
| manifest `recovery/rc0-patch-set.json` | 10,042 | `586bbb6fc6a9865e4419627e1251296fd5a5050e4072bc03811f7592c88a94e1` |

The applier reported 68 ordered writes, 114 bytes differing against the constructed template (82 prefix bytes plus the 32 preserved-body bytes), and touched 10 output ranges starting at offset 0. Byte-reading the prefixed decode body against the stock decode shows the 32 body bytes only where the manifest puts them: three bytes in each of the six node pointer words (`0x160da4`/`0x160da8`, `0x165fc4`/`0x165fc8`, `0x169994`/`0x169998`) and one byte in each of the 14 matrix cells at `0x1799b4`-`0x179a60`.

The encoder reported `out_size 0x2828c0`, `total_len 0x146602`, `pages 0x52`, chunk `0x4000`, mode 4, 499,525 tokens, 81 interior feed boundaries with a token ending at each, no F1 geometry hits, `total_len mod 0x4000 = 0x2602`, and structural invariants passing. The record file's stored word 1 equals 0x14 plus its 0x1465ee stream bytes.

`replace-record4` printed the derived container words and the gates it ran. The stored record-length word was compared against the stream the container carries (`0x146602 = 0x14 + 0x1465ee`), `record header word 0` moved `0x00282800` to `0x002828c0` and word 2 stayed `0x52`, and the main checksum word moved `0x8f0ab18f` to `0x8f0a6b52` (recomputed over the changed complemented main range). `fflzd`, reading that stored word from the record file, consumed `0x146602` bytes, produced `0x2828c0` bytes and returned the patched decode byte for byte. `verify` reports 40/40 contract fields on the emitted container.

`diff` reports 0 bytes changed for the identity records, the version words, the selector, the range fill, the main pre-table, the table tail, image 5 and both restore lists, and non-zero counts only for the segment table (3 bytes: entry 4's load and size words), the image-4 record header (3 bytes: the decoded-size and record-length words), the image-4 stream (1,273,536 bytes), the window tail (155 bytes, the slack head the longer record consumes) and the trailer (3 bytes: words 5 and 6).

Byte-reading the emitted DAT gives record header `{0x002828c0, 0x00146602, 0x00000052, 0x00004000, 0x00000004}`, segment entry 4 `{4, 0x0061c340, 0x002828e8, 0x0089ec00, 0x00352e00, 0x00160000, 0x00160000, 0x20, 1}` and trailer words 5/6 `0x0061c340`/`0x002828e8` with the `0xffff`/`0xaaaa` halves intact. The record extent is `0x146602` of the `0x160000` window (92 per cent, `0x199fe` = 104,958 bytes of slack), a growth of `0x9b` = 155 bytes over the stock `0x146567` that matches the slack head the diff shows consumed.

One independent re-read closes the chain on shipped bytes rather than on tool state: complementing media `[0x160000, 0x160000+0x146602)` out of the rebuilt DAT and running `fflzd` on that slice with its own stored words decodes to the patched decode byte for byte.

This run is pipeline evidence for a placement the report accepted by static inference, and the camera run that followed is E-FILMSIM-INSTALL-150: the emitted container was installed, the body boots, and the arrays are live in an ordinary menu. The runtime writer absence is answered for that observed boot path; the value-identical-writer residual stays in section 5.3.


## 6. The emulation case's relocation parameters

The FILMSIM-EMUL-01 case placed the relocated arrays at `0x00958000` and `0x00958040`, and those are scaffolding the case chose, not patch addresses (emulated case input, `filmsim-offer-patched.json`).
They sit in the case's synthetic region map beside its own state blocks, where the runner admits any address, and the case is no evidence about placement; the emulation README says the same.
The accepted host-build destinations are `0x0061c340`/`0x0061c380`; neither the rejected transfer-resource pair nor the synthetic emulation pair is used.
What the case proves is address-independent: the executed list walk and draw-time check read the arrays only through the node's `+0x20`/`+0x24` words, so the same code offers seven entries with the arrays at any admitted address.
The case's array bytes need no change: they are the same payloads in the prefix manifest. The six pointer values and current creation-template bytes are pinned in section 2.

## Reproduction

All commands read host files only.

```sh
# stock values array 0x0078319c, labels 0x0078d4f8, the three node pointer pairs
xxd -s $((0x78319c-0x61c400)) -l 80  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x78d4f8-0x61c400)) -l 112 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x7823a4-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x785d74-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x77d184-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# conversion table rows 5 and 6, and the matrix rows the cells go into
xxd -s $((0x77ac30-0x61c400)) -l 176 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x795dac-0x61c400)) -l 184 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

xxd -s $((0x7dcf50-0x61c400)) -l 16 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x810fa8-0x61c400)) -l 32 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
devenv shell -- cc -std=c11 -O2 -Wall -Wextra -Werror private-data/analysis/x-m1-1.50/filmsim/patch-01/descriptor-coverage.c -o /tmp/filmsim-descriptor-coverage
/tmp/filmsim-descriptor-coverage \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin 0061c400 \
  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin 00bf1a00 \
  private-data/analysis/x-m1-1.50/lz/plain-01100000.bin 01100000 \
  private-data/analysis/x-m1-1.50/decode/loader-40000.bin 00040000 \
  private-data/analysis/x-m1-1.50/decode/loader-d0000.bin 000d0000 \
  private-data/analysis/x-m1-1.50/decode/boot-40048.bin 00040048 \
  private-data/analysis/x-m1-1.50/decode/boot-50048.bin 00050048 \
  private-data/analysis/x-m1-1.50/filmsim/lang-en-0039c000.bin 0039c000 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 00130000:00ca025c:0024bac8:1 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 003bc000:0106025c:0007cfd4:1
```

The prefix census used the following corrected artifact ranges (byte-read). The two boot address mappings are `0` and `0x10000`; payload extraction offsets are not their runtime bases.

```sh
devenv shell -- cc -std=c11 -O2 -Wall -Wextra -Werror private-data/analysis/x-m1-1.50/filmsim/patch-01/extension-census.c -o /tmp/filmsim-extension-census
env FILMSIM_CENSUS_OUTPUT=private-data/analysis/x-m1-1.50/filmsim/patch-01/prefix-census.txt \
  /tmp/filmsim-extension-census 0061c340 0061c400 \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin 0061c400 \
  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin 00bf1a00 \
  private-data/analysis/x-m1-1.50/lz/plain-01100000.bin 01100000 \
  private-data/analysis/x-m1-1.50/decode/loader-40000.bin 00040000 \
  private-data/analysis/x-m1-1.50/decode/loader-d0000.bin 000d0000 \
  private-data/analysis/x-m1-1.50/decode/boot-40048.bin 00000000 \
  private-data/analysis/x-m1-1.50/decode/boot-50048.bin 00010000 \
  private-data/analysis/x-m1-1.50/filmsim/lang-en-0039c000.bin 0039c000 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 00130000:00ca025c:0024bac8:1 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 003bc000:0106025c:0007cfd4:1 \
  private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin 0061c000:00140048:00000200:0
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 --start-address=0x1704 --stop-address=0x175c private-data/analysis/x-m1-1.50/lz/id4.elf
```

The last command demonstrates the fixed `0x0089ec00` copy destination and `0x1c2` size (byte-read). This ELF's `.text` VMA is zero; LLVM filters start/stop before `--adjust-vma`, so those arguments are file-relative offsets while the printed addresses are runtime addresses.
