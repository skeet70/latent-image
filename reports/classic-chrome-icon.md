# Classic Chrome icon (X-M1 1.50)

Design and implementation record for an icon for the `CLASSIC CHROME` film-simulation row that [the Classic Chrome patch](classic-chrome-patch.md) added without one, and for the combined flash candidate that carries it with the live-view preview fragment and the frames-remaining fix. It inventories every surface that draws a film icon, maps the icon font and the Q-menu vector resources, fixes the route and its byte budget, describes the original artwork, records the frames-remaining defect and its fix, and defines the combined candidate, its reviewer packet and the abbreviated camera test. No camera or USB device is touched.

Inputs, byte-identical to the pins: DAT `FWUP0002.DAT` SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; the decodes under `private-data/analysis/x-m1-1.50/` (image 4 at `0x0061c400`, image 5 at `0x00bf1a00`, plain at `0x01100000`, image 2 at `0x00130000`); the reviewed CC candidate DAT `16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89` and its decodes (image 4 at `0x0061c200`, SHA-256 `00493fab…ff6a`).

Evidence labels: **byte-read** (read from a pinned image in this pass), **static-inference**, **emulated**, **inherited** (named report, not re-derived), **lead** (sibling-model source, X70, not X-M1 evidence), **unproved**.

## 1. Summary and decisions

1. **Route: one new icon-font glyph, U+F073, reached through the free special label id `0x83d`.** Every list surface that shows a film icon draws a string id whose text is a private-use code point rendered from `FONT_ICON.FDL`; the Q tile accepts the same string ids. The glyph is added without moving any stock font byte: its record sits in the font segment's loaded window tail, one directory word points at it, and one per-mille halfword sizes it like the stock film icons (section 4).
2. **Surfaces covered:** the shooting-menu, Fn and type-9 list rows (list icon and preview icon fields), the shooting-menu value icon, the RAW-conversion list row, and the Q film tile and Q film list. The Q list gains PRO Neg Std and PRO Neg Hi rows with their stock icon strings, so it offers the same eight films as the shooting list (section 5.3).
3. **Surfaces left as they are:** the live-view info overlay, drawn by `FUN_01146cf0` through `0x01147c00` (emulated), shows no film icon for PROVIA and none for CC, because its switch ends at film value 12 and needs new code to reach 13 (section 2.3); the FILM 1-3 bracket lists, which cannot select CC; the unlabelled nine-row list.
4. **Budget:** no prefix-window byte. The font segment's window tail holds the 714-byte glyph record, the Q values (72), the Q labels (144) and the 1,780-byte Q tile from `0x7cfe0`, each start rounded to 16 bytes, of its 12,332 bytes; image 2 changes the 66-byte `0x83d` record and the Q glyph `0x37c` record pointer in place; image 4 changes five words, three of them icon fields of the reviewed CC label arrays (section 6).
5. **The reviewed CC simulation bytes stay.** The CC patch sets, stubs, data block, `0x83e` record and gates are carried unchanged. The only words inside reviewed CC arrays that change are the three label-icon fields, from 0 to `0x83d`.
6. **Timing (owner decision 2026-10-22):** the icon goes into the first CC flash, together with the live-view preview fragment designed by the flash-prep session and the frames-remaining fix. The reviewed `16c830ad…2c89` is not flashed. Before the flash, an abbreviated RAM-push session tests the icon surfaces, CC live view and the frame count with the CC simulation unchanged (section 11).
7. **Frames-remaining fix (owner-approved 2026-10-22):** with CC selected the reviewed candidate shows frames remaining 0 and MEMORY FULL, because the stock film-class mapper `FUN_00e42414` sends film 13 to an invalid class (section 9.4). Three words of that mapper change so 13 takes PROVIA's class; every other input keeps its stock class.
8. **Combined candidate (design B, with the full-size Q tiles of section 14):** flash DAT SHA-256 `98a66780f464731a184f1128aab0eb4b954adcdbbed99a214480720bd5a81606`; RAM payload of 162 publications, allocation `0x21fc`, contract `04df9a5ae4574f0c28f2c2ce67a581a96c7f092036ece6c316eced593fa1de32`, unreviewed (sections 9 and 10). The first submission, `78f75260…a7ba` with the C-sub-C glyph and the 161-publication payload `243587bc…e9ec`, is superseded.

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
- U+F072's span changes from 0 to `0x3f2f3 - 0xd680` = `0x31c73` bytes in the flash image, starting at U+F074's record. In the RAM payload U+F073's begin is allocation-relative (`0x827a53` at base `0x00c20000`), so U+F072's span is `0x81a3d3` bytes, about 8.5 MB from the font into unrelated RAM. Both loaders have two callers each, in text paths, and no string in the 35 slots or the special records contains U+F072; the native census records the flash result (section 9.2).
- U+F073's per-mille entry at `0x7c26b` changes from 1000 to 800.

Every other code keeps its directory words, so its loader result is unchanged.

## 4. Why not a new Q vector icon

The Q tile's stock rows are 80 x 48 `GVFF` drawings. A new one needs a Q id, and the id space is closed: the record table ends at `0x8c5`, whose slot is already the vector block at `0x0037b84c` (byte-read). Repurposing a stock Q id would need a census of every Q id reference to find one no surface draws, and authoring `GVFF` paths adds a second artwork format. The string-id route reuses the one glyph on every surface, and the Q drawer already renders string ids (patch-02 revision 2 uses `0x49a`/`0x496` there, inherited); whether a string id lays out well in the 80 x 48 tile is the emulator and owner check of section 9.

The emulator check found the string id drawn at list size in the Q tile. The Q-tile fragment of section 14 makes that census, repurposes the unreferenced Q glyph `0x37c` for an original `GVFF` tile and keeps the string id on the list surfaces.

## 5. Write set

### 5.1 Icon string

Label id `0x83d` is the only free special id: `0x83c` is the blank id every unlabelled field uses, `0x83e` is the CC name, and `0x83f` upward needs the 264-byte relocated record table the CC report rejected (classic-chrome-patch section 4.7). It already passes every text accessor (`FUN_01179b0c` resolves it, emulated in E-LABEL-RESOLVE-150), and D3, which planned to use it, is not installed and must be re-planned around CC anyway. One stock code site draws it (section 5.4): screen 170's title. Its record at RAM `0x0037b7c6` becomes UTF-16 U+F073, a zero unit, zero fill, and layout `{540, 24, 30, 36, 48}`, the stock list-icon layout of `0x489` and `0x49a`. One id serves both icon fields, so the preview field draws with the list icon's layout and the special class's fixed entry width 50 (byte-read, `0x0117de64`); the emulator check decides whether that renders correctly (section 9.2).

### 5.2 Changed bytes

| Image | Address (RAM) | Old | New | Role |
| --- | --- | --- | --- | --- |
| font | `0x003f821d` (unaligned word) | `0x0000d680` (begin `0xd680`, flags 0) | `0x0103f2f3` | U+F073 directory word |
| font | `0x0043826b` (unaligned halfword) | `0x03e8` | `0x0320` | U+F073 per-mille size 800; only byte `0x0043826b` changes |
| font | `[0x00438fe0, 0x004392aa)` | `0x00` | glyph record, 714 bytes, SHA-256 `b4bd628d…5247` | U+F073 outline |
| font | `[0x004392b0, 0x004392f8)` | `0x00` | Q values: rows 0, 3, 1, 7, `0xc`, 5, 6, 2, terminator | Q film list |
| font | `[0x00439300, 0x00439390)` | `0x00` | Q labels: `0xff00077b`, `0xff00077c`, `0xff000773`, `0xff000777`, `0xff00077a`, `0xff000779`, `0xff000778`, `0xff00037c`, terminator | Q film list (section 14) |
| image 2 | `[0x0037b7c6, 0x0037b808)` | 24 `0xe1xx` units, layout `{20, 20, 30, 36, 48}` | U+F073, layout `{540, 24, 30, 36, 48}` | icon string `0x83d` |
| image 4 | `0x0061c320` | 0 | `0x83d` | CC shooting quadruple field 0 (inside the reviewed array) |
| image 4 | `0x0061c324` | 0 | `0x83d` | CC shooting quadruple field 1 (inside the reviewed array) |
| image 4 | `0x0061c3dc` | 0 | `0x83d` | CC RAW-conversion quadruple field 1 (inside the reviewed array) |
| image 4 | `0x0078b024` | `0x00789e2c` | `0x00439300` | Q node labels pointer |
| image 4 | `0x0078b020` | `0x00782348` | `0x004392b0` | Q node values pointer |
| image 2 | `0x0022d28c` | `0x0027dbfc` | `0x00439390` | Q glyph `0x37c` record pointer to the Q tile (section 14) |
| font | `[0x00439390, 0x00439a84)` | `0x00` | Q tile, 1,780-byte `GVFF` block | CC Q film tile (section 14) |

The Q arrays are data read through the node pointers only; their placement shares the font tail's ownership argument (section 6). The shooting values array that the Q values mirror is the CC candidate's at `0x0061c340`.

### 5.3 Q list contents

The Q list mirrors the shooting list: PROVIA, Velvia, ASTIA, B&W, SEPIA with their stock vector ids, PRO Neg Std and PRO Neg Hi with their stock icon strings `0x49a` (`<f202>`) and `0x496` (`<f201>`), and CC with `0x83d`. A list with CC but without the two PRO Neg films would offer CC in Q while a stored PRO Neg still showed PROVIA's tile. The Q cell is property `0xfe` like the shooting list, so the availability matrix that RC0 and CC set for keys `0xb`, `0xc` and `0xd` applies unchanged.

