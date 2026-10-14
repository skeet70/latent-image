# D3 recipe patch set: two compiled recipe rows on the film-simulation unlock (1.50)

Host-only work on the pinned official 1.50 artifacts. No camera, no USB device and no storage is touched by the design; the host build of revision 2 runs host tools over pinned files only. This report turns design D3 of [the reach report](named-recipe-reach.md) section 4.2 into a byte-level patch set that ships in the same image as [FILMSIM-PATCH-01](filmsim-patch-01.md), states the owner-facing behaviour of the feature and keeps a byte count against the measured window. The source that emits the patch set exists (`host/firmware/BuildCompiledRecipes.hs`, `menu/recipe_compiled.S` with `menu/recipe_compiled.ld`) and has been built and run over the pinned artifacts (section 10).
Evidence labels: **byte-read** for a direct read of a pinned artifact in this pass, **static-inference** for a conclusion drawn from those bytes, **inherited** for a named record reused without re-derivation, **unproved** for a named missing fact.
The patch set is **not installable**: the placement is accepted by the widened-window test (E-FILMSIM-PREFIX-WIDENING-150), the write set and the hook have no independent review, the hook has no emulation case, and the install decision stays the owner's. A separately host-built D3/Q artifact remains noninstallable pending review.
**Revision 2, 2026-10-10.** Revision 1 (E-RECIPE-D3-PATCH-150) is superseded where the two disagree. The changes: (a) the recipe-B property-walk defect is fixed, the property table now carries its own address derivation instead of following the row-table pointer (sections 5.2 and 5.4); (b) the Q cell offers the same nine rows through its own nine-row label array in the Q format (word 0 only per record), with the stock first-five Q glyphs preserved, so the revision-1 five-row Q limitation is gone (sections 4.3, 4.5, 6.2); (c) the prefix layout is re-derived at base `0x0061c1fc` with exact offsets, because the content is 508 bytes and the `0x0061c240` base shape holds 448 (sections 2, 3, 7); (d) the corrected hook is 76 bytes in `movw`/`movt` form and the manifest carries 155 word writes, folding in the 14 RC0 availability cells so both PRO Neg rows keep the behaviour RC0 shipped (sections 4.1, 5.5); (e) the compiled source landed and the emit ran over the pinned artifacts, with the hook's assembled state frozen by hash (section 10). The 68-byte residual slack `[0x0061c1fc, 0x0061c240)` of FILMSIM-PATCH-02's manifest is not free in this build (section 7).
Markdown paragraphs are one line each.

## 1. What D3 changes

The combined image is RC0 plus D3 in one prefix: the film-simulation unlock's relocated values and labels arrays, its 14 matrix cells and its six node pointer words, plus the recipe material below and the Q cell's two pointer words and its own nine-row label array.
The combined prefix is `0x204` = 516 bytes at RAM `0x0061c1fc`, of which 508 bytes are content and 8 are zero fill, and it is the corrected-floor shape of [the widening report](filmsim-prefix-widening.md) section 6 (base `0x0061c1fc`, decoded size `0x282a04`, segment size `0x282a2c`): the content exceeds the 448 bytes a `0x0061c240` base leaves, so the load takes the corrected floor, which that report's ownership test admits.
Five changes make up D3 itself (byte-read for every address and current byte, static-inference for their effect):

1. The two relocated arrays grow from 192 bytes to 240: two `{row, child}` values pairs and two 16-byte label quadruples are appended, so the list shows nine entries.
2. The film-simulation conversion table at `0x0077ac30` gets two rows rewritten in place, rows 2 and 4, the only two rows no values array in the image names. Row 2 becomes `{0xd, 0x7ffffffe, 0}` and row 4 becomes `{0xe, 0x7ffffffe, 3}`, which gives each recipe a stored code the record can carry (`0xd`, `0xe`) while the downstream value stays the recipe's base mode's (`0` PROVIA, `3` Velvia).
3. The commit hook at the film-simulation case applies the eight parameter values of the selected recipe, so a recipe is a complete preset rather than a mode.
4. The name lives in the one writable text buffer the stock build has, the inline label record of string id `0x83d` in image 2. The second row carries its base mode's stock label quadruple; `0x83e` stays the stock empty record and no accessor gate moves.
5. The Q cell offers the same nine rows through its own nine-row label array in the Q record format (one id in word 0, words 1-3 zero), with the five stock Q glyphs preserved byte for byte, the two PRO Neg rows keeping the patch-02 string ids `0x49a`/`0x496`, and each recipe row carrying its base mode's Q glyph (no new glyph or text id is invented).

Six values arrays can name a film conversion row, and none of them names row 2 or row 4 (byte-read in this pass): the shared array at `0x0078319c` and the three bracket children at `0x0077ddf4`, `0x00786724` and `0x0078a0e8` name rows `{0, 3, 1, 7, 0xc}`, the nine-row list at `0x00780008` adds rows `0xb`, `9`, `8` and `0xa`, and the Q cell at `0x00782348` repeats the first five. RC0 appends rows 5 and 6, so rows 2 and 4 are the only unclaimed rows left, and both of their current col2 values (`2` and `6`) sit on all-zero availability-matrix rows, which is why no stock surface can reach them. The compiler re-checks both facts against the pinned stock decode before it emits and refuses a build that violates either.

