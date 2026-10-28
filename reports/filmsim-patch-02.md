# FILMSIM-PATCH-02: the Q cell, the FILM 1-3 brackets and the RAW-conversion list (1.50)

**Host-only patch set, `installable:false`.** It extends the installed RC0 prefix downward by 256 bytes and covers two of the three surfaces RC0 left open: the Q menu cell and the FILM 1-3 bracket children. The RAW-conversion list is excluded on capacity with its row-to-code mapping decoded and recorded. No camera or USB access, no owner operation proposed, no live write specified.
**Revision 2, 2026-10-09; changes required by the independent byte review (`agent://ReviewRc0bBytes`). Revision 1 is superseded and not installable.** The review found two byte defects. First, the new bracket label rows reused string ids `0x3da`/`0x3db` and `0x3d8`/`0x3d9`, which resolve to the `<f1b5>` MONOCHROME+G and `<f1b3>` MONOCHROME+R filter pairs, not PRO Neg; the intended ids are `0x3d2`/`0x3d3` and `0x3d0`/`0x3d1` (`<f202>` PRO Neg. Std and `<f201>` PRO Neg. Hi). Second, the FILM 1-3 availability matrices' key 11/key 12 rows are zero in all 22 columns, so the mode-apply path substitutes the mode default for a chosen PRO Neg value; revision 2 adds the 24 cells that put the new rows on the stock allowed pattern (section 2.4). The write set is 96 ordered words in all, and the revision-2 chain has since been built and measured (section 6). Revision 1's build artefacts were replaced at the canonical paths by the revision-2 build; the revision-1 container stays superseded and **not installable**, with its hashes retained as history.
**Review status:** an independent Sol6.1-high byte review of revision 2 re-derived the claims of sections 2.2, 2.3, 2.4 and 6.1 from the pinned bytes and the id4 code. It verified the label ids, the 24 cell writes and their addresses, the descriptor entries, the query/default/apply semantics and the manifest census, and it found two prose defects (section 2.4's per-slot default description and the filter-id pairs in the revision summaries), both fixed and confirmed; the only manifest change after the review is the `totals.revision_note` string, and the word list is untouched.
Evidence labels are **byte-read**, **static-inference**, **hardware-observed** and **unproved**.
The machine-readable form is `private-data/analysis/x-m1-1.50/filmsim/patch-02/patch-set.json` (19,409 bytes, SHA-256 `848caf992e0b74bdf96525ff1273d66153d603d9fd2a91d9dceafe152fff10f7`), written in FILMSIM-PATCH-01's schema-3 manifest form and applied over the installed RC0 decode `update/prefix/id4-patched.bin` (SHA-256 `a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551`).
Stock source addresses are relative to image 4 (`lz/id4-0061c400.bin`, base `0x0061c400`, SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`); the RC0 source offset is `address - 0x0061c340` and the patch-02 output offset is `address - 0x0061c240`.
The window contract, the table-copy-before-decompression order and the prefix ownership argument are inherited from [filmsim-patch-01](filmsim-patch-01.md) section 5 and extended downward by `0x100` bytes; nothing here re-opens that argument except the floor check below.

## 1. Surfaces and dispositions

| Surface | Node | Arrays | Disposition |
| --- | --- | --- | --- |
| Q menu cell | `0x0078b000` (property `0xfe`) | values `0x00782348`, labels `0x00789e2c` | covered: values pointer retargeted to the installed seven-row array `0x0061c340`, labels array relocated and extended |
| FILM 1 bracket child | `0x00786774` (property `0x100`) | values `0x0077ddf4`, labels `0x0077d4b4` | covered: values pointer retargeted to `0x0061c340`, labels pointer retargeted to one shared relocated array, and the key 11/key 12 rows of matrix `0x00791514` gain cells 5-8 (section 2.4) |
| FILM 2 bracket child | `0x00786da8` (property `0x101`) | values `0x00786724`, labels `0x0078dcb0` | covered: same, matrix `0x00791964` |
| FILM 3 bracket child | `0x007898d4` (property `0x102`) | values `0x0078a0e8`, labels `0x00788ce4` | covered: same, matrix `0x007953f8` |
| RAW-conversion list | `0x0078bd5c` (property `0x22b`) | values `0x0078ed30`, labels `0x0078427c` | not covered: 192 bytes of relocation do not fit the window (section 3), mapping decoded (section 4) |
| unlabelled nine-row list | `0x0078d2d0` | values `0x00780008`, labels `0x007879f4` | not applicable: rows resolve to col2 `{0,3,1,4,7,9,8,0xa,5}` and name no hidden film row |

**Byte-read:** the three bracket children each hold their own five-row values array and their own labels array, and all three values arrays and all three labels arrays are byte-identical to each other (`{0,0} {3,0} {1,0} {7,0} {0xc,0}` plus the terminator; label pairs `970/971 972/973 974/975 980/981 988/989` plus the terminator). One shared relocated labels array therefore serves all three, and one relocation of the values reference serves all three because the installed array `0x0061c340` already names the same five rows plus `5` and `6`.
The Q cell's labels records carry one Q glyph id each in word 0 (`0xff00077b 0xff00077c 0xff000773 0xff000777 0xff00077a`) with words 1-3 zero.
An aligned-word census over the pinned images finds each of the eight array addresses named exactly once, in its node's own `+0x20`/`+0x24` field (inherited byte-read census, E-FILMSIM-Q-SURFACE-150), so no other stock data can supply these cells after the retargets.

## 2. The write set

**Byte-read, checked by a word-level census of the manifest:** the manifest specifies 96 ordered word writes: 48 relocated-record words, 16 new-value words, 8 pointer retargets and 24 availability-cell changes. The prefix grows by 256 bytes; 79 prefix bytes and 48 preserved-body bytes differ from the constructed zero-prefix-plus-RC0 template (24 in the eight pointer words, three bytes each, and 24 in the availability cells, one byte each), giving the applier's 127-byte differ count for revision 2, which no built output measures yet (revision 1's run reported 103 differing bytes for its 72 writes). Prefix current bytes are creation-template bytes and equal the table preload at those RAM addresses. All table columns show the numerical word value, stored little-endian.
**Static-inference:** the write set contains relocations, appended values and 24 availability cells. The 24 cells are the only in-place stock-value changes, each `0 -> 1` in the low byte of an all-zero cell; every pointer word only retargets a reference. The two appended records per array are new values with no stock source; the other 48 array words and their terminators are byte-identical copies of stock records at new addresses.

| Orders | RAM address | Output offset | Class | Content |
| --- | --- | --- | --- | --- |
| 1-20 | `0x0061c240` | `0x00000000` | relocation | Q label records 0-4 from `0x00789e2c` (one Q glyph per record word 0) |
| 21-28 | `0x0061c290` | `0x00000050` | new values | Q rows 5-6: `0x49a` (1178 `<f202>`) and `0x496` (1174 `<f201>`) |
| 29-32 | `0x0061c2b0` | `0x00000070` | relocation | Q label terminator from `0x00789e7c` |
| 33-52 | `0x0061c2c0` | `0x00000080` | relocation | bracket label records 0-4 from the stock bracket arrays (`970/971` `972/973` `974/975` `980/981` `988/989`) |
| 53-60 | `0x0061c310` | `0x000000d0` | new values | bracket rows 5-6: `0x3d2/0x3d3` (978 `<f202>`, 979 `<f202>PRO Neg. Std`) and `0x3d0/0x3d1` (976 `<f201>`, 977 `<f201>PRO Neg. Hi`) |
| 61-64 | `0x0061c330` | `0x000000f0` | relocation | bracket label terminator |
| 65-72 | node fields | see below | relocation | eight pointer retargets, labels before values per node |
| 73-96 | six matrix rows | see below | in-place value change | FILM 1-3 availability matrices: cells 5-8 (`+0x14..+0x20`) of the key 11 and key 12 rows, `0 -> 1` (section 2.4) |

### 2.1 New Q labels at `0x0061c240` (orders 1-32)

```
00000000: ff00077b 00000000 00000000 00000000   PROVIA (Q glyph)
00000010: ff00077c 00000000 00000000 00000000   Velvia (Q glyph)
00000020: ff000773 00000000 00000000 00000000   ASTIA (Q glyph)
00000030: ff000777 00000000 00000000 00000000   B&W (Q glyph)
00000040: ff00077a 00000000 00000000 00000000   SEPIA (Q glyph)
00000050: 0000049a 00000000 00000000 00000000   PRO Neg Std, string id 1178 "<f202>"
00000060: 00000496 00000000 00000000 00000000   PRO Neg Hi, string id 1174 "<f201>"
00000070: ffffffff ffffffff ffffffff ffffffff   terminator
```

Each record is 16 bytes, one id in word 0, parallel to the installed values rows `{0,3,1,7,0xc,5,6}` (byte-read stock format at `0x00789e2c`).
**Byte-read:** the Q id space ends at `n = 0x8c5` and holds no PRO Neg record, so rows 5-6 carry plain string ids. Both the grid's pixel-coordinate drawer `0x01112380` and the cell-coordinate drawer `0x011121d4` accept Q vector ids (`0xff000000|n`) and plain string ids (`<= 0x83d`). The `0x49a`/`0x496` strings are the existing `<f202>`/`<f201>` shooting-list icons (E-FILMSIM-SURFACE-COST-150). Section 7.1 binds the grid consumer and its string metrics; visual acceptance of the chosen `w=120` variants remains open.

### 2.2 New bracket labels at `0x0061c2c0` (orders 33-64)

```
00000000: 000003ca 000003cb 00000000 00000000   <f06a> / <f06a><f06f>/STANDARD (row 0)
00000010: 000003cc 000003cd 00000000 00000000   <f06b> / <f06b><f070>/VIVID (row 3)
00000020: 000003ce 000003cf 00000000 00000000   <f06c> / <f06c><f071>/SOFT (row 1)
00000030: 000003d4 000003d5 00000000 00000000   <f06d> / <f06d>MONOCHROME (row 7)
00000040: 000003dc 000003dd 00000000 00000000   <f06e> / <f06e>SEPIA (row 0xc)
00000050: 000003d2 000003d3 00000000 00000000   <f202> / <f202>PRO Neg. Std (row 5)
00000060: 000003d0 000003d1 00000000 00000000   <f201> / <f201>PRO Neg. Hi (row 6)
00000070: ffffffff ffffffff ffffffff ffffffff   terminator
```

Records 0-4 and the terminator are byte-identical to the stock arrays; rows 5-6 use the bracket-format strings the slot already carries as ids `976`-`979`, mirroring the stock pair `970`/`971`.
**Byte-read (`filmsim/tools/strid` over the EN slot table):** `970` = `<f06a>` (`w=480`, `n=1`), `971` = `<f06a><f06f>/STANDARD` (`w=120`), `976` = `<f201>`, `977` = `<f201>PRO Neg. Hi`, `978` = `<f202>`, `979` = `<f202>PRO Neg. Std`, all with the bracket pair's widths. The stock records' ids (`0x3ca` = `970` through `0x3dd` = `989`) match those table indices one for one.
Revision 1 carried `0x3da`/`0x3db` and `0x3d8`/`0x3d9` for the two new rows (the review's finding). Those indices are `986`/`987` = `<f1b5>` / `<f1b5>MONOCHROME+G FILTER` and `984`/`985` = `<f1b3>` / `<f1b3>MONOCHROME+R FILTER`, so the revision-1 rows would have drawn monochrome-filter glyphs and names; revision 2's ids above are the `<f202>`/`<f201>` pair the list needs.
The row order parallels the installed values rows, so row 5 is PRO Neg Std and row 6 is PRO Neg Hi.

### 2.3 Pointer retargets (orders 65-72)

| Order | RAM address | Output offset | Current | New | Role |
| --- | --- | --- | --- | --- | --- |
| 65 | `0x0078b024` | `0x0016ede4` | `0x00789e2c` | `0x0061c240` | Q labels pointer, relocation |
| 66 | `0x0078b020` | `0x0016ede0` | `0x00782348` | `0x0061c340` | Q values pointer, relocation |
| 67 | `0x00786798` | `0x0016a558` | `0x0077d4b4` | `0x0061c2c0` | FILM 1 labels pointer, relocation |
| 68 | `0x00786794` | `0x0016a554` | `0x0077ddf4` | `0x0061c340` | FILM 1 values pointer, relocation |
| 69 | `0x00786dcc` | `0x0016ab8c` | `0x0078dcb0` | `0x0061c2c0` | FILM 2 labels pointer, relocation |
| 70 | `0x00786dc8` | `0x0016ab88` | `0x00786724` | `0x0061c340` | FILM 2 values pointer, relocation |
| 71 | `0x007898f8` | `0x0016d6b8` | `0x00788ce4` | `0x0061c2c0` | FILM 3 labels pointer, relocation |
| 72 | `0x007898f4` | `0x0016d6b4` | `0x0078a0e8` | `0x0061c340` | FILM 3 values pointer, relocation |

Each pointer word changes three bytes in the preserved body (`0x00` in byte 3 is unchanged). The eight words plus the 24 availability cells of section 2.4 make 48 preserved-body bytes differ from the installed decode in revision 2; the revision-1 build's byte-read `cmp` of its constructed output against `update/prefix/id4-patched.bin` measured the 24 pointer bytes across 24 lines, and its only other body difference is the cells now added.
The Q cell's values pointer moves from its own five-row array to the installed seven-row array; that is what fixes the icon fallback, because the row lookup `FUN_00bf93e0` now finds the pair whose row number is `5` or `6` and returns that record's word 0, while its no-match path returns index 0 (byte-read `0xbf9548`), which is why the panel drew PROVIA's glyph for a PRO Neg value before.

### 2.4 The FILM 1-3 availability cells (orders 73-96)

**Byte-read:** the availability descriptor list at `0x0079944c` (110 entries of `{property, flag, matrix}` at stride 12, terminator `{0,0,0}` at `0x00799974`) carries entries 98-100 at `0x007998e4`/`0x007998f0`/`0x007998fc`: `{0x100, 0, 0x00791514}`, `{0x101, 0, 0x00791964}` and `{0x102, 0, 0x007953f8}`. Each matrix is rows of `{key, 22 cells}` at stride `0x5c`, the key in word 0 and the cell for column `c` at `+4*(c+1)`. The mode columns are the list at `0x00792fac` (`1, 0, 0x1f, 0x20, 0x24, 0x23, 0x22, 0x21, 5, 0x18, 8, 0xb, 0xc, 0xd, 0x12, 0x13, 0x11, 0x10, 0x16, 0x15, 0x17, 0x2c`, terminated by `0x2d`), so cells 5-8 are the columns of modes `0x24`, `0x23`, `0x22` and `0x21`. The rows keyed 11 and 12 are the col2 values of PRO Neg Std and PRO Neg Hi (conversion-table rows 5 and 6 of `0x0077ba70`/`0x00778464`/`0x0077892c` read `{9, 0x7ffffffe, 0xb}` and `{0xa, 0x7ffffffe, 0xc}`). In all three matrices and in the installed decode those two rows are zero in all 22 columns.

**Byte-read and static-inference (consumers):** `FUN_00631b74` is the availability query: it finds the mode's column index in the list at `0x00792fac`, walks the property's matrix rows by key, and returns allowed when the row's cell is 1 or 2 (`(cell - 1) <= 1`, unsigned) and 0 otherwise (id4 `0x631b74`-`0x631d54`); the cell it reads is at `row + 4*column + 4`, and cell value 2 also marks the row `FUN_00631d58` returns as the mode default. The value-or-default wrappers `FUN_0063f148`/`FUN_0063f1b4`/`FUN_0063f220` (id4 `0x63f148`/`0x63f1b4`/`0x63f220`) read the property's current value, resolve its col2 through `FUN_0061d514(prop, value, 1, 3)`, ask `FUN_0061d534(mode, prop, col2)`, and write the mode default from `FUN_0061d544` back when the query returns 0; they are called in sequence from the apply routine at `0x6434f8` (calls at `0x643718`/`0x643720`/`0x643728`), reached through `0x6438ec` from `0x6439ac`, whose mode argument comes from `FUN_0063b4c8`, a class-mapping dispatch whose table returns `0x21`-`0x24` among its outputs. With the rows left zero, a PRO Neg Std/Hi choice in a FILM 1-3 slot is substituted by the mode default at apply time; the lists' draw path has no case for properties `0x100`-`0x102` in `FUN_00d52684` (default 0, so no draw-time greying, byte-read of the case set), which is why the defect would show as a silent substitution rather than a greyed row.
**Byte-read (stock pattern) and the revision-2 write:** at cells 5-8, the four bracket mode columns, each matrix allows its offered stock values with a 1, except one row per matrix that carries 2, the row `FUN_00631d58` returns as that slot's mode default: key 0 (PROVIA) for FILM 1, key 3 (VIVID) for FILM 2 and key 1 (SOFT) for FILM 3. The keys a surface does not offer (7, 9, 8, 0xa) and the new keys 11 and 12 hold 0 in those cells. Revision 2 writes 1 (allowed, not the mode default) into cells 5-8 of the six key 11/key 12 rows, 24 words at `+0x14..+0x20`, matching the stock 1-valued rows, so the new rows are allowed in the bracket mode columns the way the stock values are and none becomes a default candidate.

| Matrix | Property | Key 11 row | Key 12 row | Cells |
| --- | --- | --- | --- | --- |
| `0x00791514` | `0x100` FILM 1 | `0x00791628` | `0x00791684` | `+0x14..+0x20` |
| `0x00791964` | `0x101` FILM 2 | `0x00791a78` | `0x00791ad4` | `+0x14..+0x20` |
| `0x007953f8` | `0x102` FILM 3 | `0x0079550c` | `0x00795568` | `+0x14..+0x20` |

**Static-inference:** the cells sit in the preserved body (output offsets `0x00175...` and `0x00179...`), not in the 256-byte window, so they change no placement or capacity. The reading that columns 4-7 are the mode classes the apply path uses rests on the stock rows' own shape and on `FUN_0063b4c8`'s table; the apply chain itself (`0x639c14` -> `0x6439ac` -> `0x6438ec` -> `0x6434f8`) is byte-read.

## 3. Placement, geometry and the running byte budget

**Byte-read:** the free window is `[0x0061c1fc, 0x0061c400)`, 516 bytes. The table-block tail holds the last plain-extent terminator `0xffffffff` at `0x0061c1f8` and reads `0` at `0x0061c1fc` (payload-complement.bin at media `0x140228`), so retained content ends at `0x0061c1fb` and the floor claim of E-RECIPE-PREFIX-TAIL-150 is reproduced from bytes.

| Placement | Range | Bytes | Used | Free after |
| --- | --- | --- | --- | --- |
| RC0 prefix values and labels (installed) | `[0x0061c340, 0x0061c400)` | 192 | 192 | 324 |
| Q labels array | `[0x0061c240, 0x0061c2c0)` | 128 | 320 | 196 |
| shared bracket labels array | `[0x0061c2c0, 0x0061c340)` | 128 | 448 | 68 |
| RAW values array + labels array | not placed | 192 | 640 | -124 |

**Static-inference:** the geometry invariant holds by construction: `load + decoded_size = 0x0061c240 + 0x002829c0 = 0x0089ec00`, the stock bss start, and `load + segment_size = 0x0061c240 + 0x002829e8 = 0x0089ec28`. The load stays 64-byte aligned and `0x44` bytes above the floor. `rebuild-update replace-record4` enforces the same relation from the stock record and passed in the revision-1 build (section 6.3).
**The RAW list is excluded on capacity.** Its relocation needs 64 bytes of values array plus 128 bytes of labels array; the window has 68 bytes left, a 124-byte shortfall. Reclamation does not close it: this patch's own retargets make the stock Q labels extent `[0x00789e2c, 0x00789e8c)` and the three stock bracket label extents (96 bytes each) unreachable in the constructed output, alongside the values extents (48 bytes each) and the two extents RC0 already freed, and the largest reclaimed contiguous extent is 96 bytes. That holds the 64-byte values array but not the 128-byte labels array, so the surface stays 60 bytes short even with reclamation, and a labels array must be contiguous (byte-read walk format, 16-byte records to a 16-byte terminator).
The proposed 320-byte shared-array variant `{Q glyph, RAW icon, 0, RAW name}` is rejected by the Q consumer: `FUN_0116ae00` reads word 1 as well as word 0 and draws each nonzero id (section 7.1). The RAW icon in word 1 would therefore add a second draw to the Q cell. Stock zeros are not evidence of unused fields.
The allocation order follows the surfaces' enumeration and cost: the Q cell is the owner-visible defect and is covered first, the brackets cost the same 128 bytes with their strings already in the slot, and the RAW list is the most expensive surface at 192 bytes and the last of the three. Pairwise, Q plus the RAW list also fits (320 bytes, 4 left) and would leave the brackets stock.

## 4. The RAW-conversion list's row-to-code mapping

**Byte-read:** the list's values array `0x0078ed30` names rows `{0, 1, 2, 5, 9}` with labels `{0, 1529, 0, 1530}` through `{0, 1199, 0, 1548}`, and the node's property `0x22b` selects the conversion table at `0x00775500` through the pointer word at `0x0081b948` (`0x0081b09c + 4*property`, the same word family that binds property `0xfe` to `0x0077ac30`). Table `0x00775500` has 10 rows of `{absent, absent, code}` plus the `0xffff` terminator, and the converter `FUN_00631a00` (`FUN_0061d514(prop, value, from, to)`, columns `0 = row, 1 = col0 stored, 2 = col1, 3 = col2 effective`) resolves row to code in its third column.

| Values row | Name (strid pair) | Record code | Name the code decodes to |
| --- | --- | --- | --- |
| 0 | STANDARD (1529/1530) | 0 | PROVIA |
| 1 | VIVID (1531/1532) | 5 | Velvia |
| 2 | SOFT (1533/1534) | 3 | ASTIA |
| 3 | PRO Neg. Hi (1535/1536) | 7 | PRO Neg. Hi |
| 4 | PRO Neg. Std (1537/1538) | 6 | PRO Neg. Std |
| 5 | MONOCHROME (1539/1540) | 6 | MONOCHROME |
| 6 | MONOCHROME+R (1541/1542) | 7 | MONOCHROME+R |
| 7 | MONOCHROME+Ye (1543/1544) | 8 | MONOCHROME+Ye |
| 8 | MONOCHROME+G (1545/1546) | 9 | MONOCHROME+G |
| 9 | SEPIA (1199/1548) | 0xa | SEPIA |

**Byte-read:** the name column comes from the string-pair sequence in the EN slot, which the stock labels bind row by row (rows 0, 1, 2, 5 and 9 carry pair 0, 1, 2, 5 and 9; the icon field of row 9 carries the `w=120` variant `1199` of `<f06e>` rather than pair 9's `1547`). Rows 3 and 4 then carry pairs 3 and 4, `<f201>`/`PRO Neg. Hi` and `<f202>`/`PRO Neg. Std`.
**Byte-read and static-inference:** the code column decodes independently in the plain image: `FUN_01118a00` reads the record block's `+0x164` word and switches on `value + 1` to draw `PROVIA` (0), `ASTIA` (3), `Velvia` (5), `PRO Neg. Std` (6) or `PRO Neg. Hi` (7) from the ASCII name table at `0x11205d8`, and on the `-1` marker switches on the block's `+0x134` word minus 6 to draw `MONOCHROME` (6), `MONOCHROME+R` (7), `MONOCHROME+Ye` (8), `MONOCHROME+G` (9) or `SEPIA` (0xa). Every one of the ten rows agrees between the two orders, which binds rows 3 and 4 to PRO Neg. Hi and PRO Neg. Std with codes 7 and 6. The overlap at codes 6 and 7 is a two-field domain, not a collision: colour films reach `+0x164` and the monochrome family and SEPIA reach `+0x134` behind the `-1` marker.
**Byte-read and static-inference:** the selection writer and processing-family selector are now traced (section 7.3). Rows 0-4 write the colour channel; rows 5-9 write the monochrome/sepia channel, which overrides the colour channel in the request builder. In particular, RAW row 3 becomes lower develop value `0xc` and row 4 becomes `0xb`, rather than monochrome values with overlapping numeric codes. The metadata display block's `+0x164`/`+0x134` fields above are a different record layout from the lower develop record's `+0x508`; their offsets must not be interchanged.
To add the two PRO Neg rows the list would name values rows `3` and `4` (codes `7` and `6`) with label pairs `1535/1536` and `1537/1538` in the RAW format. What blocks that is only the byte budget of section 3.

## 5. What each surface shows afterwards

| Surface | After this patch set |
| --- | --- |
| Q menu cell | Seven rows: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, offered from the installed values array. The five stock rows draw their Q glyphs and the two new rows draw `<f202>` and `<f201>`. The current-value icon follows the selection: a PRO Neg Std value now draws `<f202>` instead of PROVIA's glyph. |
| FILM 1-3 bracket children | Each of the three lists offers seven rows with the bracket strings: `<f06a>`, `<f06b>`, `<f06c>`, `<f06d>`, `<f06e>` and the new `<f202>` and `<f201>` icons with the names `<f202>PRO Neg. Std` and `<f201>PRO Neg. Hi`. The draw path has no case for properties `0x100`-`0x102` (`FUN_00d52684` default 0), so no row is greyed at draw time; the apply path does consult the descriptor-bound matrices, and the 24 added cells (section 2.4) keep a chosen PRO Neg Std/Hi value from being substituted by the mode default. |
| RAW-conversion list | Unchanged: five rows STANDARD, VIVID, SOFT, MONOCHROME, SEPIA. The two PRO Neg rows need 192 bytes the window does not carry (section 3) and are ready to add on the decoded mapping of section 4. |

**Static-inference:** the list walk, count and scroll read the arrays only through node `+0x20`/`+0x24`, so the FILMSIM-EMUL-01 offer-walk proof covers the new wiring without new case bytes; what it does not cover is label rendering, which stays a visual question (section 7). The bracket children stay ungated at draw time (no `0x100`-`0x102` case in `FUN_00d52684`), while the apply path's three wrappers read the descriptor-bound matrices; the patch's only availability change is the 24 cells of section 2.4, which put the new rows on the stock non-default pattern. The revision-1 report's claim that properties `0x100`-`0x102` have no availability descriptor is withdrawn.

## 6. Container build

### 6.1 Revision-2 status

Revision 2 was built 2026-10-09 by the runtime worker with no failure and no rerun, and the applier census matched the manifest's word list: 96 ordered writes, 127 differing bytes, eleven touched spans, 48 preserved-body bytes. The revision-2 artefacts are at the canonical paths and their pins are in the table below; the revision-1 build artefacts were replaced there and their hashes stand as history. Revision 1's container stays superseded and **not installable**.

**Runtime-measured outputs (revision 2):** decode 2,632,128 bytes (`0x2829c0`); encoder `out_size 0x2829c0`, `total_len 0x146657`, `pages 0x52`, `chunk 0x4000`, mode 4, 499,559 tokens, 81 interior feed boundaries with a token ending at each, no F1 geometry hits, `total_len mod 0x4000 = 0x2657`, structural gates passing; `fflzd` round trip of the emitted record consumed `0x146657` of `0x146657` and produced `0x2829c0` bytes byte-equal to the decode; `replace-record4` set record header word 0 to `0x002829c0`, word 1 to `0x00146657` (`0x14 + 0x00146643`) and kept word 2 at `0x52`, moved load to `0x0061c240` with the 448-byte stage-header prefix against stock, kept segment size `0x002829e8` and bss `0x0089ec00`/`0x00352e00`, and set the main checksum word to `0x8f0a5391`; `verify` reports 41/41 contract fields. Emitted-DAT byte-reads: segment entry 4 `{4, 0x0061c240, 0x002829e8, 0x0089ec00, 0x00352e00, 0x00160000, 0x00160000, 0x20, 1}` with the terminator at `0x1403bc`, trailer marker `0xaaaa`, size `0x20`, span `0x24a0000`/`0x24a0000` and words 5/6 `0x61c240`/`0x2829e8`; in the emitted decode the bracket rows 5-6 read `0x3d2`/`0x3d3` and `0x3d0`/`0x3d1` (records 0-4 stock, four-word terminator), all 24 matrix words read 1 with the other 18 cells of the six rows zero, the Q rows 5-6 read `0x49a`/`0x496`, and the pointers read `0x61c340`/`0x61c240` (Q) and `0x61c340`/`0x61c2c0` (each bracket node).

**Diff against the installed image (revision 2):** segment table 2 bytes, image-4 record header 2 bytes, image-4 stream 1,288,598 bytes, window tail 85 bytes and trailer 2 bytes, plus the 2-byte main checksum word at file `0x20c` that the summary's part rows exclude by design; everything else 0. The vs-prefix stream and window-tail counts were re-measured and match revision 1's because `total_len` stayed `0x146657`; the record is the same size and its content hash moved. Against stock the same rows read 4, 4, 1,287,375, 240 and 4 bytes, so the vs-stock stream count moved from revision 1's 1,287,363 to 1,287,375 (+12). The placement argument of section 3 does not change, because the new cells sit outside the window.

### 6.2 Rebuild commands (runtime worker)

All commands read and write host files only.

```sh
bazel-bin/host/firmware/apply-patch-set \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/patch-set.json \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/id4-patched.bin
bazel-bin/lz/lz-encode 0x160000 \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/id4-patched.bin \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/record4.bin
bazel-bin/host/firmware/rebuild-update replace-record4 --decoder bazel-bin/lz/fflzd --load-address 0x0061c240 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/record4.bin \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/id4-patched.bin \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/FWUP0002-patch-02.DAT
bazel-bin/host/firmware/rebuild-update verify private-data/analysis/x-m1-1.50/filmsim/patch-02/FWUP0002-patch-02.DAT
bazel-bin/host/firmware/rebuild-update diff \
  private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/FWUP0002-patch-02.DAT
```

| Artefact | Revision | Bytes | SHA-256 |
| --- | --- | --- | --- |
| manifest `filmsim/patch-02/patch-set.json` | 2 (current) | 19,409 | `848caf992e0b74bdf96525ff1273d66153d603d9fd2a91d9dceafe152fff10f7` |
| extended decode `filmsim/patch-02/id4-patched.bin` | 2 (current) | 2,632,128 (`0x2829c0`) | `a3675b9d21deb933a0025fda191f47a03059524dba5f2f7b7e96477b55fdfe6b` |
| record `filmsim/patch-02/record4.bin` | 2 (current) | 1,336,919 (`0x146657`) | `18e8e5ab915d6a88f54648e305d90ea1951c3202351333e1237f34da8b98482f` |
| container `filmsim/patch-02/FWUP0002-patch-02.DAT` | 2 (current) | 38,405,148 | `6b087012f0d7fd418d26b04a4fb8f88080fbf3e4f8f9ec7154ffaa43d3f7bb60` |
| extended decode `filmsim/patch-02/id4-patched.bin` | 1 (superseded, not installable, bytes replaced) | 2,632,128 (`0x2829c0`) | `871f11317af8dc91959894b68a78a21dc07bc2760560f41b9caa05094c04d564` |
| record `filmsim/patch-02/record4.bin` | 1 (superseded, not installable, bytes replaced) | 1,336,919 (`0x146657`) | `dd88e553c060bb6b028259cc411c2425a8bc7b6513b7bcc2c0a88999b14ba7da` |
| container `filmsim/patch-02/FWUP0002-patch-02.DAT` | 1 (superseded, not installable, bytes replaced) | 38,405,148 | `989f4d9b1f070e9ce7c947bd380626a938635f29b7ff9c41cdbe21bf65ddd4da` |
| manifest `filmsim/patch-02/patch-set.json` | 1 (superseded) | 12,868 | `81d8e0d5434e445bb300a441fb92fb1e30d970bee2f9fcb3262f8ed67be6988f` |

Revision 1's manifest entry printed 12,862 bytes; the file's actual size was 12,868 bytes under the same SHA-256 (the review's size finding), and the corrected value is above. The three revision-1 build artefacts were replaced at the canonical paths by the revision-2 build on 2026-10-09, so their recorded hashes are the retained history and their bytes no longer exist on disk; the revision-1 manifest bytes likewise. Every revision-2 pin above is measured on the artefact it names. The staged evidence copies at `filmsim/patch-02/rev2/` are byte-identical to the three canonical files, and `rev2/record4.decoded` carries the decode hash, the `fflzd` round-trip equality.

### 6.3 Revision-1 build outputs (superseded, retained as history)

**Byte-read outputs (revision 1):** the applier verified the input hash, checked every write's current bytes against the constructed output and reported 72 ordered writes with 103 differing bytes over five touched spans (`0x0..0x100` and the four node fields). The encoder reported `out_size 0x2829c0`, `total_len 0x146657`, `pages 0x52`, `chunk 0x4000`, mode 4, 499,559 tokens, 81 interior feed boundaries with a token ending at each, no F1 geometry hits, `total_len mod 0x4000 = 0x2657`, and structural invariants passing. `replace-record4` moved `load 0x0061c400 -> 0x0061c240` with a 448-byte prefix against stock, kept the stage header at decode offset 448, recomputed `record header word 0` to `0x002829c0`, word 1 to `0x00146657` (`0x14 + 0x00146643` stream bytes) and word 2 at `0x52`, moved the main checksum word `0x8f0ab18f -> 0x8f0a5259`, and ran its gates (record header arithmetic, structural invariants, fflzd record and rebuilt-stream round trips, emission gates, stream-carried identity) all passing; the record fills `0x146657` of the `0x160000` window (92 per cent). `verify` reports 41/41 contract fields on the emitted container.
Byte-reading the emitted DAT gives segment entry 4 `{4, 0x0061c240, 0x002829e8, 0x0089ec00, 0x00352e00, 0x00160000, 0x00160000, 0x20, 1}` and trailer words 5/6 `0x0061c240`/`0x002829e8` with the `0xffff`/`0xaaaa` halves intact (verify fields above).

**Diff against the installed image (revision 1)** (`update/prefix/FWUP0002-prefix.DAT`, SHA-256 `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8`, 38,405,148 bytes): identity records, version words, selector, range fill, main pre-table, table tail, image 5 and both restore lists change 0 bytes; the segment table changes 2 bytes (entry 4's load and size words), the image-4 record header 2 bytes (decoded-size and record-length words), the image-4 stream 1,288,598 bytes, the window tail 85 bytes (the slack head the longer record consumes) and the trailer 2 bytes (words 5 and 6). Against stock the same rows read 4, 4, 1,287,363, 240 and 4 bytes. The revision-2 body differs (the corrected label ids and the 24 cells) but keeps the same record size; the measured revision-2 diff is in section 6.1, where the vs-stock stream count moved from these 1,287,363 bytes to 1,287,375.

## 7. Residuals

**Source-inspected, with static-inference:** the three independent source questions are answered in sections 7.1-7.3 (E-FILMSIM-SELECTOR-CONSUMERS-150). This establishes rendering dispatch, bracket label-field consumers and RAW processing-family selection, not a visually accepted or installed patch.
**Unproved visual acceptance:** the selected Q strings must be seen in the complete cell, selected and unselected, without overlap or clipping. The source passes each string's own declared dimensions to the renderer; accepting a string id does not prove that `w=120`, `w=440` or the bracket variant looks right.
**Static-inference:** the apply wrappers and the query's cell arithmetic are byte-read, but the mode argument's provenance chain (`0x639c14` -> `0x6439ac` -> `0x6438ec` -> `0x6434f8`, mode from `FUN_0063b4c8`) is not executed on a patched image, and whether any other path consults these matrices with a different mode class is unproved; the stock rows' own shape is the support for columns 4-7 being the classes that matter. The revision-1 claim that properties `0x100`-`0x102` have no availability descriptor is withdrawn: they are entries 98-100 of the list at `0x0079944c`.
**Rebuild status:** the revision-2 chain is built and its applier, encoder, round-trip, header and container numbers are measured (section 6.1); what stays absent is any camera-side look at a patched image.
**Inherited:** the runtime-writer absence for the prefix (filmsim-patch-01 section 5.3) and the Q-cell direct-path variant of FUN_00bf93e0 (E-FILMSIM-Q-SURFACE-150 limits).
**Static-inference:** an image carrying this patch set and the D3 recipe prefix does not fit the tail; the window forces one combined decision (E-FILMSIM-SURFACE-COST-150).

### 7.1 Q grid: actual consumer and string dispatch

**Byte-read:** the Q root table word at `0x0081bca4` names `0x0078e4a0`; its child list at `0x0077da94` contains the film child pointer at `0x0077dad0`, `0x0078b000`. `FUN_0116ae00` selects root `0x7a`, walks its children with `FUN_00bf8d20`, and, when `FUN_011724c0` returns 0, calls `FUN_00bf93e0(root, property, current_row, field)` for fields 0 and 1. Property `0xfe` has no special case in `FUN_011724c0` and returns 0. The accessor's child-node path reads node `+0x20` to match the value and selects the corresponding 16-byte label at node `+0x24`; no match retains index 0.

**Byte-read:** at `0x0116b334..0x0116b3d0`, field 0 is drawn at pixel `x=(c+1)*20`, `y=r*24`, where `c=8*(visible_index & 3)+1`; nonzero field 1 is drawn at `x=20*c+60` with the same y. Each goes through `FUN_01112e78` and its unbounded-dimension branch to `FUN_01112380`. The stock and proposed Q records have word 1 zero, so they take only the first draw. This is the root/child path, not execution evidence for the accessor's separate direct-node path.

**Byte-read:** `FUN_01112380` handles vector ids through `FUN_0117e048`/`FUN_0117e0a4` with font selector `0x18`, and nonzero plain ids through `0x83d` through `FUN_0117dce0`/`FUN_0117dd48`/`FUN_0117ddb0`. The latter read the id's width, height and font fields at `0x00130000 + 10*id + {4,6,8}` for these ids. Both branches call `FUN_01179da0`. A coordinate of `-20` requests the string's declared position; this grid supplies explicit pixel coordinates. The source does not replace a plain string's width with the vector glyph width or scale it to the Q cell.

### 7.2 FILM 1-3: label fields are consumer-bound

**Byte-read:** root lookup words `0x0081bb40..0x0081bb48` name the three bracket nodes for menu ids `0x21..0x23`. Their property fields are `0x100..0x102`. `FUN_00bf93e0` copies a matched label record's four words and returns the requested field index; the field argument is not a film code or a conversion-table column.

**Byte-read:** the current-value branch of `FUN_01164e1c` asks for field 0 at `0x011654d8..0x011654e8`, constructs a draw record and falls through `FUN_01112e78` when the surface-specific `FUN_0116e264` returns 0. The option-list branch asks for field 1 at `0x011657d0..0x011657dc` and falls through the same renderer at `0x011658b4..0x011658c4` when `FUN_0116f948` returns 0. Neither special dispatcher has a bracket-menu case for `0x21..0x23`. `FUN_011662a0` also uses field 0 for the current value and field 1 for list entries, with the same special-dispatch/fallback split. These consumers bind `{current-value icon, option-list icon/name, 0, 0}` for the bracket records, not merely the appearance of their stock contents.

**Scope:** this proves the roles of the two populated bracket fields, not that words 2 and 3 are universally ignored by all generic layouts. It does not execute the mode-apply wrappers or the 24 new matrix cells. The execution acceptance remains a chosen PRO Neg value surviving the real apply path in each slot and each intended mode column, with the stock per-slot defaults unchanged.

### 7.3 RAW: row writer, family precedence and lower develop value

**Byte-read:** `FUN_00cc453c` passes the selected property and row to `FUN_00d46010`. For property `0x22b`, that writer branches on the original row rather than treating the converted scalar as a unique film identity. Rows `{0,1,2,3,4}` write `{0,5,3,7,6}` at `0x00954d80`, the RAW parameter block's `+8`. They clear `0x00954d94` (`+0x1c`) only when its prior value is 6 through 10. Rows `{5,6,7,8,9}` write `{6,7,8,9,10}` at `+0x1c` and set `+8` to 0. Other row values clear both channels. The related property `0x231` cannot replace a current `+0x1c` value in 6 through 10. `FUN_00d588b8`'s `0x22b` branch reverses these channels to the selected row, checking `+0x1c` first.

**Byte-read and static-inference:** a RAW conversion request such as `FUN_00cc549c` supplies parameter block `0x00954d78` to `FUN_00db3c64`. That packer puts the pointer at request offset `+0xb8`, sets message `0x116` and payload length `0xc4`, and posts through `FUN_00db2ed4`. `FUN_0120b40c` copies a `0x01xx` message's payload to `0x014ea7b4`; the pointer is then at `0x014ea86c`. `FUN_012546a8` reads that pointer and builds the processing parameters at `0x01514218`. This is a static dataflow argument, not an executed queue-history claim.

**Byte-read:** `FUN_012546a8` first maps parameter word 2 (`+8`) `{0,3,5,6,7}` to lower film values `{0,1,3,0xb,0xc}` at `0x01514220`. It then reads word 7 (`+0x1c`): values `{6,7,8,9,10}` override that film value with `{4,8,9,10,5}`. The two switches establish the family precedence even when both channels contain values. For the ten defined rows, the final lower values are therefore `{0,3,1,0xc,0xb,4,8,9,10,5}`. The overlap of RAW codes 6/7 cannot select the wrong family after the row writer has set the channels.

**Byte-read:** `FUN_01254f8c` passes `0x01514218` to `FUN_00ea1838`, which calls `FUN_00ebf880`; the latter copies its parameter word 2 to the lower develop record at `+0x508`. PRO Neg Std/Hi therefore reach the existing lower film domain as `0xb`/`0xc`. This is not an equation between metadata offsets and processing offsets, nor proof of finished pixels. RAW output and the patched Q layout remain execution/visual acceptance work; neither changes revision 2's RAW exclusion or the combined placement decision with [#17](https://github.com/skeet70/latent-image/issues/17).

## Reproduction

All commands read host files only.

The finite source cut for E-FILMSIM-SELECTOR-CONSUMERS-150 used the existing `af/words` executable without rebuilding it and LLVM 21.1.8 for disassembly. Input identities, source-function references, real-word stdout and A32 cuts are retained privately under `private-data/analysis/film-selector-proof/`; `source-proof.txt` records the command parameters and limitations. No build, compiler, emulator or camera was run for this cut.

```sh
# Q node, its arrays, the three bracket nodes and their arrays
xxd -s $((0x78b000-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x782348-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x789e2c-0x61c400)) -l 96  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x786774-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x786da8-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x7898d4-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x77d4b4-0x61c400)) -l 96  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x78dcb0-0x61c400)) -l 96  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x788ce4-0x61c400)) -l 96  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# RAW list node, arrays, its conversion table and the table-pointer word
xxd -s $((0x78bd5c-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x78ed30-0x61c400)) -l 48  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x78427c-0x61c400)) -l 96  -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x775500-0x61c400)) -l 176 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x81b494-0x61c400)) -l 8   -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# window floor: table-block tail, media 0x140228 = RAM 0x0061c1e0
xxd -s $((0x140228)) -l 32 -e -g4 private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin

