# Classic Chrome icon (X-M1 1.50)

Design and implementation record for an icon for the `CLASSIC CHROME` film-simulation row that [the Classic Chrome patch](classic-chrome-patch.md) added without one. It inventories every surface that draws a film icon, maps the icon font and the Q-menu vector resources, fixes the route and its byte budget, describes the original artwork, and defines how the icon composes with the reviewed CC candidate, the live-view preview fragment and the release plan, including the abbreviated review and camera test. No camera or USB device is touched.

Inputs, byte-identical to the pins: DAT `FWUP0002.DAT` SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; the decodes under `private-data/analysis/x-m1-1.50/` (image 4 at `0x0061c400`, image 5 at `0x00bf1a00`, plain at `0x01100000`, image 2 at `0x00130000`); the reviewed CC candidate DAT `16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89` and its decodes (image 4 at `0x0061c200`, SHA-256 `00493fab…ff6a`).

Evidence labels: **byte-read** (read from a pinned image in this pass), **static-inference**, **emulated**, **inherited** (named report, not re-derived), **lead** (sibling-model source, X70, not X-M1 evidence), **unproved**.

## 1. Summary and decisions

1. **Route: one new icon-font glyph, U+F073, reached through the free special label id `0x83d`.** Every list surface that shows a film icon draws a string id whose text is a private-use code point rendered from `FONT_ICON.FDL`; the Q tile accepts the same string ids. The glyph is added without moving any stock font byte: its record sits in the font segment's loaded window tail, one directory word points at it, and one per-mille halfword sizes it like the stock film icons (section 4).
2. **Surfaces covered:** the shooting-menu, Fn and type-9 list rows (list icon and preview icon fields), the shooting-menu value icon, the RAW-conversion list row, and the Q film tile and Q film list. The Q list gains PRO Neg Std and PRO Neg Hi rows with their stock icon strings, so it offers the same eight films as the shooting list (section 5.3).
3. **Surfaces left as they are:** the live-view info overlay draws no film icon for PROVIA and none for CC, because its switch ends at film value 12 and needs new code to reach 13 (section 2.3); the FILM 1-3 bracket lists, which cannot select CC; the unlabelled nine-row list.
4. **Budget:** no prefix-window byte. The font segment's window tail holds 608 new bytes of its 12,332 (glyph record 376, Q values 72, Q labels 144, alignment 16); image 2 changes the 66-byte `0x83d` record in place; image 4 changes five words, three of them icon fields of the reviewed CC label arrays (section 6).
5. **The reviewed CC simulation bytes stay.** The CC patch sets, stubs, data block, `0x83e` record and gates are carried unchanged. The only words inside reviewed CC arrays that change are the three label-icon fields, from 0 to `0x83d`.
6. **Timing (owner decision 2026-10-22):** the icon goes into the first CC flash, together with the live-view preview fragment designed by the flash-prep session. Before the flash, an abbreviated RAM-push session tests the icon surfaces and CC live view with the CC simulation unchanged (sections 8 and 9).

## 2. Surface inventory

### 2.1 Two icon classes

The firmware draws film icons in two ways (byte-read):

- **Font glyphs through string ids.** A label id at or below `0x83d` resolves through `FUN_01179b0c` (plain) to UTF-16 text, either from the language slot's string table (ids below `0x83c`) or from the 66-byte special records at image-2 RAM `0x0037b784` (`0x83c`-`0x83e`). Film icons are private-use code points in that text: U+F06A STANDARD, U+F06B VIVID, U+F06C SOFT, U+F06D B&W, U+F06E SEPIA, U+F201 PRO Neg Hi, U+F202 PRO Neg Std, U+F1B3-F1B5 the filter modes; U+F06F-F071 are the PROVIA, Velvia and ASTIA wordmarks inside the row names and U+F069 is the FILM SIMULATION item icon. The text fonts carry no private-use code, so these draw from `FONT_ICON.FDL`.
- **Q vector graphics through Q ids.** An id `0xff000000 | n`, `n <= 0x8c5`, resolves through `FUN_0117e15c` to a 12-byte record `{u32 (h << 16) | w, u32 type, u32 pointer}` at image-2 RAM `0x0022a8b4 + 12n` (`n = 0x8c5` uses the record at `0x0037bab0`). All 2,246 records point at `GVFF` vector blocks in image 2 (1,622 type 1, 624 type 2): a 56-byte header with float bounds, solid paints as RGBA floats and OpenVG-style path segments (layout lead: x70view `tools/gvff.py`). The id space is closed at `0x8c5` (`FUN_0117e15c` returns 0 above it).

