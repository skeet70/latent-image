# Classic Chrome patch (X-M1 1.50, RC2)

Design, implementation and host verification for [spec/10 section 19.5](../spec/10-film-recipes.md#195-classic-chrome-rc2) and [CLASSIC-CHROME-01](https://github.com/skeet70/latent-image/issues/79): every patch site, its placement, the patch-owned data block, the flash container built on RC0, the RAM variant's specification, the native and emulator evidence, the changed-byte explanation and the reviewer packet (sections 11 and 12). No camera or USB device was touched. The candidate is a flash container for the RC0 body; it is not installed and needs the steps of section 12 before an install.

Inputs, all byte-identical to the pinned decodes: DAT `FWUP0002.DAT` SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; image 4 decode base `0x0061c400` SHA-256 `f2e2acab…dcff16`; image 5 decode base `0x00bf1a00` SHA-256 `3cd501c0…fd892a`; plain image base `0x01100000` SHA-256 `ee211bda…2d889d`; image 2 decode base `0x00130000` SHA-256 `236d12a1…20a5`; the fallback adjustment image at DAT payload `0xe0048`, which equals the body's own adjustment bytes over every window from `0xa000` to `0xcfff` (E-ADJ-READ-150). The installed state the patch builds on is RC0 (`extensions/rc0.json`, output SHA-256 `acb0fbbb…ca8`, E-FILMSIM-INSTALL-150).

Evidence labels: **byte-read** (instruction word or data byte read from a pinned image in this pass), **static-inference** (conclusion drawn from those bytes or the decompilation), **inherited** (named report or ledger entry, not re-derived), **hardware-observed**, **unproved**.

## 1. Summary and decisions

The patch adds one film-simulation row, `CLASSIC CHROME`, to the three shooting-menu surfaces RC0 extended and to the RAW-conversion list. It stores film code `0xd` (col0 and col2), carries `0xd` through capture to the develop record, renders it as PROVIA's processing with three items replaced by patch-owned data (3x3 colour matrix, 2x2 chroma matrix, tone record), and writes FilmMode `0x600`. All code is five small hooks plus in-place rewrites; all new bytes live in the RC0 decode-prefix window and in two image-4 converter functions the patch replaces anyway.

Decisions, approved by the director on 2026-10-18 after the design review:

1. **Develop code 8 is carried as a marker, not as `+0x104 = 8`.** Writing 8 into develop record `+0x104` makes a reader that runs before the develop builder (`FUN_00d61350`, from the preset expander) read past its 8-byte table and switch on the SEPIA stage flag, and three entry families reach the stages without passing `FUN_00d5d294` (section 5). The patch keeps stock `FUN_00d78418` (which already maps col2 `0xd` to code 0) and marks the record with `+0x110 = 0xd` through a 4-word rewrite of the `+0x110` setter. The census of every reader, writer and block copy of `+0x110` is in section 4.4.
2. **Availability for key `0xd` uses the seven columns RC0 gave PRO Neg, not PROVIA's 22.** PROVIA is the only allowed value in 14 columns, which is what greys the FILM SIMULATION item there; giving CC those cells would open the list and keep CC in 15 stock modes (section 4.1).
3. **The label uses the stock special id `0x83e`, not a new id `0x83f`.** The `0x83f` route needs a 264-byte relocated record table that does not fit beside the RAW-conversion arrays. No stock consumer produces `0x83e`, and raising the gates changes no stock surface (census in section 4.7). D3 uses `0x83d`; there is no label clash.
4. **FilmMode is written by `FUN_00eab258`, not `FUN_00eaac74`.** `FUN_00eaac74` is the Saturation encoder. Both get in-place changes (section 4.6).
5. **The tone record is served by a hook at the two `FUN_00690924` call sites in the tone stage, not by a patched copy of the 37-record table with key 37.** The table cannot grow in place and a copy is 2,740 bytes; no key is allocated, and every stock key still resolves through the unmodified reader and table (section 4.5).
6. **The Q cell and the FILM 1-3 bracket lists stay stock.** The prefix window is full at 484 of 512 bytes with the RAW-conversion list, which the verification needs; Q also has no CC glyph (issue 44). A stored CC shows PROVIA's Q glyph, as PRO Neg does on the RC0 body today (section 4.8).
7. **Live view previews CC; movie records CC as a consequence.** In `16c830ad…2c89` both stay PROVIA because `FUN_00eee958` maps 13 to 0. On 2026-10-22 the owner required a CC preview before the first flash; the live-view fragment widens `FUN_00eee958` with 5 in-place words and leaves every reviewed CC byte unchanged (section 4.10). It joins the icon candidate.
8. **D3 is incompatible as designed and must be re-planned around CC.** CC takes conversion row 2, code `0xd` and most of the prefix window (section 10). The CC patch set is reviewable and revertible separately from RC0 (section 11).
9. **Delivery.** The flash container is built and reviewed (section 8.1). On 2026-10-18 the owner directed a RAM-push test before any flash: the CC RAM payload is built on the imported RAM-push route with no recipe-bank code, emulated and under review (section 8.2), and section 8.4 is the camera session script for its first run.

## 2. Behaviour on the body

- SHOOTING MENU > FILM SIMULATION, the Fn-assigned list and the type-9 node `0x0077d184` show eight rows: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, CLASSIC CHROME. The CC row has no icon and no help line (fields 0-2 are id 0, which the row drawer skips); in the emulated list its icon cell draws empty and its name draws as `CLASSIC CHROME` (section 9.3).
- CC is offered in the same mode columns as ASTIA, Velvia and PRO Neg (mode values 0, `0x1f`, P, S, A, M and C). Elsewhere a stored CC is replaced by the mode default exactly as those films are.
- PLAYBACK > RAW CONVERSION > FILM SIMULATION shows six rows, the stock five then CLASSIC CHROME; a RAF converted with it is rendered as CC.
- A CC frame is rendered with PROVIA's processing except the colour matrix, the chroma matrix and the tone curve, which come from the patch's data block; luma-band (hue range), chroma gain, H/S tone and the tone scalar stay PROVIA's (spec 19.5.4 default).
- EXIF/maker note: FilmMode `0x600`, which exiftool names Classic Chrome; Saturation reports the COLOR setting as it does for PROVIA.
- Live view and movie (section 4.10): the MOVDC builders pass the film through `FUN_00eee958`. In `16c830ad…2c89` it maps 13 to 0, so CC previews and records movie as PROVIA. With the live-view fragment it passes 13, the MOVDC record carries `+0x110 = 0xd` and the reviewed hooks serve the data block there, so CC previews and records movie as CC (native and emulated; the panel output is unproved until the camera test).
- The C position stores `0xd` in its 4-bit film field like any other film (no clamp in getter, setter, C copy or area-3 read, byte-read in section 4.1).
- Q menu and FILM 1-3 bracket lists: unchanged five rows.

## 3. The film value path

| Step | Site | Stock behaviour for `0xd` | Patch | Evidence |
| --- | --- | --- | --- | --- |
| Menu row to stored and live value | conversion table `0x0077ac30` via pointer `0x0081b494` | no row carries `0xd` | row 2 becomes `{0xd, 0x7ffffffe, 0xd}` | byte-read |
| Live col2 to settings object | `FUN_00736a74` (table `0x00817180`), caller `0x00739930` | unmatched value returns 0 | 3-word rewrite passes 0..13 | byte-read |
| Canonical `+0x9c` to capture `+0x508` | `FUN_00738fe8` (same table), caller `0x00eb1a34` in `FUN_00eb17a4` | returns 0 | 3-word rewrite passes 0..13 | byte-read |
| Capture to develop request | `FUN_00e55918`: request[9] = `+0x508` | passes | none | byte-read |
| Develop code | `FUN_00d78418` via `FUN_00d78a80` (`0x00d78a8c`) | `sub r1,r1,#1; cmp r1,#11` sends 13 to the default `mov r3,#0`: code 0 | none | byte-read |
| Marker | `FUN_00d7851c` via `FUN_00d78538` (`0x00d794e8`, same request[9]) | stores constant 0 to record `+0x110` | stores `0xd` when the film value is `0xd` | byte-read |
| Stages | colour, chroma, tone data reads | PROVIA's data | hooks serve the data block when `+0x110 == 0xd` | byte-read |
| Metadata | `FUN_00eab258` FilmMode, `FUN_00eaac74` Saturation | 0 for both | `0x600`; PROVIA's Saturation arm | byte-read |

The table `0x00817180` holds 13 identity entries and its terminator at `0x008171e8`, with a second table at `0x008171f0`, so it cannot grow (byte-read). A film bracket copies `+0x608` into `+0x508` instead (`0x00eb1a18`-`0x00eb1a38`, byte-read); brackets cannot select CC in this design.

## 4. Patch sites

Image-4 addresses are decode addresses; the image-4 patch-set file offset is `address - 0x0061c200` (the new load, section 6). Image-5 and plain edits are in place. "Words" are little-endian A32 or data words.

### 4.1 Film-simulation list, conversion row and availability

| # | Address | Current | New | Role | Evidence |
| ---: | --- | --- | --- | --- | --- |
| 1 | id4 `0x0077ac48` | `0x00000002` | `0x0000000d` | row 2 col0 | byte-read |
| 2 | id4 `0x0077ac50` | `0x00000002` | `0x0000000d` | row 2 col2 | byte-read |
| 3 | id4 `0x0079608c` | `0x00000002` | `0x0000000d` | availability key-2 row becomes key `0xd` | byte-read |
| 4-10 | id4 `0x00796094`, `098`, `0a0`, `0a4`, `0a8`, `0ac`, `0x007960e4` | `0` each | `1` each | cells for columns 1, 2, 4, 5, 6, 7, 21 (modes 0, `0x1f`, M, A, S, P, C) | byte-read |
| 11-13 | id4 `0x007823c8`, `0x00785d98`, `0x0077d1a8` | `0x0061c380` on the RC0 body (stock `0x0078d4f8`) | `0x0061c2b0` | labels pointers of the shooting, Fn and type-9 nodes | byte-read, RC0 write set |
| - | id4 `0x007823c4`, `0x00785d94`, `0x0077d1a4` | `0x0061c340` on the RC0 body | unchanged | values pointers | byte-read |

Row 2 is inert in stock: no values array in the images names row 2 (shooting, Fn, type-9, Q, FILM 1-3 and the nine-row list name rows `{0,3,1,7,0xc}` or `{0,3,1,7,0xb,9,8,0xa,0xc}`), no default block holds col0 2 (ordinary film nibble `0x00`, C default nibble `0`), and the availability rows keyed by its col2 2 are all zero (byte-read). The converter `FUN_00631a00` walks to the `0xffff` terminator with no count, first match wins (`0x00631a84` `cmp r4,r1`), so a stored `0xd` resolves to row 2 and back (byte-read). Using row index 2 rather than an appended row 13 matters: a values token of `0xd` takes the render probe's item-level arm (`0x00d5528c` `cmp r6,#13`), which skips the availability check (byte-read), so an appended row 13 would be shown in every mode. The table is followed directly by property `0x13c`'s table at `0x0077acd8`, so it cannot be extended in place anyway (byte-read). Properties `0x100`-`0x102` point at their own copies (`0x0077ba70`, `0x00778464`, `0x0077892c`) and are not affected (byte-read).

The availability matrix is a key scan ending at key `0x7ffffffe` (`0x00631d18`), with no count; row 13 at `0x00796144` is that terminator and the next matrix starts at `0x007961a0`, so a row cannot be appended (byte-read). The key-2 row is unreachable once row 2 no longer carries col2 2. Cells take value 1 (allowed, not default); value 2 marks the mode default for `FUN_00631d58` (byte-read). The render probe greys every entry when fewer than two values are allowed (`0x00d55360` `cmp r0,#1; movls r4,#3`, byte-read). Stock allows only PROVIA in 14 of the 22 columns (column 3, mode `0x20`, and columns 8-20), so giving key `0xd` PROVIA's 22 cells would make the FILM SIMULATION item selectable in those modes and keep a stored CC in them through `FUN_0063f0a0` (static-inference); the seven-column pattern leaves every stock mode's list and default unchanged (static-inference).

Shooting arrays on top of RC0 (section 6.1 for addresses): values become RC0's seven pairs plus `{2, 0}` and the terminator (72 bytes, at the RC0 address `0x0061c340`); labels become RC0's seven quadruples plus the CC quadruple `{0, 0, 0, 0x83e}` and the 16-byte `-1` terminator (144 bytes, moved to `0x0061c2b0` because the RC0 labels end at the image-4 body). The two RC0-orphaned stock arrays (`0x0078319c`, `0x0078d4f8`) are not used: the recipe bank's boot stub claims them.

C slot and persistence (byte-read): the getter (`0x00620d44` `ldrb +0x24; and #15`, `0x00620d90` `ldrb +0x8a; lsr #4`), the setter (`0x00629404` `bfi r2,r5,#0,#4`, `0x00629450` `bfi r2,r5,#4,#4`), the C copy and the area-3 parity check pass `0xd` unchanged. The C-record compare `FUN_00d4bbc4` converts through `FUN_0061d564`, so CC compares as itself in the seven columns and as PROVIA elsewhere (static-inference).

### 4.2 RAW conversion

RAW conversion does not use its conversion table (`0x00775500`); the row token is mapped to a field by code (byte-read):

| Step | Site | Stock | Patch | Evidence |
| --- | --- | --- | --- | --- |
| Commit row to colour field | id5 `FUN_00d46010` arm `0x00d460cc`-`0x00d46234` (property `0x22b`) | rows 0, 1, 2, 4, 3 to colour `[0x00954d80]` = 0, 5, 3, 6, 7; rows 5-9 to mono variants; other rows to 0 | row 10 to colour 8: `0x00d461ec`-`0x00d461f8` `e5832008 e2466005 e3560004 8a000004` become `e356000a 03a02008 e5832008 ea000004` | byte-read current; new words assembled with llvm-mc by the menu slice |
| Colour field to highlighted row | id5 `FUN_00d588b8` arm `0x00d5ada4`-`0x00d5ade4` (17 words) | colour 0, 5, 3, 6, 7 to rows 0, 1, 2, 4, 3, else row 0 | adds colour 8 to row 10, same outputs otherwise | byte-read current |
| Settings record to develop request | plain `FUN_012546a8` `0x012548a0`-`0x012548dc` (16 words) | `ldr r3,[r4,#8]; sub r3,#3; cmp #4; ldrls pc`: 3 to 1, 5 to 3, 6 to `0xb`, 7 to `0xc`, else 0, stored to `0x01514220` (request word 2) | adds 8 to `0xd`; the jump-table arms after `0x012548dc` become dead | byte-read current |
| Request to capture `+0x508` | `FUN_01254f8c` → id5 `FUN_00ea1838` → `FUN_00ebf880` (`0x00ebf8c8` `str r3,[r0,#0x508]`) | passes | none | byte-read |
| Values array | node word `0x0078bd7c` = `0x0078ed30`, 5 pairs then the terminator, next node at `0x0078ed60` | cannot grow | relocated: stock pairs plus `{10, 0}` and terminator, 56 bytes; node word to the new address | byte-read |
| Labels array | node word `0x0078bd80` = `0x0078427c`, 5 quadruples `{0, icon, 0, name}` then terminator, next node at `0x007842dc` | cannot grow | relocated: stock quadruples plus `{0, 0, 0, 0x83e}` and terminator, 112 bytes | byte-read |

Property `0x22b` has no availability descriptor (byte-read of the 110-entry list at `0x0079944c`), so the row needs no matrix change. The RAW job's film value enters `+0x508` without passing `FUN_00738fe8` (byte-read), so the marker setter and the stage hooks apply to it as to a shot. The new words for the three code sites are listed in the private slice notes and are rebuilt and disassembled at build time; the colour code 8 is the design's choice in that field's encoding (0 PROVIA, 3 ASTIA, 5 Velvia, 6 PRO Neg Std, 7 PRO Neg Hi), not a value read from a sibling.

### 4.3 Converters

| Function | Extent | Current entry words | New entry words | Evidence |
| --- | --- | --- | --- | --- |
| `FUN_00738fe8` (canonical to capture) | `[0x00738fe8, 0x0073904c)`, 100 bytes; one caller `0x00eb1a34` | `e52db004 e28db000 e3500000` | `e350000d` cmp r0,#13; `83a00000` movhi r0,#0; `e12fff1e` bx lr | byte-read |
| `FUN_00736a74` (live to settings object) | `[0x00736a74, 0x00736adc)`, 104 bytes; one caller `0x00739930` | `e52db004 e28db000 e3500000` | same three words | byte-read |

Stock returns the table's identity value for 0..12 and 0 for anything unmatched, including negative values; the rewrite returns the same for every input except 13 (static-inference over byte-read tables). Inside both extents the only branch sources are the functions' own instructions, the only external branches are the single callers above, and no aligned or unaligned data word and no `movw`/`movt` pair in images 4, 5, plain or 2 names an address inside them (byte-read). The remaining 88 and 92 bytes become patch-owned code space (section 6.2).

### 4.4 Develop marker

`FUN_00d7851c` (`[0x00d7851c, 0x00d78538)`, one caller `0x00d78544` in `FUN_00d78538`) stores 0 to record `+0x110`. Its callers pass the film value in r1: request[9] from capture `+0x508` in the still builder (`0x00d794e8`), and the `FUN_00eee958`-validated local in the three movie builders that call it (`0x00edce7c`, `0x00edd2d0`, `0x00eef350`), which can only hold 0..12 (byte-read).

| Address | Current | New |
| --- | --- | --- |
| `0x00d7851c` | `e52db004` | `e351000d` cmp r1,#13 |
| `0x00d78520` | `e28db000` | `03a0300d` moveq r3,#13 |
| `0x00d78524` | `e3a03000` | `13a03000` movne r3,#0 |
| `0x00d78528` | `e5803000` | unchanged, str r3,[r0] |
| `0x00d7852c` | `e28bd000` | `e12fff1e` bx lr |

**Census of develop record `+0x110`** (byte-read over id5, id4 and plain, 2026-10-18). The develop record is 0x244 bytes (`FUN_00d77abc` clears 0x244, `FUN_00d6afe4` copies 0x244). It lives at one of seven bases, each named only at offset 0 and only in id5: statics `0x0156af00`, `0x0157453c`, `0x0157ba9c` (the still job), `0x015b7e30`, `0x01699d8c` (movie), `0x0169b954` (the `FUN_00eef1e4` path), and a stack record in `FUN_00eef4b0`; a census of every literal word and `movw`/`movt` pair in the three images whose value falls inside any record range finds no other pointer. The operand census (`[rN,#0x110]`, `#272`, `add/sub` forming `+0x110`, `ldrd`/`strd` at `+0x108`/`+0x10c`) over 237 id5, 64 id4 and 166 plain sites, intersected with the call closure of every function that receives a record pointer, leaves:

| Site | Word | Access | Base | Verdict |
| --- | --- | --- | --- | --- |
| id5 `0x00d791f8` | `e5803110` | init `str r3,[r0,#0x110]`, r3 = 0 | record (`FUN_00d791b0`) | writer of 0 |
| id5 `0x00d78540` | `e2800e11` | `add r0,r0,#272`, then the setter's store | record (`FUN_00d78538`) | the patched marker setter |
| id5 `0x00d671d4` | `e5912110` | `ldr r2,[r1,#0x110]` after `0x00d671cc cmp r3,#2` | record (`FUN_00d671b4`) | reached only for code 2; a CC record has code 0 |
| id4 `0x0073f700` | `e5903110` | `ldr r3,[r0,#0x110]` in the code-2 arm of a switch on `+0x104` in `FUN_0073f684` | record shape | reached only for code 2, and no branch, literal or `movw`/`movt` names `0x0073f684`, so it is statically unreachable |
| id5 `0x00d7954c` | `e5941110` | `ldr r1,[r4,#0x110]` | develop request, not a record | not a record |
| id5 `0x00d655e4` | `e5c62110` | `strb r2,[r6,#0x110]` in `FUN_00d65524` | hardware parameter block | not a record |
| id4 `0x0069307c`, `0x00693194`, `0x00693250` | `e2833e11` | `add r3,r3,#272` in register packers | output sub-block `+0x101c` and register image `0x014c6b24` | not a record |
| id5 `0x00d756f0` | `e5832110` | `str r2,[r3,#0x110]` in `FUN_00d753b8` | a 0xd1c-byte `DFTS`/`DFAL` structure at `*(ctx+0x28c) + 0xd7e54` filled from the 0xcb8-byte structure `0x014687f8` | not a record; no copy runs from it into a record |

No `ldm`, `stm`, `ldrd` or `strd` in the closure covers `+0x110`. Block copies: `FUN_00d77a48` copies `[+0, +0xdc)` into the record (excludes `+0x110`); `FUN_00d6afe4` copies the whole record into the `CYTS`/`VPTS` debug dump areas (callers `FUN_00d6b02c`, `FUN_00d6b460`, `FUN_00d6b3f8`), which no code in images 4, 5 or plain reads (the region-id service `0x00dbed74` hands them out as opaque bytes); `FUN_00d6ae18` reads other fields. Verdict: a `0xd` at `+0x110` of a code-0 record indexes no table, takes no filter branch and reaches no rendering path; the debug-dump copy carries it to an external reader whose interpretation is unproved and which has no rendering effect (byte-read sites, static-inference verdict). The native census cases of section 9.1 execute the still path with the marker injected on the stock images.

### 4.5 Stage hooks

Each hook replaces one `bl` to a stock data-read function with a `bl` to a patch stub; the stub tests the record held in a callee-saved register, serves the data block for a CC record and otherwise tail-branches to the stock function with r0-r2 and lr intact. The stock callees are leaves or AAPCS functions that clobber only r0-r3, r12 and flags, which is all the stubs touch; none uses the stack.

| Stage | Site | Current | Register state at the site | Stub action for `+0x110 == 0xd` | Evidence |
| --- | --- | --- | --- | --- | --- |
| Colour matrix data | id5 `0x00d67284` in `FUN_00d671b4` | `ebffff85` bl `0x00d670a0` | r0 = 72-byte destination (9 x u64), r1 = id-1, r5 = record | write the nine data-block halfwords zero-extended to u64 | byte-read |
| Chroma data | id5 `0x00d60f60` in `FUN_00d60ea8` and `0x00d64ef4` in `FUN_00d64e2c` | `ebffff57`, `ebffef72` bl `0x00d60cc4` | r0 = 32-byte destination (4 x u64), r1 = id-1, r4 = record | write the four halfwords in the order `FUN_00d60cc4` produces (fields `+0, +4, +6, +2`) zero-extended to u64 | byte-read |
| Tone record | id5 `0x00d677ac` and `0x00d677bc` in `FUN_00d675d8` | `ebe4a45c`, `ebe4a458` bl `0x00690924` | r0 = curve id, r1 = 9-byte head destination (stack, nonzero), r2 = 28-halfword curve destination (stack, nonzero), r7 = record | copy the data block's 9 head bytes and 28 curve halfwords | byte-read |

What runs around each hook stays stock for CC: the colour stage still selects PROVIA's id, validates it (`0x00d6722c` `mov r1,#20`), applies the `+0xe4 == 24` first-row override, sign extension and the `+0x140 == 7` scaling, and writes block `+0xd2..+0xe5` (and `+0x1020`, `+0x109c..+0x10ad` in its second instance); the chroma stage still skips codes 2 and 3, validates (`mov r1,#10` at `0x00d60efc` and `0x00d64e80`), applies PROVIA's five gains (`FUN_00d60d5c`, `d60d88`, `d60dc0`, `d60e3c`, `d60e68`) and writes block `+0xf26`, `+0xf4a..+0xf51` (and `+0x1024`, `+0x10f8..+0x10ff`); the tone stage still resolves PROVIA's family and ids, its blend `(w*A + (128-w)*B + 64) >> 7` returns the CC curve unchanged when A = B, and it writes block `+0xe6..+0x12b` (and `+0x10ae..+0x10f3` in its second instance) and PROVIA's scalar (byte-read sites, static-inference effect). The override paths `adj[0xb5] == 1` and `adj[0xcb74] == 1` call `FUN_00690924` from other sites (`0x00d67614`, `0x00d67648`) and are not hooked, so an override replaces CC's curve exactly as it replaces every film's; both bytes are 0 in the fallback image (byte-read).

The hooked functions are reached from all three entry families (the builder `FUN_00d5d294`, `FUN_00eef3f0` and `FUN_00edccd8`, byte-read call sites), so a marked record is rendered alike on every path that sets the marker; the movie paths cannot set it while `FUN_00eee958` stays stock.

Stub sizes (A32, linked into the converter extents, section 6.2): colour 7 words and chroma 7 words sharing a 7-word copy loop (84 bytes), tone 17 words (68 bytes). The branch words at the five sites are link output (`bl` from id5 `0x00d6xxxx` to id4 `0x0073xxxx`, well inside the A32 range).

**Tone-id selection under CC (review N2).** `cc_tone` serves the one data-block curve to both `FUN_00690924` calls, so CC ignores every input `FUN_00d675d8` uses to choose curve ids A and B and the blend weight: the develop code `+0x104`, the mode byte from `FUN_0073f940` (`+0x140`, `+0xdc`, `+0x1d0`, `+0x1d4`), level1 `+0xf8`, level2 from `FUN_0073fc64` (`+0x154` from capture `+0x2254`, or 0 when `+0x19c = 2`), the filter index from `FUN_0073f9d0` (`+0xe4`, `+0xe8`, `+0xec`, and the mode-2 `+0x124 = 7` arm) and the weight `+0x13c`. This matches the X70's Classic Chrome, which selects key 38 at every DR setting (inherited lead, x70view research/film-simulation.md). On PROVIA in the still job (mode 0, filter 0, level1 2) none of these inputs changes the tone id: family 1 resolves to key 1 at every level2 0..5 (byte-read curve-id block `0xcb77`, section 4.5 sources), and with A = B the weight has no effect. So for PROVIA-equivalent settings CC loses no tone-id behaviour. Inputs that do change tone ids in stock: level2 (the DR/`+0x154` setting) for Velvia, ASTIA and the PRO Neg films only; a nonzero filter index on any film (the scene tables `0xd68298`/`0xd67e84`); and the override bytes `adj[0xb5]`/`adj[0xcb74]`, which bypass the hook as they bypass every film. Highlight tone, shadow tone and DR act through stages CC keeps as PROVIA's (`FUN_00d679f8`, `FUN_00d69f8c`, the `0xd68108` tone scalar) (static-inference over byte-read tables). Whether a filter index can be nonzero for a still with CC selected (advanced-filter or scene modes) is unproved; the builder cases cover presets 2 and 15 only.

### 4.6 Metadata

FilmMode is `FUN_00eab258` (`[0x00eab258, 0x00eab35c)`, one caller `0x00eab368`, stored at maker-note record `+0xf0` with valid flag `+0xec`). Its output domain {0, `0x120`, `0x200`, `0x500`, `0x501`, `0xffff`} for col2 {0, 1, 3, `0xb`, `0xc`, B&W/SEPIA family} matches exiftool's FujiFilm FilmMode names, and the RC0 body wrote "Pro Neg. Std" for col2 `0xb` (E-FILMSIM-RENDER-REPEAT-ANALYSIS-150) (byte-read, inherited). `FUN_00eaac74` is the Saturation encoder (`+0x40`).

| # | Address | Current | New | Note |
| ---: | --- | --- | --- | --- |
| F1 | `0x00eab2e0` | `ea00000b` b `0x00eab314` | `ea00000f` b `0x00eab324` | default arm tests for 13 |
| F2 | `0x00eab2f0` | `00eab324` | `00eab334` | col2 4 shares the remaining `0xffff` arm |
| F3 | `0x00eab2f4` | `00eab32c` | `00eab334` | col2 5 likewise |
| F4 | `0x00eab324` | `e30f0fff` | `e353000c` cmp r3,#12 (r3 = col2 - 1) | |
| F5 | `0x00eab328` | `ea000008` | `03a00c06` moveq r0,#0x600 | |
| F6 | `0x00eab32c` | `e30f0fff` | `13a00000` movne r0,#0 | col2 0 and above 13 keep 0 |

| # | Address | Current | New | Note |
| ---: | --- | --- | --- | --- |
| S1 | `0x00eaac88` | `ea00004e` | `ea00004b` b `0x00eaadbc` | default arm tests for 13 |
| S2 | `0x00eaacb0` | `00eaadb8` | `00eaadb0` | col2 9 to the merged filter arm |
| S3 | `0x00eaacb4` | `00eaadc0` | `00eaadb0` | col2 10 likewise |
| S4-S9 | `0x00eaadb0`-`0x00eaadc4` | `e3000301 ea00000a e3000302 ea000008 e3000303 ea000006` | `e2430007 e2800c03 ea000009 e353000d 0affffc0 e3a00000` | `0x300 + col2 - 7` for col2 8-10; 13 joins the mode arm `0x00eaacc8` that PROVIA uses |

Every stock input keeps its output under both edits (static-inference over the byte-read switch tables). The other `+0x508` readers (`0x00e53308`, `0x00e537e4`, `0x00e54614`, `0x00e6e6a8`, `FUN_00d79db4`, `FUN_00ea9c30`, `FUN_00eaa888`, `FUN_00eaec80` sites, the plain live-view icon switches `0x0114b6c4`, `0x01146cf0`) give col2 `0xd` the values they give PROVIA without change (byte-read); `FUN_00ea9c30` writes maker-note `+0x108`, whose tag is unidentified (open question 5).

### 4.7 Label

Route: stock special id `0x83e`. Its record is the third 66-byte special record at image-2 RAM `0x0037b808` (`0x0037b784 + 66*2`), currently 66 zero bytes, followed by unrelated data at `0x0037b84c` (`GVFF`); in the DAT it is stored complemented at raw offset `0x00eeba64` (byte-read). No `#0x83e` immediate exists in plain, no node values or labels array names it, and the four aligned `0x0000083e` words in id4/id5 sit in numeric tables (byte-read), so no stock surface draws it.

New record content: UTF-16LE `CLASSIC CHROME` (14 units), a zero unit, zero fill to `+0x38`, layout halfwords `{40, 0, 30, 36, 48}`. Source (byte-read, review N4): the per-id layout records at image-2 `0x00130000 + 10*id` give `{40, 0, 30, 36, 48}` for the PRO Neg names `0x49c` and `0x498`, which render on the RC0 body (E-FILMSIM-INSTALL-150), while the stock names carry a nonzero second halfword (`0x48b` `{40, 96, …}`, `0x48f` `{40, 144, …}`, `0x4a0` `{40, 240, …}`); the stock special records carry `{1, 1, 30, 36, 48}` (`0x83c`) and `{20, 20, 30, 36, 48}` (`0x83d`). The meaning of the second halfword is unbound; rendering on the body is the owner's visual check (open question 2). The name buffer holds 27 units plus the terminator (`host/FujiAF/LabelRecord.hs`).

Gates: a compare-form census over every `movw rX,#0x83b..#0x83e` in the plain image finds 33 class gates, not the 30 that recipe-design section 17.6 lists: it misses two `id-1` gates with immediate `#0x83c` (`0x01178508` in `FUN_011784f0`, `0x011787b4` in `FUN_01178750`) and misclassifies `0x01164308` as a non-gate (byte-read). For `0x83e`, 23 direct-compare gates change `#0x83d` to `#0x83e` (`e300R83d` to `e300R83e`) and the two `#0x83c` gates change to `#0x83d`; the eight `id-1 #0x83d` gates, the locator and the ten `#0x83b` splits already treat `0x83e` as a special id (byte-read). The record base is formed only by six `movw r3,#0xb784; movt r3,#0x37` pairs and is not touched. Sites, current and new words: `0x011122c0`, `0x01112448`, `0x01112664`, `0x011127e8`, `0x01112a74`, `0x0111373c`, `0x01113994`, `0x011640f0` (`e300a83d`), `0x01164308`, `0x0116bbbc` (`e300883d`), `0x0116bcb4`, `0x01178564`, `0x01178694`, `0x01178cf4`, `0x01178fb8`, `0x01179a58`, `0x01179abc`, `0x01179b14`, `0x01179efc`, `0x0117a734`, `0x0117deb8`, `0x0117def4`, `0x0117df7c` (each `#0x83d` to `#0x83e`), and `0x01178508`, `0x011787b4` (`#0x83c` to `#0x83d`) (byte-read).

Quadruple: preview icon, list icon and help line are id 0, which the row drawer `FUN_01163f2c` skips before drawing (`cmp r0,#0; beq` after the getter at `0x01164bb8`, `0x01164c10`, `0x01164c8c`, `0x01164cf0`, byte-read); the blank id `0x83c` would draw its inline text `?AM?` wherever a drawer does not test for it. Tolerance of id 0 by the other consumers of the shared labels array is unproved and is a viewer check (section 9).

**Census of id `0x83e`** (byte-read, 2026-10-18). No stock code or data produces `0x83e` as a label: plain has no `#0x83e` immediate; the two id5 `movw rX,#0x83e` (`0x00bfea90` compares an event code, `0x0106ab08` is a table byte offset) are not labels; the aligned words `0x0000083e` are an adjustment-parameter key in the pair table at `0x007cd880` (passed to `FUN_0069f8fc`), a numeric curve at `0x0100d8b4`/`0x0100d8b8` and a constant table at `0x010a5664`; no `0xff00083e` or `0xfe00083e` word exists in images 2, 4, 5 or plain; no node or Q labels array names it; the English string table ends at id `0x83b`. All 46 `movw rX,#0x83b..#0x83e` compares in plain are unsigned bounds or three equality tests against the blank id, so raising a bound by one changes the branch of id `0x83e` only (id 0 wraps in the two `id-1` gates and stays refused). Writing the record and raising the 25 gates therefore changes no stock surface (static-inference over the byte-read census). D3 names `0x83d` (`nameRecordId = 0x83d` in `host/firmware/BuildCompiledRecipes.hs`, quadruple `{0x48a, 0x489, 0x48c, 0x83d}`) and neither writes nor names `0x83e`: no clash. Two follow-ups for other lanes: `BuildCompiledRecipes.hs` states that `0x83e` stays the stock empty record with no gate edit, which is false on a CC body; and the recipe bank's relocated label table copies `0x83c`..`0x83e` from whichever image 2 seeds it, so it must be seeded from the CC image 2 or the CC name resolves empty.

Why not `0x83f`: a new id needs all 33 gates raised plus the six base pairs moved to a relocated table holding the three stock records and the new one (264 bytes), because the slot after `0x83e` overlaps the `GVFF` structure (byte-read). 264 bytes do not fit beside the RAW-conversion arrays (section 6.4). `0x83e` also leaves `0x83d` to D3 and `0x83f` upward to the recipe bank.

### 4.8 Surfaces left stock

| Surface | Why stock | Consequence | Cost to add later |
| --- | --- | --- | --- |
| Q cell `0x0078b000` (values `0x00782348`, labels `0x00789e2c`) | no CC glyph in the closed `0xff000000 | n` space (issue 44); no window space | a stored CC draws PROVIA's Q glyph (no-match returns record 0), as PRO Neg does on the RC0 body | 144 bytes and two pointer words; the Q drawer renders plain string ids, so `0x83e` text may draw there (width unproved) |
| FILM 1-3 brackets `0x00786774`, `0x00786da8`, `0x007898d4` (properties `0x100`-`0x102`) | own tables and 11-row matrices; no window space; bracket frames copy `+0x604 + 4*frame` into `+0x508` (`FUN_00e81a34`), so the marker would apply, but this is unexercised | brackets cannot select CC | 6 table words, 15 matrix words, 144-168 bytes, 3-6 pointer words |
| nine-row list `0x0078d2d0` | blank labels, reachability unbound | none | - |
| Movie and live-view validator `FUN_00eee958` | in `16c830ad…2c89` it keeps the MOVDC path at 0..12 | that image previews and records movie as PROVIA | widened by the live-view fragment, 5 in-place words (section 4.10) |

### 4.9 Film value 13 beyond the develop path

Scope: every store review M1 names, on the candidate bytes (`16c830ad…2c89`). No reader indexes a table with the film value without a bound. Correction (2026-10-22): a bounded reader outside this scope, the film-class mapper of the frames-remaining estimate, gives every count 0 for CC (section 4.11).

Method. Units: image 4 (`0x0061c200`), image 5 (`0x00bf1a00`), plain (`0x01100000`), kernel0 (`0x00040000`), kernel1 (`0x000d0000`) and image 2 (`0x00130000`), from the flash-boot-check staging of the candidate. Overlays 44, 46 and 47 are byte-equal to plain at `[0, 0xac5a0)`, `[0xc0000, 0xdcca0)` and `[0xe0000, 0x34a5e0)` except for the 79 CC plain bytes, so the plain census covers them. All units were disassembled as A32; kernel0, kernel1 and plain were also disassembled as Thumb. The census propagated constants through `movw`/`movt`, literal loads, `add`/`sub` and register-offset addressing. It also took aligned and unaligned data words (including Thumb `+1` forms), Thumb `movw`/`movt` pairs, every `bl`/`b` to the accessors, Ghidra decompilation of images 4, 5 and plain, and mailbox and task registrations through `FUN_00724cdc`/`FUN_00725dd8`. Pointers held in globals were followed where the bytes name the global. No reference to any store below exists in kernel0, kernel1, plain/overlays or image 2.

Mailbox 48 is task `movdc`. `FUN_01039500` creates mailbox `0x30` (`0x100`-byte messages) and task `0x30` "movdc" with entry `FUN_01028b04`. The id's domain word at `0x007cad9c + 47*4` is 1 (entries for 45, 46 and 62 hold 2). `FUN_0072503c` copies the payload into a block from the OS table at `0x0096bd20` (`+0x170`) and posts it (`+0xac`/`+0xb0`), and `FUN_00725318` copies it out to the receiver's buffer. The task receives at `0x01028b48` (`FUN_00725318(0x30, 0x016fea2c, -1)`) and dispatches through 16 state tables at `0x0102c088`: `0x4002` `FUN_010324e8`, `0x4004` `FUN_01033b90`, `0x4005` `FUN_01029d00`, `0x4006` `FUN_010344f0`, `0x4008` `FUN_01034820`, `0x4012` `FUN_0102d458`, `0x4017` `FUN_01032274`. Each handler copies the 56-byte block into `0x01703120`, so the film is at `0x0170312c`. Senders besides `FUN_010398b0`/`FUN_01039b48` are `FUN_01039638` (`0x4002`), `FUN_01039c7c` (`0x4006`), `FUN_01039e48` (`0x4008`), `FUN_0103a32c` (`0x4012`) and `FUN_0103a594` (`0x4017`), all fed by `FUN_00f3b7a0` (film at block `+0xc`). Six functions read `0x0170312c`: `FUN_010299a8`, `FUN_0102d364`, `FUN_0102e2a4`, `FUN_0102e67c`, `FUN_0102e978` and `FUN_0102f380`. Each copies it into config `+0x5c` (template `0x01699584`, `FUN_00ed8b10`). `FUN_00ed8b6c` → `FUN_00ee2adc` → `FUN_00ee2320` stores it at `0x016995e0` and sets bit 0 of `0x01698a37`. `FUN_00ee5684` and `FUN_00ee559c` pass the config to `FUN_00edccd8` and `FUN_00edd120`, which call `FUN_00eee958(+0x5c)` (`cmp r0,#12; ldrls pc,[pc,r0,lsl #2]`, default stores 0). They hand the result to `FUN_00d78a80` and `FUN_00d78538`, so 13 becomes code 0 and marker 0 (byte-read).

| Store | Writer | Readers found (image 5 unless named) | Verdict for 13 | Persisted |
| --- | --- | --- | --- | --- |
| Settings object `+0x3c` (`0x0096ebd0 + k*0x2158`, k = 0, 1) | `FUN_0075b46c` (`0x00739944` after `FUN_00736a74`; `FUN_0074f0e0`) | image 4: `FUN_0075b454` only, from `0x00739924` (equality compare before the setter), `FUN_0074f09c` and `0x00759618` (`FUN_00759250` dirty-bit copy to the other object). None of the 481 functions holding the object or slot base reads `+0x3c`/`+0x9c` directly | compared or copied; no index | No. RAM outside the area-2 source `[0x009c282c, 0x009c2944)` and the area-3 buffer `[0x0089fa80, 0x0089fab4)`. The persisted film is the settings-image nibble at `0x009c28a6` (area 2 at orderly power-off, `0x006a8074`) and the C record (section 4.1) |
| `FUN_0074f09c` result (8 callers) | settings `+0x3c` | `0x00f163c8`/`0x00f1642c` store into the plan; the `0xd`/`0xc` tests in `FUN_00f162e4` are on `FUN_0074e7f0`'s value, not the film. `0x00f2976c` MCST dump. `0x00f2f598` `FUN_00f2f56c`. `0x00f388b8`/`0x00f38e88` → struct `+0x14` → `FUN_00eef3f0` → `FUN_00eef1e4` → `FUN_00eee958`. `0x00f50428` `film-4 < 2`. `0x00fa7a64` → `FUN_00fa7a5c` (`FUN_00f300d0`, `FUN_00f31674` → config `+0x5c`; `FUN_00f3b7a0` → mailbox 48; `FUN_00fa364c` → `0x016d748c`) | bounded (`FUN_00eee958` maps to 0; `sub #4; cmp #1` false as for PROVIA) or stored onward | No |
| Bracketing plan `0x016a026c + 0x200 + 4i` | `FUN_00f162e4` | `FUN_00f18b94`, `FUN_00f19204`, `FUN_00f1d770`, `FUN_00f1dd84` (`(i + 0x7ba)*4 + 0x0169e584`) → `FUN_007544b8` → slot `i` `+0x9c`. `0x00f19cec`, `0x00f1e9dc` (frame 0) → `FUN_007573f4` → `0x00919288`. `FUN_00f1cf68` saves the 12 film bytes around a restore from the backup | stored onward; no index | No (RAM) |
| Backup `0x4171783c` (alias of RAM `0x0171783c`, 0x218 bytes) | `FUN_00f17ad8` from `0x00f1cfe4` | `FUN_00f17abc` copies it to `0x016a026c` (film words kept), to stack copies in `FUN_00f23cb8`/`FUN_00f31674` (read only by `FUN_00f7a9ec` at `+0x158`..`+0x18f`), to `0x41717d54` (`FUN_00f24bd0`, pointer `0x016b6008`) and to `0x4171814c` (`FUN_00f36eac`, pointer `0x016bfa94`); no read of a copy's `+0x200` found | not read as film | No. It is a RAM save of the plan within one power session |
| `0x00919288` | `FUN_007573f4` (`0x00f19cf0`, `0x00f1e9e0`; `0x00f16914` writes 0) | `FUN_007573d4` only → `FUN_00fa7a6c` → `0x00f22aac` → `0x016b5ab8` → `FUN_00f23088` (`0x00f231fc`) block `+0xc` → `FUN_010398b0` → mailbox 48 (`0x4004`/`0x4006`) | bounded (`FUN_00eee958`) | No (RAM) |
| Slot `+0x16c + i*0xa8 + 0x9c` | `FUN_0075cf7c` via `FUN_007544b8` | `FUN_0075cf64` from `FUN_00754460` (`0x00f2cec0`: block `+0xc` → `0x4005` to mailbox 48 and the saved copy at `0x016b7918 + 0x58`; `0x00f2d958`: imgdc struct) and from `0x00eb1a30` (capture, patched converter, section 3). The whole-slot copies `FUN_00752928`/`FUN_0075ae78` have no caller that reads `+0x9c` of the copy | bounded or stored onward | No (RAM) |
| `MCST` dump `+0x100` | `FUN_00f29318` (only when adjustment byte `0xa8` bit 7 is set; the fallback image holds 0) | none in any unit; buffer `*(*(state+0xc)+0x50)`, consumer not found | write-only debug data | not determined (unproved) |
| `FUN_00f2f56c` record `+0x9c` | the callback itself | it is posted as message `0x3f3a` to imgdc (mailbox 47) by `FUN_00fead98` from `0x00f32470`. The only `0x3f3a` entry (`0x00ff58a8`, sub-table state 4 under `FUN_00ff563c`) is `0x00ff2e78`, which returns 2. No other aligned `0x3f3a` word and no `#0x3f3a` compare exists, so no invoker was found | not reached in the census (unproved) | No |
| Struct posted from `0x00f2d958` (byte `+0x80`) | `FUN_00f2d8b0` → `FUN_00feac00` → message `0x3f49` to imgdc (`FUN_00fe9024`: mailbox `0x2f`, task "imgdc", entry `FUN_00fe8f68`) | imgdc state 4 → `FUN_0100297c` → `FUN_01002860`/`FUN_01002280` → `FUN_01000860` copies the struct to `0x016f8314`. Its neighbours `+0x7e` to `+0x9b` are read; the film byte `0x016f8394` has no reader | not read (unproved for pointer readers) | No |
| `0x016d748c` (`0x016d7390 + 0xfc`) | `FUN_00fa364c` | the record pointer is published as `0x016b5458` by `FUN_00f20628`. Its 17 load sites in `FUN_00f207f8`..`FUN_00f2230c` dereference other offsets; none reads `+0xfc` | not read (unproved for pointer readers) | No |

Labels: writers, readers and verdicts are byte-read unless a cell says unproved. The persistence column is static-inference: each address is compared with the area-2 source (`0x006a8074` copies `0x409c282c`, `0x118` bytes, byte-read) and with the area-3 record buffer ([custom-settings](custom-settings.md) section 2). The reading of `0x4171783c` as an alias of low RAM is inherited from [full-system emulation](full-system-emulation.md).

Emulated (one emulator slot, snapshot `cc-candidate-1/boot-snapshot`, probe route of review M1, 825,036,179 instructions, `failure=None`). Committing CLASSIC CHROME set `0x00970d64` (object 1 `+0x3c`) to `0x0d` at once and `0x0096ec0c` (object 0) to `0x0d` after leaving the menu; object 0 read `0x0c`, the previously highlighted row, right after OK. The same commit set `0x009c28a6` to `0xd0` (film nibble) and `0x016995e0` to `0x0d`. Before the walk all of these read 0. `0x00919288`, the plan words, the backup, the six slot `+0x9c` words of both objects, `0x016b5ab8`, `0x0170312c`, `0x016f8394`, `0x016b797c` and `0x016d748c` stayed 0: their writers belong to capture and bracket planning, which the menu walk does not run. The probe has no shutter input (keys `up`, `down`, `left`, `right`, `ok`, `back`, `q`, `play`, `fn`, `dial-push`, `dial-left`, `dial-right`), so no capture was emulated.

### 4.10 Live view and movie (live-view fragment)

Owner requirement, 2026-10-22: live view previews Classic Chrome before the first flash. The change is a separate fragment, `host/firmware/classic-chrome-liveview-image5.json`, applied over the CC image 5 (`e740aeb2…89bb`, output `b361ed47…2535`); no reviewed CC byte changes. It goes into the icon candidate (`agent/cc-icon` integrates both into one flash candidate and one RAM-push payload).

**Which builder drives live view.** The MOVDC builders `FUN_00edd120` (full build: record init, film fragment, preset 15/16/17, `FUN_00d5da2c`) and `FUN_00edccd8` (incremental: the same record setters, then the stage wrappers `FUN_00d5e208`.. into block `+0x101c`) build develop record `0x01699d8c` into parameter block `0x01699fd0` from the MOVDC config `0x01699584`, whose `+0x5c` is the film (`0x016995e0`). Their only entries are `FUN_00ee559c` (called from `FUN_00ed9040`, `FUN_00ed95cc`, `FUN_00ed9af0`, `FUN_00edb630` and `FUN_00ee3dac`) and `FUN_00ee5684` (from `FUN_00ee364c`, which the config push `FUN_00ed8b2c`/`FUN_00ed8b6c` calls); both build only when bit 0 of `0x01698a37` marks the config dirty (byte-read). Emulated: with the CC RAM payload resident (8.2.5 `e2e-reviewed-1`), each row highlighted in FILM SIMULATION ran the marker setter with r0 = `0x01699e9c` (record `0x01699d8c + 0x110`) and r1 = that row's film value, then the colour, tone and second chroma hooks (`0x00d67284`, `0x00d677ac`, `0x00d677bc`, `0x00d64ef4`), in task `0x2b`; the CC row gave r1 = 0 (21 marker entries, all on this record). So the body rebuilds the MOVDC block for each highlighted film, and live view and the menu's film preview use this path (static-inference over the emulated entries). `FUN_00f387c0`/`FUN_00f38e0c` → `FUN_00eef3f0` → `FUN_00eef1e4` (record `0x0169b954`, block `0x016c05a4`) never entered the marker setter during menu navigation; what it drives is unidentified. The first chroma hook `0x00d60f60` was not entered: live view uses the second stage instances (preset 15). That the ISP draws the panel from `0x01699fd0` is **unproved**: the emulator models no ISP.

**Film reflection option.** None. The English label pool (payload strings `0x1120000`-`0x1141000`, 4,849 strings) holds no "NATURAL LIVE VIEW", "PREVIEW PIC. EFFECT" or "PREVIEW EXP." label; "REFLECT SHOOTING COND." is the RAW-conversion item (byte-read, label-pool scope).

**The fragment.** `FUN_00eee958(film, &out)` stores the film for 0..12 through a 13-entry jump table and stores 0 with return `0x84` otherwise. Its four callers (`0x00edcddc` in `FUN_00edccd8`, `0x00edd230` in `FUN_00edd120`, `0x00eef2c0` in `FUN_00eef1e4`, `0x00eef514` in `FUN_00eef4b0`) ignore r0 and pass `out` only to `FUN_00d78a80` (develop code; `FUN_00d78418` maps 13 to 0) and, except `FUN_00eef4b0`, to the marker setter `FUN_00d78538` (byte-read; no other load of the local in any caller). Nothing else names the function: no branch, literal word or `movw`/`movt` pair in images 4, 5 or plain points into `[0x00eee958, 0x00eeea84)` (byte-read).

| Address | Current | New |
| --- | --- | --- |
| `0x00eee960` | `e350000c` cmp r0,#12 | `e350000d` cmp r0,#13 |
| `0x00eee964` | `979ff100` ldrls pc,[pc,r0,lsl #2] | `95810000` strls r0,[r1] |
| `0x00eee968` | `ea00000c` b default | `93a00000` movls r0,#0 |
| `0x00eee96c` | `00eee9b0` table[0] | `9a000041` bls `0x00eeea78` (epilogue) |
| `0x00eee970` | `00eee9bc` table[1] | `ea00000a` b `0x00eee9a0` (default arm) |

Films 0..12 give the stock output, 13 passes through, anything else takes the stock default arm. The prologue and epilogue stay, so the frame the default arm unwinds is unchanged. Table words 2..12 and the 13 arms become unreachable. RAM push: all five are activation words published in one masked window, never in setup, because the stock `ldrls` reads `0x00eee96c`/`0x00eee970` as jump targets for films 0 and 1. A task suspended inside the function resumes correctly on either side: suspended before `0x00eee960` it runs the new body; after the stock `cmp #12` the new `strls`/`movls`/`bls` act on that compare's flags and give the stock result; the table words were never instruction addresses.

**Consequences.** On the MOVDC record a CC setting now gives develop code 0 and `+0x110 = 0xd`, so the reviewed hooks serve the data block there as they do for stills. `FUN_00eef1e4`'s record gets the same. `FUN_00eef4b0` always receives film 0 (its caller `FUN_00edc560` stores 0 at struct `+4`), so it stays PROVIA for every film. Movie recording uses the same MOVDC config and block, so movie records CC too; keeping it PROVIA would need a new test of a recording state inside the builders, a new mechanism the owner did not ask for. The `+0x110` census of section 4.4 already covers records `0x01699d8c` and `0x0169b954`. Live view and movie metadata are not changed.

At MOVDC start, `FUN_00ee39c4` sets `0x01698a40` bit 7 (`FUN_00ee54fc`) and `FUN_00ee561c` builds the block through `FUN_00edc560` → `FUN_00eef4b0` with film 0, clearing the dirty bit; the film build follows only at the next config push (byte-read). Whether live view after power-on shows the stored film at once is not established for any film; the owner check covers it (12.2.1 item 4, 12.2.8 A2b).

**Verification (native and emulated, 2026-10-22).**

- Site cases (`--liveview-id5`): family `validator-*`, `FUN_00eee958` on stock and live-view image 5 for inputs 0..19, `0x7fffffff`, `0x80000000` and `0xffffffff`; expectations read from the stock jump table: 46/46 pass, 368/368 in total (`private-data/analysis/x-m1-1.50/native-case/classic-chrome/sites-lv/`).
- Builder cases: path `lv` runs `FUN_00d77abc`, `FUN_00d791b0`, the `FUN_00edd120` film fragment (`FUN_00eee958`, `FUN_00d78a80`, `FUN_00d78538`) and preset 15, then `FUN_00d5da2c`. Stock col2 13 equals col2 0; on the CC image without the fragment col2 13 is PROVIA with marker 0; with the fragment, col2 0..12 records and blocks are byte-identical to stock, col2 13 has `+0x104 = 0`, `+0x110 = 0xd`, equals the PROVIA block outside the colour, tone and chroma ranges and the substitution oracle inside them (second instances `+0x109c..+0x10ec`, `+0x10f8..+0x10ff`). 115/115 checks with the still checks (`builder-lv/`).
- Full-system emulator: `xm1-viewer --ram-push-e2e --flash-candidate --trajectories emulation/cc-liveview-trajectories.json` on a test DAT of `16c830ad…2c89` plus the fragment (`4fa8cad4…5931`, `liveview-parts-1/`, not a flash target) and on `16c830ad…2c89`. With the CC row highlighted, config film is `0xd` on both; the record reads `+0x104 = 0`, `+0x110 = 0xd` with the fragment and 0 without. With the fragment the block differs from the PROVIA-row block at 60 bytes, all inside the second-instance colour, tone and chroma ranges, and equals the native builder case's CC block at every range byte; without it the two blocks are identical (`liveview-e2e-lv-2/`, `liveview-e2e-cc-2/`).
- Same emulator, segment `lv-cc-committed` (`liveview-e2e-lv-3/`): after OK on the CC row the record still reads `+0x110 = 0xd` and the block's range bytes equal the CC-row block. The harness's `back` left SHOOTING MENU open, so the shooting screen with the menu closed was not reached.
- Camera: the abbreviated RAM-push test of the icon change carries an owner check that CC live view looks muted, not like PROVIA (12.2.1).

### 4.11 Frames remaining (combined-candidate fix)

With CC selected, the reviewed candidate `16c830ad…2c89` shows frames remaining 0 and MEMORY FULL in the emulator (PROVIA 660 on the same card). The stock film-class mapper `FUN_00e42414` (image 5) sends film 13 to class 6, which the estimate's validator `FUN_00ed5868` rejects, so every count in the table at `0x009432a8` is 0; the film comes from `+0x80` of the state block `*0x01529fb4`, a copy section 4.9 did not enumerate, and the reader is bounded, so 4.9's verdict rule passed it. The fix rewrites `0x00e424ac`-`0x00e424b4` to `cmp r0, #13; movhi r0, #6; movls r0, #0`, which gives 13 PROVIA's class and every other input its stock class; it is in the combined candidate, not in this report's reviewed bytes. Evidence, the census of other film consumers and the shutter question: [the icon report](classic-chrome-icon.md) sections 9.4 and 9.5.

## 5. Why the develop code stays 0

The brief's leading architecture wrote develop code 8 and wrapped `FUN_00d5d294` at its single call `0x00d5da50` so the stages saw code 0. Byte reads rule it out as the primary design:

- `FUN_00d61350` reads `+0x104` from the preset expander `FUN_00d77a48`, which every still job calls before `FUN_00d5da2c` (`0x00e56018`, `0x00e56088`, `0x00e56240`). With code 8 it reads byte `0x00d61c60`, one past its 8-byte table (only code 3 set), value `0xf5`; `FUN_00d61388` folds it into the flag byte from `adj[0xaf93]` (`0x7f`) and sets bit 3, the stage SEPIA enables (`FUN_00d613c4`) (byte-read, effect static-inference).
- `FUN_00eef3f0` and `FUN_00edccd8` call the colour, tone, chroma and H/S stage wrappers directly, outside `FUN_00d5d294` (byte-read call sites).
- Fourteen `recbkfrm` tasks share the still develop entry `FUN_00ea17d0`, and `FUN_00d5d294` makes no OS calls, so a wrapper's "current CC record" word is shared state that two records can contend for (byte-read task table, static-inference).
- With code 8 in the record, four readers index past their tables (`0x00d60de8`, `0x00d67840`, `0x00d69e2c`, `0x00d61374`) and five land on entries other modes use (`0x00d676b0`, `0x00d676e4`, `0x00d69e44`, `0x00d6a17c`, by mode) (byte-read).

Costed alternatives (static-inference): a corrected wrapper needs about 9 code sites, the `FUN_00d61350` fix and a runtime-written word, about 230 cave bytes; keeping code 8 with every reader clamped needs about 13 code sites and 230 cave bytes. The marker design needs 5 branch sites, 4 in-place setter words and 152 bytes of stub code, and adds no state outside the record. If the director requires `+0x104 = 8`, the corrected wrapper is the fallback; its `FUN_00d78418` rewrite (a byte-table form inside the function's own 148 bytes) and `FUN_00d61350` bound are in the private slice notes.

## 6. Placement and ownership

### 6.1 Prefix window

The image-4 load moves from RC0's `0x0061c340` to `0x0061c200`, a 64-byte-aligned base like every prior build and inside the window `[0x0061c1fc, 0x0061c400)` (decoded size `0x00282a00`, segment size `0x00282a28`, so `load + decoded = 0x0089ec00`; the size rule is inherited from filmsim-prefix-widening.md section 6 and checked by `applyLoadAddress`).

| Range | Bytes | Content | Ownership | Evidence |
| --- | ---: | --- | --- | --- |
| `[0x0061c200, 0x0061c264)` | 100 | CC data block (section 7) | widening test, E-FILMSIM-PREFIX-WIDENING-150; no independent placement review | inherited |
| `[0x0061c264, 0x0061c29c)` | 56 | RAW-conversion values | same | inherited |
| `[0x0061c29c, 0x0061c2b0)` | 20 | zero | same | - |
| `[0x0061c2b0, 0x0061c340)` | 144 | shooting labels | same | inherited |
| `[0x0061c340, 0x0061c388)` | 72 | shooting values; first 56 bytes are RC0's | RC0, installed and reviewed (FILMSIM-PATCH-01, E-FILMSIM-INSTALL-150) | inherited, hardware-observed |
| `[0x0061c388, 0x0061c3f8)` | 112 | RAW-conversion labels | same as RC0 | inherited |
| `[0x0061c3f8, 0x0061c400)` | 8 | zero | same | - |

The prefix holds data only, which is the class of content RC0 already reads from it on the body (hardware-observed seven entries). The layout keeps RC0's values array at its address and moves only the three labels pointers, so the RAM variant can reach the same final bytes in an order that never leaves a node naming a half-written array (section 8.2). Runtime ownership of `[0x0061c200, 0x0061c340)` is in section 6.5; the window below `0x0061c340` has the widening report's own test and no named independent placement review.

### 6.2 Converter extents

The stub code lives in the two converter functions the patch must replace anyway (section 4.3), after their new three-word bodies: colour and chroma stubs plus the copy loop in `[0x00738ff4, 0x0073904c)` (84 of 88 bytes), the tone stub in `[0x00736a80, 0x00736adc)` (68 of 92 bytes). Ownership is by replacement: each function has one external caller, no branch, data word or `movw`/`movt` pair anywhere in images 4, 5, plain or 2 names an address inside it, and the patch rewrites its entry (byte-read). The code therefore executes from inside image 4's own executing code extent, so the open question of instruction fetch from the prefix (recipe-patch-d3.md section 5.7) does not apply to this design.

### 6.3 In-place edits

All other changes rewrite existing bytes: image 4 (conversion row, availability row, node pointers, RC0's 68 writes), image 5 (marker setter, five hook branch words, FilmMode, Saturation, two RAW sites), plain (25 gate words, the RAW request block) and image 2 (the `0x83e` record).

### 6.4 Byte budget

| Item | Bytes | Where |
| --- | ---: | --- |
| Shooting values and labels | 216 | prefix |
| RAW values and labels | 168 | prefix |
| CC data block | 100 | prefix |
| Prefix used / available | 484 / 512 | base `0x0061c200` |
| Colour, chroma stubs and copy loop | 84 | `FUN_00738fe8` extent, 88 available |
| Tone stub | 68 | `FUN_00736a74` extent, 92 available |
| In-place words | image 4: 2 + 8 + 3 + 2 (+ RC0's 68); image 5: 4 + 5 + 6 + 9 + 4 + 17; plain: 25 + 16; image 2: 66 bytes | in place |

The `0x83f` label route would add 264 prefix bytes and does not fit; Q or brackets would add 144 bytes each and do not fit.

### 6.5 Window ownership at runtime

Scope: `[0x0061c200, 0x0061c340)`, the 320 bytes below RC0's base that CC newly occupies, on the stock 1.50 DAT (`43d67afa…1490`, image-4 load `0x0061c400`), RC0 (`acb0fbbb…ca8`, load `0x0061c340`) and the candidate (`16c830ad…2c89`, load `0x0061c200`). Run artifacts are in `private-data/analysis/classic-chrome-patch/window-watch/`.

**Census (byte-read).** `window-coverage.c` and `extension-census.c` (filmsim-prefix-widening.md section 8) over the window, eleven inputs per body: images 4, 5 and plain, both loaders, both boot records, the EN slot, the complemented id2 and id3 ranges of the body's DAT and its table block (stock: the payload export). The CC images are `candidate-build-1/patched-*.bin`; its images 4 and 5 are byte-identical to the emulator's decodes of `16c830ad…2c89` (`00493fab…ff6a`, `e740aeb2…89bb`) and its write list equals the pinned one except for the parameters hash.

| Hit class | Stock | RC0 | CC |
| --- | --- | --- | --- |
| Aligned word naming the window | 0 | 0 | 5: the three labels pointers `0x0061c2b0` at node `+0x24` (`0x0077d1a8`, `0x007823c8`, `0x00785d98`), the RAW values pointer `0x0061c264` at `0x0078bd7c`, and the segment-4 load word `0x0061c200` (table block `0x0061c0d4`) |
| `movw`/`movt` pair naming the window | 0 | 0 | 3, the stubs' data-block bases: `0x00736a8c` (`0x0061c220`), `0x00739000` (`0x0061c204`), `0x0073901c` (`0x0061c216`) |
| Unaligned word naming the window | 1 (id5 `0x0101808b`, `0x0061c200`) | same | same |
| Pairs anchored in the table block that intersect the window | 4: id5 straddles `0x0101808b`, `0x010c94f1`; table header `0x0061c000`; extent descriptor `0x0061c1a4` | same 4 | same 4, plus the four pointer words above and `{0x0061c200, 0x00282a28}` at `0x0061c0d4` |

The id5 hits are the straddles filmsim-prefix-widening.md section 3 classified (offset table and high-entropy data, no reference). Every CC-only hit is a patch-owned reader or the decode descriptor. The stubs read the window with `ldrb`/`ldrh` and store only through the caller's r0-r2 (`0x00736a8c`-`0x00736ac0`, `0x00739000`-`0x00739040`).

Load-path writers and other candidates (byte-read):

- Table-block copy: boot record `0x1f5c` copies one 128 KiB unit from media `0x140000` to `0x0061c000`, covering the window (inherited, filmsim-prefix-widening.md 5.1; the emulated copy below is 131,072 bytes). Media `[0x1401fc, 0x140400)` is zero in all three DATs (raw `0xff`); segment record 4 holds load/size `0x0061c400/0x00282828` (stock), `0x0061c340/0x002828e8` (RC0), `0x0061c200/0x00282a28` (CC).
- Image-4 load, `FUN_00002250`: the bss fill `FUN_00003844([rec+0xc], [rec+0x10] >> 2, 0)` targets the record's bss word `0x0089ec00`, not the load; `FUN_00003d3c(load, [0x4228] = 0x42e40000)` then sets the decompressor destination base. No fill or clear starts below the load.
- Stacks: the only constant stack tops are `0x70002000` (loaders `0x40250`/`0xd0250`), `0x000a0000` (`0x436f4`) and `0x00130000` (`0xd36f4`); task stacks come from TCB fields (`0x436e4`), and image 4's exception entry (`0x0061c7d4`, `0x0061c888`) sets SP from another mode's SP. None lies in `[0x00600000, 0x00640000)`.
- MPU: region 0 is `[0, 0x40000000)` from the boot record (inherited, recipe-patch-d3.md 5.7). The application's only MPU programming, `FUN_0073badc` through `0x0061c700`/`0x0061c724`, takes its ranges from the extent descriptor at `0x0061c1a4` (`0x0073bcd4`/`0x0073bcd8` load `0x01100000`, `0x00360000`) and the extent entries, so regions 5-7 cover only the plain image.
- DMA and media buffers: the decode source staging is `0x42e40000` (`[0x4228]`); the other load-path transfers are listed under the emulated run below.

**Emulator watch (emulated).** Each body was cold-booted from reset in one emulator slot by the retained upstream emulator, using a copy of the `cc-candidate-1` flash-boot-check runtime (staged trees identical, native engine `ff5ec4a1…d31d`), driven along the flash-boot-check route (12 x UP, OK, OK, 40 M instructions, MENU, 90 M) and then into FILM SIMULATION. The native engine's passive access probe (`access_probe_begin`) watched canonical `[0x0061c200, 0x0061c340)` and its `0x40000000` alias from instruction 0 on both cores: CPU loads, stores, fetches and exclusives, and model bus reads and writes (decompressor output, media transfers and fills go through `write_bus`). The driver was an inline command, not added to the repository.

| Body | Path executed | Records in the window | Window at the end |
| --- | --- | --- | --- |
| Stock | boot (80,000,280), SHOOTING MENU (450,019,207), FILM SIMULATION open (560,024,079), menu exit, PLAY, MENU, OK; end 836,038,640 | 1: the table-block copy, model bus write `[0x0061c000, 0x0063c000)` at instruction 4,936,037 | 320 zero bytes, the copy's media bytes |
| RC0 | same route; SHOOTING MENU 450,019,375; end 836,039,868 | 1: the same copy at 4,936,037 | 320 zero bytes |
| CC | same route; FILM SIMULATION, cursor to row 7, OK (733,032,441: film cell `0x009568b8` = `0x0d`, nibble `0x0096ccee` = `0xd`), list reopened on row 7; end 1,101,048,745 | 328: the copy at 4,936,037; the first decompressor job's output `[0x0061c200, 0x0066919e)` at 4,978,037 (job started 4,971,237, source `0x42e40014`, 131,052 compressed bytes); two emulator-host reads of `[0x0061c200, 0x0089ec00)` at the application entries of both cores (7,867,227 and 8,164,533; its loader validation, SHA-256 equal to the mapped decode); 324 CPU reads by the stock labels reader `0x00bf9564`/`0x00bf956c`/`0x00bf9574`/`0x00bf957c` of the shooting labels `[0x0061c2b0, 0x0061c330)`, 81 per PC, from 354,998,270 to 1,089,782,888 | equal to image-4 decode bytes `[0, 0x140)` (`cf1c7ea8…5e9c`); first 100 bytes equal `data-block.bin` |

No body recorded a CPU store, fetch or exclusive access in the window, and no model write other than the copy and, on CC, the decode. On every body the bss fill (`0x0089ec00`, `0x352e00` bytes) started at 4,961,319, after the copy and before the first decompressor job; the other load-path transfers (loader DMA `0x0089f700` to `0x01460000`, graphics fills and copies at `0x0228c800`-`0x027b3200`) lie outside the window. The data block `[0x0061c200, 0x0061c264)` and the RAW arrays were not read, because the emulator models no ISP develop and RAW conversion was not reached: with no card present, PLAY from live view left the live-view display and the following MENU opened SHOOTING MENU, so PLAYBACK > RAW CONVERSION was not entered on any body.

Positive control (emulated): the CC boot to SHOOTING MENU with the probe over `[0x0061c000, 0x0061c340)` recorded CPU stores at `0x1f78` to `0x0061c188` (4,937,187) and at `0x0073be70` to `0x0061c1c0`, `0x0061c1d8`, `0x0061c1f0` (42,938,271 to 58,011,473), the writers the hardware captures show (filmsim-prefix-widening.md 5.2); inside `[0x0061c200, 0x0061c340)` it recorded the same copy, decode, host reads and labels reads as the CC run and no store.

**Alignment and decode.** Each condition the widening gives for RC0 holds at `0x0061c200`:

- Base alignment: `0x0061c200` is 512-byte aligned. RC0's `0x0061c340` is 64-byte aligned only and was decoded by the body's hardware (E-FILMSIM-INSTALL-150, hardware-observed), so CC's base meets every power-of-two alignment RC0's met.
- Size invariants: `0x0061c200 + 0x00282a00 = 0x0089ec00` (bss start) and `0x0061c200 + 0x00282a28 = 0x0089ec28`; `rebuild-update verify` passes the image-4 size, bss and header-arithmetic checks (`0x52` chunks of `0x4000`, mode 4, record length `0x00146733` within the `0x160000` flash window) (byte-read, section 9.5).
- Table-block end: the last retained byte is `0x0061c1fb`, below the base; `[0x0061c1fc, 0x0061c200)` is zero.
- Feed and destination rules (byte-read, boot-40048): the job loop feeds the received stream up to its last whole chunk of the header's chunk size (`0x4000`), less the 20-byte header, or the whole remainder at the record's end (`0x3f64`-`0x3fa0`); each job's destination is `[0x42b8] + [0x400042e0]` (base plus accumulated output, `0x3e38`-`0x3e5c`, `0x3fc4`-`0x3fe0`), written by `FUN_00003c54` to `0xfff43048` with no mask or alignment test; the length field is bits 0-18 of `0xfff43044` and bit 24 selects a method other than 4; the output count is read back from `0xfff4304c` (`0x3cf0`). The base enters only as the first job's destination.
- Continuation destinations (emulated): 34 jobs per body, 11 for image 4. Stock's image-4 continuations include destinations that are not word aligned (`0x00761617`, `0x007e4dc2`), as do RC0's (`0x00669362`) and CC's (`0x0066919e`), so the stock path already hands the engine unaligned continuation destinations in this model.

**Unproved.**

- The hardware decompressor has not decoded this stream, or any stream, with a first destination of `0x0061c200`. Its first-destination alignment rule is undocumented; the hardware evidence reaches 64-byte alignment (RC0).
- Decoder fidelity: the emulator's decompressor output equals the mapped decode and the build's pre-encode image (emulated, byte-compared), which shows the encoder and two software decoders agree; agreement of the hardware engine with them is inherited from the stock and RC0 bodies, not shown for this stream. The per-job output counts, and so the continuation destinations above, are the model's.
- Paths not executed: capture, develop rendering, playback and RAW conversion; their writers are unwatched. The emulator has no cache model and treats MPU writes as no-ops.
- A value-identical writer is excluded for the emulated path (no store at all) and not on hardware.

## 7. Data block and the private parameter file

The data block is patch-owned read-only data at `0x0061c200`. The build fills it from a private parameter file; public tests fill it from a synthetic set.

```
+0x00  u8[4]   magic 'C','C','D','1'
+0x04  u16[9]  colour matrix, row-major, X-M1 adjustment encoding (the form of the nine fields at adjustment 0xacc3 + 18*(id-1))
+0x16  u16[4]  chroma, in FUN_00d60cc4's output order: fields +0, +4, +6, +2 of the 0xb077 encoding
+0x1e  u8[2]   zero
+0x20  u8[9]   tone record head (record +4..+12)
+0x29  u8      zero (record +13)
+0x2a  u16[28] tone curve (record +14..+0x45)
+0x62  u8[2]   zero
```

The stubs copy halfwords as the stock reads produce them (zero-extended to u64 for colour and chroma), so the stock post-processing (sign fill, scaling, gains, blend) applies to the CC values unchanged.

Input format the build needs from the cc-model worker, at `private-data/analysis/classic-chrome/candidate.json`:

```json
{
  "schema": "latent-image.classic-chrome-parameters.v1",
  "target": {"model": "X-M1", "firmware_version": "1.50",
             "firmware_sha256": "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"},
  "approximation_of": "Classic Chrome, EXR Processor II with X-Trans CMOS II (X-T1 5.52, X70 1.10)",
  "provenance": {"method": "…", "inputs": [{"path": "…", "sha256": "…"}], "e_method": 124, "e_model": null},
  "colour_matrix": [[0, 0, 0], [0, 0, 0], [0, 0, 0]],
  "chroma": [0, 0, 0, 0],
  "tone_record": {"head": [0, 0, 0, 0, 0, 0, 0, 0, 0], "curve": [0, "… 28 values"]},
  "reuse": {"luma_band": "PROVIA", "chroma_gain": "PROVIA", "hs_tone": "PROVIA", "tone_scalar": "PROVIA"}
}
```

Rules the build enforces (`host/FujiAF/ClassicChrome.hs`): `target` equals the pinned 1.50 DAT; `colour_matrix` is three rows of three integers, each the raw u16 field value in the X-M1 adjustment encoding, a 13-bit two's-complement value with bits 13-15 clear and 1024 = 1.0 (`0x1f00` is -256; the build refuses a field at or above `0x2000`); `chroma` is the four raw u16 fields, same encoding, in the `0xb077` storage order `+0, +2, +4, +6` (the build permutes them to the block order); `tone_record.head` is nine bytes and `tone_record.curve` is 28 integers, non-decreasing, at most `0x3fff`, ending at `0x3fff`; `provenance.e_method` is an integer in hundredths of mean CIEDE2000 and `provenance.e_model` the same or `null` until the owner's photo session; `reuse` must be all `PROVIA` in format version 1, because the design serves only the three data items (a different luma band, gain or scalar needs a further record-aware hook and a format change). Unknown fields are refused; JSON numbers must be integers (`//host:json` strict decoding). The build prints the SHA-256 of the parameter file and of the block; neither enters the public tree.

The public synthetic set `host/firmware/classic-chrome-synthetic.json` (identity matrix `0x400` on the diagonal, chroma `{0x400, 0, 0, 0x400}`, head of zeros, a linear 28-point ramp ending at `0x3fff`) exists only to exercise the stubs, the build and the cases; its provenance field states that it carries no Fujifilm data. cc-model writes the fitted set to `private-data/analysis/classic-chrome/candidate.json` in this format; it may be rewritten, and every build and case records the file's SHA-256.

## 8. Delivery variants

### 8.1 Flash container

`//extensions:build-extension` starts from the stock DAT, so the CC extension re-includes RC0's 68 writes. Steps in container order:

| Step | Kind | Content | Supported today |
| --- | --- | --- | --- |
| 1 | `image-patch-set`, segment 4, `output_base` `0x0061c200` | prefix blobs, RC0 words, conversion row, availability row, node pointers, converter bodies and stubs | yes (`--load-address` passed; prefix length up to `0x10000`) |
| 2 | `image-patch-set`, segment 5 | marker setter, five hook words, FilmMode, Saturation, two RAW sites | yes (in place only) |
| 3 | `image2-record` | the `0x83e` record (`id`, `text`, `layout`, `current_hex`), run as `rebuild-update write-record` with a current-bytes check and a read-back of name and layout | yes (added 2026-10-18) |
| 4 | `plain-patch-set` | 25 gate words and the RAW request block as an `apply-patch-set` manifest over the extracted plain window (segment 6, decode base `0x01100000`, the window's SHA-256 `ee211bda…889d` pinned, every current word checked), then `replace-plain` of the whole window | yes (added 2026-10-18) |

Because the data block carries private values, the generated patch sets, the manifest and the output digest of a real candidate stay under `private-data/`; the public tree holds the build tool, the stub source, the synthetic parameter set and the tests. `//host/firmware:build-classic-chrome PARAMETERS_JSON NEW_OUTPUT_DIRECTORY` validates the parameter file, takes the linked stubs from `//host/firmware:classic-chrome-stubs` (A32 assembly, `host/firmware/classic-chrome-stubs.S` and `.ld`; assembly because the stubs are register-contract shims at stock call sites), checks every current word and blob against the pinned stock decodes and RC0's write set (`recovery/rc0-patch-set.json`, SHA-256 `586bbb6f…94e1`), and writes `cc-image4.json`, `cc-image5.json`, `cc-plain.json`, the extension manifest `classic-chrome.json`, `data-block.bin` and the changed-word listing. `//extensions:build-extension --unpinned` builds a development image and prints its digest; after review the digest is pinned with `build-classic-chrome --output-sha256` and the pinned manifest must reproduce it. The flash image then needs `rebuild-update verify`, the native cases of section 9, `//emulation:flash-boot-check` and the owner's per-run go naming its digest (spec 07 H3).

### 8.2 RAM payload over the RAM-push route (built, emulated, reviewed)

The owner directed a RAM-push test before any flash. The CC RAM payload is a second payload kind of the imported route ([RAM-DELIVERY-01](https://github.com/skeet70/latent-image/issues/77), [RAM-PUSH-TRUST-01](https://github.com/skeet70/latent-image/issues/78), [transport README](../transport/README.md)): it reuses the generic route (USB push-and-run in an ordinary boot with stored `0xf7 = 0x01`, the bootstraps, the allocation, the two-core masked installer with per-line D-clean and `ICIALLU` on both cores, exact-reverse rollback, the relinker mechanics) and nothing of the recipe bank. The bank+D3 payload, its bytes and its pins are unchanged.

#### 8.2.0 The route and how CC fits it

- **Delivery:** USB push-and-run in an ordinary boot. The body's stored config byte `0xf7 = 0x01` makes a cable attach enumerate `04cb:ff80` service mode 3 while the camera stays in ordinary use (hardware-observed). No jig session and no SD-boot, so the E-SERVICE-SHOOT-150 limit of a jig write does not apply.
- **Upload and activation:** debug opcode `0x200001` function 3 writes the runtime (installer, publication table, mailbox) byte by byte into a stock-heap allocation with readback. The stock `0x450006` query then calls a temporarily retargeted callback slot (`0x015c22ac`, stock `0x00dc7874`), restored and verified after each call. Function 3 never writes a stock word; every publication is one aligned `str` by the installer.
- **Quiescence:** the installer runs on CPU0 in the USB-task callback with IRQs masked and parks CPU1 IRQ-masked in a stock worker job (`0x0072503c`, channel `0x58`); it validates every old word, then stores all new words, then maintains caches, then releases CPU1. Rollback (`ram-unload`) is the exact reverse with old-or-new validation.
- **Cache maintenance:** `DCCIMVAC` on every publication line, `DSB`, `ICIALLU`, `DSB`, `ISB` on both cores, plus stock range maintenance over the allocation before upload.
- **Checks:** the host refuses unaligned or duplicate words, words inside the allocation, the callback slot or the bootstrap window, and any build whose publication-contract digest (over index, phase, address, old, core) or source-identity pins differ from the payload's review record. The installer has no allowlist.
- **Lifetime:** resident until power-off. Images 2, 5 and plain load once per power session and image 4 at boot; no mode change reloads them. Hardware-observed residency on the bank route: a 94-word payload through menus, a capture and playback; a 250-word payload through menu use, film-list commits and the Q tile. After a power cycle every publication word read its old value.

| Question | Answer | Evidence |
| --- | --- | --- |
| Can it write image 4, image 5, plain and image 2 while menu and develop are quiescent? | Yes: every class has been written on the body by this route inside the masked two-core window. A task already preempted inside a changed stock function is covered by the single-word form of every CC code change (8.2.3) | hardware-observed (E-RAM-PUSH-FULL-SESSION-150), byte-read installer |
| Does it stay resident through menu use, capture and RAW conversion? | Menu use and capture: yes, observed on the bank route. RAW conversion: never exercised by any push or emulator run (8.2.5 step 4); residency there is static-inference (no loader runs mid-session) | hardware-observed, static-inference |
| Does the route execute code overlapping the CC window, the converter extents or the hook sites? | No. It executes the bootstrap request window `[0x01469b20, 0x01469b40)`, the heap allocation, and stock `0x0061c4f4`, `0x0061c490`, `0x0089cda4`, `0x0089cdc4`, `0x006b0c3c`, `0x006b0aa8`, `0x0072503c`, `0x00042634`, `0x00dc7874`; none is in `[0x0061c200, 0x0061c400)`, `[0x00736a74, 0x00736adc)`, `[0x00738fe8, 0x0073904c)` or the five hook sites | byte-read |
| Is any CC word outside the route's rules? | No: all 131 publications are aligned, distinct and outside the allocation, slot and bootstrap window; the CC payload has its own contract, counts, ABI 3 and pins | computed, `ClassicChromeRam.hs` |
| Is `[0x0061c200, 0x0061c340)` free on the RC0 body? | Yes: free zero RAM. Written only by the boot table-block copy (zeros), named by nothing in the RC0 images, untouched in the emulated RC0 boot (section 6.5), hardware-observed zero in an ordinary RC0 boot on 2026-10-18 and then used by the D3 push in the same session | byte-read, emulated, hardware-observed |

#### 8.2.1 No recipe-bank code (review finding B1)

The imported bank runtime (`563cbaa3…`) is the one that wrote the aspect field 0 behind the persistent save hang (E-SAVE-HANG-ASPECT-150), so the CC runtime links, publishes and reaches no bank code. It is linked from exactly two objects, `classic_chrome_ram.o` (`menu/classic_chrome_ram.S`) and `ram_push_installer.o` (the unchanged `menu/ram_push_installer.c` compiled against a generated copy of `menu/ram_push_abi.h` whose count, setup-count and ABI-version lines are substituted, each substitution checked to hit once), with `menu/classic_chrome_ram_runtime.ld` and the stock-symbol script `menu/classic_chrome_ram_stock.ld`. The complete symbol list of the base-`0x00c20000` link (`kernel.syms`):

- CC: `__cc_image_start`, `cc_ram_convert`, `cc_ram_marker`, `cc_ram_filmmode`, `cc_ram_saturation`, `cc_ram_raw_commit`, `cc_ram_raw_highlight`, `cc_ram_raw_request`, `cc_ram_colour`, `cc_ram_chroma`, `cc_ram_copy`, `cc_ram_tone`, `cc_ram_data`, `cc_ram_matrix`, `cc_ram_chroma_fields`, `cc_ram_tone_head`, `cc_ram_data_end`, `__cc_image_end`.
- Installer: `__ram_push_start`, `__ram_push_installer_start`, `ram_push_install`, `ram_push_rollback`, `ram_push_status`, `execute_operation`, `publish_mailbox`, `run_rendezvous`, `ram_push_rendezvous_locked`, `validate_old_words`, `validate_rollback_words`, `publish_words_locked`, `store_new_words`, `restore_old_words`, `line_seen_before`, `synchronize_current_core`, `post_peer_park`, `wait_for_signal`, `read_peer_report`, `peer_park_action`, `peer_park_complete`, `peer_cache_action`, `peer_park_request`, `peer_park_job`, `ram_push_cpu0_window_exit`, `ram_push_cpu1_window_exit`, `ram_push_publications`, `rendezvous`, `ram_push_mailbox` and the section bounds `__ram_push_publications_start/_end`, `__ram_push_rendezvous_start/_end`, `__ram_push_mailbox_start/_end`, `__ram_push_end`.
- Absolute: `RAM_PUSH_BASE`, `RAM_PUSH_PUBLICATION_COUNT` and the twelve `cc_stock_*` continuation and reader addresses.

No symbol has the prefix `rb_`, `recipe_bank` or `__rb_`, and the link map names no other object. The builder (`host/FujiAF/ClassicChromeRam.hs`) refuses a link whose symbols or map contain either; `//host/firmware:classic-chrome-ram-tests` covers the refusal. No CC publication names a bank site, and no CC code calls anything but the stock continuations and readers listed.

#### 8.2.2 The payload

| Item | Value |
| --- | --- |
| ABI version | 3 (bank+D3 stays 2) |
| Publications | 131: 84 setup, 47 activation |
| Allocation size | `0x1560` (5,472 bytes): CC code and data at the base (`cc_ram_data` at base `+0x120`, image `0x184` bytes), the installer at `align32(base + 0x184)` |
| Contract digest (index, phase, address, old, core) | `0030860616947ebaa1cade89c36ca40d43ffaaf610258400220b1879cf4432d9` |
| Runtime SHA-256, candidate block `833b6452…2774` | base `0x00c20000` `ce050d3a1ac978112b2e58b02e11a26d34d5cfa1d9cb7748f59b26d97814ea17`; `0x00c21f88` `c5f6241f61a3165efe913c28f5f19866518689846c646c903784cebea6d559bb`; e2e base `0x00a7e8c8` `e01952a67169196b9021fb202abcd458519c62578fd5746cb5115092537805eb` |
| Runtime SHA-256, synthetic block | base `0x00c20000` `c42fa36a26c8a6c848003a3dd96f5063a18295c514b2314a5706ed0183e1a2c9` |
| Review record | `transport/ram-push-cc-review.json`, separate from the bank record: 16 pins plus the publication contract. The camera binary compiles every pin into `RamPushCcReviewedPins` whatever its status and requires a relinked build to match them; it does not read verdicts. `//transport:ram-push-cc-review-verdicts` is the procedural gate before a go: it failed while the pins were `unreviewed` and passes since review CcRamPushReview (commit `b4e1448`, verdict approve) moved all 16 pins and the contract to `approved`. `//transport:ram-push-cc-review-bytes` checks that the pinned files match the record |
| Read plans | `private-data/analysis/classic-chrome-patch/ram-push/read-plans/cc-ram-push-prepush.json` (51 windows: every old word plus the route's reviewer windows) and `cc-ram-push-post-reboot.json` (42 windows); both pass `rehearse read-ram` |

Rebuilding gives byte-identical runtime and manifest. A parameter-only revision changes only the 100 bytes at `cc_ram_data` inside the allocation and leaves the contract digest unchanged, but the parameter file `classic_chrome_parameters` is a `mechanism` pin and the CC payload has no mechanical content check (it links no bank code, so the bank ELF allowlist does not apply). A new parameter set is therefore a mechanism revision that needs one independent review under spec 07 section 3.2 before a go (review note N3).

#### 8.2.3 Changed-word groups and what each mechanism word does

Every publication's old word is the RC0 body's (image 4 at `0x0061c340`, zero below it; stock images 5, plain and 2); the shape check in `ClassicChromeRam.hs` refuses any other diff. Data words make the same final bytes as the flash candidate at those addresses; code changes are single aligned words, so a task already inside a changed stock range continues on stock bytes.

| Phase | Group | Words | Content |
| --- | --- | ---: | --- |
| setup | RAW values `[0x0061c264, 0x0061c29c)`, shooting labels `[0x0061c2b0, 0x0061c340)` | changed words only | the flash candidate's arrays; nothing names them yet |
| setup | image-2 `0x83e` record `[0x0037b808, 0x0037b84c)` | changed words | `CLASSIC CHROME`, layout `{40, 0, 30, 36, 48}`; no surface names `0x83e` yet |
| setup | plain label gates | 25 | `#0x83d` to `#0x83e`, two `#0x83c` to `#0x83d` (section 4.7) |
| setup | conversion row 2, availability key row | 2 + 8 | `{0xd, -, 0xd}`; key `0xd`, cells 1, 2, 4, 5, 6, 7, 21; no values array names row 2 yet |
| activation | entry words (mechanism) | 8 | see below |
| activation | hook words (mechanism) | 5 | see below |
| activation | shooting labels pointers; values tail | 3 + 3 | `0x007823c8`, `0x00785d98`, `0x0077d1a8` to `0x0061c2b0`; then `0x0061c380` = `0x7ffffffe`, `0x0061c384` = 0; then `0x0061c378` from `0x7ffffffe` to 2, which names the CC row |
| activation | RAW labels `[0x0061c388, 0x0061c3f8)`; RAW node words | changed words + 2 | old RC0 labels words, unreferenced after the pointer move; then `0x0078bd80` before `0x0078bd7c` |

RC0's `0xffffffff` at `0x0061c3f8` and `0x0061c3fc` lies past every array and stays unpublished (the flash candidate has 0 there).

Mechanism words, each a single aligned word written inside the masked rendezvous:

| Address | Stock word | New | Effect |
| --- | --- | --- | --- |
| `0x00738fe8` | `e52db004` | `b cc_ram_convert` | canonical film to capture `+0x508`: 0..13 pass, else 0 (stock passed 0..12) |
| `0x00736a74` | `e52db004` | `b cc_ram_convert` | live film to settings object `+0x3c`, same mapping |
| `0x00d7851c` | `e52db004` | `b cc_ram_marker` | develop record `+0x110` = 13 for film value 13, else 0 (stock: always 0) |
| `0x00eab2e0` | `ea00000b` (FilmMode default arm) | `b cc_ram_filmmode` | col2 13 returns `0x600` through the stock epilogue `0x00eab350`; every other default goes to stock `0x00eab314` (0) |
| `0x00eaac88` | `ea00004e` (Saturation default arm) | `b cc_ram_saturation` | col2 13 takes PROVIA's mode arm `0x00eaacc8`; others stock `0x00eaadc8` (0) |
| `0x00d461ec` | `e5832008` (RAW commit) | `b cc_ram_raw_commit` | row 10 stores colour 8, others store the stock 0; resumes at stock `0x00d461f0` |
| `0x00d5ada4` | `e3530000` (RAW highlight) | `b cc_ram_raw_highlight` | colour 8 highlights row 10 and exits through `0x00d5b344`; others re-run the stock compare and resume at `0x00d5ada8` |
| `0x012548a4` | `e2433003` (plain RAW request) | `b cc_ram_raw_request` | settings field 8 sets request word 2 (`0x01514220`) to 13 and resumes at `0x01254928`; others resume stock at `0x012548a8` |
| `0x00d67284` | `ebffff85` | `bl cc_ram_colour` | `+0x110 = 13`: nine data-block halfwords as u64; else tail to stock `0x00d670a0` |
| `0x00d60f60`, `0x00d64ef4` | `ebffff57`, `ebffef72` | `bl cc_ram_chroma` | `+0x110 = 13`: four halfwords as u64; else stock `0x00d60cc4` |
| `0x00d677ac`, `0x00d677bc` | `ebe4a45c`, `ebe4a458` | `bl cc_ram_tone` | `+0x110 = 13`: nine head bytes and 28 curve halfwords; else stock `0x00690924` |

#### 8.2.4 Native cases on the RAM images

The generators take a RAM set: RC0 decodes with the manifest's publications applied in order (each must find its old word) and only the CC part of the runtime mapped (text 288 bytes, data 100 bytes; no installer or bank bytes). All pass: site cases 510/510 (three RAM sets x 160, plus 30 stock RAW references) and builder checks 136/136 (70 flash plus 3 x 22 RAM). Stock films col2 0..12 give block, record and final state byte-identical to the stock images; CC col2 13 matches the substitution oracle inside the colour, tone and chroma ranges and stock PROVIA outside them, presets 2 and 15, and the RAM CC block equals the flash CC block byte for byte (candidate `f76aaf5a…d5fc` preset 2, `ea5bf245…74d2` preset 15; synthetic `b31623dd…3b3d`, `4b827681…cec5`). Flash-mode cases are unchanged (322/322, 70/70).

#### 8.2.5 Emulator rehearsal (emulated, `--ram-push-e2e --classic-chrome`)

Receipts under `private-data/analysis/classic-chrome-patch/ram-push/`, RC0 DAT, `0xf7` NAND, from the e2e checkpoints. `e2e-run-1` to `-3` and `e2e-raw-run-1` ran before review with `--unreviewed-relink`; `e2e-reviewed-1` ran the reviewed path. `e2e-run-1` installed correctly and then failed at segment `cc-stock-menu` with `expected MenuState 1 272 3, got 1 272 2`: the trajectory expected the HIGHLIGHT TONE list (menu 272) to open on row 3, and the camera opens it on row 2, its stored setting. The trajectory now expects row 2; runs 2 and 3 passed every segment (review note N5).

1. **Install: pass.** Allocation `0x1560` at `0x00a7e8c8`, relink, push: mailbox ABI 3, status 0, 131 validated, 84 setup and 47 activation stored, peer report 131 lines, every new word read back.
2. **Fetch watch: fetches recorded.** From attach to install with the camera idle at live view, the settings converter entry `0x00736a74` executed 18 times (run 1) and 13 times (RAW run), r0 = 0; no other entry word and no hook site was entered. The idle path therefore does reach a rewritten entry; the single-word swap inside the masked window is what makes this safe: a task entering after the store takes the trampoline, one already past the entry word runs the unchanged stock body.
3. **Menus: pass.** FILM SIMULATION lists eight rows, row 7 draws `CLASSIC CHROME` with an empty icon cell; committing it sets `0x009568b8 = 0x0d`, settings object `+0x3c = 0x0d`, the film nibble `0x009c28a6 = 0xd0` and `0x016995e0 = 0x0d`; reopening lands on row 7; the stock HIGHLIGHT TONE segment matches. While rows were highlighted the emulator entered the marker entry and the colour, tone and chroma hooks.
4. **RAW conversion: blocked by an emulator model gap.** With a virtual card holding the owner's `DSCF3095.RAF`, PLAY showed a busy indicator, then the emulator stopped strictly on an unknown MMIO write to `0xfff63118` (PC `0x0065a4a4`); PLAYBACK > RAW CONVERSION was not reached. Not a payload result; the playback device model needs that register before this step can run.
5. **Unload and cold restart: pass.** With PROVIA selected, `ram-unload` restored 131/131 with old words verified, the list shows seven rows, and a cold restart read every CC word as its old value.
6. **Reviewed relink: pass** (`e2e-reviewed-1`, 2026-10-19, after the re-pin, no `--unreviewed-relink`; receipt `e2e-reviewed-1/e2e-segments.json`). From the attach checkpoint: `ram-alloc` at `0x00a7e8c8` (5472 bytes, heap break `0x00a83000` before and after); relink mode `reviewed` against `reviewedCcContract`, runtime `e01952a6…05eb`; `ram-push` of 131 publications with install mailbox ABI 3, status 0, validated 131, setup 84, activation 47, peer lines 131, CPU0 park and ack spins 7847 and 27, CPU1 10434, bound `0x80000`; every CC menu segment matched; `ram-unload` rollback validated 131, restored 131, status 0; cold restart read 131 words, all old. The observing after-unload segment recorded one step at `[1,254,0]` where its trajectory lists `[1,254,6]`, the same difference `e2e-run-3` recorded.

#### 8.2.6 Review state and what remains

The CC payload is H2 new mechanism under spec 07 section 3.2 (13 mechanism words redirect stock control flow, all publications outside the allocation, new contract and ABI). Review CcRamPushReview (`private-data/analysis/ram-push-review/review-cc-ram.md`) approved it at `b4e1448` with two must-fix items, both closed: M1, the camera's reviewed CC path had never run (closed by `//transport:ram-push-cc-reviewed-build-check` and the reviewed e2e of 8.2.5); M2, this section's account of the review record (corrected in 8.2.2). Its notes N1 and N2 are operating rules in 8.4.8, N3 is in 8.2.2, N4 is 8.4.5 and N5 is in 8.2.5. Open: RAW conversion with the payload resident (emulator model gap above; on the body, 8.4.4 U5-U6); spin counts near 131 publications on the body.

### 8.3 Version gate

Flash: the extension manifest's base SHA-256 and version words, each patch set's `target.firmware_sha256` and `decode_sha256`, the current-word check of every write and the manifest's `output_sha256` (byte-read in `BuildExtension.hs` and `ApplyPatchSet.hs`); the container replaces code and data together, so no runtime check is needed. RAM: the pre-push read of every old word (8.4) and the installer's masked old-word validation of all 131 publications before any store. Any other firmware fails both.

### 8.4 Camera session script: first Classic Chrome RAM push

A script, not a run, modelled on [the bank RAM-push session script](ram-push-session-script.md). Nothing here authorizes a camera operation; the run waits for the review of 8.2.6 and the owner's go (8.4.9). Record every item in `private-data/sessions/<date>-cc-ram-push/notes.md`; any item not done is a stop.

#### 8.4.1 Before the session

1. **Host:** on the session commit, `devenv shell -- bazel test //transport:ram-push-tests //transport:cli-rejections //host/firmware:classic-chrome-ram-tests //transport:ram-push-cc-review-bytes //transport:ram-push-cc-review-verdicts //transport:ram-push-cc-camera-pins` passes, and `devenv shell -- bazel run //transport:ram-push-cc-reviewed-build-check` exits 0 after printing `reviewed CC contract accepted at 0xc20000`, `reviewed CC contract accepted at 0xc21f88` and a `refused as expected` line for each of its ten perturbation cases. Rehearsals exit 0 with these final lines: `rehearse ram-alloc 0x1560` prints `rehearsal: ram-alloc; no native provider, USB enumeration or camera execution;allocation-bytes=5472`; `rehearse read-config "$PWD/private-data/read-plans/gate-byte.json"` prints `rehearsed read-config: 4 windows`; `rehearse read-ram "$PWD/private-data/read-plans/cc-ram-push-prepush.json"` prints `rehearsed read-ram: 51 read requests; no write sent`; `rehearse read-ram "$PWD/private-data/read-plans/cc-ram-push-post-reboot.json"` prints `rehearsed read-ram: 42 read requests; no write sent` (all observed on `b4e1448`, 2026-10-18).
2. **Output paths** must not exist: `private-data/sessions/xm1-cc-ram-alloc-1`, `private-data/analysis/classic-chrome-patch/ram-push/operator-linked-1`, `private-data/sessions/xm1-cc-ram-push-1`. An existing OUTDIR refuses before USB.
3. **Body:** the RC0 body (seven film rows), ordinary boot with no buttons held, stored `0xf7 = 0x01` (it enumerates `04cb:ff80` on attach), USB attached and left attached until 8.4.7 ends, exactly one `04cb:ff80` device on the bus. No `commit-f7-*` command is part of this session.
4. **Power:** a freshly charged battery showing full, or the mains-backed dummy supply ([install-procedure.md section 5](install-procedure.md#5-power-arrangements)). Reserve two uninterrupted hours of daylight: 8.4.2a and 8.4.4 are shot outdoors or by a window.
5. **Card and equipment:** an empty camera-formatted card with room for 40 FINE+RAW frames; the ColorChecker Classic, the tripod and the lens of [the model report 8.1](classic-chrome-model.md#81-checklist). The laptop sits by the tripod so the cable stays attached from 8.4.2 to 8.4.7.
6. **Pre-session record, photographed:** SHOOTING MENU > FILM SIMULATION (seven rows, a stock row selected), the Q film tile, the C slot values (dial to C, read, dial back; no CUSTOM SET or CUSTOM RESET), DR, WB, COLOR, HIGHLIGHT, SHADOW, SHARPNESS, NR, image quality. These are the owner's settings to restore after the session.

#### 8.4.2 Model reads before the push (standing read grant)

1. `devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/gate-byte.json"`: product `X-M1`, `0xf7` nonzero.
2. `devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/cc-ram-push-prepush.json"`: one pass, 51 requests, about 7.9 s plus overhead. The model decodes every CC publication address and requires its RC0 `old` word (131 words, including the 128 words never read on the body before), service mode 3, table `0x015c22a0`, slot `0x00dc7874`, and the route's reviewer windows at their recorded values (queue-size word `0x40a0fbac = 0x24`, prehook `0x4089ea94 = 0x00726478`, trace state, execcpu rows). Any mismatch stops the session before any write (S-PREREAD).

#### 8.4.2a Before-set on RC0

Runs after 8.4.2 and before L1, with the cable attached and nothing pushed; the body is RC0. The RC0 frames and conversions here are both the `e_model` inputs of [the model report 8.2](classic-chrome-model.md#82-analysis) and the RC0 before-set for U6 (spec 19.5.6 item 4); a push leaves no trace after unload or power-off, so no separate pre-install photo session is needed.

1. Set the camera settings of the model report 8.1 (FINE+RAW, L 3:2, ISO 200, DR100, WB Daylight R0 B0, COLOR, SHARPNESS, HIGHLIGHT, SHADOW and NR 0, M exposure, 2 s self-timer, flash off) and photograph them as the session settings record.
2. Chart in daylight on the tripod, as 8.1 describes: PROVIA/STANDARD at 0 EV, then -2, -1 and +1 EV by shutter speed only; write down the speeds.
3. Without moving the camera: PRO Neg Std at the same four exposures, then PRO Neg Hi at the same four.
4. One natural scene on the tripod at one exposure: PROVIA, then PRO Neg Std, then PRO Neg Hi.
5. Move the camera back to the chart framing of item 2 and leave it untouched until U3 is shot.
6. In-camera RAW CONVERSION of the 0 EV PROVIA chart RAF, three times, every other conversion setting as shot: to PROVIA, to Velvia, to ASTIA. Note the three file names; the PROVIA conversion is the U6 reference.
7. Select PROVIA/STANDARD in SHOOTING MENU > FILM SIMULATION and return to the shooting screen with no menu open.

Further material of the model report 8.1 (more natural scenes, conversions of their RAFs) is optional and may be shot here in the same way. Any freeze, error dialog or frame not written is S-UNEXPECTED; nothing has been pushed, so the session ends with an orderly power-off.

#### 8.4.3 Load commands

Run from the repository root in one shell, no power-off between L1 and L4, camera idle at the shooting screen with no menu open, no shot being written and no card activity. Each live command first prints `output: <OUTDIR>`; a stop prints one `camera:` line on stderr and exits nonzero.

```sh
ALLOC_OUT="$PWD/private-data/sessions/xm1-cc-ram-alloc-1"
LINK_OUT="$PWD/private-data/analysis/classic-chrome-patch/ram-push/operator-linked-1"
PUSH_OUT="$PWD/private-data/sessions/xm1-cc-ram-push-1"
SIZE=0x1560
devenv shell -- bazel run //transport:camera -- ram-alloc "$SIZE" "$ALLOC_OUT"
BASE="$(devenv shell -- jq -r .base "$ALLOC_OUT/ram-alloc-state.json")"
devenv shell -- bazel run //transport:camera -- ram-relink "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT"
devenv shell -- bazel run //transport:camera -- ram-push "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT/ram-push-manifest.json" "$PUSH_OUT"
```

| Step | Command | Camera effect | Expected and record |
| --- | --- | --- | --- |
| L0 | the assignments; the owner confirms in chat that a stock film row is selected and the dial is not at C | none | `echo "$SIZE"` prints `0x1560` |
| L1 | `ram-alloc` | allocates 5,472 bytes, peer-core range maintenance, restores the callback slot | `ram-alloc: retained;state=…/xm1-cc-ram-alloc-1/ram-alloc-state.json;relink before ram-push`. Record `jq .` of the state: `kind` `xm1-ram-alloc-state`, `size` 5472, `product` `X-M1`, `config_f7` nonzero, `cpu0` `0x00004003`, `table_pointer` `0x015c22a0`, `slot_old` `0x00dc7874`, `base` nonzero and 8-byte aligned |
| L2 | `BASE=…` | none | `echo "$BASE"`: `0x` plus eight hex digits, last digit `0` or `8` |
| L3 | `ram-relink` | none (host build with the pinned CC relinker) | `ram-relink: manifest=…/operator-linked-1/ram-push-manifest.json`. Record `sha256sum "$LINK_OUT"/runtime.bin "$LINK_OUT"/ram-push-manifest.json`; the manifest must name 131 publications and the contract digest of 8.4.9 |
| L4 | `ram-push` | runtime upload with readback, INSTALL rendezvous, 131 stores, slot restore, readback | `ram-push: installed;state=…/xm1-cc-ram-push-1/ram-push-state.json;power-off clears RAM`. The camera already required: mailbox magic `0x31504d52`, ABI 3, status 0, validated 131, setup 84, activation 47, restored 0, service core `0x4003`, peer status 0 with 131 lines, park and ack spins below the bound `0x80000`. Note the wall time and paste all output |

#### 8.4.4 Owner checks with the payload resident

| Code | Owner action | Must show |
| --- | --- | --- |
| U1 | SHOOTING MENU > FILM SIMULATION | eight rows; the last draws `CLASSIC CHROME` with no icon; the seven RC0 rows unchanged |
| U2 | Select CLASSIC CHROME, leave the menu, reopen FILM SIMULATION | the cursor sits on CLASSIC CHROME; live view looks like PROVIA (known limitation) |
| U3 | Camera untouched since 8.4.2a item 5, the chart at the 0 EV shutter speed: shoot FINE+RAW with CLASSIC CHROME, then select PROVIA and shoot again, then select CLASSIC CHROME again | both frames written normally; playback shows the CC frame different from the PROVIA frame |
| U4 | Model, after the session from the card copy: `exiftool -FilmMode` on both JPEGs | CC frame `Classic Chrome`, PROVIA frame `F0/Standard (Provia)` |
| U5 | PLAYBACK, open the 0 EV PROVIA chart RAF of 8.4.2a item 2, RAW CONVERSION > FILM SIMULATION | six rows, the last `CLASSIC CHROME`; this path was never executed in the emulator (8.2.5 step 4) |
| U6 | Convert that RAF to CLASSIC CHROME, then the same RAF to PROVIA (all other conversion settings as shot) | both JPEGs written; model, after the session: `classic-chrome-model photo-identical` passes on this PROVIA conversion against the 8.4.2a item 6 PROVIA conversion (decoded Y, Cb and Cr planes identical); the CC conversion's FilmMode is `Classic Chrome`. `exiftool` differences between the two PROVIA conversions are recorded as information only |
| U7 | Optional C check: dial to C, open FILM SIMULATION, select CLASSIC CHROME, shoot one frame, dial back | the C frame is CC. Do not run CUSTOM SET with CLASSIC CHROME selected: it writes area 3 persistently |

Any stop, freeze, error dialog, a stock row rendering differently, or a hang at U5/U6 is S-UNEXPECTED.

#### 8.4.5 Stored-CC fallback: not in the first session

The first session ends with a stock film selected before `ram-unload` and power-off (8.4.6), so no film code `0xd` is stored on the RC0 body. Checking that a stored `0xd` falls back to PROVIA on RC0 is a later, separately named option, "stored-CC fallback check", outside this go. It powers off with CC selected and the payload resident, which persists film code `0xd` in the RC0 ordinary block at orderly power-off, a value no RC0 menu produces; this is the class of state behind E-SAVE-HANG-ASPECT-150, and the PROVIA fallback rests on static reading (`FUN_00631a00` returns 0 for an unmatched code, review N1 of the flash review, section 4.9). If the owner names that option in a later go, it states the persistent effect and the recovery: on RC0, select a stock row in FILM SIMULATION, then an orderly power-off (review note N4).

#### 8.4.6 Leaving the payload

| Code | Action | Must show |
| --- | --- | --- |
| E1 | Owner: select PROVIA/STANDARD in SHOOTING MENU > FILM SIMULATION | that row under the cursor |
| E2 | Owner: dial to C; if FILM SIMULATION shows CLASSIC CHROME there (U7), select the stock row recorded for C; dial back. No CUSTOM SET | C shows its recorded film |
| E3 | Owner: back to the shooting screen, no menu, no card activity; confirm in chat | none |
| E4 | Model: `devenv shell -- bazel run //transport:camera -- ram-unload "$PUSH_OUT/ram-push-state.json"` | `ram-unload: rollback verified; allocation retained until power-off`; the camera required validated 131 and restored 131 |
| E5 | Owner: SHOOTING MENU > FILM SIMULATION, RAW CONVERSION menu if convenient | seven rows; RAW CONVERSION five rows |
| E6 | Owner: power off normally | none |

#### 8.4.7 Reboot check

Power on normally, same card, cable attached. B1 normal start-up and live view; B2 FILM SIMULATION seven rows with the E1 row selected, Q five rows; B3 watched settings equal the 8.4.2a item 1 record, C values equal the 8.4.1 item 6 record; B4 a chart shot and playback normal. B5 model: `devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/cc-ram-push-post-reboot.json"` (42 requests, about 6.5 s): all 131 publication words equal their `old` values and slot `0x015c22ac = 0x00dc7874`. Stored `0xf7 = 0x01` is unchanged by this session. After B5 the owner may restore the 8.4.1 item 6 settings.

#### 8.4.8 Stop rules

Every stop ends camera operations for the session; nothing is retried; a new attempt needs a new go. Power-off is the recovery for every outcome: the allocation, runtime, callback retarget and all publications are RAM state.

Operating rules. Run L1, L4 and E4 only with the camera idle at the shooting screen (review N1): the 131 stores land in one masked window, but a task preempted while walking the film labels with the old base `0x0061c380` in a register would resume over the moved terminator and RAW labels. Never install CC while the bank+D3 payload is resident, or the reverse (review N2): the two publish 95 of the same addresses, so the second push refuses on its old-word check, and the session script has no state for that pair.

| Code | Seen | Action |
| --- | --- | --- |
| S-PREREAD | 8.4.2 mismatch, refusal or disconnect | no write was sent; stop |
| S-HOST | L0 or L3 fails | before L1 nothing to undo; after L1 run `ram-free "$ALLOC_OUT/ram-alloc-state.json"` (expect `ram-free: allocation freed`), power off, 8.4.7 |
| S-ALLOC-REFUSED / S-ALLOC-UNCERTAIN | `ram-alloc refused` / `ram-alloc uncertain …;send nothing further and power off` | refused: nothing to free; uncertain: mandatory power-off, 8.4.7 |
| S-PUSH-REFUSED | `ram-push refused: …` before upload (manifest, runtime, pins, base, size, product, slot, old word) | `ram-free`, power off, 8.4.7; if the reason is the allocation guard, power off without `ram-free` |
| S-PUSH-REFUSED-PARK | `installer refused before publication: CPU1 park rendezvous timed out` | power off without `ram-free`, 8.4.7 |
| S-PUSH-UNCERTAIN | `ram-push uncertain: …;send nothing further and power off` (including completion timeout, status -8) | mandatory power-off now, no button first, 8.4.7 |
| S-UNEXPECTED | anything in 8.4.4 differing from its row | stop using the camera; photograph; if the menu responds select the recorded stock film; no `ram-unload` unless the model asks after reading the journals; power off, 8.4.7 |
| S-UNLOAD-REFUSED / -UNCERTAIN | `ram-unload refused` / `ram-unload uncertain` | power off, 8.4.7 (uncertain: mandatory, now) |
| S-USB | cable pulled, host crash, or a command without a final line for 10 minutes | treat as uncertain: power off, 8.4.7 |
| S-HUNG | no response to the power switch within 60 s | remove the battery for 10 s, reinsert, 8.4.7 |

#### 8.4.9 What the go names

The owner's H2 go names: the operation (ordinary-boot RAM push of the Classic Chrome payload: the 8.4.2a before-set on RC0, `ram-alloc 0x1560`, `ram-relink` at the returned base, `ram-push` of 131 publications, the 8.4.4 checks, `ram-unload`, power-off and the 8.4.7 read, with `ram-free` as the not-installed contingency); the session commit, whose CC files equal reviewed `b4e1448` (review CcRamPushReview); the publication contract digest `0030860616947ebaa1cade89c36ca40d43ffaaf610258400220b1879cf4432d9`; and the reference runtimes the pinned relinker reproduces, `ce050d3a1ac978112b2e58b02e11a26d34d5cfa1d9cb7748f59b26d97814ea17` at `0x00c20000` and `c5f6241f61a3165efe913c28f5f19866518689846c646c903784cebea6d559bb` at `0x00c21f88` (data block `833b6452…2774`). The session's own runtime depends on the base `ram-alloc` returns; the camera binary rebuilds it with the pinned relinker and requires byte equality, and L3 records its SHA-256 before L4 runs. The stored-CC fallback check (8.4.5) is outside this go.

Before the go, the owner may look at the menus in the viewer (`//emulation:xm1-viewer`, SHOOTING MENU > FILM SIMULATION, on the flash candidate as in 11.3 or on the RAM push with `--ram-push-e2e --classic-chrome`); this check is optional.

### 8.5 Result of the first RAM-push session (2026-10-21)

The 8.4 script ran on the RC0 body at `ace9320` under the owner's go and passed with no stop (E-CC-RAM-PUSH-SESSION-150, hardware-observed). The pre-push read matched all 131 old words and the reviewer windows; `ram-push` installed in 64.5 s with INSTALL status 0, validated 131, setup 84, activation 47 and peer status 0; `ram-unload` verified rollback with restored 131, and the post-reboot read found every old word and the callback slot restored.

- Label rendering (open question 2, shooting-menu list only): FILM SIMULATION shows an eighth row with an empty icon cell and `CLASSIC CHROME` drawn as text, and the cursor stays on it after reopening. This is the first hardware rendering of the `0x83e` record. The Q tile was not observed.
- RAW conversion (open question 7): with the payload resident, RAW CONVERSION offers six rows with CLASSIC CHROME last, converts to CC, and its PROVIA conversion of a before-set RAF is plane-identical to the RC0 before-set conversion (U6, item 4).
- Capture: a shot frame and a conversion both carry FilmMode `Classic Chrome` (U4); on flat blocks of a pixel-aligned conversion pair, CC renders chromatic colours at about 0.6 times PROVIA's chroma and slightly lighter, with live view staying PROVIA-like as designed.
- Deviations: no ColorChecker or tripod, incandescent light at ISO 4000 with Incandescent WB.
- Implementation (host analysis of the card copy): on the pixel-aligned RAW conversions the body renders the data block's matrix, chroma 2x2 and tone record over PROVIA's selectors as the model's candidate stage predicts, and the look moves from PROVIA in the direction of Fujifilm's Classic Chrome LUT ([model report 8.4 and 8.5](classic-chrome-model.md#84-implementation-check-on-the-raw-conversion-set)).

Open: the look acceptance (spec 19.5.6 items 1-3) and the ColorChecker `e_model`, which need a chart session at low ISO in daylight or a fixed-WB light; the Q tile and C slot (U7); the stored-CC fallback (8.4.5); the flash decision. The session ended with stored `0xf7` returned to 0 (E-STORED-F7-RESTORE-RUN-150), so the next RAM session first needs `commit-f7-enable` in a jig session.

## 9. Verification results (2026-10-18, host and emulated)

All runs use the pinned inputs; patched images come from `build-classic-chrome` on the synthetic set (`host/firmware/classic-chrome-synthetic.json`, SHA-256 `8dfa7865…57ab`) and on cc-model's candidate (`candidate.json` SHA-256 `188c521d…3e92`; data block `833b6452…2774`, identical to the block from its earlier revision `867083e1…2de3`).

### 9.1 Develop builder (native harness)

`//emulation:classic-chrome-builder-cases` executes the stock still develop path on the X-M1 1.50 bytes: stage a runs `FUN_00d77abc`, `FUN_00d791b0`, `FUN_00d79408` (request[9] = col2) and the preset expander `FUN_00d77a48`; stage b runs `FUN_00d5da2c` into the hardware parameter block, over the fallback adjustment image at `0x41acd800` and the loader helpers. 70/70 checks pass for both parameter sets, with presets 2 and 15 (the second covers the second colour, tone and chroma instances):

| Check | Result |
| --- | --- |
| Stock films col2 0..12 | patched images give block, record and full final state byte-identical to the stock images' run |
| Census: stock images with `+0x110 = 0xd` injected, col2 0..12 | block unchanged for every code except code 2 (B&W, colour range only, the filter branch the census names); record differs only at `+0x110` |
| Stock images, col2 13 | the PROVIA (col2 0) block |
| CC, col2 13 on patched images | marker `+0x110 = 0xd`; outside the colour, tone and chroma ranges the block equals the stock PROVIA block; inside them it equals a substitution oracle (stock images with colour id 1, chroma id 1 and tone key 1 overwritten by the parameter set) byte for byte |

The CC block differs from PROVIA at block `+0xd4..+0xe4` (colour), `+0xf2..+0x124` (tone), `+0xf4a..+0xf51` (chroma) and, with preset 15, `+0x109c..+0x10ac`, `+0x10ba..+0x10ec`, `+0x10f8..+0x10ff`. To run the stock path, the runner admitted ARMv7-R forms it uses (UMULL/SMULL without S, MLS, SXTH, register-offset LDRSH/LDRSB and LDRD/STRD, STMDB with the base lowest in the list), each with a fixture and refusal cases (emulation/README.md, admission entry of 2026-10-18). `//emulation:native-runner-tests` passes.

### 9.2 Patch sites (native harness)

`//emulation:classic-chrome-site-cases`: 322/322 cases pass on stock and patched images. Converters (identity 0..12, 13 only on patched); dispatch and marker (code table unchanged, `+0x110 = 0xd` only for col2 13); FilmMode (`0x600` only for col2 13) and Saturation (col2 13 equals col2 0) in gated and ungated modes; the three stubs with marker 13 (data block in stock output format) and 0 (stock readers' output); RAW sites (only row 10 to colour 8, colour 8 to row 10, field 8 to request col2 13 change); offer list (stock 5/5, RC0 7/7, patched 8/8 in mode 0; every state 3 in PROVIA-only mode 5); label accessor (stock refuses `0x83e`, patched returns the CLASSIC CHROME record; `0x83c`, `0x83d`, `0x83f` unchanged). The five hook branch words are exercised by 9.1.

### 9.3 Full-system emulator

`//emulation:flash-boot-check` on the candidate DAT `16c830ad…2c89` passes (receipt `private-data/flash-boot-checks/cc-candidate-1/flash-boot-receipt.json`; boot at 80,000,280 instructions, SHOOTING MENU at 450,017,580). From that boot snapshot, in one emulator slot, the probe walked SHOOTING MENU to FILM SIMULATION: the list opens with eight rows, the cursor reaches row token 2 at index 7, and that row draws `CLASSIC CHROME` with an empty icon cell after SEPIA, PRO Neg Std and PRO Neg Hi. Committing it sets the live film cell `0x009568b8` to `0xd` and the ordinary film nibble (`0x0096ccee` high nibble) to `0xd`, and reopening the list lands on that row. The emulator models no ISP, so rendering is shown by 9.1, not here.

### 9.4 RC0 regression

- `extensions/rc0.json` still builds to `acb0fbbb…ca8` with the extended tooling.
- The CC image 4 differs from the installed RC0 image 4 at 60 words, all CC-group writes (converters, conversion row, availability row, three labels pointers, two RAW pointers); RC0's seven values pairs and seven labels quadruples are carried byte for byte into the new prefix (byte-compared).
- Native: RC0 arrays still offer 7/7 in mode 0 (9.2); stock films' develop blocks are unchanged (9.1).

### 9.5 Container

`rebuild-update verify`: 42/42 contract checks. `rebuild-update diff` against stock and against RC0: carried parts (identity records, selector, fill, both restore lists) unchanged; changed parts are the segment-table load and size words, image-4 header and stream, the gap after image 4, the image-5 stream, the plain window, 18 bytes of the image-2 extent and the trailer image words. Main checksum `0x8f09f8c9`.

## 10. Interactions

- **D3** (reports/recipe-patch-d3.md, not installed) is incompatible as designed: it rewrites rows 2 and 4 to codes `0xd`/`0xe` and fills the whole window. CC now owns row 2, code `0xd` and 484 of the window's 512 bytes. D3 must be re-planned around them (codes `0xe`/`0xf`, row 4 plus a relocated row, space outside the window). Its source states that `0x83e` stays the stock empty record, which is false on a CC body.
- **RC0b / FILMSIM-PATCH-02** (Q and bracket arrays in `[0x0061c240, 0x0061c340)`) competes for the same window.
- **Recipe bank** uses ids `0x83f`..`0x849`, the two RC0-orphaned arrays for its boot stub and a relocated label table that must be seeded from the CC image 2. Its gate list (recipe-design 17.6) must add `0x01164308`, `0x01178508` and `0x011787b4` (section 4.7), independently of CC.

## 11. Changed-byte explanation and reviewer packet

### 11.1 Candidate

| Item | Value |
| --- | --- |
| Candidate DAT | `private-data/analysis/classic-chrome-patch/candidate-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89` |
| Pinned manifest | `private-data/analysis/classic-chrome-patch/candidate-pinned/classic-chrome.json`; `build-extension` without `--unpinned` reproduces the digest |
| Base | stock 1.50 `43d67afa…1490`; the image carries RC0's writes, so it replaces the installed RC0 container `acb0fbbb…ca8` |
| Parameters | `candidate.json` `188c521d…3e92`, data block `833b6452…2774` |
| Stubs | capture blob `0c3cac9f…8d49`, settings blob `c4e69037…6800` (`//host/firmware:classic-chrome-stubs`) |
| Revert | first select a stock film in SHOOTING MENU > FILM SIMULATION and, if CC is stored in C, register a stock film in C (CUSTOM SET) or run CUSTOM RESET; then reinstall RC0 `acb0fbbb…ca8` (or stock) through the same update route. With `0xd` left in the film nibble or the C slot, RC0 and stock find no conversion row (`FUN_00631a00` sets `0x0089edc8` = 1 and returns 0, `0x00631ab8`-`0x00631ac8`), so the body shows and renders PROVIA (review N1, byte-read) |

### 11.2 Every changed byte

The per-word listing (address, current, new, group, role) is `candidate-pinned/changed-words.md` and `.json`; it stays private because the prefix blob carries the data block. Summary by group, so CC is reviewable and revertible apart from RC0:

| Image | Group | Writes | Changed bytes | Content |
| --- | --- | ---: | ---: | --- |
| image 4 | RC0 | 20 | 32 | RC0's 14 availability cells and 6 node pointer words (labels destination `0x0061c2b0` instead of `0x0061c380`) |
| image 4 | CC | 19 | 401 | prefix (data block, RAW values, shooting labels, shooting values, RAW labels), the two converter blobs, conversion row 2, key `0xd` row (8 words), two RAW node words |
| image 5 | CC | 45 | 133 | marker setter (4), five hook `bl` words, FilmMode (6), Saturation (9), RAW commit (4) and highlight (17) words |
| plain | CC | 41 | 79 | 25 label gates, 16 RAW request words |
| image 2 | CC | 1 | 18 | the `0x83e` record |

Container-level changes follow from these: image-4 load `0x0061c340` to `0x0061c200` and its size words, re-encoded image-4 and image-5 records, the main checksum. Every write's current bytes were checked against the pinned decodes by `build-classic-chrome` and again by `apply-patch-set`, `write-record` and the plain step.

### 11.3 Operation class and gates (spec 07 section 3)

The install is H3 (firmware update). The gates below were cleared for `16c830ad…2c89`, which the owner will not flash: the first flash is a later candidate that adds a CC icon, and section 12.2 lists the gates it must clear on its own digest. Before a run:

1. One independent review of the new mechanism bytes (stub source and linked blobs, the five hook words, the marker setter, the converters, the FilmMode, Saturation and RAW code words, the gate edits, the image-2 record) and of the data words and container diff. Reviewer inputs: this report, `host/firmware/classic-chrome-stubs.S`/`.ld`, `host/FujiAF/ClassicChrome.hs`, the private `candidate-pinned/` directory and the case outputs under `private-data/analysis/x-m1-1.50/native-case/classic-chrome/`.
2. `//emulation:flash-boot-check` on the exact image: passed (9.3). Rerun on main `0f0b694` (2026-10-22, receipt `private-data/flash-boot-checks/cc-flash-prep-1/flash-boot-receipt.json`): passed with the same milestones (boot 80,000,280, SHOOTING MENU 450,017,580) and final frame `7e8e4479…` as `cc-candidate-1` and `cc-review-1`; the staged emulator tree differs from the review's (USB endpoint, cable-input and mode-dial changes), the native engine does not.
3. The owner's visual check: satisfied on hardware by the RAM-push session (E-CC-RAM-PUSH-SESSION-150, 8.5): the owner's photographs of SHOOTING MENU > FILM SIMULATION show the eighth row, an empty icon cell with `CLASSIC CHROME` drawn as text, from the same data words this image carries (8.2.3). The viewer run is not needed. For the icon candidate, its own RAM-push camera test comes before the flash.
4. The owner's per-run go naming the operation (firmware update) and the digest of the image flashed.
5. Boot-path note: the image-4 load `0x0061c200` is 64-byte aligned like RC0's and decodes in the emulator's boot path; the hardware decompressor has not yet decoded this base.

Install session: section 12.2.

### 11.4 Acceptance after install (spec 19.4 and 19.5.6)

The before-set of the RAM session (8.4.2a), shot on the RC0 body with nothing pushed, is the RC0 reference for item 4.

1. Boot, menus, capture and playback normal; settings and C slot retained.
2. FILM SIMULATION shows eight rows with `CLASSIC CHROME` last; selectable in P/S/A/M/C and modes 0/`0x1f`; survives a power cycle.
3. A CC JPEG differs from PROVIA on a fixed scene; exiftool reads FilmMode `Classic Chrome`.
4. RAW CONVERSION offers CLASSIC CHROME; the fixed RAFs re-render in CC, and stock modes re-render byte-identical to the conversions made on the RC0 body in the photo session.
5. Live persistence and capture with CC stored (review M1): select CC in the shooting menu, shoot and review the frame; register CC in C and shoot at C; power off and on with CC stored in the shooting setting and in C, check both still show CC, then shoot again in each. Any stop, freeze, error dialog or stock-mode change ends the session (spec 07 section 3.3).
6. Known limitations: without the live-view fragment (`16c830ad…2c89`) live view previews CC as PROVIA and movie records PROVIA; with it both are CC (section 4.10). The Q cell shows PROVIA's glyph for a stored CC and does not offer CC; FILM 1-3 brackets cannot select CC; the CC row has no help line, and no icon without the icon change; CC's tone curve does not follow DR or filter selection (section 4.5), which for stills matches PROVIA's own behaviour.
7. Look acceptance (CIEDE2000 bounds, discrimination, neutrals, adjustments) per spec 19.5.6 with cc-model's thresholds.

## 12. Open questions and the flash install session

### 12.1 Open questions

1. Live view: answered statically and in the emulator by section 4.10 (the MOVDC builders on record `0x01699d8c`); open are the ISP's use of block `0x01699fd0` for the panel, until the camera test, and what the `FUN_00f387c0` → `FUN_00eef1e4` path drives.
2. Rendering of the `0x83e` name on hardware, the shooting-menu tile and the RAW list; tolerance of id 0 in fields 0-2 by every labels consumer on the body.
3. RAW conversion: the binding of field `0x00954d80` to the record at `*0x014ea86c` field `+8` (static-inference), and the default row a CC RAF opens with.
4. Maker-note field `+0x108` (`FUN_00ea9c30`) tag identity; external interpretation of `+0x110 = 0xd` in the debug dumps.
5. Census limits (review N6): every census here decodes A32 only and follows no pointer loaded from heap or reached through an indirect call; Thumb units and indirect consumers of the film value or the develop record are outside it.
6. Window `[0x0061c200, 0x0061c340)` (section 6.5). Answered for the emulated cold boot to SHOOTING MENU and FILM SIMULATION on stock, RC0 and CC: the only writers are the table-block copy and, on CC, the image-4 decode; the census finds no reference outside the patch's own readers and the decode descriptor; the base meets every rule the load path imposes. Open: no hardware decode with a first destination of `0x0061c200`, the hardware engine's alignment rule beyond 64 bytes, the unexecuted capture, develop, playback and RAW-conversion paths, and an independent placement review of the range.
7. RAM push (section 8.2): RAW conversion with the payload resident (not reached in the emulator, first observed at 8.4.4 U5-U6); idle-path entry of the rewritten converter word on silicon; spin counts and wall time at 131 publications; the 128 CC old words first read at 8.4.2.
8. Whether CC needs a non-PROVIA luma band, chroma gain or tone scalar.
9. Scope of the review approval (review N5): it binds DAT `16c830ad…2c89` with data block `833b6452…2774`. A rebuilt parameter set changes only `[0x0061c204, 0x0061c264)` plus the container encoding; diff the new image against this one to confirm nothing else changed, then rerun the site cases, builder cases and flash-boot check on the new digest.
10. Prefix placement (review N3): no independent placement review of `[0x0061c200, 0x0061c340)` exists; the reviewer's reference census agrees with section 6.5 (only the node pointers and the stub `movw`/`movt` pairs name the range).
11. Film value 13 at unreached consumers (review M1, section 4.9). No reader was found for five copies of the film: the `MCST` dump `+0x100` (buffer `*(*(state+0xc)+0x50)`, written only when adjustment byte `0xa8` bit 7 is set), the record `+0x9c` that callback `FUN_00f2f56c` fills (imgdc message `0x3f3a`, whose only table entry `0x00ff2e78` does not invoke it), imgdc's copy byte `0x016f8394` (struct posted from `0x00f2d958`), `0x016d748c` (record published through `0x016b5458`) and the plan copies at `0x41717d54`/`0x4171814c` (`+0x200`). The census does not follow pointers that reach these buffers through heap or indirect calls. No capture or bracket was emulated, so the plan, slot, `0x00919288` and mailbox-48 stores were only read at 0.

### 12.2 Flash install session script

A script, not a run, modelled on [the install procedure](install-procedure.md) and [the RC0 install session](filmsim-install-session-script.md). Nothing here authorizes a camera operation; the install is H3 and runs only after the owner's per-run go of 12.2.9. Record every item in `private-data/sessions/<date>-cc-flash/notes.md`; any item not done is a stop.

The flash target is not `16c830ad…2c89`. The owner chose a later candidate that adds a CC icon (built on `agent/cc-icon`) as the first flash. The script takes two parameters that name that candidate: `IMAGE`, the absolute path of its pinned DAT, and `DIGEST`, its SHA-256. `16c830ad…2c89` serves as the baseline the parameters are checked against.

#### 12.2.1 The install bytes and their gates

`DIGEST` is the combined candidate `78f7526002608540a8a1c6957306f29760371cd5146d2455587a0900bacaa7ba` (CC, the icon, the live-view fragment and the frames-remaining fix; [the icon report](classic-chrome-icon.md) sections 8-11; it keeps the image-4 load at `0x0061c200`). Gates `DIGEST` must clear before the go:

1. Its pinned manifest rebuilds to `DIGEST` byte for byte on the session commit, as the baseline does below.
2. One independent review of the new bytes (the icon change, the live-view fragment of section 4.10 and the frames fix of section 4.11), scoped by [the icon report's reviewer packet](classic-chrome-icon.md#10-reviewer-packet) against the baseline: diff the two images and explain every byte that differs.
3. `//emulation:flash-boot-check -- --firmware "$IMAGE"` passes.
4. The RAM-push camera test of the combined payload has run on the body before the flash ([the icon report](classic-chrome-icon.md#11-abbreviated-camera-test) section 11): the owner has seen the icon in FILM SIMULATION, on the shooting-menu value and on the Q tile; with CLASSIC CHROME selected frames remaining equals PROVIA's and no MEMORY FULL appears; and live view looks muted, not like PROVIA, both while the CC row is highlighted in the menu and on the shooting screen after the menu closes. It is this candidate's visual check.
5. The 12.2.2 container check passes on `IMAGE`.
6. The open boot-path item stays open: the hardware decompressor has never decoded an image-4 load at `0x0061c200`; RC0's `0x0061c340` is the only modified base decoded on this body (11.3 item 5, 12.1 item 6). If the icon candidate moves the image-4 load, the move is a new base, also never decoded on hardware.

Baseline `16c830ad…2c89`:

| Item | Value | Evidence |
| --- | --- | --- |
| Image | `private-data/analysis/classic-chrome-patch/candidate-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89` | 11.1 |
| Reproduction on main `0f0b694` (2026-10-22) | `build-classic-chrome --output-sha256 16c830ad… candidate.json` writes parts equal to `candidate-pinned/` apart from `recorded_at`, with data block `833b6452…2774` byte-equal; `build-extension` on the pinned manifest and the stock DAT `43d67afa…1490` prints PASS with the pinned digest, and its output (`flash-prep-rebuild-1/`) is byte-identical to the image | byte-read |
| Sources since the review | The review ran at `agent/cc-patch` `73ba53b`, closing `9763602`. Unchanged on main: `classic-chrome-stubs.S`/`.ld`, `ClassicChrome.hs`, `BuildClassicChrome.hs`, `ApplyPatchSet.hs`, `LabelRecord.hs`, `extensions/`, `lz/`, `recovery/rc0-patch-set.json`, `native-runner.c`, `FlashBootCheck.hs`, `FlashBootRoute.hs`, the synthetic set and `ClassicChromeTests.hs`. Changed: `UpdateContainer.hs` adds a second segment-table layout (`RecorderCompact`, the recovery recorder container) and selects table offsets by layout, a stock-layout container keeping the stock offsets; `Host.hs` closes a duplicated descriptor on exception; the site and builder case generators gain the RAM image sets of 8.2.4; the retained upstream emulator gains USB endpoints, a cable input, a mode-dial input and an IRQ trace cursor. The byte-identical rebuild and the unchanged flash-boot frame show none of these reaches the image | byte-read |
| Review | Approved, final verdict for this digest (`private-data/analysis/classic-chrome-review/review.md`) | 11.3 item 1 |
| Flash-boot check | Passed on main `0f0b694`, receipt `private-data/flash-boot-checks/cc-flash-prep-1/flash-boot-receipt.json` | 11.3 item 2, emulated |
| Visual check | Satisfied on hardware by the RAM-push photographs, for this image's menu row | 11.3 item 3, E-CC-RAM-PUSH-SESSION-150 |

#### 12.2.2 What the update does to stored state

**Byte-read** on the baseline image: the factory template the refresh reads (raw DAT `0xe025c`, complemented, 270,336 bytes, `X-M1` at `+0x15c`, `55 aa` at `+0x202`) and both restore lists (raw `0x24a027c` and `0x24a0404`, 98 and 3 pairs, 1,487 bytes) are byte-identical in the stock, RC0 and baseline containers. The template holds `00` at `0x80`, `0x81`, `0xa2` and `0xf7`. The lists restore `0xa2` and not `0x80`, `0x81` or `0xf7`; their first pair `(0x0000, 0x80)` ends below `0x80`. The icon candidate must carry the same bytes; with `STOCK` the stock DAT, both commands print nothing on a pass:

```sh
cmp --ignore-initial=918108 --bytes=270336 "$STOCK" "$IMAGE"     # template, raw [0xe025c, 0x12225c)
cmp --ignore-initial=38404732 --bytes=416 "$STOCK" "$IMAGE"      # restore lists, raw [0x24a027c, 0x24a041c)
```

The path is S4 of [the install procedure](install-procedure.md#2-what-the-camera-does-when-inside-the-update): `0xdb91e0` snapshots `[0, 0x42000)`, refreshes it from the template, stamps `+0x1e8`/`+0x1e9` and `+0x204`/`+0x205`, restores the listed bytes and commits the area. It runs only when `0x014e3578 & 3` is nonzero; that flag's setter is unmapped.

**Hardware-observed** for the RC0 install (captures of E-PREINSTALL-CONFIG-150 and E-FILMSIM-READBACK-150, compared against the template on 2026-10-22): before and after, every byte of `[0, 0x42000)` outside the lists equals the template except the stamps `0xd8`, `0x1e8`, `0x1e9` and `0x4069`-`0x406e`; the update stamp moved `01 54` to `01 78`, so the refresh ran on the equal-version update; the 224 restore-covered body bytes held; area-2 bank 1 was unchanged and bank 0 gained only photo records. The official 1.00 to 1.50 update returned `0x80` from `0x22` and `0x81` from `0xff` to `0x00` (E-ADJ-GATE-BYTE-150).

Body now: window-0 captures since the f7 enable show `0x80`, `0x81` and `0xf7` as the only non-template bytes outside the lists and stamps, and `commit-f7-restore` left `0xf7 = 00`, `0x80 = 22`, `0x81 = 00` (E-STORED-F7-RESTORE-RUN-150; not reread in a later boot).

Expected after the update (**static-inference**): the executing updater is RC0's, whose image 5 holding `0xdb91e0` is stock, and with the check above passing the container parts the updater reads for S4 equal RC0's, so the RC0 outcome repeats. `0xf7` and `0x81` stay `00`, `0x80` returns from `0x22` to `0x00`, `0xa2` is restored at `00`, the stamps are rewritten with the values they hold, and every other byte of `[0, 0x42000)` already equals the template or is restored. Window 0 then equals the 2026-10-16 reference that `commit-f7-enable` checks against. If the refresh did not run, `0x80` stays `0x22`; the body has operated normally with either value. `[0x400000, 0x40fd00)` is outside the refresh. The mapped writer touches firmware blocks `[0, 0x126)`, the configuration area and the directory-journal pair, not the area-2 ordinary settings or the area-3 C slot; the C slot across an update has neither a byte read nor an owner report (E-FILMSIM-INSTALL-150 names manual settings, date, time and language). The owner should see the settings, C values, date, time and language as before, no setup menu, stored `0xf7 = 0` (ordinary boot enumerates PTP) and eight FILM SIMULATION rows.

#### 12.2.3 Delivery to the card

**Hardware-observed, 2026-10-22, read-only** (`private-data/sessions/20261022T060247Z-ptp-summary/`): `gphoto2 --summary` (gphoto2 2.5.32, libgphoto2 2.5.34) sent OpenSession, GetDeviceInfo twice, GetStorageIDs, GetObjectHandles, GetStorageInfo, 76 GetDevicePropDesc and CloseSession, nothing else. DeviceInfo lists SendObjectInfo `0x100c` and SendObject `0x100d`, but the only storage, `0x10000001` (DCIM layout, root holding only `DCIM`), reports Access Capability `0x0002`, read-only with object deletion. `gphoto2 --abilities` prints `File upload support: yes` from libgphoto2's static driver table, and kio reports the store `0444` and `DCIM` `0777` (its generic folder mode); neither reads the card's access. The camera does not offer PTP upload, and an upload attempt would be a write the device declares unsupported, so this script makes none. Whether the camera would refuse SendObjectInfo is untested. No other mapped route writes a file to the card in the slot, so delivery needs one card removal into a host card reader.

| Code | Action | Must show |
| --- | --- | --- |
| D1 | Camera off, USB unplugged. Remove the card and insert it in the host's reader | Card mounted read-write |
| D2 | `ls -la` the card root | `DCIM` with folder `963_FUJI` (the RAFs `DSCF3149`-`3163`, `3167`, `3168`, `3171` that A4 needs), no `FWUP*.DAT`, free space well above 38,405,148 bytes. Do not format the card: unlike the RC0 run, A4 needs these RAFs on it. The lookup opens `A:\FWUP0002.DAT` by name (install procedure section 1); that other files are ignored is **static-inference**, untested on this body |
| D3 | `cp --no-clobber "$IMAGE" "$CARD/FWUP0002.DAT" && sync` | Exit 0; the name exactly `FWUP0002.DAT` |
| D4 | Unmount, remove and reinsert the card in the reader, mount, then `ls -la "$CARD"` and `sha256sum "$CARD/FWUP0002.DAT"` | The size of `IMAGE` and `DIGEST`, read after a fresh mount so the bytes come from the card. Any other value: delete the copy and repeat D3-D4 |
| D5 | Unmount, eject, insert the card in the camera with the camera off; close the door | |

A partly written file cannot be picked up during the copy: the card is in the reader, and the camera opens the file only at a DISP/BACK power-on. A truncated or damaged copy fails D4. Inside the camera, a damaged file fails one of the three sums (status 5) before S1 (**byte-read**); the branch for a file shorter than its header declares is **unproved**. The cached-success byte `0x15c1e58` survives until power is removed, so after any aborted attempt switch off and cut the supply before another one (install procedure section 7).

#### 12.2.4 Before the session

| Code | Owner action | Must show |
| --- | --- | --- |
| P0 | Model: on the session commit, `sha256sum "$IMAGE"` and the 12.2.1 gates for `DIGEST`: rebuild, review, flash-boot receipt, the icon RAM-push test result and the 12.2.2 check | `DIGEST`; every gate recorded |
| P1 | Photograph the watched settings and the C (CUSTOM) values; the RAM session waived this | Record in the notes |
| P2 | Confirm the adjustment backup (E-ADJ-BACKUP-DURABLE-150, `c82ac937…199c`, two locations) | No new capture is needed: since that backup only `0x80`, `0x81` and `0xf7` changed (12.2.2) |
| P3 | SHOOTING MENU > FILM SIMULATION shows a stock film (PROVIA after the RAM session) and the C slot holds a stock film (review N1) | Stock film in both; RC0 cannot store CC, so this is a check |
| P4 | Dummy-battery supply on mains, connector secured, not on a shared switch; camera on a stable surface; lens mounted; USB unplugged | Install procedure section 5 |
| P5 | D1-D5 done | Card in the camera, door closed |
| P6 | Camera off. Start a video of the screen and a clock | |

#### 12.2.5 Camera acts

The screen order is the RC0 run's (E-FILMSIM-INSTALL-150, E-INSTALL-TIMING-150) and the vendor procedure; intermediate wording on this body is **unproved**. Do not advance an unfamiliar screen by guessing.

| Code | Owner action | Must show |
| --- | --- | --- |
| I1 | Hold DISP/BACK, switch on, release when the firmware screen appears | The firmware upgrade screen with body version 1.50 |
| I2 | Inspect before pressing anything | The update offered as 1.50 to 1.50, BODY route, no file error, low-battery or refusal. Anything else: abort (stop rules) |
| I3 | If item selection appears, select BODY and press MENU/OK once | BODY, both versions 1.50 |
| I4 | At FIRMWARE UPGRADE OK?, select OK and press MENU/OK once; at a battery-cover/power warning with BACK CANCEL, pause without pressing its OK | Last screen with a cancel |
| I5 | Press the final OK once, only under the go | PROCESSING with progress squares. This is the point of no return |
| I6 | Touch nothing: no button, card door, battery door, POWER switch, cable or supply | Progress; the RC0 install took 57.3 s from confirm to completion with a longest unchanged stretch of 13.7 s, which does not bound this image |
| I7 | Wait for the completion screen asking to turn the camera off | Completion; record the screen and elapsed time |
| I8 | Switch off as instructed and wait for shutdown. Leave the card in the camera | Ordinary shutdown |

`FWUP0002.DAT` stays on the card. It runs only at a DISP/BACK power-on; delete it with the reader at the next card removal.

#### 12.2.6 Stop rules

Every stop ends camera operations for the session; nothing is retried; a new attempt needs a new go.

- Before I5: a wrong version, model, route or file message, no equal-version offer, low or doubtful power, an unfamiliar screen: cancel where offered, switch off, cut the supply, report.
- During I6: never cut power, open a door or press a button, whatever the screen shows. A stalled or blank display is recorded, not interrupted.
- After I8, a body that does not boot or hangs: stop. Do not power-cycle repeatedly or follow generic battery-removal advice; keep the last display, the supply and card state, and the video (install procedure section 9 step 1).
- The setup menu, a lost setting or changed C values: stop for host analysis; do not re-enter settings to hide the difference.
- Seven rows instead of eight with normal operation: do not reflash; photograph the list and the mode; this is the placement-residual path of the RC0 script section 7.
- During acceptance: any freeze, error dialog, stock-mode change or unexpected behaviour (spec 07 section 3.3).

#### 12.2.7 Recovery

- Interruption residues and which a repeat repairs: [install procedure section 8](install-procedure.md#8-interruption-residues-and-which-are-recoverable-by-repeating-the-update). Between S1 and the end of S3 the body cannot boot and no repair is demonstrated; the owner accepted that residual on 2026-10-09 ([install procedure section 3](install-procedure.md#3-preconditions)).
- A decode failure of the new image-4 base at boot has the same symptom as that window and the same coverage.
- Coverage by failure class: [recovery status](recovery-status.md#failure-coverage-and-smallest-next-experiment). The v5 rollback is specific to the recorder incident's block; the firmware-derived RAM recovery followed by the ordinary updater is a host model only; SD-boot entry is the rapid power toggle with Fn and the inward sub-command-dial press (E-SDBOOT-ENTRY-TIMING-150) and needs a card change.
- Revert from a working CC body: 11.1 revert row (stock film in the shooting setting and in C, then RC0 `acb0fbbb…ca8` through this same route).

#### 12.2.8 Acceptance after install (11.4 without a chart)

| Code | Owner action | Must show |
| --- | --- | --- |
| A1 | Boot normally, no USB | No setup menu; date, time, language, the P1 settings and C values unchanged; capture and playback work. BODY VERSION 1.50 is weak evidence (install procedure section 9 step 3) |
| A2 | SHOOTING MENU > FILM SIMULATION; photograph the list | Eight rows, `CLASSIC CHROME` last with the icon the RAM-push test showed (11.4 item 2) |
| A2b | With CLASSIC CHROME selected, close the menu and look at live view; select PROVIA and compare; also after a power cycle with CC stored | CC live view looks muted, not like PROVIA, at once after power-on as well (section 4.10) |
| A3 | On the 2026-10-21 tabletop scene or another fixed scene, same exposure, shoot CC then PROVIA; play both back | Both frames normal; model: exiftool FilmMode `Classic Chrome` and `F0/Standard (Provia)` (11.4 item 3) |
| A4 | RAW CONVERSION of `DSCF3149.RAF` with default settings to CLASSIC CHROME, PROVIA, Velvia and ASTIA | Six rows with CLASSIC CHROME last. Model: `classic-chrome-model photo-identical` against the RAM-session conversions, CC to `DSCF3169`, PROVIA to `DSCF3164`, Velvia to `DSCF3165`, ASTIA to `DSCF3166`. Identity is expected if `IMAGE` carries the baseline's data block and develop-path words, which the RAM payload reproduced, and stock code elsewhere (**static-inference**). A difference is a finding for host analysis, not a safety stop (11.4 item 4) |
| A5 | Select CC, shoot, review; register CC in C and shoot at C; power off and on; both still show CC; shoot in each (review M1) | No stop, freeze or error (11.4 item 5) |
| A6 | Check the row in P, S, A, M and C, and that it is greyed where ASTIA and Velvia are | Same availability as ASTIA and Velvia (11.4 item 2) |
| A7 | Return the shooting setting to the owner's choice; shoot one stock-mode frame and play it back | Stock mode normal |

Not evaluated without a chart: the spec 19.5.6 bounds for items 1-3 and `e_model`; the look stands on the relative checks listed under that spec section. 11.4 item 6 lists the known limitations the owner will see. A byte-level readback of the installed image has no CC plan yet; a jig `read-config` of the gate-byte plan, a separately named read with a supply-cut ending, would test the 12.2.2 expectation (`0x80 = 00`, `0x81 = 00`, `0xf7 = 00`).

#### 12.2.9 What the go names

The owner's per-run H3 go names the operation and the digest: "Go: firmware update of XM1-body-01 from the camera's update menu with FWUP0002.DAT SHA-256 `DIGEST`, copied by card reader to the card root", with `DIGEST` written out in full as the icon candidate's 64-digit SHA-256. A go that names another digest, another file or another delivery is not this go, and `16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89` is not a flash target.

## 13. Reproduction

Host reads only. `R=private-data/analysis/x-m1-1.50` with the decodes named above (`lz/id4-0061c400.bin`, `lz/id5-00bf1a00.bin`, `lz/plain-01100000.bin`, `filmsim/id2-00130000.bin`, listings `lz/id4.dis`, `lz/id5.dis`, `filmsim/plain.dis`, and `payload.bin` for the fallback image at `0xe0048`).

```sh
R=private-data/analysis/x-m1-1.50
I4=$R/lz/id4-0061c400.bin; I5=$R/lz/id5-00bf1a00.bin; PL=$R/lz/plain-01100000.bin; I2=$R/filmsim/id2-00130000.bin
w(){ od -An -tx4 -j $(( $2 - $3 )) -N ${4:-4} $1; }      # w FILE ADDR BASE [BYTES]
# conversion table, its pointer, the next table; availability key-2 row and terminator
w $I4 0x77ac30 0x61c400 180; w $I4 0x81b494 0x61c400; w $I4 0x79608c 0x61c400 92; w $I4 0x796144 0x61c400 8
# converters, table 0x817180 and its neighbour
w $I4 0x738fe8 0x61c400 100; w $I4 0x736a74 0x61c400 104; w $I4 0x817180 0x61c400 120
# develop dispatch, marker setter, its reader
awk '/^ *d78418:/{f=1} f{print; n++} n>37{exit}' $R/lz/id5.dis
awk '/^ *d7851c:/{f=1} f{print; n++} n>11{exit}' $R/lz/id5.dis
awk '/^ *d671c4:/{f=1} f{print; n++} n>6{exit}' $R/lz/id5.dis
# hook sites and the out-of-table preset read
for a in 0xd67284 0xd60f60 0xd64ef4 0xd677ac 0xd677bc; do w $I5 $a 0xbf1a00; done
awk '/^ *d61350:/{f=1} f{print; n++} n>14{exit}' $R/lz/id5.dis; od -An -tx1 -j $((0xd61c58-0xbf1a00)) -N 9 $I5
# FilmMode and Saturation encoders
awk '/^ *eab258:/{f=1} f{print; n++} n>66{exit}' $R/lz/id5.dis; awk '/^ *eaac74:/{f=1} f{print; n++} n>95{exit}' $R/lz/id5.dis
# RAW sites
w $I5 0xd461ec 0xbf1a00 16; w $I5 0xd5ada4 0xbf1a00 68; w $PL 0x12548a0 0x1100000 136; w $I4 0x78bd7c 0x61c400 8
# label record, gates, base pairs
od -An -tx1 -j $((0x37b808-0x130000)) -N 72 $I2
for a in 0x11122c0 0x1164308 0x1178508 0x11787b4 0x117dc30 0x117dc34 0x117dfe8; do w $PL $a 0x1100000; done
# branch references into the converter extents (expect only internal branches and 0x739930, 0xeb1a34)
for f in $R/lz/id4.dis $R/lz/id5.dis $R/filmsim/plain.dis; do awk -F'\t' '$2 ~ /^b/ { if (match($3, /0x[0-9a-f]+/)) { t=strtonum(substr($3,RSTART,RLENGTH)); if ((t>=0x738fe8 && t<0x73904c) || (t>=0x736a74 && t<0x736adc)) print FILENAME": "$0 } }' $f; done
# fallback adjustment bytes: tone overrides and the preset flag byte
od -An -tx1 -j $((0xe0048+0xb5)) -N 1 $R/payload.bin; od -An -tx1 -j $((0xe0048+0xcb74)) -N 3 $R/payload.bin; od -An -tx1 -j $((0xe0048+0xaf93)) -N 1 $R/payload.bin
```