# descriptor entries for properties 0x100-0x102: the pointer words at 0x7998ec/0x7998f8/0x799904
xxd -s $((0x7998e4-0x61c400)) -l 36 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
private-data/analysis/x-m1-1.50/filmsim/tools/wsearch private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin 0x61c400 0x791514

# the three availability matrices as keyed rows (key, then 22 cells; terminator row 0x7ffffffe)
awk -f private-data/analysis/x-m1-1.50/filmsim/tools/avail.awk -v p=791514 private-data/analysis/x-m1-1.50/filmsim/id4.mwt
awk -f private-data/analysis/x-m1-1.50/filmsim/tools/avail.awk -v p=791964 private-data/analysis/x-m1-1.50/filmsim/id4.mwt
awk -f private-data/analysis/x-m1-1.50/filmsim/tools/avail.awk -v p=7953f8 private-data/analysis/x-m1-1.50/filmsim/id4.mwt

# the six key 11/key 12 rows in the installed decode (patch-02's input): all cells zero
xxd -s $((0x791628-0x61c340)) -l $((4*0x5c)) -e -g4 private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin

# conversion-table rows 5 and 6 of property 0x100's table (col2 0xb and 0xc)
xxd -s $((0x77ba70-0x61c400)) -l 84 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# the string ids the two new bracket rows carry, resolved from the EN slot table
private-data/analysis/x-m1-1.50/filmsim/tools/strid private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin 1 | awk '$1>=970 && $1<=979'