### 2.2 Surfaces

| Surface | Data path | Class | Stock film icons | CC before this change | CC with this change |
| --- | --- | --- | --- | --- | --- |
| Shooting menu, Fn and type-9 lists (nodes `0x007823a4`, `0x00785d74`, `0x0077d184`) | labels array quadruple `{preview icon, list icon, help, name}` by row; row drawer `FUN_01163f2c` skips id 0 | font via string id | list icon `0x489`-style ids, text `<f06a>` etc., width 80; preview icon width 120 | fields 0-2 are 0: no icon, no help | fields 0 and 1 are `0x83d` (text U+F073) |
| Shooting-menu value icon (FILM SIMULATION item, current value) | the same labels array by value through `FUN_00bf93e0` field 0 | font via string id | preview icon | none | `0x83d` |
| RAW-conversion list (node `0x0078bd5c`, property `0x22b`) | its own labels array, field 1 icon | font via string id | `<f06a>` etc. | 0 | `0x83d` |
| Q film tile and Q film list (node `0x0078b000`, values `+0x20`, labels `+0x24`) | Q labels `{id, 0, 0, 0}`; the label drawer `0x011121d4` renders Q ids and string ids (inherited, filmsim-patch-02 section 2.1) | Q vector, string id accepted | `0xff00077b`, `0xff00077c`, `0xff000773`, `0xff000777`, `0xff00077a` (80 x 48) | stored CC finds no row, draws row 0 (PROVIA) | new arrays: the eight shooting-list films, CC row 2 labelled `0x83d` |
| Live-view info overlay (plain `0x0114b6c4` and `0x01146cf0`, section 2.3) | switch on film col2 - 1 over 12 entries to Q ids | Q vector | Velvia `0x3da`, ASTIA `0x3df`, B&W family `0x386`, filters `0x388`, `0x389`, `0x387`, SEPIA `0x3dd`, PRO Neg `0x3dc`, `0x3db`; PROVIA none | default arm: none, as PROVIA | unchanged |
| Third film switch (plain `0x01150cd0`, callers `0x01150eb8`, `0x01150f18`) | switch on a film index -1..7 to Q ids including `0x3de` | Q vector | as listed | unidentified surface (open question 3) | unchanged |
| FILM 1-3 bracket lists (`0x00786774`, `0x00786da8`, `0x007898d4`) | own five-row tables | font via string id | stock | cannot select CC | unchanged |
| Nine-row list `0x0078d2d0` | blank labels | none | none | none | unchanged |

### 2.3 The live-view overlay

Both plain functions read the film col2 value, subtract 1, and dispatch over a 12-entry inline jump table (`0x0114b720 cmp r2, #11`, `0x01146ec0 cmp r2, #11`); col2 0 (PROVIA) and col2 13 (CC) take the default arm, which draws nothing (byte-read). The table cannot grow in place: the word after it is the first case's code. Giving CC an overlay icon needs one branch rewrite per function plus a stub of four instructions each (`cmp r2, #12`, a `movw` of the id, the two continuations), 32 bytes of executable space this design does not own, and an icon id the overlay draw (`FUN_0113a64c` then `FUN_01132f04`) accepts, which is unproved for string ids. Showing no overlay icon for CC matches PROVIA's behaviour, so the overlay stays stock in this candidate (open question 2).

## 3. The icon font

### 3.1 Location and sharing

`FONT_ICON.FDL` is segment-table entry id 3: media and flash `0x01060000`, window `0x80000`, size `0x7cfd4`, RAM load `0x003bc000` (byte-read, table block at media `0x140000`). Boot loads it once through `FUN_006aa6f8` (id4, called from `0x0073b514`), which copies `(window + 0x1ffff) >> 17` = 4 blocks, the whole window, so RAM `[0x003bc000, 0x0043c000)` holds the window's media bytes and `[0x00438fd4, 0x0043c000)` is loaded tail (media `0x00`, DAT raw `0xff`). The plain font registry at `0x011804a4` (eight 24-byte records `{name, length, address, size}`) gives ICON `{0x003bc000, 0x7cfd1}` and the seven text fonts (ARABIC, BIG5, GB, JIS, KSC, LATIN, THAI) a shared address `0x0045c000`.