### 5.4 The stock draw of `0x83d`: screen 170

The section-4 census of the special-label report counted node labels and missed one code site (review M2, byte-read): `0x0111d2d0 movw r0, #0x83d; mov r1, #0; bl 0x011397a4` in `FUN_0111d240`, the draw handler of screen 170 (entry 169 of the handler table `0x0110f9f8`, dispatched at `0x011089c8` with the screen number in halfword `0x015e46c4 + 0x480`). `FUN_011397a4` is the common title drawer, so screen 170 draws `0x83d` as its title; it is the only stock draw of `0x83d`. The stock record holds `?` and 24 icon-font letters spelling `PB_IMG_SEARCH_WGPS_BAR_F` with layout `{20, 20, 30, 36, 48}`; with this change the title draws U+F073 with layout `{540, 24, 30, 36, 48}`.

Reachability (byte-read chain, verdict unproved):

- Screens are requested as packet tokens `0x4000 | n` (`FUN_01108a2c`, type `0x4000` to `FUN_0110871c`). In code, token `0x40aa` is formed in one place, the arm `0x00db7708` of `FUN_00db68dc` (image 5), which maps UI event codes to screen tokens; it is entry 87 of the table for event codes `0x300`-`0x38e` (`0x00db729c`), so event `0x357` requests screen 170. No `movw` or `mov` immediate forms `0x40aa`; one aligned halfword of that value in image 4 data, two in plain data and nine in image 5 data are not classified.
- In code, event `0x357` has one producer, `0x00d45cd8` in `FUN_00d45988`, on message `0x816` (optionally after `0x93c` or `0x90e`); the word `0x357` also sits in an image-4 list of event codes at `0x00789694`, not classified. `FUN_00d45988` has two calls, both in `FUN_00ca0320`, which is action 3 of modal 94 and is named only by its action table `0x00cf4c8c`. The dispatcher `FUN_00bfb028` indexes the matrix as `state * width + event` (`0x00bfb0a4`); modal 94's matrix `0x00cdd0d0` has width 3, and action 3 sits in state 1, event 0.
- Modal 94 runs from `FUN_00c11ad0`, which sets its state to 0, 1 or 2 (halfword `0x014677a0`) when the playback state word `0x0094f64c` (`0x00944580 + 0xb0cc`) is 30, 31 or `0x16a`; `FUN_00ca0320` itself continues only for 31, and then only when `0x00944580 + 0xe464` is `0x804` or `+0xb0d4` is 48 and `FUN_00d45614` returns nonzero.
- Which X-M1 operation sets playback state 31 is not established. The record text points at a with-GPS image-search bar. The X-M1 offers PLAYBACK MENU > IMAGE SEARCH (`0x7869a8`, label `0x656`), whose search types are BY DATE 0, BY UPLOAD MARK 2, BY FACE 3, BY SCENE 4, BY TYPE OF DATA 5 and BY FAVORITES 6: value 1 has no row, and this body has no GPS. That suggests screen 170 belongs to a search type the X-M1 does not offer (static-inference, not proof).

Decision: keep `0x83d`. Moving the icon needs an id no stock screen draws: no free special id remains, an ordinary id below `0x83c` needs text in all 35 language slots, and a Q glyph id in a list-icon field goes through the row drawer's text path, which is not established to render a Q id. The camera test carries an owner check of IMAGE SEARCH (section 11, I8), and the open question is in section 12.

## 6. Placement and budget

| Region | Used | Available | Notes |
| --- | ---: | ---: | --- |
| image-4 prefix window | 0 | 28 | stays at CC's 484 of 512 |
| font window tail `[0x00438fd4, 0x0043c000)` | 608 | 12,332 | record at `0x00438fe0`, arrays to `0x00439240` |
| image-2 record `0x83d` | 66 in place | 66 | no growth |
| image-4 words | 5 | in place | three CC label-icon fields, two Q node pointers |

Ownership of the font tail (byte-read census): the boot loader `FUN_006aa6f8` copies `(window + 0x1ffff) >> 17` = 4 blocks, the whole window; nothing else in images 4, 5, plain or 2 names `[0x00438fd4, 0x0043c000)` or its `0x40000000` alias with an aligned word or a `movw`/`movt` pair except the two Q node pointers this change writes and three aligned words inside numeric tables (id4 `0x008973d0` and `0x0089740c` in the float and 2/π tables, id5 `0x010d94f4`); the update writer programs every media block of the main range (install-procedure S3, inherited), so the tail bytes reach flash and RAM. No emulated watch of the tail exists. The font segment's size word stays `0x7cfd4`.

Layout (`FujiAF.ClassicChromeIcon`): the glyph record starts at font offset `0x7cfe0`, and the Q values, the Q labels and the first free offset `iconTailEnd` each start at the end of the previous item rounded up to 16 bytes, so a record of any length moves only what follows it. The current 376-byte record gives Q values at `0x7d160`, Q labels at `0x7d1b0` and `iconTailEnd` `0x7d240`; the redesign candidates give record lengths of 406 (A), 398 (A2) and 714 (B) bytes, Q values at `0x7d180`, `0x7d170` and `0x7d2b0`, and `iconTailEnd` at `0x7d260`, `0x7d250` and `0x7d390`, all inside the `0x80000` window.

## 7. Artwork

The icon is original pixel art for this project (`host/firmware/classic-chrome-icon.txt`, AGPL-3.0-only), drawn on the 45 x 45 em the font uses for a 36-pixel request. It is design B of the owner's 2026-10-22 redesign, in the style of the stock SEPIA icon: the X-M1 film-cartridge silhouette drawn solid, with the word CHROME knocked out of it in original 5 x 11 letters with one-pixel strokes and three sprocket holes along the lower right. Classic Chrome is one film, so the PRO Neg family's letter-and-subscript form (the first submission, C-sub-C) is dropped. No Fujifilm pixel, outline or program byte is copied, and later models' Classic Chrome icon was not consulted.

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
..##########################################.
..##########################################.
..###.....#.###.#....##.....#.###.#.....####.
..###.#####.###.#.###.#.###.#..#..#.########.
..###.#####.###.#.###.#.###.#.#.#.#.########.
..###.#####.###.#.###.#.###.#.#.#.#.########.
..###.#####.###.#.###.#.###.#.###.#.########.
..###.#####.....#....##.###.#.###.#....#####.
..###.#####.###.#.#.###.###.#.###.#.########.
..###.#####.###.#.##.##.###.#.###.#.########.
..###.#####.###.#.##.##.###.#.###.#.########.
..###.#####.###.#.###.#.###.#.###.#.########.
..###.....#.###.#.###.#.....#.###.#.....####.
..##########################################.
..##########################################.
..##########################################.
..##########################################.
..###########################..###..###..###.
..###########################..###..###..###.
..##########################################.
..#########################################..
..########################...................
..########################...................
...######################....................
```

The encoder (`host/FujiAF/ClassicChromeIcon.hs`) splits the art's row spans into y-monotone groups and emits each as two edge chains of `b5`, `92`, `96`, `03` and `07` instructions. Every coordinate lies on a pixel edge, `round(p * 256 / 45)`, so two groups that meet share one boundary coordinate. The first submission placed group edges at pixel centres; the firmware draws glyphs with partial pixel coverage, so each group covered half of its first and last rows and grey seams showed where groups met, across the whole icon in line with the sprocket holes (emulated, `private-data/analysis/classic-chrome-icon-emu/redesign-1/diag-emulator-list-cell-8x.png`). The B program is 708 bytes (SHA-256 `f701df847dee3e07011df42f3eac28625ae53239331116d6f131f8d8c44935eb`) in 23 groups; the record is 714 bytes (`b4bd628d…5247`). The emulator draws it without seams on the list, the shooting-menu value and the Q tile (section 9.3).

The Q tile is drawn in the same design at the stock 80 x 48 tile size (`host/firmware/classic-chrome-q-tile.txt`, encoded as GVFF by `FujiAF.Gvff`; section 14).

## 8. Composition

### 8.1 With the CC candidate and the live-view fragment

The candidate is built by `//extensions:build-extension` from a combined manifest that `//host/firmware:build-classic-chrome-icon build --image5-fragment host/firmware/classic-chrome-liveview-image5.json --image5-fragment host/firmware/classic-chrome-frames-image5.json CC_MANIFEST OUT` writes. Its parts are: the image-4 patch set (the pinned `cc-image4.json` entries unchanged plus the five icon entries, applied as later overlay writes), the image-5 patch set (the pinned `cc-image5.json` entries unchanged plus flash-prep's five live-view words `0x00eee960`-`0x00eee970`, which widen `FUN_00eee958` to film value 13, and the three frames words `0x00e424ac`-`0x00e424b4`), the pinned `cc-plain.json` unchanged, the image-2 records `0x83d` (new) and `0x83e` (CC, unchanged), and a font patch set over segment 3. The generator refuses a combined set whose pinned entries differ from the CC files and any overlap between fragments. The candidate replaces the RC0 container on the body as the CC candidate would.

### 8.2 With RC0 and the release plan