The record's film field carries `0xd` or `0xe`. Every read path recovers the recipe row from it with a scan over the conversion table's col0 (`FUN_00631a00` matching arm 1, byte-read at `0x631a84`: `cmp r4, r1` on the row's first word, return the row index for `arg3 = 0`), so the menu highlights the recipe row with no display hook, and every downstream consumer sees the base mode's col2 (`0` or `3`), which the availability matrix and both value-keyed dispatchers already accept (inherited, FILMSIM-OFFER-01 sections 3 and 5).

## 2. Combined decode-prefix contract

| Field | Stock | RC0 build (192) | Combined build (516) |
| --- | --- | --- | --- |
| image-4 load, trailer word 5, segment entry 4 word 1 | `0x0061c400` | `0x0061c340` | `0x0061c1fc` |
| record 4 header word 0, decoded size | `0x00282800` | `0x002828c0` | `0x00282a04` |
| segment entry 4 word 2, trailer word 6 | `0x00282828` | `0x002828e8` | `0x00282a2c` |
| record 4 header word 1, total length | `0x00146567` | `0x00146602` (measured) | encoder-measured |
| record 4 header word 2, chunk count | `0x52` | `0x52` | encoder-measured |
| record 4 header words 3/4, feed page and mode | `0x4000` / `4` | unchanged | unchanged |
| main checksum, raw `0x20c` | `0x8f0ab18f` | `0x8f0a6b52` (measured) | recomputed |
| segment 4 bss words, image-5 load, plain extents, the 32-byte stage header at `0x61c400`, identity records, version words, selector, restore lists | stock | unchanged | unchanged |

Both invariants hold for the combined shape: `load + decoded_size = 0x0089ec00` (stock bss start) and `load + segment_size = 0x0089ec28`, which `0x61c1fc + 0x282a04 = 0x89ec00` and `0x61c1fc + 0x282a2c = 0x89ec28` satisfy (static-inference over the geometry rule of FILMSIM-PATCH-01 section 5.1).
The prefix region is zero at rest in both representations of the pinned container (byte-read, [the widening report](filmsim-prefix-widening.md) section 1: 516 bytes all `0x00` in the payload export, all raw `0xff` in the shipped container, zero in three captured boots), so the constructed output for a combined manifest is 516 zero bytes followed by the stock decode, and every prefix write's current bytes are `00000000`.
The RC0 array addresses `0x61c340` and `0x61c380` are inside this build's Q-labels and row-table blocks; nothing points at them as arrays after the pointer words move.

## 3. Prefix layout

| Address | Size | Content |
| --- | ---: | --- |
| `0x0061c1fc` | 80 (`0x50`) | values array, nine `{row, child}` pairs and the `{0x7ffffffe, 0}` terminator |
| `0x0061c24c` | 160 (`0xa0`) | labels array, nine 16-byte quadruples and the 16-byte `-1` terminator |
| `0x0061c2ec` | 160 (`0xa0`) | Q labels array, nine 16-byte Q records and the 16-byte `-1` terminator |
| `0x0061c38c` | 16 | recipe A and recipe B commit-row tables, eight bytes each |
| `0x0061c39c` | 16 | parameter property table, eight halfwords |
| `0x0061c3ac` | 76 (`0x4c`) | hook body (entry) |
| `0x0061c3f8` | 8 | zero fill to the image-4 body at `0x61c400` (free slack) |

Used bytes 508 of 516. The source, data and entry offsets the compiler emits against: the hook entry is `0x0061c3ac`, the row tables `0x0061c38c` and the property table `0x0061c39c`, and the compiler's summary prints them plus the link form (`--defsym=RC_BASE=0x0061c3ac`, with `rc_row_tables`, `rc_props` and `menu_commit` carried by `menu/recipe_compiled_layout.S`).
The eight node pointer words carry `0x0061c1fc` (values), `0x0061c24c` (labels) and `0x0061c2ec` (Q labels); they are the only `image-4` body words D3 moves besides the two film rows and the 14 matrix cells, and they move because the arrays grew past the old addresses.
The hook derives the row tables and the property table with its own `movw`/`movt` pair each (section 5.2), so the property walk's start is independent of the row-table pointer.

## 4. The write set

The manifest carries 155 ordered word writes: the 127 prefix words as six blob writes (word ranges 1-20 values, 21-60 shooting labels, 61-100 Q labels, 101-104 row tables, 105-108 property table, 109-127 hook), the six film-row words (128-133), the fourteen availability cells (134-147) and the eight pointer words (148-155). 620 bytes are written; 44 preserved-body bytes differ (4 in the two film rows, 14 in the availability cells, 24 in the eight pointer words) and the remaining changed bytes are the prefix content's non-zero bytes, which the compiler counts into the manifest totals.

### 4.1 Writes outside the prefix

| # | Artifact and address | Bytes | Current bytes | New bytes | Role |
| --- | --- | ---: | --- | --- | --- |
| 1 | image-4 decode `0x0077ac48`, film table row 2 | 12 | `02000000 feffff7f 02000000` | `0d000000 feffff7f 00000000` | value change, free stored code `0xd` with PROVIA's col2 |
| 2 | image-4 decode `0x0077ac60`, film table row 4 | 12 | `08000000 feffff7f 06000000` | `0e000000 feffff7f 03000000` | value change, free stored code `0xe` with Velvia's col2 |
| 3 | image-5 decode `0x00d50a48` | 4 | `0400a0e1` (word `0xe1a00004`) | `572ee3eb` (word `0xebe32e57`, `bl 0x61c3ac`) | code change, the case's first instruction becomes a branch to the hook |
| 4 | image-2 decode `0x0037b7c6` (56 bytes of the `0x83d` record) | 56 | `3f0050e1 42e15fe1 49e14de1 47e15fe1 53e145e1 41e152e1 43e148e1 5fe157e1 47e150e1 53e15fe1 42e141e1 52e15fe1 46e10000 00000000` (the debug string `PB_IMG_SEARCH_WGPS_BAR_F`) | `57004100 52004d00 20005300 54004100 4e004400 41005200 44000000` then seven zero words | value change, the compiled name `WARM STANDARD` in UTF-16LE with terminator and zero fill |
| 5 | image-2 decode `0x0037b7fe` (the `0x83d` layout halfwords) | 10 | `1400 1400 1e00 2400 3000` | unchanged | no write: the layout already matches an ordinary menu id |
| 6 | image-4 decode `0x00795db4`-`0x00795e60` (14 words) | 56 | `00000000` each | `01000000` each | value change, RC0's availability cells: the `0xfe` matrix key `0xb`/`0xc` rows (PRO Neg Std/Hi), columns 1, 2, 4, 5, 6, 7 and 21 |

Rows 1 and 2 are byte-read from `lz/id4-0061c400.bin`; the row addresses are `0x77ac30 + 12k` for k = 2 and 4. Row 3 is byte-read from `lz/id5-00bf1a00.bin` at decode offset `0x15f048`. Row 4 is byte-read from `filmsim/id2-00130000.bin` at offset `0x24b7c6`, and its container form is the byte complement at raw DAT `0x00eeba22` (byte-read in `FWUP0002.DAT`: the same bytes complemented), so the container write is 56 complemented bytes at that raw offset.
The site patch word is `0xebe32e57` = `bl 0x61c3ac` from `0xd50a48`, the encoded offset is `-0x7346a4`, well inside the BL range; the word is compiler output of `armBranchWord` (//host:recipe-bank) and the runtime worker's apply chain disassembles it at exercise time.
The name write is 14 UTF-16 halfwords plus zero fill; the buffer holds up to 27 characters plus a terminator (inherited, E-LABEL-RESOLVE-150), so a compiled name of 13 characters fits with room.
Row 6 is byte-pinned from [FILMSIM-PATCH-01](filmsim-patch-01.md) section 2 (its orders 49-62, the same 14 words): without them the mode-apply path substitutes the mode default for a chosen PRO Neg value, so the combined build carries them and both PRO Neg rows keep the behaviour RC0 shipped. The cells are word writes of `1` into the all-zero key-`0xb` row at `0x00795dac` and key-`0xc` row at `0x00795e08` (byte-read), the ASTIA/Velvia column pattern, value `1` meaning allowed and not a mode default.

### 4.2 Prefix words

The 127 prefix words are the six blocks of section 3 with current bytes `00000000`, checked against the constructed output of section 2. Their content is spelled out in sections 4.3, 4.4 and 5.2, and the compiler emits the exact word list into `patch-set-image4.json` as the six blob writes named above.

### 4.3 The arrays

```
values @ 0x0061c1fc (80 bytes)
0061c1fc: 00000000 00000000 03000000 00000000    {0, 0} then {3, 0}
0061c20c: 01000000 00000000 07000000 00000000    {1, 0} then {7, 0}
0061c21c: 0c000000 00000000 05000000 00000000    {0xc, 0} then {5, 0}
0061c22c: 06000000 00000000 02000000 00000000    {6, 0} then {2, 0}
0061c23c: 04000000 00000000 feffff7f 00000000    {4, 0} then the terminator
```

Pairs 8 and 9 name conversion rows 2 and 4, which are the recipe rows; the first seven pairs are RC0's, with row 5 PRO Neg Std and row 6 PRO Neg Hi, byte-identical to the RC0 build's array.

```
labels @ 0x0061c24c (160 bytes), nine quadruples plus a 16-byte -1 terminator
0x61c24c  {0x48a, 0x489, 0x48c, 0x48b}   PROVIA
0x61c25c  {0x48e, 0x48d, 0x490, 0x48f}   Velvia
0x61c26c  {0x492, 0x491, 0x494, 0x493}   ASTIA
0x61c27c  {0x49f, 0x49e, 0x4a2, 0x4a0}   B&W
0x61c28c  {0x4b0, 0x4af, 0x4b2, 0x4b1}   SEPIA
0x61c29c  {0x49b, 0x49a, 0x499, 0x49c}   PRO Neg Std (RC0)
0x61c2ac  {0x497, 0x496, 0x495, 0x498}   PRO Neg Hi (RC0)
0x61c2bc  {0x48a, 0x489, 0x48c, 0x83d}   recipe A: PROVIA's icons and help, compiled name id 0x83d
0x61c2cc  {0x48e, 0x48d, 0x490, 0x48f}   recipe B: Velvia's stock quadruple, no compiled text
0x61c2dc  ffffffff ffffffff ffffffff ffffffff
```

The quadruple roles are preview icon, list icon, help line and name (inherited, FILMSIM-PATCH-01 section 2.2). Both recipe rows take their base mode's icons and help line; recipe B takes its base's name as well, so it reads as a second Velvia entry in the list. The only compiled text is recipe A's name.

```
Q labels @ 0x0061c2ec (160 bytes), nine Q records plus a 16-byte -1 terminator
0x61c2ec  ff00077b 00000000 00000000 00000000   PROVIA (stock Q glyph)
0x61c2fc  ff00077c 00000000 00000000 00000000   Velvia (stock Q glyph)
0x61c30c  ff000773 00000000 00000000 00000000   ASTIA (stock Q glyph)
0x61c31c  ff000777 00000000 00000000 00000000   B&W (stock Q glyph)
0x61c32c  ff00077a 00000000 00000000 00000000   SEPIA (stock Q glyph)
0x61c33c  0000049a 00000000 00000000 00000000   PRO Neg Std, string id 0x49a (patch-02)
0x61c34c  00000496 00000000 00000000 00000000   PRO Neg Hi, string id 0x496 (patch-02)
0x61c35c  ff00077b 00000000 00000000 00000000   recipe A: PROVIA's Q glyph
0x61c36c  ff00077c 00000000 00000000 00000000   recipe B: Velvia's Q glyph
0x61c37c  ffffffff ffffffff ffffffff ffffffff
```

A Q record carries one id in word 0 and zeros in words 1-3 (byte-read stock format at `0x00789e2c`, FILMSIM-PATCH-02 section 2.1), and the Q drawer renders Q vector ids `0xff000000 | n` and plain string ids in the same function (inherited). The first five records are the stock glyphs byte for byte; the recipe rows take their base mode's glyph, so the Q cell draws each recipe as its base mode's icon and no new glyph or text id is invented. The values array above is shared with the Q cell through the Q node's values pointer, which is the wiring FILMSIM-PATCH-02's consumer reading established (the row lookup `FUN_00bf93e0` walks the values pairs and returns the parallel label record's word 0), so both recipe rows resolve in the Q cell as they do in the list.

### 4.4 The tables and the hook body

```
tables @ 0x61c38c
61c38c: 02 01 03 00 02 01 00 0b    recipe A rows: 0x104=2, 0x110=1, 0x111=3, 0x10e=0, 0x10f=2, 0xef=1, 0xed=0, 0x141=0xb
61c394: 01 02 02 01 03 00 00 08    recipe B rows: 0x104=1, 0x110=2, 0x111=2, 0x10e=1, 0x10f=3, 0xef=0, 0xed=0, 0x141=8
61c39c: 04 01 10 01 11 01 0e 01 0f 01 ef 00 ed 00 41 01    property table, halfwords LE
61c3ac: hook body, 76 bytes (19 words, section 5.2)
```

The row indices are compiler output. `host/firmware/BuildCompiledRecipes.hs` resolves each recipe field's menu-name token to a menu row through the pinned node map (`private-data/analysis/x-m1-1.50/filmsim/nodes-id4.txt`: each property's setting node carries its menu row tokens and its decoded label names, e.g. dynamic range's node `0x00789ea4` offers rows `0 1 2 3` as AUTO, 100 %, 200 %, 400 % and noise reduction's node `0x0078d108` offers rows `4 3 2 1 0` as HIGH to LOW) and then confirms the row's stored code and its reverse lookup in the pinned conversion table of that property (byte-read tables at `0x775428`, `0x775e24`, `0x77643c`, `0x77a810`, `0x778620`, `0x779f10`, `0x77a60c`, `0x77af48` out of the image-4 decode at compile). A token with no pinned row, a table that disagrees with a pinned code, an ambiguous code and an unknown input field are refused; nothing is approximated. The two tables above are what the compiler emitted for the pinned instance of section 6.1, byte-identical to revision 1's tables.

### 4.5 Relocations and value changes

Eight writes are relocations: the values-array pointer word and the labels-array pointer word of each of the three list nodes (`0x007823c4`/`0x007823c8`, `0x00785d94`/`0x00785d98`, `0x0077d1a4`/`0x0077d1a8`), and the Q cell's labels and values pointer words (`0x0078b024`/`0x0078b020`). The list nodes move from the stock shared arrays to `0x0061c1fc`/`0x0061c24c`; the Q cell moves from its own stock arrays to `0x0061c2ec` (labels) and the shared `0x0061c1fc` (values). The six list words are the same words RC0 already moves, with different destinations because the arrays grew; the Q pair is what removes the five-row Q limitation.
The container's own size propagation is also destination-only: the image-4 load word and the decoded-size and segment-size words, the trailer image words and the record length words follow the prefix extent, not a value the patch chose.
Every other write changes a value: the two conversion-table rows (stored code and downstream code), the 14 availability cells, the 400 bytes of array content, the two row tables, the property table, the hook body, the site instruction and the name text.
The site patch is the only code change in an existing image; the hook body is the only new code.

## 5. The commit hook

### 5.1 Attachment and state at the site

The hook attaches at `0x00d50a48`, the first instruction of the `case 0xfe` arm of the menu commit `FUN_00d4fc74` (byte-read: the prologue saves the row in `r5` at `0xd4fc80`, the case table entry for property `0xfe` is `0xd50a48`, and the arm continues `bl 0x63458c(r4)`, `bl 0x61c91c(0xfe, r8, 0)`, `bl 0x63c51c(52, 0, 0xfe)`, `bl 0x641438(3, 0)`, then the shared tail at `0xd52674`, whose notification `FUN_00d4d604(77, prop)` is a no-op).
At that instruction the registers hold the row token in `r5` (the `{row, child}` word of the selected values-array entry), the row's col2 in `r4`, its col0 in `r8` and the property `0xfe` in `r6`.
The patch replaces `mov r0, r4` with `bl 0x61c3ac`, and the hook re-executes that instruction on the way out, so the stock arm continues unchanged.

### 5.2 The hook body

```
61c3ac: e92d4ff8   push  {r3, r4, r5, r6, r7, r8, r9, r10, r11, lr}
61c3b0: e3550004   cmp   r5, #4
61c3b4: e30c438c   movw  r4, #:lower16:0x61c38c     ; r4 = the recipe row tables
61c3b8: e3404061   movt  r4, #:upper16:0x61c38c
61c3bc: 02844008   addeq r4, r4, #8                 ; recipe B's table
61c3c0: 0a000001   beq   0x61c3cc
61c3c4: e3550002   cmp   r5, #2
61c3c8: 0700001a   bne   0x61c3ec                   ; not a recipe row, return
61c3cc: e30c639c   movw  r6, #:lower16:0x61c39c     ; r6 = the property table, independent of r4
61c3d0: e3406061   movt  r6, #:upper16:0x61c39c
61c3d4: e3a07008   mov   r7, #8
61c3d8: e0d600b2   ldrh  r0, [r6], #2               ; property
61c3dc: e4d41001   ldrb  r1, [r4], #1               ; that property's conversion-table row
61c3e0: eb1cce23   bl    0xd4fc74                   ; the stock menu commit for one property
61c3e4: e2577001   subs  r7, r7, #1
61c3e8: 1afffffa   bne   0x61c3d8
61c3ec: e8bd4ff8   pop   {r3, r4, r5, r6, r7, r8, r9, r10, r11, lr}
61c3f0: e1a00004   mov   r0, r4
61c3f4: e12fff1e   bx    lr
```

Revision 1 derived the property walk's start from the row-table pointer (`add r6, r4, #16`), which lands on `0x61c348` for recipe B and walks the wrong eight halfwords: the property table's last four (`0x010f`, `0x00ef`, `0x00ed`, `0x0141`) and then four instruction halves of the hook itself (`0xf4f8`, `0xe92d`, `0x0004`, `0xe355`), so four of recipe B's parameters would never apply and four commits would carry code halves as properties. Revision 2 derives the property table with its own `movw`/`movt` pair, so both rows walk `0x0061c39c`-`0x0061c3ab`, and the ABI is otherwise identical to revision 1: the same ten pushed registers, the same re-executed `mov r0, r4`, the same single re-entry target.
The 19 words above are the emit contract: `build-compiled-recipes` refuses a `HOOK.bin` that differs in any slot, and the checks are what caught the two encoding defects of this revision's own first build (a transposed `push` word in the contract and a `movt` decoder taking the wrong halfword) and one assembly defect (a numeric `bl 0x00d4fc74` assembling `imm24 = target / 4` with no relocation, which targets garbage from `0x61c3e0`; the source now branches to the `menu_commit` symbol `menu/recipe_compiled_layout.S` defines, so the link resolves it against the link address). No numeric branch or address operand survives in the hook: the two table addresses and the commit target are link relocations against those layout symbols.
The source `menu/recipe_compiled.S` was assembled and linked with `menu/recipe_compiled_layout.S` and `menu/recipe_compiled.ld` (clang `--target=armv7r-none-eabi`, lld, `--defsym=RC_BASE=0x0061c3ac`) on 2026-10-10: 76 bytes, SHA-256 `a073fdf7399c5c17076b5dd34eb5b461f4c98cccf11d30515564559faa2985ee`, `rc_hook` `0x0061c3ac` size `0x4c`, `rc_row_tables` `0x0061c38c`, `rc_props` `0x0061c39c` (runtime-measured; the freeze number for this source state). The layout object carries the hook's three global absolute symbols (`rc_row_tables`, `rc_props`, `menu_commit`), so any link that consumes `recipe_compiled.o` resolves them to the prefix layout values; `menu/recipe_compiled.ld` assigns none of them and keeps only the `RC_BASE` placement and the two asserts.

### 5.3 Why it re-enters the stock commit

Each property's case arm in `FUN_00d4fc74` does more than a setter call, and it does different things for different properties (byte-read of the arms in this pass): it converts the row to col0 and col2 through that property's table, pushes the col2 downstream value into a live cell (the film arm stores into `0x009568b8`, sharpness into `0x009568a0`, noise reduction into `0x0095699c`, dynamic range into `0x009568bc`, highlight tone into `0x00956b40`, shadow tone into `0x00956b44`), writes the col0 through the property setter, and some arms add property-specific work (colour re-derives its stored code from the cell it just wrote, ISO reads the row token to distinguish the second AUTO row, white balance notifies mode-dependent state).
Re-entering `FUN_00d4fc74(property, row)` runs that work, which is why the hook carries one call per property instead of eight hand-written sequences. The nested calls are for the eight parameter properties and never for `0xfe`, so the patched site cannot be re-entered and the recursion depth is one level.

### 5.4 What it must not disturb

`r4` and `r8` must survive for the two instructions after the site (`mov r0, r4` at `0xd50a48` and `mov r1, r8` at `0xd50a54`); the hook's push covers every callee-saved register and the pop precedes the `mov r0, r4`, so both are restored from the outer frame before use.
`r6` must survive to the shared tail at `0xd52674`, which loads it into `r1` for the notification; it is saved and restored with the rest.
The stock arm's own calls and write are not skipped, reordered or duplicated: the hook runs before them and returns into the arm.
The record target resolution is untouched: the hook writes through the stock commit, which resolves the target through `FUN_006439e0()`, so a C-position selection lands in the bank's slot record and an ordinary selection in the ordinary block, with no direct write to the settings-image mirror, the slot area or area 3.
The caller's stack is balanced (ten registers pushed and popped), which also keeps SP 8-byte aligned across the nested calls.
No other state is read or written: the hook holds no flag, no cell and no lock.

### 5.5 Cost

76 bytes of code and 32 bytes of tables (two eight-byte row tables and the sixteen-byte property table), 108 bytes in all, plus the four-byte site patch in image 5. The code has no literal pool; the `movw`/`movt` pairs and the `bl` resolve through link relocations `menu/recipe_compiled.ld` binds.

### 5.6 If it runs at an unexpected moment

A row token outside `{2, 4}` at the site cannot occur from the shipped arrays, and if it did the hook returns after the push/pop with the same register and stack state as the unpatched instruction (`r0 = r4`, every other register restored).
A zeroed or half-written row table degrades to each property's row 0, which is a valid stored code for all eight properties (dynamic range AUTO, highlight and shadow tone HARD, colour HIGH, sharpness HARD, noise reduction LOW, white balance AUTO, ISO AUTO), so a corrupted table produces a wrong but valid preset and cannot fault.
The hook never runs from an interrupt, from another property and from the media or boot paths: the patched word exists only at one case-arm entry, reached by the menu commit for property `0xfe`.
The site is not reachable before the prefix exists: the decode writes the prefix before any code runs (inherited, FILMSIM-PATCH-01 section 5.3 and the widening report section 5.1), so no boot order exposes an uninitialised hook.

### 5.7 The one unproved premise: instruction fetch from the prefix

The hook executes from `0x61c3ac`, which is below image 4's load address. The static evidence that this is fetchable (byte-read and static-inference, gathered in revision 1):

- The MPU region that covers `0x61c1fc`-`0x61c400` is region 0, base `0`, size field 29 (`1 GiB`, covers `[0, 0x40000000)`), programmed by the pinned boot record at `0x19e8`-`0x19f8` through the helper at `0x1a84`-`0x1a9c` with attribute word `0x3b` (`mov r4, #58` then the enable bit): no execute-never bit is set and the same region carries the boot record's own executing code.
- The only MPU programming in the application images is the two-register helper at `0x61c700`-`0x61c720` and its disable twin at `0x61c724`-`0x61c738`, called from one site (`0x73bacc`) in a protect-range routine. It writes the base register, a constant `3` to `c6,c1,4` and an attribute word built as `(size << 1) | 1 | (permissions << 8)`, which never sets a high attribute bit. An exhaustive word scan for `mcr p15,0,rX,c6,c1,2` and `mcr p15,0,rX,c6,c2,0` over image 4, image 5, image 2, both loaders and both boot records finds no other site.
- The prefix is written by the same image-4 decompression pass that writes the image-4 code at `0x61c400`, one stream, engine write then entry jump, and image 4's code executes after that write, so the region is written-then-fetched in the stock load path already.

Residual (unproved): no hardware observation of a fetch from `[0x61c1fc, 0x61c400)` exists, the boot ROM's initial region attributes are not decoded here, and a hidden execute-never elsewhere in the runtime MPU state is not excluded. A probe or an emulated run of the patched commit is the cheap settlement.

### 5.8 Coexistence with the stock save action

The C-position route is unchanged and the hook depends on it. `CUSTOM SET` runs `FUN_00c74e5c`, which calls `FUN_00d4e2a0(4, 0, 0)`: the routine converts source and destination rows through property `0x16d`, returns when the destination code is not 0 (`0xd4e2e4 cmp r4, #0; 0xd4e2e8 popne`), writes the mode property `0xe9`, walks about ninety properties from the current record into slot 0 and stores the `0x28`-byte record to the settings-image mirror at `0x140`, which commits area 3 on the spot (inherited, custom-settings sections 4 and 4.1).
The hook does not call that action, does not change it and does not write the mirror itself, so its destination-zero check keeps its stock meaning. A recipe selected at the C position sits in the bank's slot record with the film code `0xd` or `0xe` and the eight parameter fields; `CUSTOM SET` then carries all of them into slot 0, because the copy reads the film property as the stored code and both base col2 values are allowed in the C mode column `0x2c` (inherited, FILMSIM-OFFER-01 section 3), and commits. `CUSTOM RESET` rewrites slot 0 from the per-slot defaults `0x007cb804`, whose film byte is `0`, so the recipe token clears with the rest of the record.

## 6. Owner-facing operation

### 6.1 What the two compiled recipes are

The patch set carries two recipes; their content is what the compiler compiles from the host JSON instance `private-data/analysis/x-m1-1.50/recipes/d3-compiled.json`, and the two instances below are the ones this write set encodes (byte-read as stored codes and rows, with the menu names resolved from the pinned nodes). The compiler's emit prints the same rows and codes it wrote, and its row tables are the tables of section 4.4 byte for byte.

| Field | Recipe A, named `WARM STANDARD` | Recipe B, stock Velvia label |
| --- | --- | --- |
| base mode | PROVIA (col2 `0`) | Velvia (col2 `3`) |
| dynamic range | 200 % (row 2, code 2) | 100 % (row 1, code 1) |
| highlight tone | MEDIUM HARD (row 1, code 1) | STANDARD (row 2, code 2) |
| shadow tone | MEDIUM SOFT (row 3, code 3) | STANDARD (row 2, code 2) |
| colour | HIGH (row 0, code 3) | MEDIUM HIGH (row 1, code 4) |
| sharpness | STANDARD (row 2, code 2) | MEDIUM SOFT (row 3, code 3) |
| noise reduction | MEDIUM LOW (row 1, code 1) | LOW (row 0, code 0) |
| white balance | AUTO (row 0, code 0) | AUTO (row 0, code 0) |
| ISO | 400 (row `0xb`, code `0xb`) | 200 (row 8, code 8) |

Recipe A's name is 13 characters, inside the 27-character buffer; the name text is fixed at build time and no camera operation types it. The colour menu's middle value is named `MID` on this body (node map); the schema's `STANDARD` spelling is not its menu name and the compiler takes the menu name.

### 6.2 Recall

The owner opens FILM SIMULATION from the shooting menu or the Fn menu. The list now shows nine entries: PROVIA/STANDARD, Velvia/VIVID, ASTIA/SOFT, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, `WARM STANDARD`, and a second Velvia entry.
Selecting `WARM STANDARD` or the second Velvia entry writes the recipe into the current record: the film field takes the recipe's stored code and the eight parameter fields take the recipe's values, each through the same path a manual change of that setting takes, including the live update of the processing state that each property's case performs. The menu row stays highlighted while the stored code is the recipe's.
The Q menu's film-simulation cell offers the same nine rows and scrolls them with the sub-command dial: the first five rows draw their stock Q glyphs, the PRO Neg rows draw their `<f202>`/`<f201>` icon strings and each recipe row draws its base mode's Q glyph. Selecting a recipe from the Q cell takes the same commit path as the list (the same property `0xfe` case arm), so both recipe rows are selectable there and the row lookup marks the selected row.
Re-selecting the row after a manual parameter edit re-applies the compiled values.
Selecting any other film row clears the recipe token in the same write, because the film field takes that row's code.

### 6.3 Save and persistence

In ordinary shooting modes (`class 0`) the eight parameter writes and the film code land in the ordinary block, which persists through area 2 at the next orderly power-off and is lost on an abrupt loss, as any ordinary setting is (inherited, P10).
At the C position (`class 3`) they land in the bank's slot record in RAM. Moving the dial away and back keeps them and a power cycle loses them, which is the stock behaviour of every C-position edit (hardware-observed, E-CPOS-EDIT-VOLATILITY-150). `SET-UP > CUSTOM SET` then carries the record into slot 0 and commits area 3 on the spot, so after it the recipe is what C returns on the next boot, including its token and its eight values.
`SET-UP > CUSTOM RESET` returns the C record to the factory defaults and clears the token, and a jig cycle resets both homes of the ordinary film value as it resets every other setting (inherited, E-CUSTOM-RESET-150 and the reset-origin records).

### 6.4 What the owner should know before the install

- The name is compiled. There is one compiled name in this design and one stock-labelled row; nothing on the body edits recipe names or recipe values, and no menu entry names a recipe except the film-simulation list row itself.
- The Q menu's film-simulation cell shows the two recipe rows as their base modes' Q glyphs, because the Q id space is closed (P5) and this design adds no glyph and no text id to it; the compiled name renders as list-row text on the text surfaces. The FILM 1-3 bracket children keep their own five stock rows and a bracket cannot select a recipe (RC0b scope, not incorporated here).
- The identity is the stored code, not the values. After a recipe selection, a manual edit of one parameter leaves the film code and the row highlight in place; the row then names a base mode plus the values the owner has while the compiled recipe's other values stay behind. Selecting another film row, or `CUSTOM RESET`, clears it.
- The eight parameter fields are ordinary settings: selecting a recipe overwrites the owner's current dynamic range, tone, colour, sharpness, noise reduction, white balance mode and ISO with the compiled values, including ISO.
- Recipe B repeats its base mode's label, so the list shows Velvia twice; the second entry is the compiled recipe. The alternative, one compiled row only, is section 7.
- Nothing in the design adds a menu item, a settings screen or a stored name, so the recipe rows appear only where the film-simulation list already appears.

## 7. Byte budget and drop order

| Item | Bytes | Note |
| --- | ---: | --- |
| values array | 80 | nine `{row, child}` pairs and the terminator |
| shooting labels | 160 | nine quadruples and the terminator |
| Q labels | 160 | nine Q records and the terminator, the Q exposure of the two recipe rows |
| recipe A and B row tables | 16 | new |
| parameter property table | 16 | new |
| hook body | 76 | new code, the corrected 19-word form |
| zero fill | 8 | to `0x61c400` |
| **combined prefix** | **516** | base `0x0061c1fc`, the widening report's corrected-floor shape |
| measured window | 516 | the corrected floor `0x0061c1fc` to `0x0061c400` |
| content | 508 | the running count against the window |
| unclaimed by this design | 8 | 516 minus 508, at `[0x0061c3f8, 0x0061c400)` |

Revision 1's content was 340 bytes (its 68-byte hook included) and the nine-row Q labels add 160, which is 500 of the 516 bytes with the old hook. The corrected hook is 76 bytes (the `movw`/`movt` form the independent table addresses need), so the content is 508 and the free remainder is 8 bytes at the top of the window.
The 68 bytes `[0x0061c1fc, 0x0061c240)` that FILMSIM-PATCH-02's manifest lists as residual slack are not free here: any 508-byte content in the 516-byte window occupies that band (this layout puts the values array and the head of the shooting labels there), so a bank bootstrap placed there would overlap the prefix. The bootstrap lane confirmed on 2026-10-10 that its stub takes the freed stock-array runs instead (`0x0078319c`, `0x0078d4f8`, `0x00782348`, `0x00789e2c`) and that the prefix is this artifact's; the two write sets are disjoint.
If a later build needs the space, the drop order is: the Q labels array as a unit (160 bytes, the Q cell returns to its stock five rows and its values pointer returns to the stock array), then recipe B's row content (its values pair, its shooting quadruple, its Q record and its row-table byte, leaving one compiled row), then individual recipe fields (each removed field saves two table bytes and removes that field from the recipe model).
Nothing in the current design needs to be dropped: the content fits the measured window with 8 bytes to spare.

## 8. Residuals

- The hook has no emulation case and no hardware observation; its fetch from the prefix is argued from the MPU region and the load order, not observed (section 5.7). The five exercise cases of section 10.5 are the scheduled runtime work.
- The 14 availability cells are byte-pinned from FILMSIM-PATCH-01's accepted write set and are unreviewed in this combination; they are what keeps the two PRO Neg rows from being replaced by the mode default at apply time.
- The image-5 record operation is landed (`rebuild-update extract-record5`/`replace-record5` carry the record-4 gate chain and slide the stream inside its window `[0x002c0000, 0x005a0000)`), and the build chain of section 10.4 is unrun past the emit: record-5 header word 1 (stream length) and the main checksum word move with the record-5 substitution, word 2 (chunk count) only when `ceil(total_len / 0x4000)` changes, and the container build is runtime work.
- A second compiled name is reachable at low cost and is outside this patch set. The record for `0x83e` exists at `0x37b808` (all zero, byte-read) and the locator already admits it (`sub r2, r0, #1; cmp r2, #0x83d` at `0x117dfe4`-`0x117dff4` admits id `0x83e`), but four to five entry points refuse it with a bound immediate: the text accessor `0x1179b14` (`movw r3, #0x83d`), the metric reader `0x117def4`, the line query `0x1178cf4` and the line layout `0x1178fb8`, all byte-read here, plus the draw wrapper's own compare inside `FUN_01179da0` (inherited, E-LABEL-RESOLVE-150). Changing each immediate from `0x83d` to `0x83e` is a one-byte edit in the plain image, which is an uncompressed extent, and the `0x83e` record then needs its ten layout halfwords `{20, 20, 30, 36, 48}` and its own 56-byte text. The 2026-10-09 scope keeps `0x83e` stock empty and the record copy reads the current image 2 (the one carrying the `0x83d` write at decode `0x0037b7c6`).
- Whether the shooting-menu tile echoes the current film value, and therefore shows the recipe name or the item name, is unproved.
- Whether any path other than `FUN_01179b0c` reads the `0x83d` record's inline buffer is unproved; the id-value census in E-LABEL-RESOLVE-150 found the special record base named only by the special-class family.
- The Q cell renders plain string ids (the patch-02 rows) and Q vector glyphs in one draw function (inherited); which declared width suits a string id in the Q cell is unproved, and this build keeps patch-02's `w=120` choice.
- The two rewritten conversion rows remove the old col0 values `2` and `8` from the table. No stock surface selects them (the availability matrix rows keyed col2 `2` and `6` are all zero, inherited FILMSIM-OFFER-01 section 3), but a record written by other means and carrying `2` or `8` would no longer convert; the scan would return 0 and set the shared not-found flag at `0x0089edc8`.
- The load `0x0061c1fc` is 4-byte aligned; every prior build used 64-byte-aligned loads. The widening report's ownership test admits the shape and no pinned evidence states a load-alignment requirement, but this is a review question.
- In a combined image the freed stock-array runs carry the bootstrap lane's stub, so a decode-versus-stock comparison at those runs must expect stub words; the compiler reads them only from the pinned stock decode (its SHA-256 is pinned) and refuses any other input.
- The install decision, the widened-window review and the container build stay outside this report.

## 9. Reproduction

All commands read host files only.

```sh
R=private-data/analysis/x-m1-1.50
I="$R/lz/id4-0061c400.bin"

# the film conversion table: rows 2 and 4 and the terminator
od -A x -t x4 -v -w12 -j $((0x77ac30-0x61c400)) -N 168 "$I"

# the eight parameter tables the compiler resolves rows in
for a in 0x775428 0x775e24 0x77643c 0x77a810 0x778620 0x779f10 0x77a60c 0x77af48; do
  od -A x -t x4 -v -w12 -j $((a-0x61c400)) -N 84 "$I"; done

# every values array that could name a film conversion row
for a in 0x78319c 0x780008 0x782348 0x77ddf4 0x786724 0x78a0e8; do
  od -A x -t x4 -v -j $((a-0x61c400)) -N 80 "$I"; done

# the case arm and its registers
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=$((0xd4fc74-0xbf1a00)) --stop-address=$((0xd50a94-0xbf1a00)) "$R/lz/id5.elf"

# the site instruction's current bytes
od -A x -t x4 -v -j $((0xd50a48-0xbf1a00)) -N 4 "$R/lz/id5-00bf1a00.bin"

# the 0x83d record (inline buffer and layout) in image 2, and in the container
od -A x -t x2 -v -j $((0x24b7c6)) -N 66 "$R/filmsim/id2-00130000.bin"
od -A x -t x2 -v -j $((0xeeba22)) -N 66 private-data/firmware/x-m1-1.50/FWUP0002.DAT

# the MPU region 0 programming and the application's only MPU helper
devenv shell -- sh -c 'sh '"$R"'/lz/mkelf.sh private-data/analysis/x-m1-1.50/decode/boot-40048.bin 0 /tmp/boot.elf &&
  llvm-objdump -d --triple=armv7-none-eabi --start-address=0x19e4 --stop-address=0x1a60 /tmp/boot.elf'
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x61c400 \
  --start-address=$((0x61c700-0x61c400)) --stop-address=$((0x61c740-0x61c400)) "$R/lz/id4.elf"

# the menu row tokens and names the compiler's bindings come from
# (each parameter node's value line and label line, e.g. 0x789ea4 dynamic range,
#  0x785cf0 highlight tone, 0x78d134 shadow tone, 0x77d158 colour,
#  0x77d514 sharpness, 0x78d108 noise reduction, 0x77f868 white balance,
#  0x783de0 ISO)
awk -f "$R/filmsim/tools/nodes.awk" "$R/filmsim/nodes-id4.txt" 2>/dev/null || true

# the "0x83e" bound sites the second-name option would edit
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x1100000 \
  --start-address=$((0x1179b0c-0x1100000)) --stop-address=$((0x1179b30-0x1100000)) "$R/filmsim/plain.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x1100000 \
  --start-address=$((0x117dee0-0x1100000)) --stop-address=$((0x117df10-0x1100000)) "$R/filmsim/plain.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x1100000 \
  --start-address=$((0x1178ce0-0x1100000)) --stop-address=$((0x1178d00-0x1100000)) "$R/filmsim/plain.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x1100000 \
  --start-address=$((0x1178fb8-0x1100000)) --stop-address=$((0x1178fd0-0x1100000)) "$R/filmsim/plain.elf"
```

## 10. Built source and the runtime chain

### 10.1 Source status

`host/firmware/BuildCompiledRecipes.hs` (the typed compiler and emitter), `menu/recipe_compiled.S`, `menu/recipe_compiled_layout.S` (the hook and its layout symbols) and `menu/recipe_compiled.ld` (the link script) are landed and built. The emit ran over the pinned artifacts on 2026-10-10 and produced `patch-set-image4.json` (155 word writes), `patch-set-image5.json` (the site word) and `id2-patched.bin` (image 2 with the compiled name) in `private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled/`, beside the assembled `hook.bin` (SHA-256 `a073fdf7399c5c17076b5dd34eb5b461f4c98cccf11d30515564559faa2985ee`, section 5.2). No container is built and nothing is installed. The source is stable at that freeze hash and an independent Sol review of the write set, the hook contract and the placement is requested before any build is offered to the owner.

### 10.2 Input contract

`RECIPES.json` takes the envelope of [schemas/recipe-set.schema.json](../schemas/recipe-set.schema.json) (schema_version 1, tier RC1, origin host-authored, the pinned target, exactly two recipes). Two deviations follow from this artifact's shape and are enforced: `name` is optional and at most one recipe carries it (the artifact owns one compiled name record, `0x83d`; the schema's mandatory name models the RC1 name-and-help id pairs this artifact does not carry), and a `help` line is refused (help text needs the recipe-id record class above `0x83e`). Parameter values are the menu-name tokens of section 4.4's mapping (`200`, `MEDIUM HARD`, `MID`, `FLUORESCENT-1`, `H25600`, ...); the schema's tone integers are not accepted because the 1.50 owner record binds no menu's sign side. The compiler refuses: a token with no pinned row, a pinned code the table disagrees with, an ambiguous code, an unknown field, a count other than two recipes, more than one name, a name outside the record buffer's 27 characters or the charset, a wrong input SHA-256, film codes `0x0d`/`0x0e` already taken, a stock values array already naming row 2 or 4, and a `HOOK.bin` that differs from the 19-word patch/ABI contract in any slot.

### 10.3 The Bazel stanza (landed in host/firmware/BUILD.bazel)

```python
# The D3/Q compiled-recipe rows' compiler: the typed recipe-set mapping over
# the pinned conversion tables, the schema-3 manifests for image 4 and image 5,
# and the patched image-2 carrying the compiled name (reports/recipe-patch-d3.md).
haskell_binary(
    name = "build-compiled-recipes",
    srcs = ["BuildCompiledRecipes.hs"],
    ghcopts = ["-Wall", "-Werror", "-O2", "-XGHC2021", "-threaded"],
    deps = [
        "//host:aeson",
        "//host:base",
        "//host:bytestring",
        "//host:directory",
        "//host:host-support",
        "//host:json",
        "//host:label-record",
        "//host:recipe-bank",
        "//host:update-container",
    ],
)
```

### 10.4 Emit, assemble and apply chain (runtime worker)

```sh
# assemble and link the hook (the freeze state of section 5.2); a link that
# consumes recipe_compiled.o without this script (the bank's combined link)
# adds recipe_compiled_layout.o, whose three global absolute symbols carry the
# same values
NIX_HARDENING_ENABLE= clang --target=armv7r-none-eabi -march=armv7-r -marm \
  -nostdlib -static -fuse-ld=lld -Wl,--no-dynamic-linker -Wl,--build-id=none \
  -Wl,-T,menu/recipe_compiled.ld -Wl,--defsym=RC_BASE=0x0061c3ac \
  menu/recipe_compiled.S menu/recipe_compiled_layout.S -o hook.elf && llvm-objcopy -O binary hook.elf hook.bin

# emit the manifests and the compiled image 2
bazel-bin/host/firmware/build-compiled-recipes \
  private-data/analysis/x-m1-1.50/recipes/d3-compiled.json \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin \
  hook.bin private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled

# apply: image 4 (the 516-byte prefix at 0x0061c1fc), image 5 (the site word),
# then the container chain replace-record4 --load-address 0x0061c1fc,
# replace-record5 and write-label 'WARM STANDARD' (or replace-image2 with
# id2-patched.bin). apply-patch-set checks every current byte and the pinned
# input hashes; the manifests are not live-write inputs.
bazel-bin/host/firmware/apply-patch-set \
  private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled/patch-set-image4.json \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin OUT/id4-patched.bin
bazel-bin/host/firmware/apply-patch-set \
  private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled/patch-set-image5.json \
  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin OUT/id5-patched.bin
```

The bank's record copy reads `id2-patched.bin` (spec `image2`, base `0x00130000`) and its `image2_compiled_name` guard decodes the `0x83d` record of that exact file, so a stock image 2 fails the bank build instead of dropping the name.

### 10.5 Exercise contract (scheduled runtime cases)

- row-path-a: menu commit `FUN_00d4fc74(0xfe, values entry {2, 0})`; the hook reads the recipe A row table at `0x0061c38c` and issues eight nested `FUN_00d4fc74` calls (properties `0x104`, `0x110`, `0x111`, `0x10e`, `0x10f`, `0xef`, `0xed`, `0x141`), then the stock arm continues with `mov r0, r4` and `mov r1, r8` intact.
- row-path-b: the same with values entry `{4, 0}` and the recipe B row table at `0x0061c394`; the property walk still starts at `0x0061c39c` (the corrected independent address).
- stock-rows: commit with values entry in `{0, 3, 1, 7, 0xc, 5, 6}`; the hook returns after its push and pop with no property call and no register change.
- reverse-lookup: `FUN_00631a00`'s col0 scan over `0x0077ac30` for stored codes `0x0d` and `0x0e` returns row indices 2 and 4.
- q-consumer: `FUN_00bf93e0` over the Q node's arrays returns record word 0 for rows 2 and 4 (`0xff00077b` and `0xff00077c`), with the first five Q glyphs unchanged.

## 11. What the design needs next

The write set is complete at byte level and its placement is inside the accepted widened window. The gates before a build can be offered to the owner are an independent Sol review of the write set, the hook contract and the placement, and the five native-harness cases of section 10.5 against mapped prefix bytes. The record-5 substitution and the container chain are runtime work over the landed `rebuild-update` operations.
The owner's decision and the readiness of the film-simulation unlock itself gate the install, and the recipe rows ship only in the same image as that unlock: the recipe rows live in the unlock's relocated arrays and the hook's effect is only reachable from a list that names their rows.