The icon font is shared by all 35 languages. `FUN_006aa63c(lang)` maps the language through `0x007c8ff0` to a font index 0-2, but all three entries of its font table `0x007c907c` carry a zero enable word at `+0xc`, which the loader tests before loading (`0x006aa67c`-`0x006aa684`), and no other code names that table; the function then loads only the language's string slot (byte-read). The seven text fonts load through `FUN_006aa750` into `0x0045c000`; their dense directories cover U+0000-U+3000 (LATIN, THAI) or up to U+FFE6 (CJK, ARABIC), and none has a glyph at U+F069-F073 or U+F201 (byte-read).

### 3.2 Format

DynaComware-style `FDL Font`, byte-read against the X-M1 loaders and matching the X70 recovery (lead: x70view `decomp/app/graphics/fonts/font_raster_loader.c`):

| Font offset | Field | Value |
| --- | --- | --- |
| `0x00` | magic | `FDL Font` |
| `0x1c` | glyph count | `0xf726` (codes U+0000-U+F725) |
| `0x2e` | directory | `0x51` |
| `0x32` | command data | `0x3dced` |
| `0x36`, `0x38` | format, directory format | 4, `0x100` (dense) |
| `0x3a` | properties | `0x7c175` |
| `0x3e` | `info` chunk | offset `0x7c10e`, `0x67` bytes: `ICON_NEW-UCS-CNS`, `Standard\Unicode\UCS-CNS`, version `0100`, `201404151529` |

The directory holds `count + 1` little-endian words at `0x51 + 4 * code`: `flags << 24 | begin`, with the glyph's end the next word's low 24 bits. The two X-M1 glyph loaders, `FUN_01197f98` (raster) and `FUN_0119f74c` (outline), compute `count = end - begin`, return absent for 0 (`0x0119800c subs; beq`, `0x0119f7c4 subs; beq`), add the command-data base, read `adv_x, adv_y` when `flags & 1` and four vertical bytes when `flags & 2`, and for any nonzero flags take the program length from the record's next halfword instead of `count`; only the vertical-alternate path, which needs `flags & 4`, uses `count` afterwards. Neither loader compares an offset with the registry size (byte-read), and the stock properties already extend three bytes past the registry's `0x7cfd1`.

A glyph is an outline program: `fa 00 LEN16`, then two-byte instructions (opcode, packed `src << 4 | dst` register byte) with byte coordinate pairs `(y, x)` in 1/256 of the em, 255 meaning 256. `b5` loads a point into a register, `92` and `96` draw a line to a point on the left and right edge, `9a`/`9b`/`9e`/`9f` draw quadratic chains, `03` and `07` join two registers on the left and right edge, `70 00` separates groups. Each group fills, row by row, from the leftmost left-edge sample to the rightmost right-edge sample, so a glyph is a union of y-monotone spans (semantics byte-read from the programs; fill rule from the X70 decoder and rasterizer, lead, reproduced by the X-M1 renders in section 9). Glyphs are outlines, not bitmaps: the same program draws at every requested size.

Property key 10 at `0x7c177` is the per-mille size table for U+F000-U+F725 (`advance` at `0x7c185 + 2 * (code - 0xf000)`); the loader scales a requested height h to an em of `ceil(h * 1000 / advance)`. The film icons carry 800, so a 36-pixel request draws a 45-pixel em; absent codes carry 1000 (1,199 codes) or 600 (U+F000).

The font holds 822 glyphs; 1,199 codes in U+F000-F725 and 3,584 in U+E201-EFFF are absent. U+F072 and U+F073 sit right after the film block U+F069-F071, are absent from the font, from every text font and from every string in the 35 language slots (byte-read census of aligned UTF-16 units).

### 3.3 Adding a glyph