The icon is a property of the CC image and needs CC's row, so it composes only with the RC0+CC recipe of the release plan; RC0 alone is unchanged. The first flash is the RC0+CC+icon+live-view candidate; the reviewed `16c830ad…2c89` is not installed. Reverting is as for CC: select a stock film, then reinstall RC0 `acb0fbbb…ca8`; the icon leaves no persistent state outside the image.

### 8.3 With D3 and the recipe bank

The icon takes special id `0x83d`, which D3 planned to use and which the recipe bank's accessor plan leaves alone (it serves `0x83f` upward). A re-planned D3 needs names from the bank's id class. The bank's relocated label table must be seeded from this image 2, now with the `0x83d` record.

### 8.4 RAM-push variant

The RAM payload is a new payload kind, `ClassicChromeIcon` (builder `//host/firmware:build-classic-chrome-icon-ram`, review record `transport/ram-push-cc-icon-review.json`), on the unchanged CC route and installer. It carries the CC payload's 131 publications and adds 31: 16 setup (15 words of the `0x83d` record, the per-mille word) and 15 activation (two directory words, two label-icon words, the Q glyph `0x37c` tile pointer, then the Q labels pointer, then the Q values pointer, flash-prep's five live-view words and the three frames words, all in one masked window). The glyph record, the Q arrays and the Q tile are content in the allocation (`0x21fc` bytes), so U+F073's directory word names `allocation_record - 0x003f9ced`, the Q pointers name the allocation copies and the `0x37c` record names the allocation tile; the builder refuses an offset at or above `0x01000000`. One CC publication changes: the RC0 body holds `0x49c` at `0x0061c3dc` (RC0's RAW labels), CC publishes 0 there (index 124), and the combined payload publishes `0x83d` in the same entry, phase and position, because the installer refuses duplicate addresses; `carried-publications.json` records it. The per-mille change touches only word `0x00438268` (bytes `01 e8 03 e8` to `01 e8 03 20`). Everything else follows the CC route (classic-chrome-patch section 8.2).

Content layout (`FujiAF.ClassicChromeIconRam`): the record starts right after `__ram_push_end`, and the Q values, the Q labels and the Q tile each start at the end of the previous item rounded up to 8 bytes; the allocation ends at the tile's end. With the 714-byte design-B record and the 1,780-byte tile the content is `[+0x1760, +0x21fc)`: record `+0x1760`, Q values `+0x1a30`, Q labels `+0x1a78`, tile `+0x1b08`. The allocation size names the payload kind in `transport/FujiAF/RamPush.hs` (`payloadAllocationSize`), so a record or tile of another length is a mechanism revision with a new contract and re-pin. The installer's peer re-maintenance covers `[__ram_push_start, __ram_push_end)` only; the content past it is covered by `ram-alloc`'s peer prepare over the whole allocated block before upload and CPU0's whole data clean and invalidate (transport README, `peerPrepareBytes`).

## 9. Verification

All runs use the pinned inputs; private receipts are named in each row. The combined images are CC (`16c830ad…2c89`) plus the icon writes of section 5.2, the live-view fragment (`45211718…a015`) and the frames fragment (`2e5f1b0c…d58a`).

### 9.1 Build