# code-to-name decode of the record codes (FUN_00631a00 table walk, FUN_01118a00 name switches)
# --start-address/--stop-address take image-relative offsets; --adjust-vma prints absolute addresses
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 --start-address=$((0x631a00-0x61c400)) --stop-address=$((0x631b50-0x61c400)) private-data/analysis/x-m1-1.50/lz/id4.elf
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x01100000 --start-address=$((0x118f60-0x1100000)) --stop-address=$((0x1190c0-0x1100000)) private-data/analysis/x-m1-1.50/filmsim/plain.elf
xxd -s $((0x11205d8-0x1100000)) -l 128 private-data/analysis/x-m1-1.50/lz/plain-01100000.bin

# availability query, mode-default and the three apply wrappers (same offset convention)
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 --start-address=$((0x631b74-0x61c400)) --stop-address=$((0x631d58-0x61c400)) private-data/analysis/x-m1-1.50/lz/id4.elf
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 --start-address=$((0x63f148-0x61c400)) --stop-address=$((0x63f290-0x61c400)) private-data/analysis/x-m1-1.50/lz/id4.elf

# the constructed output against the installed decode: revision 2's census is 48 differing body bytes
# (the eight pointer words plus the 24 availability cells); revision 1 measured 24 of them
cmp -l -i 256:0 -n 2631872 \
  private-data/analysis/x-m1-1.50/filmsim/patch-02/id4-patched.bin \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin
```