The directory is dense and its words are monotonic, and the command data runs to the `info` chunk with no free byte, so a glyph can be added in place only by moving every following record and rewriting every following directory word, about 25 KB for a code near the end. That is a font rebuild for flash and thousands of RAM publications for a push.

The loaders' flag rule gives a smaller route that moves nothing:

- The record lives anywhere after the command-data base reachable by a 24-bit offset; here, the loaded window tail at font offset `0x7cfe0` (RAM `0x00438fe0`), as `{u16 0, u16 0, u16 length, program}`.
- U+F073's directory word becomes `0x01000000 | (0x7cfe0 - 0x3dced)` = `0x0103f2f3`. Its `count` is negative (its end is still U+F074's begin `0xd680`), which the loaders accept as present, and flag 1 makes them take the length from the record. `adv_x = adv_y = 0` is what an unflagged record yields.
- U+F072's span changes from 0 to `0x3f2f3 - 0xd680` bytes starting at U+F074's record; no string contains U+F072, and the native census records its new result (section 9.1).
- U+F073's per-mille entry at `0x7c26b` changes from 1000 to 800.

Every other code keeps its directory words, so its loader result is unchanged.

## 4. Why not a new Q vector icon

The Q tile's stock rows are 80 x 48 `GVFF` drawings. A new one needs a Q id, and the id space is closed: the record table ends at `0x8c5`, whose slot is already the vector block at `0x0037b84c` (byte-read). Repurposing a stock Q id would need a census of every Q id reference to find one no surface draws, and authoring `GVFF` paths adds a second artwork format. The string-id route reuses the one glyph on every surface, and the Q drawer already renders string ids (patch-02 revision 2 uses `0x49a`/`0x496` there, inherited); whether a string id lays out well in the 80 x 48 tile is the emulator and owner check of section 9.

## 5. Write set

### 5.1 Icon string

Label id `0x83d` is the only free special id: `0x83c` is the blank id every unlabelled field uses, `0x83e` is the CC name, and `0x83f` upward needs the 264-byte relocated record table the CC report rejected (classic-chrome-patch section 4.7). No stock surface names `0x83d` (inherited census, special-label-resolution section 4), it already passes every text accessor (`FUN_01179b0c` resolves it, emulated in E-LABEL-RESOLVE-150), and D3, which planned to use it, is not installed and must be re-planned around CC anyway. Its record at RAM `0x0037b7c6` becomes UTF-16 U+F073, a zero unit, zero fill, and layout `{540, 24, 30, 36, 48}`, the stock list-icon layout of `0x489` and `0x49a`. One id serves both icon fields, so the preview field draws with the list icon's layout and the special class's fixed entry width 50 (byte-read, `0x0117de64`); the emulator check decides whether that renders correctly (section 9.2).

### 5.2 Changed bytes

| Image | Address (RAM) | Old | New | Role |
| --- | --- | --- | --- | --- |
| font | `0x003f821d` (unaligned word) | `0x0000d680` (begin `0xd680`, flags 0) | `0x0103f2f3` | U+F073 directory word |
| font | `0x0043826b` (unaligned halfword) | `0x03e8` | `0x0320` | U+F073 per-mille size 800; only byte `0x0043826b` changes |
| font | `[0x00438fe0, 0x00439158)` | `0x00` | glyph record, 376 bytes, SHA-256 `d8f69314…0429` | U+F073 outline |
| font | `[0x00439160, 0x004391a8)` | `0x00` | Q values: rows 0, 3, 1, 7, `0xc`, 5, 6, 2, terminator | Q film list |
| font | `[0x004391b0, 0x00439240)` | `0x00` | Q labels: `0xff00077b`, `0xff00077c`, `0xff000773`, `0xff000777`, `0xff00077a`, `0x49a`, `0x496`, `0x83d`, terminator | Q film list |
| image 2 | `[0x0037b7c6, 0x0037b808)` | 24 `0xe1xx` units, layout `{20, 20, 30, 36, 48}` | U+F073, layout `{540, 24, 30, 36, 48}` | icon string `0x83d` |
| image 4 | `0x0061c320` | 0 | `0x83d` | CC shooting quadruple field 0 (inside the reviewed array) |
| image 4 | `0x0061c324` | 0 | `0x83d` | CC shooting quadruple field 1 (inside the reviewed array) |
| image 4 | `0x0061c3dc` | 0 | `0x83d` | CC RAW-conversion quadruple field 1 (inside the reviewed array) |
| image 4 | `0x0078b024` | `0x00789e2c` | `0x004391b0` | Q node labels pointer |
| image 4 | `0x0078b020` | `0x00782348` | `0x00439160` | Q node values pointer |