| Item | Value |
| --- | --- |
| Combined DAT (design B with the Q tiles) | `private-data/analysis/classic-chrome-icon/build-b-1-pinned/FWUP0002.DAT`, SHA-256 `98a66780f464731a184f1128aab0eb4b954adcdbbed99a214480720bd5a81606`; the pinned manifest `parts-b-1-pinned/classic-chrome-icon.json` (`670da4dc…91ca`) reproduces it, and q-icon's independent build of the same parts gives the same digest |
| Parts | `cc-icon-image4.json`, `cc-icon-image5.json` (CC entries plus live-view and frames fragments), `cc-icon-font.json` (directory word, per-mille, record 714, Q values, Q labels, tile 1,780 bytes), `cc-icon-image2.json` (the Q glyph `0x37c` pointer `0x0022d28c`: `0x0027dbfc` to `0x00439390`), the pinned `cc-plain.json`, records `0x83d` and `0x83e` |
| Changed bytes against `16c830ad…2c89` | image 2 56 bytes in 2 writes (`0x83d` record, tile pointer), font window 1,592 bytes in 6 writes, image-4 decode 12 bytes, image-5 decode 18 bytes (live view 15, frames 3); otherwise only the main checksum word and the re-encoded image-4 and image-5 records; plain unchanged (q-icon's diff of the same DAT) |
| `//emulation:flash-boot-check` | passed on `98a66780` (`private-data/flash-boot-checks/cc-icon-98a66780/` and, independently, `cc-icon-b-qtile-1/`) |
| Regression | RC0 rebuilds to `acb0fbbb…9ca8` and CC to `16c830ad…2c89` with the changed tools; every reviewed builder-digest source is byte-identical; `//transport:ram-push-review-bytes` and `//transport:ram-push-cc-review-bytes` pass |
| RAM payload | 162 publications (100 setup, 62 activation), allocation `0x21fc`, installer extent `0x15c0`, contract `04df9a5ae4574f0c28f2c2ce67a581a96c7f092036ece6c316eced593fa1de32`, approved by review CcIconReview (2026-10-23) at `b8c7fd9`; runtimes `4f871b2a…ebd3` at `0x00c20000`, `2fce1f08…e16a` at `0x00c21f88`, `03284c54…fe58` at `0x00a7e8c8` (`private-data/analysis/classic-chrome-patch/ram-push/icon-b-relink-*/`); read plans `cc-icon-b-ram-push-prepush.json` (57 windows) and `cc-icon-b-ram-push-post-reboot.json` (48); `//transport:ram-push-cc-icon-reviewed-build-check` accepts at both reference bases and refuses every perturbation |

### 9.2 Native cases

`//emulation:classic-chrome-icon-cases` at `8fc0c53` on the design-B candidate: 9,097 of 9,097 pass, none rerun (`private-data/analysis/x-m1-1.50/native-case/classic-chrome-icon-b-qtile-1/`, results.md and results.json). The combined side's image-2 and font windows equal those of `98a66780…1606` (`--combined-dat`); images 4, 5 and plain are built from the contract and were not compared against the DAT's decodes in this run (the first submission's run compared them and found them equal; the B change touches none of them). No runner admission was needed.

| Family | Result |
| --- | --- |
| FDL loaders `FUN_01197f98`, `FUN_0119f74c`, 2,029 codes each side | U+F073: CC absent; combined present, count `0x2c4` (the 708-byte program), cursor `0x7cfe6`, advance (0, 0). U+F072 gains a nonzero span as before. Every other code identical (4,058 + 4,058) |
| Size reader `FUN_011a023c` | combined U+F073 per-mille 800 (8) |
| Text `FUN_01179b0c`, glyph table | `0x83d` gives `0x0037b7c6` holding U+F073 on combined; `0x83e` gives `CLASSIC CHROME`; stock ids unchanged (36 + 15) |
| Label getters, shooting, Fn, type-9 and RAW nodes | combined `0x83d` at value 2 fields 0 and 1 and RAW value `0xa` field 1; every other cell equal (240 + 272) |
| Q node, Q records, kind and drawer | combined rows 0-7 name the five stock vector ids, `0xff000779`, `0xff000778`, `0xff00037c` over values 0, 3, 1, 7, `0xc`, 5, 6, 2; all eight are vector kind 5 and draw at 80 x 48; Q glyph `0x37c` resolves to `0x00439390`, a `GVFF` block equal to the 1,780-byte tile; image 2 differs from CC outside the special records only in that pointer word (72 + 92 + 64 + 13 + 13) |
| Frames mapper `FUN_00e42414` and validator `FUN_00ed5868` | combined equals stock for every input except 13, which gives class 0 instead of 6; both preempted mixed states give the stock class (95 + 57) |
| Site and builder cases with the combined image 5 (first submission's run; image 5 is unchanged by B) | 368 of 368 and 115 of 115 |

### 9.3 Emulator

Flash candidate runs (`xm1-viewer --ram-push-e2e --flash-candidate`, trajectories `emulation/classic-chrome-icon-trajectories.json`, receipts `private-data/analysis/classic-chrome-icon-emu/`):

| Check | Reviewed CC `16c830ad` (`baseline-1`, `baseline-2`) | Combined B `98a66780` (`combined-b`, `b-qtile-1`) |
| --- | --- | --- |
| Frames remaining, PROVIA / Velvia / CC | 660 / 660 / 0 with MEMORY FULL on menu close; mapper class 6, validator -1, all 47 counts 0 | 660 / 660 / 660, no menu message; the 188-byte table at `0x009432a8` is byte-identical for all three (`combined-b/cc-live-view-frames-660.png`) |
| Icon on the FILM SIMULATION list and the shooting-menu value | no icon | CHROME cartridge on both, no seams (`combined-b/cc-film-list-row.png`, `combined-b/cc-shooting-menu-film-value.png`) |
| Q film tile, CC and both PRO Neg | CC draws PROVIA's tile; PRO Neg not offered | CHROME tile at the stock 80 x 48 size; PRO Neg Std and Hi with their stock tiles; focused and unfocused (`combined-b/cc-q-film-cell.png`, `combined-b/proneg-std-q-film-cell.png`, `combined-b/proneg-hi-q-film-cell.png`; all segments in `b-qtile-1/`) |
| No card (first-start route from the `flash-boot-check` snapshot, `no-card-1/`) | CC committed; frames table 99999 in all 47 entries for PROVIA and CC, no count drawn, MEMORY FULL flag 0 (`cc16c830-cc.out`) | reaches the shooting screen; CC committed; the same table, no count drawn, flag 0; Q shows the CHROME tile (`b-provia-live-view-no-card.png`, `b-cc-live-view-no-card.png`, `b-cc-q-no-card.png`) |
| Segments | | `classic-chrome-icon-trajectories.json` 26 of 26 passed; `classic-chrome-q-tile-trajectories.json` 14 of 14 passed (q-icon) |

RAM-push rehearsal of the 162-publication payload (`xm1-viewer --ram-push-e2e --classic-chrome-icon --unreviewed-relink`, receipt `private-data/analysis/classic-chrome-icon-emu/ram-e2e-b2/e2e-result.json`): install with 162 validated, 100 setup and 62 activation words stored, peer lines 162; list icon, commit (film 13), shooting-menu value, Q tile, Q dial to PROVIA and back, menu close with no message and a frames table equal to PROVIA's, PROVIA reselection, `ram-unload` (162 old words verified), the list after unload with seven rows, and a cold restart reading all 162 words old: every segment passes. An earlier run (`ram-e2e-b`) is void: a concurrent viewer run restaged the shared runtime directory `private-data/emulator-runtimes/xm1-ram-push-e2e`, so its cold restart booted the flash DAT; e2e runs must not overlap.

Not covered by the emulator: the RAW-conversion list (playback stops on the known MMIO gap), the live-view preview colour (synthetic camera source), the shutter (no S1/S2 input in the key model), and any hardware rendering.

### 9.4 Frames remaining 0 with CC selected

Emulated on the reviewed CC candidate and byte-read in image 5 (stock bytes, unchanged by CC):

- The count table at `0x009432a8` (47 u32 counts; `0x009432ac` is the drawn value) is filled by the loop `0x00e43730`-`0x00e43a80` in `FUN_00e4361c` (caller `0x00dd01c8`, parameter block `0x015455f4`). Per entry it calls the film-class mapper `FUN_00e42414` (`0x00e43960`) with the film col2 from block `+0x10`, then `FUN_00ed5b0c`, whose validator `FUN_00ed5868` returns -1 when the class exceeds 5 (`0x00ed588c`), and the count becomes 0. The table is recomputed whenever the list cursor lands on a row.
- `FUN_00e42414` is a compare chain: col2 0, 1, 2, 3, 5, 6, 11, 12 to class 0, col2 4, 7, 8, 9, 10 to class 2, anything else to 6 (`0x00e424ac cmp r0, #12; 0x00e424b0 movne r0, #6; 0x00e424b4 moveq r0, #0`). Its only other caller, `0x00e43e48` in `FUN_00e43d84` (callers `0x00dd7790`, `0x00de6570`, `0x00e43fac`), passes the class to `FUN_00ed6718`, which returns 0 for a class above 5; that 0 is stored to `0x01546ce4 + 0xe0` or `0x01545910 + 0xe0`.
- The film reaches the block from `FUN_00dd0064` (`0x00dd00ac`-`0x00dd00b0`), which copies `+0x80` of the 1000-byte state block `*0x01529fb4` (`0x0095a6b4`). `FUN_0063219c` copies that block whole between `0x00956838` (the live block, film cell `0x009568b8`), `0x00956c20`, `0x0095a6b4`, `0x0091845c`, `0x0093a954` and plain `*0x0093ade4`; the only direct `+0x80` writer is the setter `FUN_0063458c` (`0x0063459c`).
- The section 4.9 census missed it for two reasons: it enumerated the film stores it knew (settings objects, plan, slots, capture `+0x508`, mailbox copies) and not this state-block family, and it counted a reader as safe when it indexed nothing out of range, while this reader is bounded and its default class 6 is an explicit invalid value that a later validator turns into a zero count.
- Fix (owner-approved, `host/firmware/classic-chrome-frames-image5.json`): `0x00e424ac` `e350000c` to `e350000d` (`cmp r0, #13`), `0x00e424b0` `13a00006` to `83a00006` (`movhi r0, #6`), `0x00e424b4` `03a00000` to `93a00000` (`movls r0, #0`). Film 13 takes class 0, PROVIA's size class; 0-12, 14, 15 and negative inputs keep their stock classes (native cases 9.2); a task preempted inside the three words gets the stock class (both mixed states executed).
- Shutter gating: unproved. A zero count drives the count display, the MEMORY FULL indication (flag `0x0152b908`, set at `0x00dd70ec`, read only by display code) and warning id 20 (`FUN_00d16bc0`), which feeds item-availability predicates (`FUN_00d19324` group); no link from them to S2 was found (static-inference), and the emulator has no shutter input. The 2026-10-21 RAM session wrote CC frames with the same stock image-5 code resident, so on hardware the shutter did not refuse at a zero count in that state (hardware-observed write, static-inference link). The fix makes the candidate independent of the answer.

### 9.5 Census of other film consumers

Byte-read over images 4, 5 and plain on the CC decodes (notes `private-data/analysis/classic-chrome-icon/film-census.md`):

- `FUN_00e42414` has exactly the two callers above; no literal, `movw`/`movt` pair or Thumb form names it. The fix covers both.
- The state block's `+0x80` film is copied into five parameter blocks at `+0x10` (`0x015455f4`, `0x01545580`, `0x01545668`, `0x01532fe0`, and the `FUN_00dcd4cc` stack blocks); only `FUN_00e4361c` and `FUN_00e43d84` read that `+0x10`. The other 13 readers of `+0x80` are compare chains that send 13 down col2 0's path, availability walks, the reviewed converter, or the reviewed plain overlays.
- Of the 89 jump tables with 12 or 13 entries on the CC decodes (15 in image 4, 31 in image 5, 43 in plain; review N1 added image 4 `0x0085ec80`, a data-type switch in a float conversion loop), a film value reaches 10, all reviewed CC sites or sites covered by the builder and site cases: `0x00ea9cbc`, `0x00eaac84`, `0x00eab2dc`, `0x00d78428`, `0x00d79040`, `0x00d67398`, `0x00eee964`, `0x01146ec4`, `0x0114b724`, and `0x012546d8` (which switches on another field). No film source was found for the other 79 (static-inference by elimination). The combined image 5 has 30, because the live-view fragment removes the table at `0x00eee964`. Of 14 long compare chains, `FUN_00e42414` is the only one on a film value.
- Limits: A32 only; pointers reached through heap structures or indirect calls are not followed.

## 10. Reviewer packet

The review covers every byte outside the reviewed `16c830ad…2c89` image and the reviewed CC label words that change, and binds everything else mechanically to the reviewed candidate. The CC simulation (stubs, data block, hooks, converters, marker, FilmMode, Saturation, RAW sites, gates, `0x83e`) is out of scope.

Inputs: this report; section 5.2 and the fragments `host/firmware/classic-chrome-liveview-image5.json` and `host/firmware/classic-chrome-frames-image5.json`; the art `host/firmware/classic-chrome-icon.txt`; the source `host/FujiAF/FdlFont.hs`, `host/FujiAF/ClassicChromeIcon.hs`, `host/FujiAF/FontWindow.hs`, `host/FujiAF/LabelRecord.hs` (UTF-16 form), `host/firmware/ApplyPatchSet.hs` (overlay rule), `host/firmware/RebuildUpdate.hs` (`extract-font`, `replace-font`, `write-record --text-utf16`), `host/firmware/BuildClassicChromeIcon.hs`, `extensions/BuildExtension.hs` and `extensions/ExtensionManifest.hs` (`font-patch-set`, `text_utf16`), `host/FujiAF/ClassicChromeIconRam.hs`, `host/firmware/BuildClassicChromeIconRam.hs`, `transport/FujiAF/RamPush.hs` and the review-record plumbing; the private `parts-combined-1-pinned/`, `diff-combined-1/` and `icon-frames-relink-*/` directories; the native and emulator receipts of section 9.

1. Mechanical carry checks: every entry of the pinned `cc-image4.json`, `cc-image5.json` and `cc-plain.json` appears unchanged and in order in the combined sets (the generator refuses otherwise); the `0x83e` step is identical; `changed-bytes.json` shows nothing outside section 9.1's groups; the RAM manifest's CC publications equal the reviewed contract `00308606…32d9` entry for entry except index 124; RC0 and CC rebuild to their pins.
2. Reviewed words that change: in the flash image the three label-icon words `0x0061c320`, `0x0061c324`, `0x0061c3dc` (0 to `0x83d`), written as overlay writes over the reviewed CC label blobs; in the RAM payload only CC publication 124 at `0x0061c3dc` (old `0x49c`, CC new 0, combined new `0x83d`), because the other two are 0 on RC0 and in CC and are new icon publications.
3. Font: the U+F073 directory word and the loaders' flag rule (negative span, flag 1, record length), the U+F072 consequence, the per-mille byte, the 714-byte record against the art (pixel-edge encoding, section 7), the Q tile (section 14), and the font-tail ownership census (section 6).
4. Label data: the `0x83d` record, the Q arrays, the two Q pointers and their walker-safe order (labels before values).
5. New mechanism: the five live-view words (flash-prep's packet, classic-chrome-patch section 4.10; they rewrite five words of `FUN_00eee958` in one masked window) and the three frames words (section 9.4, with both install mixed states executed). The install mixed states are safe for every film value; the unload mixed states are not (classic-chrome-patch 4.10): a task suspended inside `FUN_00eee958` with film 13 selected when `ram-unload` restores the words can branch to an instruction word or store into its caller's code, and one suspended inside `FUN_00e42414` gives PRO Neg Hi one transient zero count. The restore order inside the masked window cannot change this, because only tasks already suspended inside the words see a mixed state. Unload therefore carries the precondition of section 11.6 (E1-E3). Rollback also restores the Q values pointer (index 152) before the labels pointer (151); a walker suspended between reading the two would pair eight value rows with the five stock label rows, which the same precondition (no menu open) excludes.
6. Tooling: the FDL parser and encoder, the font-window carrier, the overlay rule of `apply-patch-set`, the UTF-16 record text, the `font-patch-set` step, and the RAM builder's publications, relinked offset arithmetic, contract and 19 pins (`transport/ram-push-cc-icon-review.json`, approved by CcIconReview at `b8c7fd9`; `//transport:ram-push-cc-icon-review-verdicts` passes).

## 11. Abbreviated camera test

A script, not a run, for a RAM-push session of the `ClassicChromeIcon` payload on the RC0 body (XM1-body-01) under its own H2 go (11.9). It follows [classic-chrome-patch 8.4](classic-chrome-patch.md#84-camera-session-script-first-classic-chrome-ram-push) with the payload, counts and read plans of this report, and with no card: the body's SD slot has a broken pin. The CC simulation is unchanged, so the CC session's look checks (classic-chrome-patch 8.4.4 U3-U6) are not repeated. Record every item in `private-data/sessions/<date>-cc-icon-ram-push/notes.md`; any item not done is a stop.

### 11.1 Without a card

In the emulator the combined candidate `98a66780` boots from erased settings with no card to the shooting screen, opens SHOOTING MENU and FILM SIMULATION, commits CLASSIC CHROME, returns to the shooting screen and opens the Q menu with the CHROME tile (receipts `private-data/analysis/classic-chrome-icon-emu/no-card-1/`, driven from the `flash-boot-check` snapshot). That route is the first-start route, not the body's retained settings; whether the body reaches the shooting screen without a card is item 11.2 B0, before anything is pushed.

With no card the frames-remaining table at `0x009432a8` holds 99999 (`0x1869f`) in all 47 entries for PROVIA and for CC, the MEMORY FULL flag `0x0152b908` stays 0, and no count is drawn. The reviewed CC candidate `16c830ad`, which lacks the frames fix, gives the same table on the same route. Without a card the frames check (I3) therefore shows only that PROVIA and CC display the same thing; it does not exercise the fix, whose evidence stays the emulated card runs of section 9.3 until a card can be used.

Not possible without a card, so deferred to a later session: shooting and playing back a CC frame, RAW CONVERSION (classic-chrome-patch 8.4.4 U5), IMAGE SEARCH (I8, the screen-170 title check of section 5.4), and a hardware check that CC's frame count equals PROVIA's.

### 11.2 Before the session

1. **Host:** on the session commit, `devenv shell -- bazel test //transport:ram-push-tests //transport:cli-rejections //host/firmware:classic-chrome-icon-ram-tests //transport:ram-push-cc-icon-review-bytes //transport:ram-push-cc-icon-review-verdicts //transport:ram-push-cc-icon-camera-pins` passes, and `devenv shell -- bazel run //transport:ram-push-cc-icon-reviewed-build-check` exits 0 after printing `reviewed CC icon contract accepted at 0xc20000`, `reviewed CC icon contract accepted at 0xc21f88` and a `refused as expected` line for each of its ten perturbation cases. Rehearsals exit 0 with these final lines (observed 2026-10-23): `rehearse ram-alloc 0x21fc` prints `rehearsal: ram-alloc; no native provider, USB enumeration or camera execution;allocation-bytes=8700`; `rehearse read-config "$PWD/private-data/read-plans/gate-byte.json"` prints `rehearsed read-config: 4 windows`; `rehearse read-ram "$PWD/private-data/read-plans/cc-icon-ram-push-prepush.json"` prints `rehearsed read-ram: 57 read requests; no write sent`; `rehearse read-ram "$PWD/private-data/read-plans/cc-icon-ram-push-post-reboot.json"` prints `rehearsed read-ram: 48 read requests; no write sent`.
2. **Read plans:** `private-data/read-plans/cc-icon-ram-push-prepush.json` (SHA-256 `c67fe35b…95ad`) and `cc-icon-ram-push-post-reboot.json` (`c2b63478…6737`), written by `build-classic-chrome-icon-ram --read-plans` from the pinned manifest.
3. **Output paths** must not exist: `private-data/sessions/xm1-cc-icon-ram-alloc-1`, `private-data/analysis/classic-chrome-patch/ram-push/operator-linked-icon-1`, `private-data/sessions/xm1-cc-icon-ram-push-1`. An existing OUTDIR refuses before USB.
4. **Body:** RC0 (seven film rows), no card in the slot, ordinary boot with no buttons held. Stored `0xf7 = 0x01` since the 2026-10-22 re-enable (E-STORED-F7-REENABLE-RUN-150), so a plain attach enumerates `04cb:ff80`; exactly one such device on the bus. USB attached and left attached until 11.7 ends. No `commit-f7-*` command is part of this session.
5. **Power:** a freshly charged battery showing full, or the mains-backed dummy supply.
6. **Pre-session record, photographed:** SHOOTING MENU > FILM SIMULATION (seven rows, a stock row selected), the Q film tile, the C slot's film (dial to C, read, dial back; no CUSTOM SET), and what the shooting screen shows where the frame count normally appears.
7. **B0, no-card start:** power on with no card. The shooting screen and SHOOTING MENU must appear, and any no-card indication is recorded. If the body does not reach the shooting screen, or shows an error dialog that does not clear, the session ends here with nothing sent.

### 11.3 Model reads before the push (standing read grant)

1. `devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/gate-byte.json"`: product `X-M1`, `0xf7` nonzero.
2. `devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/cc-icon-ram-push-prepush.json"`: one pass, 57 requests. The model decodes every publication address and requires its RC0 `old` word (162 words, including the Q glyph `0x37c` pointer `0x0022d28c = 0x0027dbfc`), service mode 3, table `0x015c22a0`, slot `0x00dc7874`, and the route's reviewer windows at their recorded values. Any mismatch stops the session before any write (S-PREREAD).

### 11.4 Load commands

Run from the repository root in one shell, no power-off between L1 and L4, camera idle at the shooting screen with no menu open. Each live command first prints `output: <OUTDIR>`; a stop prints one `camera:` line on stderr and exits nonzero.

```sh
ALLOC_OUT="$PWD/private-data/sessions/xm1-cc-icon-ram-alloc-1"
LINK_OUT="$PWD/private-data/analysis/classic-chrome-patch/ram-push/operator-linked-icon-1"
PUSH_OUT="$PWD/private-data/sessions/xm1-cc-icon-ram-push-1"
SIZE=0x21fc
devenv shell -- bazel run //transport:camera -- ram-alloc "$SIZE" "$ALLOC_OUT"
BASE="$(devenv shell -- jq -r .base "$ALLOC_OUT/ram-alloc-state.json")"
devenv shell -- bazel run //transport:camera -- ram-relink "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT"
devenv shell -- bazel run //transport:camera -- ram-push "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT/ram-push-manifest.json" "$PUSH_OUT"
```

| Step | Command | Camera effect | Expected and record |
| --- | --- | --- | --- |
| L0 | the assignments; the owner confirms in chat that a stock film row is selected and the dial is not at C | none | `echo "$SIZE"` prints `0x21fc` |
| L1 | `ram-alloc` | allocates 8,700 bytes, peer-core range maintenance, restores the callback slot | `ram-alloc: retained;state=…/xm1-cc-icon-ram-alloc-1/ram-alloc-state.json;relink before ram-push`. Record `jq .` of the state: `kind` `xm1-ram-alloc-state`, `size` 8700, `product` `X-M1`, `config_f7` nonzero, `cpu0` `0x00004003`, `table_pointer` `0x015c22a0`, `slot_old` `0x00dc7874`, `base` nonzero and 8-byte aligned |
| L2 | `BASE=…` | none | `echo "$BASE"`: `0x` plus eight hex digits, last digit `0` or `8` |
| L3 | `ram-relink` | none (host build with the pinned icon relinker) | `ram-relink: manifest=…/operator-linked-icon-1/ram-push-manifest.json`. Record `sha256sum "$LINK_OUT"/runtime.bin "$LINK_OUT"/ram-push-manifest.json`; the manifest names 162 publications and the contract digest of 11.9 |
| L4 | `ram-push` | runtime upload with readback, INSTALL rendezvous, 162 stores, slot restore, readback | `ram-push: installed;state=…/xm1-cc-icon-ram-push-1/ram-push-state.json;power-off clears RAM`. The camera already required: mailbox magic `0x31504d52`, ABI 3, status 0, validated 162, setup 100, activation 62, restored 0, service core `0x4003`, peer status 0 with 162 lines, park and ack spins below the bound `0x80000`. Note the wall time and paste all output |

### 11.5 Owner checks with the payload resident

| Code | Owner action | Must show |
| --- | --- | --- |
| I1 | SHOOTING MENU > FILM SIMULATION | eight rows; the CC row's icon cell shows the CHROME cartridge; PRO Neg Hi, PRO Neg Std and the other RC0 rows unchanged |
| I2 | Select CLASSIC CHROME, back to the shooting menu page; repeat with PRO Neg Std and PRO Neg Hi | the FILM SIMULATION item's value shows the CC icon for CC and each PRO Neg row's stock icon for that row |
| I3 | Shooting screen with PROVIA, then CC, then PROVIA: photograph where the frame count normally appears each time | PROVIA and CC show the same thing (a number, a blank or a no-card indication); no MEMORY FULL. Record what it is (11.1: with no card this does not test the frames fix) |
| I4 | Live view with CC, then PROVIA, then CC, in the menu highlight and on the shooting screen | CC looks muted against PROVIA; no film icon in the overlay for either |
| I5 | Q menu, film tile, with CC, then PRO Neg Std, then PRO Neg Hi; dial through the films | CC shows the CHROME cartridge tile at the stock tiles' size; PRO Neg Std and Hi show their full-size stock tiles; eight films offered; the stock five unchanged |
| I8 | IMAGE SEARCH | deferred: not testable without a card (11.1) |

Any stop, freeze, error dialog or stock row drawn differently is S-UNEXPECTED.

### 11.6 Leaving the payload

| Code | Action | Must show |
| --- | --- | --- |
| E1 | Owner: select PROVIA/STANDARD in SHOOTING MENU > FILM SIMULATION | that row under the cursor |
| E2 | Owner: dial to C; if FILM SIMULATION shows CLASSIC CHROME there, select the stock film recorded for C in 11.2 item 6; dial back. No CUSTOM SET | C shows its recorded film |
| E3 | Owner: back to the shooting screen, no menu open; confirm in chat | none |
| E4 | Model: `devenv shell -- bazel run //transport:camera -- ram-unload "$PUSH_OUT/ram-push-state.json"` | `ram-unload: rollback verified; allocation retained until power-off`; the camera required validated 162 and restored 162 |
| E5 | Owner: SHOOTING MENU > FILM SIMULATION; Q film tile | seven rows; the Q menu offers the stock five with their stock tiles |
| E6 | Owner: power off normally | none |

E1-E3 are the unload precondition (section 10 item 5): a task inside `FUN_00eee958` with film 13 selected when `ram-unload` restores the live-view words can fault.

### 11.7 Reboot check

Power on normally, no card, cable attached. B1 normal start-up to the shooting screen; B2 FILM SIMULATION seven rows with the E1 row selected, Q five films; B3 C shows its recorded film. B4 model: `devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/cc-icon-ram-push-post-reboot.json"` (48 requests): all 162 publication words equal their `old` values and slot `0x015c22ac = 0x00dc7874`. Stored `0xf7 = 0x01` is unchanged by this session.

### 11.8 Stop rules

Every stop ends camera operations for the session; nothing is retried; a new attempt needs a new go. Power-off is the recovery for every outcome: the allocation, runtime, callback retarget and all publications are RAM state.

Operating rules. Run L1, L4 and E4 only with the camera idle at the shooting screen. Never install this payload while the CC payload or the bank+D3 payload is resident, or the reverse: they publish the same addresses, so the second push refuses on its old-word check.

| Code | Seen | Action |
| --- | --- | --- |
| S-NOCARD | B0: no shooting screen, or an error dialog that does not clear | nothing was sent; power off; stop |
| S-PREREAD | 11.3 mismatch, refusal or disconnect | no write was sent; stop |
| S-HOST | L0 or L3 fails | before L1 nothing to undo; after L1 run `ram-free "$ALLOC_OUT/ram-alloc-state.json"` (expect `ram-free: allocation freed`), power off, 11.7 |
| S-ALLOC-REFUSED / S-ALLOC-UNCERTAIN | `ram-alloc refused` / `ram-alloc uncertain …;send nothing further and power off` | refused: nothing to free; uncertain: mandatory power-off, 11.7 |
| S-PUSH-REFUSED | `ram-push refused: …` before upload (manifest, runtime, pins, base, size, product, slot, old word) | `ram-free`, power off, 11.7; if the reason is the allocation guard, power off without `ram-free` |
| S-PUSH-REFUSED-PARK | `installer refused before publication: CPU1 park rendezvous timed out` | power off without `ram-free`, 11.7 |
| S-PUSH-UNCERTAIN | `ram-push uncertain: …;send nothing further and power off` (including completion timeout, status -8) | mandatory power-off now, no button first, 11.7 |
| S-UNEXPECTED | anything in 11.5 differing from its row, including PROVIA and CC showing different things at I3 | stop using the camera; photograph; if the menu responds select the recorded stock film; no `ram-unload` unless the model asks after reading the journals, and then only after E1-E3 hold; power off, 11.7 |
| S-UNLOAD-REFUSED / -UNCERTAIN | `ram-unload refused` / `ram-unload uncertain` | power off, 11.7 (uncertain: mandatory, now) |
| S-USB | cable pulled, host crash, or a command without a final line for 10 minutes | treat as uncertain: power off, 11.7 |
| S-HUNG | no response to the power switch within 60 s | remove the battery for 10 s, reinsert, 11.7 |

### 11.9 What the go names

The owner's H2 go names: the operation (ordinary-boot RAM push of the Classic Chrome icon payload on the RC0 body with no card: the B0 no-card start, the 11.3 reads, `ram-alloc 0x21fc`, `ram-relink` at the returned base, `ram-push` of 162 publications, the 11.5 checks, `ram-unload` after E1-E3, power-off and the 11.7 read, with `ram-free` as the not-installed contingency); the session commit, whose icon RAM files equal reviewed `b8c7fd92259f18d8506210167a0baba4a73e9ced` (review CcIconReview); the publication contract digest `04df9a5ae4574f0c28f2c2ce67a581a96c7f092036ece6c316eced593fa1de32`; and the reference runtimes the pinned relinker reproduces, `4f871b2a9c488b2a68bfa0a6c006303d292b4d2dadce54f8635ec8e438f3ebd3` at `0x00c20000` and `2fce1f08317732091648eb58a3409f6bf472d0f7c8444ea153800f7da791e16a` at `0x00c21f88`. The session's own runtime depends on the base `ram-alloc` returns; the camera binary rebuilds it with the pinned relinker and requires byte equality, and L3 records its SHA-256 before L4 runs. I8, a CC frame, RAW CONVERSION and the frame-count check with a card are outside this go. The flash of `98a66780…1606` follows under its own H3 go.

### 11.10 Result of the session (2026-10-23)

Ran under the owner's H2 go ("yep, go ahead") from `1b65622`, on XM1-body-01 (1.50, RC0, stored `0xf7 = 1`, no card), with no stop (E-CC-ICON-RAM-PUSH-SESSION-150).

- B0: the shooting screen and SHOOTING MENU come up with a no-card message.
- 11.3: one `04cb:ff80` device; the pre-push read matched all 162 old words and the route windows.
- 11.4: `ram-alloc 0x21fc` returned base `0x00a82500`; the relinked runtime (`59e92c5d…228e`) matched the approved pins; `ram-push` installed 162 publications in 83 s.
- 11.5: I1, I2 and I5 pass. I4 passes with a hedge: the owner saw the difference clearly while scrolling and less clearly after selection. I3 is inconclusive: with no card no count is drawn for either film (11.1), and no MEMORY FULL or difference between the films was reported. I8 deferred.
- 11.6-11.7: `ram-unload` verified 162 old words; normal power-off and restart; seven film rows and five Q films; the post-reboot read found all 162 words old and slot `0x015c22ac = 0x00dc7874`.

Deferred to a card session: I3 frames, I8 IMAGE SEARCH, a CC shot and its playback, and RAW conversion.

## 12. Open questions

1. Answered by section 14: the Q tile takes Q glyph ids, and the Q-tile fragment gives CC and both PRO Neg rows full-size 80 x 48 tiles without a second special id.
2. A CC overlay icon in live view needs 32 bytes of owned executable space and proof that the overlay draw accepts a string id (section 2.3).
3. The surface drawn by the third film switch at `0x01150cd0`.
4. U+F072 resolves to a `0x31c73`-byte span in the flash image and an `0x81a3d3`-byte span in the RAM payload; no string contains U+F072 in the 35 slots or the special records, but a string written later with it would draw garbage.
5. Shutter gating by a zero frame count (section 9.4) is unproved; the fix removes the case for CC.
6. Hardware decode and rendering of the font tail and the icon are not observed.
7. Whether screen 170, which draws label `0x83d` as its title, is reachable on the X-M1 (section 5.4): its only code path needs playback state 31 in modal 94, whose setter is not bound. I8 checks IMAGE SEARCH on the body.

## 13. Reproduction

Host reads only. File bytes are the complement of media bytes.

```sh
R=private-data/analysis/x-m1-1.50; D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
rd(){ od -An -tx1 -j "$2" -N "$3" "$1"; }
rd "$D" $((0x14025c + 0x88)) $((0x24 * 2))          # segment records id 2 and 3
rd "$D" $((0x106025c)) 64                          # FONT_ICON header
rd "$D" $((0x106025c + 0x3c21d)) 8                 # U+F073 and U+F074 directory words
rd "$D" $((0x106025c + 0x7c26b)) 2                 # U+F073 per-mille
od -An -tx4 -j $((0x11804a4 - 0x1100000)) -N 192 "$R/lz/plain-01100000.bin"   # font registry
od -An -tx4 -j $((0x7c907c - 0x61c400)) -N 48 "$R/lz/id4-0061c400.bin"        # per-language font table
od -A x -t x1 -v -j $((0x24b784)) -N $((0xc6)) "$R/filmsim/id2-00130000.bin"  # special records
od -An -tx4 -j $((0x22a8b4 - 0x130000 + 12 * 0x77b)) -N 12 "$R/filmsim/id2-00130000.bin"
od -An -tx4 -j $((0xe42414 - 0xbf1a00)) -N 172 "$R/lz/id5-00bf1a00.bin"       # film-class mapper
nix shell nixpkgs#llvm -c sh -c "llvm-objcopy -I binary -O elf32-littlearm --change-section-address .data=0x1100000 --set-section-flags .data=code,alloc,load $R/lz/plain-01100000.bin /tmp/plain.elf && llvm-objdump -d --triple=armv7r-none-eabi --start-address=0x1197f98 --stop-address=0x11983b0 /tmp/plain.elf"
devenv shell -- bazel run //host/firmware:build-classic-chrome-icon -- build --image5-fragment host/firmware/classic-chrome-liveview-image5.json --image5-fragment host/firmware/classic-chrome-frames-image5.json private-data/analysis/classic-chrome-patch/candidate-pinned/classic-chrome.json NEW_PRIVATE_DIR
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/classic-chrome-icon/parts-combined-1-pinned/classic-chrome-icon.json "$(readlink -f "$D")" NEW_PRIVATE_DIR
devenv shell -- bazel build //emulation:classic-chrome-icon-cases && bazel-bin/emulation/classic-chrome-icon-cases --workers 8 --frames host/firmware/classic-chrome-frames-image5.json
devenv shell -- bazel test //host/firmware:classic-chrome-icon-tests //host/firmware:classic-chrome-icon-ram-tests //transport:ram-push-cc-icon-review-bytes
```

## 14. Full-size Q tiles


The first combined candidate `78f75260…a7ba` drew the CC icon in the Q film tile at list size, ink 47 x 32 against the stock tiles' 82 x 44, and drew PRO Neg Std and Hi there at list size too. The Q tiles now give all eight Q rows full-size tiles: CC an original 80 x 48 `GVFF` tile through the unreferenced Q glyph `0x37c`, the two PRO Neg rows the stock tiles `0x779` and `0x778`. No special label id, gate, relocation or code byte is involved; the change is the Q labels array's rows 5-7, one image-2 word and the tile in the font tail, built by `build-classic-chrome-icon` with the rest of the icon.

### 14.1 How the Q tile resolves its icon

Byte-read on the stock plain image unless noted. The Q cell (node `0x0078b000`) draws field 0 of its labels row through the label drawer `FUN_011121d4(x, y, id, flag)`, which has three classes:

| Class | Test | Size | Draw |
| --- | --- | --- | --- |
| Q glyph `0xff000000 \| n`, `n <= 0x8c5` | `id + 0x01000000 <= 0x8c5` (`0x011121f4`) | the record's halfwords: `FUN_0117e048` width, `FUN_0117e0a4` height | `FUN_01179da0(x, y, w, h, 24, id, 0, 1)` |
| `0xfd000000`, `0xfd000001` | `id + 0x03000000 <= 1` | two runtime bitmaps from plain `0x0110e434`: 260 x 96 at `0x015c2318`, 140 x 72 at `0x015da91c` | the same draw |
| string id up to the text gate (`0x83d` stock, `0x83e` CC) | `0x011122c0` | the id's layout: `FUN_0117dce0` (`+0x3c`) width, `FUN_0117dd48` (`+0x3e`) height, `FUN_0117ddb0` (`+0x40`) | the same draw, position from the arguments or, at -20, from `+0x38`/`+0x3a` |

The shooting-list row drawer `FUN_01163f2c` draws its icon fields through a descriptor with x = y = -20 and width = height = 0 (`0x01164bc4`-`0x01164be0`), so it also takes the label's layout (static-inference from the descriptor). One string id therefore has one size on every surface: `0x83d`'s layout `{540, 24, 30, 36, 48}` requests 36 pixels in the list and in the Q tile.

The stock films' tiles are separate Q glyphs, not a scale of the list glyph and not another label field: PROVIA `0x77b`, Velvia `0x77c`, ASTIA `0x773`, B&W `0x777`, SEPIA `0x77a`, each a record `{80 x 48, type 1, GVFF pointer}`. The label kind `FUN_01179a50` returns 1 for text, 2 for a bitmap, 5 and 6 for `GVFF` records of type 1 and 2. The film-tile family `0x773`-`0x77c` also holds B&W+G `0x774`, B&W+R `0x775`, B&W+Ye `0x776`, PRO Neg Hi `0x778` and PRO Neg Std `0x779` (identified by rendering the blocks with the x70view `tools/gvff.py` decoder, a layout lead; the X-M1 rendering of `0x778` and `0x779` is emulated in 14.6).

`GVFF` colours are keys. Every stock tile carries the solid paints red, black and white (RGBA floats); the `GVFF` loader in image 4 (`0x006da9b0`-`0x006daa74`) compares each solid paint with a three-entry key table when the drawing object's mode query returns 2 and substitutes that state's colour, which is how a tile draws black on white when focused and grey on dark otherwise (byte-read loader, emulated result).

### 14.2 Routes and their cost

| Route | Cost | Verdict |
| --- | --- | --- |
| Second glyph at Q size in the same font | the font tail has room, but Q draws a string id at its layout's size, so the glyph needs its own string id | needs a second id |
| Second special id `0x83f` (accessor route) | raise the 33 class gates, 25 of which are reviewed CC words in `cc-plain.json`; record `0x83f` at `0x0037b84a` overlaps Q glyph `0x8c5`'s `GVFF` block at `0x0037b84c` (the "?" tile), so the record table moves (264 bytes) or that block is proven unused | changes reviewed CC bytes; takes the recipe bank's first id |
| Another free special id | none: `0x83c` is the blank id, `0x83d` the CC icon, `0x83e` the CC name | not available |
| A stock string id whose text is one private-use glyph | 62 ids qualify, all naming present glyphs that other strings also use (U+F1B5, the G-filter icon of `0x4ab`, appears in `0x3da`, `0x3db`, `0x4ac` and `0x609`) | redrawing the glyph changes other strings |
| Q glyph: repoint an unreferenced stock record at a new `GVFF` tile | one image-2 word, one Q labels word, the tile in the font tail | chosen |

Census of Q glyph references (byte-read over images 2, 4, 5, plain, the font window and the 35 language slots of the stock DAT): code forms every Q id with a `movw`/`movt #0xff00` pair (2,207 in plain, 1 in image 4, none in image 5) or as the constant `0xff000000` (`0x0111dff8`); no literal-pool load yields a Q id, no `add`, `sub`, `orr` or `rsb` combines a Q-id register with another operand within the next seven instructions, and the three `orr #0xff000000` in image 4 (`0x0088f120`, `0x0088f18c`) and plain (`0x012c9c04`, data) are an RGB565 conversion and a misdecode; data names Q ids as aligned words. 913 of the 2,246 glyphs have no reference. `0x37c` is the 80 x 48 type-1 `EXR` tile, a mode of EXR-sensor bodies that the X-M1 lacks, and its word `0xff00037c` occurs at no alignment in any of those inputs; `0x778` and `0x779` likewise. Limits: A32 only, ids built in heap structures or through indirect calls are not followed (as section 9.5).

The record slot itself is named by no code or data: its words `0x0022d284`, `0x0022d288` and `0x0022d28c` occur at no alignment in images 2, 4, 5, plain or the font window, and no `movw #0xd284`/`#0xd288`/`#0xd28c` is followed by `movt #0x22` (byte-read). The record is reached only by `FUN_0117e048`/`0a4`/`100`/`15c` computing `0x0022a8b4 + 12n` from an id. The fragment keeps `+0` (`0x00300050`, 80 x 48) and `+4` (type 1) and changes only `+8`.

Recipe ids: the fragment uses no special id, so `0x83d` stays the CC icon, `0x83e` the CC name, and the recipe bank's `0x83f` upward (recipe-design section 17.6, names `0x83f`-`0x845`, help lines after them) and its relocated table seeded from this image 2 are unaffected. D3's re-plan is unchanged by this fragment. A recipe that wants its own Q tile can take another unreferenced glyph by the same census; the B&W filter tiles `0x774`-`0x776` are better kept for a filter unlock.

### 14.3 Write set

| Image | Address (RAM) | Old | New | Role |
| --- | --- | --- | --- | --- |
| font | Q labels rows 5, 6, 7 field 0 (`FujiAF.ClassicChromeIcon.qLabelsArray`) | (new array) | `0xff000779`, `0xff000778`, `0xff00037c` | PRO Neg Std, PRO Neg Hi, CC |
| font | `[fontRamAddress iconTailEnd, + tile length)`; design B: `[0x00439390, 0x00439a84)` | `0x00` | `GVFF` tile, 1,780 bytes (design B) | CC Q tile |
| image 2 | `0x0022d28c` | `0x0027dbfc` | the tile's address; design B `0x00439390` | Q glyph `0x37c` record pointer (record `0x0022d284`: `{80, 48}`, type 1, unchanged) |

The Q labels array is the icon's own (section 5.2), so the flash candidate and the RAM payload read rows 5-7 from one source. The icon's other writes, the `0x83d` record and the three CC label-icon words are unchanged, so the list surfaces keep the `0x83d` glyph. The tile starts at `iconTailEnd`, the first 16-aligned font offset after the Q labels array (`FujiAF.ClassicChromeIcon` places the glyph record, Q values and Q labels each at the previous end rounded up to 16); with the 714-byte design-B glyph record that is `0x7d390`. The tile's address and the image-2 word are derived from it, and `qTileFontWrites` refuses a tile that overlaps the record or either array, is not word-aligned, runs past the `0x80000` window or lands on nonzero bytes. With design B the tile ends at font offset `0x7da84`, inside the loaded tail `[0x7cfd4, 0x80000)` whose ownership section 6 establishes; the tail holds 2,736 placed bytes of 12,332. The Q values array and Q rows 0-4 are unchanged.

### 14.4 The tile

Original pixel art (`host/firmware/classic-chrome-q-tile.txt`, AGPL-3.0-only), one cell per unit of the 80 x 48 record, in design B, the owner's pick for the list icon: the solid cartridge (spool tab, canister, film strip) with `CHROME` knocked out across it in 9 x 17 letters with two-unit strokes and three sprocket holes at the strip's lower right, the stock SEPIA tile's text-on-cartridge form drawn at Q size. No Fujifilm outline, path or block byte is copied; the tile's geometry and letters are drawn here and the `GVFF` bytes are encoded here.

`FujiAF.Gvff.encodeTileGvff` writes the stock tile form: the 56-byte header (`GVFF`, version 2.1, the constant word `0x001902cf` every stock block carries, header size 56, two paths, three paints, no ramp, bounds `(0, 0, 80, -48)`), the key paints red, black and white, path 0 filling the bounds white, and path 1 filling the ink black under the nonzero rule with a red 0.75 stroke, round joins and caps, dash index `0x29a` (none). Path 1's contours are the boundary of the ink traced on pixel corners, outer boundaries counter-clockwise and holes clockwise, collinear vertices dropped; the test suite checks that the nonzero winding at every pixel centre equals the art. Segment bytes are padded with 4 - count mod 4 bytes, four when the count is a multiple of four, as in every stock block.

### 14.5 Tooling

- `//extensions:build-extension` gains the `image2-patch-set` step (segment 2, decode base `0x00130000`, after the records and before the font patch set; `rebuild-update extract-image2`, `apply-patch-set`, `replace-image2`), with parser tests in `//extensions:build-extension-tests`.
- `//host/firmware:build-classic-chrome-icon build` appends `qTileFontWrites` to the icon's font patch set and writes `cc-icon-image2.json`, an `image2-patch-set` step after the two record steps whose decode digest is the image-2 extent with both records written; the pointer word is in `write-set.json`, so `diff` checks it. Font-set blobs are padded to whole words with zero bytes, which `apply-patch-set` checks against the window (the design-B glyph record is 714 bytes).
- `//host/firmware:classic-chrome-q-tile-tests`: the encoder over synthetic art (a ring, corner-touching pixels, a notched block, an empty tile, a segment count of 12), the tile's decode and fill, and the font writes over a synthetic window.

### 14.6 Verification

Design-B build on `agent/q-icon` 8fc0c53 (unpinned; `agent/cc-icon` reproduces it as its pinned build). Receipts are private.

| Check | Result |
| --- | --- |
| Build | `build-classic-chrome-icon build` with the live-view and frames fragments over the pinned CC manifest (`private-data/analysis/classic-chrome-icon/parts-b-qtile-1/`), then `//extensions:build-extension --unpinned`: DAT SHA-256 `98a66780f464731a184f1128aab0eb4b954adcdbbed99a214480720bd5a81606`, `rebuild-update verify` passes, and the image-2 patch set's decode digest matches the extent after the record steps |
| Changed bytes against `16c830ad…2c89` (`diff-b-qtile-1/changed-bytes.json`) | image 2 56 bytes in 2 writes (the `0x83d` record, the `0x37c` pointer), font window 1,592 bytes in 6 writes, image-4 decode 12 bytes in 5 writes, image-5 decode 18 bytes in 8 writes, the main checksum word; nothing else |
| Native cases (`//emulation:classic-chrome-icon-cases --combined-dat`, `private-data/analysis/x-m1-1.50/native-case/classic-chrome-icon-b-qtile-1/`) | 9,097 of 9,097 passed, none rerun. The combined side's image-2 and font windows equal the DAT's; images 4, 5 and plain are built from the contract. U+F073 count `0x2c4`, cursor `0x7cfe6`. Q families: record accessors 64 (width 80, height 48, type 1 for all eight Q-row glyphs on stock and combined; pointers equal except `0x37c`, `0x00439390` on combined), label kind 13 (combined rows 0-7 kind 5), Q drawer 13 (combined rows 0-7 draw at 80 x 48), Q getters return `0xff000779`, `0xff000778`, `0xff00037c` for rows 5-7 |
| Emulator (`xm1-viewer --ram-push-e2e --flash-candidate --trajectories emulation/classic-chrome-q-tile-trajectories.json`, `private-data/analysis/classic-chrome-icon-emu/b-qtile-1/`) | 14 of 14 segments passed, every receipt booting or restoring `98a66780`; the CC tile (`CHROME`), PRO Neg Hi and PRO Neg Std draw at the stock tiles' size, focused black on white and unfocused grey (`segment-cc-q-film-back/screenshots/cc-q-film-cell.png`, `proneg-hi-q-film-cell.png`, `proneg-std-q-film-cell.png`, `cc-q-film-back.png`; `segment-cc-q-film-unfocused/`, `segment-proneg-std-q-film-unfocused/`), PROVIA and Velvia as before (`segment-provia-q-film-cell/`, `segment-velvia-q-film-cell/`) |
| `//emulation:flash-boot-check` | passed on `98a66780…1606`, boot to SHOOTING MENU (`private-data/flash-boot-checks/cc-icon-b-qtile-1/flash-boot-receipt.json`) |

Two viewer e2e runs without `--output` share `private-data/emulator-runtimes/xm1-ram-push-e2e`; run them one at a time or give each its own `--output`.

### 14.7 Reviewer packet addendum

Scope: the bytes of 14.3. The rest of the combined candidate is bound to the reviewed CC candidate by the generator's carry checks and `diff` (section 10).

1. Carry check: `build-classic-chrome-icon diff` against `16c830ad…2c89` shows only the section 9.1 groups plus the tile (font window) and the pointer word (image 2).
2. Q labels: rows 5-7 of `qLabelsArray` and the Q glyphs they name; Q rows 0-4 and the Q values unchanged.
3. Image 2: the one pointer word, the record's size and type words unchanged, and the Q glyph census of 14.2 for `0x37c`, `0x778` and `0x779`.
4. The tile: the `GVFF` layout against the stock blocks (header constants, key paints, path fields, padding rule), the outline encoder against the art, and the placement at `iconTailEnd` in the font tail.
5. Tooling: the `image2-patch-set` step in `ExtensionManifest.hs` and `BuildExtension.hs`, the image-2 patch set, tile append and blob padding in `BuildClassicChromeIcon.hs`, `FujiAF/Gvff.hs`, `FujiAF/ClassicChromeQTile.hs`, the Q families in `ClassicChromeIconCases.hs`.

Class: data only (label array words, an image-2 record word and a vector block that the stock drawer reads); no executable byte changes.

### 14.8 RAM-push carry

The `ClassicChromeIcon` RAM payload (`FujiAF.ClassicChromeIconRam`, `build-classic-chrome-icon-ram`, `transport/ram-push-cc-icon-review.json`) carries the Q tiles, so the abbreviated camera test (section 11) shows the real tiles. It is 162 publications, allocation `0x21fc`, contract `04df9a5a…de32`, approved by review CcIconReview at `b8c7fd9`.

| Change | Where | Publication |
| --- | --- | --- |
| Q labels rows 5, 6, 7 field 0: `0xff000779`, `0xff000778`, `0xff00037c` | the allocation copy of `qLabelsArray` | none: allocation content, reached through the existing Q labels pointer publication |
| The tile | allocation content after the Q labels copy, 8-aligned (1,780 bytes for design B) | none: allocation content |
| Q glyph `0x37c` record pointer | image 2 `0x0022d28c`, old `0x0027dbfc` (stock, also on the RC0 body), new the tile's allocation address | one activation publication |

In the masked activation window the tile pointer is published before the Q labels pointer, which is published before the Q values pointer: once the labels name `0xff00037c`, its record already points at the tile. The installer's masked old-word check covers the new word, and `ram-unload` restores it with the rest under the unload precondition of section 11. The one carried CC publication (`0x0061c3dc`, index 124) is unchanged by the tile, so `carried-publications.json` still lists only that entry.

The publication table grows by one entry, so the installer extent is `0x15c0` and the content offset and allocation follow from it. The read plans are derived from the publications, so they add the window `0x0022d28c` with its old word; the record's size and type words `0x0022d284` and `0x0022d288` are not read on the body, and their stock values (`0x00300050`, 1) are checked by the flash generator against the stock image 2 and by the native Q-record cases. The `--classic-chrome-icon` e2e covers the CC Q tile and the cold-restart read of 162 words; the PRO Neg Q tiles are covered by the flash-candidate trajectories of 14.6, not by the RAM e2e.