The Q arrays are data read through the node pointers only; their placement shares the font tail's ownership argument (section 6). The shooting values array that the Q values mirror is the CC candidate's at `0x0061c340`.

### 5.3 Q list contents

The Q list mirrors the shooting list: PROVIA, Velvia, ASTIA, B&W, SEPIA with their stock vector ids, PRO Neg Std and PRO Neg Hi with their stock icon strings `0x49a` (`<f202>`) and `0x496` (`<f201>`), and CC with `0x83d`. A list with CC but without the two PRO Neg films would offer CC in Q while a stored PRO Neg still showed PROVIA's tile. The Q cell is property `0xfe` like the shooting list, so the availability matrix that RC0 and CC set for keys `0xb`, `0xc` and `0xd` applies unchanged.

## 6. Placement and budget

| Region | Used | Available | Notes |
| --- | ---: | ---: | --- |
| image-4 prefix window | 0 | 28 | stays at CC's 484 of 512 |
| font window tail `[0x00438fd4, 0x0043c000)` | 608 | 12,332 | record at `0x00438fe0`, arrays to `0x00439240` |
| image-2 record `0x83d` | 66 in place | 66 | no growth |
| image-4 words | 5 | in place | three CC label-icon fields, two Q node pointers |

Ownership of the font tail (byte-read census, emulated watch in section 9): the boot loader writes the whole window, nothing else in images 4, 5, plain or 2 names `[0x00438fd4, 0x0043c000)` or its `0x40000000` alias with an aligned word or a `movw`/`movt` pair (three aligned-word hits sit inside numeric tables at id4 `0x008973d0`, `0x0089740c` and id5 `0x010d94f4`), and the update writer programs every media block of the main range, so the tail bytes reach flash. The font segment's size word stays `0x7cfd4`.

## 7. Artwork

The icon is original pixel art for this project (`host/firmware/classic-chrome-icon.txt`, AGPL-3.0-only), drawn on the 45 x 45 em the font uses for a 36-pixel request, in the X-M1 icon family's visual language: a film cartridge silhouette (spool tab, canister, a leader with sprocket holes) with the film's letter knocked out of the canister. Classic Chrome takes the PRO Neg family's two-letter form: a knocked-out `C` in the canister for Classic and a solid subscript `C` below the leader for Chrome, read as C-sub-C as PRO Neg's icons read N-sub-S and N-sub-H. Strokes are three pixels, the sprocket holes two, and every outer corner steps by one pixel. No Fujifilm pixel, outline or program byte is copied: the cartridge geometry, letter shapes and their outline program are drawn and encoded here. Later Fujifilm models' Classic Chrome icon was not consulted.

```
.............................................
.............................................
..........#########..........................
..........#########..........................
..........#########..........................
...######################....................
..########################...................
..########################...................
..#########################################..
..##########################################.
..###########################..###..###..###.
..#######............########..###..###..###.
..######.............#######################.
..######.............#######################.
..#####...##################################.
..#####...##################################.
..#####...##################################.
..#####...#################################..
..#####...################...................
..#####...################...................
..#####...################...................
..#####...################.....###########...
..#####...################....############...
..#####...################...#############...
..#####...################...###.............
..######.............#####...###.............
..######.............#####...###.............
..#######............#####...###.............
..########################...###.............
..########################...###.............
..########################...###.............
..########################...#############...
..########################....############...
...######################......###########...
```

The encoder (`host/FujiAF/ClassicChromeIcon.hs`) groups the art's row spans into ten y-monotone groups and emits each as two edge chains of `b5`, `92`, `96`, `03` and `07` instructions with coordinates at pixel centres, `round((p + 0.5) * 256 / 45)`: 370 program bytes, SHA-256 `096e72a85b6f3285f92d69669976cfa9009bd59a0af36a5dc72230f86971e964`. Rasterised by the reference decoder at a 45-pixel em it equals the art pixel for pixel; at 30 and 36 pixels the sprocket holes thin to one pixel, as the stock icons' do.

## 8. Composition

### 8.1 With the CC candidate and the live-view fragment

The candidate is built by `//extensions:build-extension` from a combined manifest whose parts are: the image-4 patch set (the pinned `cc-image4.json` entries unchanged plus the five icon entries), the image-5 patch set (the pinned `cc-image5.json` entries unchanged plus flash-prep's five live-view words `0x00eee960`-`0x00eee970`, which widen `FUN_00eee958` to film value 13), the pinned `cc-plain.json` unchanged, the image-2 records `0x83d` (new) and `0x83e` (CC, unchanged), and a font patch set over segment 3. The builder refuses a combined set whose pinned entries differ from the CC files. It replaces the RC0 container on the body as the CC candidate would.

### 8.2 With RC0 and the release plan

The icon is a property of the CC image and needs CC's row, so it composes only with the RC0+CC recipe of the release plan; RC0 alone is unchanged. The first flash is the RC0+CC+icon+live-view candidate; the reviewed `16c830ad…2c89` is not installed. Reverting is as for CC: select a stock film, then reinstall RC0 `acb0fbbb…ca8`; the icon leaves no persistent state outside the image.

### 8.3 With D3 and the recipe bank

The icon takes special id `0x83d`, which D3 planned to use and which the recipe bank's accessor plan leaves alone (it serves `0x83f` upward). A re-planned D3 needs names from the bank's id class. The bank's relocated label table must be seeded from this image 2, now with the `0x83d` record.

### 8.4 RAM-push variant

The RAM payload carries the CC payload's publications and adds the icon and live-view words; the glyph record and the Q arrays are content in the allocation, so U+F073's directory word names `allocation_record - 0x003f9ced` and the Q pointers name the allocation copies. One CC publication changes: the RC0 body holds `0x49c` at `0x0061c3dc` (RC0's RAW labels), CC publishes 0 there, and the combined payload publishes `0x83d` in the same entry, phase and position, because the installer refuses duplicate addresses. The per-mille change touches only word `0x00438268` (bytes `01 e8 03 e8` to `01 e8 03 20`). Setup publishes the `0x83d` record and the per-mille word; the activation window publishes the two directory words, the label-icon words, the Q labels pointer before the Q values pointer, and flash-prep's five words, which must share one masked window because two of them are jump-table words. Everything else follows the CC route (classic-chrome-patch section 8.2).

## 9. Verification

### 9.1 Native cases

Not yet merged into this branch; the case generator runs on branch `agent/cc-icon-native`.

### 9.2 Emulator

RAM-push rehearsal (emulated, branch `agent/cc-icon-ram`, receipt `private-data/analysis/classic-chrome-patch/ram-push/icon-e2e-run-3/e2e-result.json`): 158 publications installed (100 setup, 58 activation), every segment passed; the FILM SIMULATION list row, the shooting-menu value and the Q film tile draw the C-sub-C icon, PROVIA reselection, `ram-unload` (158 restored) and a cold restart (158 old words) pass. Not reached: the RAW-conversion list (emulator MMIO gap) and the live-view preview colour (synthetic camera source). The one-id layout renders in all three emulated surfaces, which answers open question 1 for the emulator.

### 9.3 Build

Branch `agent/cc-icon-build` (`f841648`): the combined DAT with the live-view fragment is SHA-256 `6c57597338e0933a17b27138233d4ab598a637a57d37bafdfda1e78f3fa40af0` (icon only: `0e556de6…42a2`); every changed byte against `16c830ad…2c89` lies in the write set, the main checksum word or the re-encoded image-4/5 streams; `//emulation:flash-boot-check` passes (`private-data/flash-boot-checks/cc-icon-6c575973/`). RC0 and CC still rebuild to their pins with the changed tools, and every reviewed builder-digest source is byte-identical. This candidate is superseded by the frames-remaining fix of section 9.4 and is not for install.

### 9.4 Blocking CC defect: frames remaining 0 with CC selected

Emulated on the reviewed CC candidate `16c830ad…2c89`: with CLASSIC CHROME selected, live view shows frames remaining 0 and MEMORY FULL (message `0x7e5`); PROVIA and Velvia show 660 on the same card. The cause, emulated and byte-read in image 5 (stock bytes, unchanged by CC):

- The count table at `0x009432a8` (47 u32 counts; `0x009432ac` is the drawn value) is filled by the loop `0x00e43730`-`0x00e43a80` in `FUN_00e4361c` (caller `0x00dd01c8`, parameter block `0x015455f4`). Per entry it calls the film-class mapper `FUN_00e42414` (`0x00e43960`) with the film col2 from block `+0x10`, then `FUN_00ed5b0c`, whose validator `FUN_00ed5868` returns -1 when the class exceeds 5 (`0x00ed588c`), and the count becomes 0.
- `FUN_00e42414` is a compare chain: col2 0, 1, 2, 3, 5, 6, 11, 12 to class 0, col2 4, 7, 8, 9, 10 to class 2, anything else to 6 (`0x00e424ac cmp r0, #12; 0x00e424b0 movne r0, #6; 0x00e424b4 moveq r0, #0`). Col2 13 gives 6, so every count is 0. Its second caller `0x00e43e48` gets the same class.
- The film value reaches the block from `FUN_00dd0064` (`0x00dd00ac`-`0x00dd00b0`), which copies `+0x80` of the state block `*0x01529fb4` (`0x0095a6b4` in the emulated runs, film at `0x0095a734`, 0 for PROVIA, `0xd` for CC).
- Why the section 4.9 census missed it: the census followed the film stores it enumerated (settings objects, plan, slots, capture `+0x508`, mailbox copies) and classed a reader as safe when it indexed nothing unbounded. This reader takes the film from a separate state copy (`0x0095a6b4 + 0x80`) that the census did not list, and it is bounded: the default class 6 is an explicit invalid class that a later validator rejects, which turns an in-range value into a zero count rather than an out-of-table read.
- Capture gating and the 2026-10-21 RAM session are not yet squared: the session wrote CC frames, so either the count gates only the display and MEMORY FULL banner, or hardware computes it differently; the owner's session notes do not mention the banner.
- Proposed fix, not made: rewrite the chain's last three words to `cmp r0, #13`, `movhi r0, #6`, `movls r0, #0` (`e350000d 83a00006 93a00000`), which keeps every stock input's class, maps 13 to class 0 (PROVIA's size class), and leaves every mixed state of a preempted task with a stock or new result. It is a code change in image 5 outside the reviewed CC bytes and joins this changeset as a reviewed-CC-adjacent fix after native cases and an emulator rerun showing 660 with CC.

## 10. Abbreviated review scope

The independent review covers the new bytes and the label-icon words, and checks mechanically that everything else is the reviewed CC candidate:

1. Mechanical carry checks, run by the reviewer: every entry of the pinned `cc-image4.json`, `cc-image5.json` and `cc-plain.json` (SHA-256 as pinned in `classic-chrome.json`) appears unchanged in the combined sets; the `0x83e` step is identical; the RAM payload's first 131 publications equal the CC payload's reviewed contract (`00308606…32d9`) entry for entry; the DAT diff against `16c830ad…2c89` touches only the ranges of section 5.2, flash-prep's five words, the font window, the re-encoded image-4 and image-5 streams and the container checksums.
2. Review of the font change: the directory word and its loader semantics in `FUN_01197f98` and `FUN_0119f74c` (negative span, flag 1, record length), the U+F072 consequence, the per-mille halfword, the record bytes decoded against the art, and the font-tail ownership census.
3. Review of the label data: the `0x83d` record, the three label-icon words, the Q arrays and the two Q pointers, including the walker-safe publication order.
4. Review of the live-view fragment: flash-prep's packet for its five words.
5. Review of the new tooling: the FDL parser, the encoder, `rebuild-update extract-font`/`replace-font`, the `font-patch-set` step, the text-unit form of `write-record`, and the RAM builder's new publications, relinked offset arithmetic, contract and pins.

Out of scope: the CC simulation code and data (stubs, data block, hooks, converters, marker, FilmMode, Saturation, RAW sites, gates, `0x83e`), which the mechanical checks bind to the reviewed bytes.

## 11. Abbreviated camera test

A RAM-push session on the RC0 body under its own H2 go, after the review of section 10 and the emulator check. Stored `0xf7` is 0 on the body (E-STORED-F7-RESTORE-RUN-150), so the session assumes `commit-f7-enable` in a jig session first (H3, its own per-run go) and `commit-f7-restore` after; if the per-boot USB toggle of issue 81 lands first, it replaces both. The CC simulation is unchanged, so the CC session's capture and RAW checks (classic-chrome-patch section 8.4.4 U3-U6) are not repeated.

1. Host: the payload's tests, review-record gates and reviewed-build check pass on the session commit; output directories do not exist.
2. Pre-push read: every publication's old word and the route's reviewer windows; any mismatch stops before a write.
3. `ram-alloc`, `ram-relink`, `ram-push` as in classic-chrome-patch 8.4.3, with the new size and publication count.
4. Owner checks with the payload resident:
   - I1 SHOOTING MENU > FILM SIMULATION: eight rows; the CC row's icon cell shows the C-sub-C icon; the seven RC0 rows unchanged.
   - I2 select CLASSIC CHROME, back to the shooting menu page: the FILM SIMULATION item shows the CC icon as its value.
   - I3 Q menu: the film tile shows the CC icon; scrolling the film value offers the eight films, PRO Neg Std and Hi with their icons, the stock five unchanged.
   - I4 live view with CC selected: the preview looks like CC, not PROVIA; switch to PROVIA and back to compare. The overlay shows no film icon for CC or PROVIA.
   - I5 select PRO Neg Std, then PROVIA: the Q tile and the list follow.
   - I6 optional, with a RAF on the card: PLAYBACK > RAW CONVERSION > FILM SIMULATION shows the CC row with its icon.
5. Leaving: select PROVIA, `ram-unload` (all publications restored), power off; reboot check reads every old word.

Any stop, freeze, error dialog or stock row drawn differently ends the session (classic-chrome-patch 8.4.8). The go names the operation, the session commit, the contract digest and the reference runtimes.

## 12. Open questions

1. Layout of the one icon string in the preview field and the Q tile: the special class gives width 50 and one layout for both fields (section 5.1).
2. A CC overlay icon in live view needs 32 bytes of owned executable space and proof that the overlay draw accepts a string id (section 2.3).
3. The surface drawn by the third film switch at `0x01150cd0`.
4. U+F072's directory span is nonzero after the change; no string contains U+F072 in the 35 slots or the special records, but a string written later with it would draw garbage.
5. Hardware decode of the font tail is not observed; the stock window is loaded by the same loader, so this is inherited behaviour, not a new path.

## 13. Reproduction

Host reads only. `R=private-data/analysis/x-m1-1.50`, `D=private-data/firmware/x-m1-1.50/FWUP0002.DAT`.

```sh
# segment table (table block at media 0x140000, DAT raw = media + 0x25c, bytes complemented)
python_free_od() { od -An -tx1 -j "$2" -N "$3" "$1"; }
python_free_od "$D" $((0x14025c + 0x88)) $((0x24 * 2))
# FONT_ICON header, the U+F073 directory word and its per-mille entry (complement each byte)
python_free_od "$D" $((0x106025c)) 64
python_free_od "$D" $((0x106025c + 0x3c21d)) 8
python_free_od "$D" $((0x106025c + 0x7c26b)) 2
# the loaders and the font registry
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --start-address=0x97f98 --stop-address=0x983b0 "$R/filmsim/plain.elf"
od -An -tx4 -j $((0x11804a4 - 0x1100000)) -N 192 "$R/lz/plain-01100000.bin"
# the per-language font table and its enable words
od -An -tx4 -j $((0x7c907c - 0x61c400)) -N 48 "$R/lz/id4-0061c400.bin"
# the special records and the Q glyph record table
od -A x -t x1 -v -j $((0x24b784)) -N $((0xc6)) "$R/filmsim/id2-00130000.bin"
od -An -tx4 -j $((0x22a8b4 - 0x130000 + 12 * 0x77b)) -N 12 "$R/filmsim/id2-00130000.bin"
```
