# FILMSIM-OFFER-01: what decides the offered film-simulation list (1.50)

Host static analysis on the pinned official 1.50 images (id4 `id4-0061c400.bin` base 0x0061c400 SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`, id5 `id5-00bf1a00.bin` base 0x00bf1a00 SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`), the body's own adjustment capture (E-ADJ-READ-150) and the reports this file extends ([film-simulation](film-simulation.md), [recipe-design](recipe-design.md), [custom-settings](custom-settings.md)).
Labels are **byte-read** for bytes read from a pinned artifact, **static-inference** for a conclusion drawn from decompiled code or bytes, **hardware-observed** for captured camera state, **inherited** where a named record is reused and **unproved** where the named fact is unavailable.
Markdown paragraphs are one line each.

## 1. Question and verdict

The question is which mechanism keeps PRO Neg Std and PRO Neg Hi out of the menu: an entry absent from the values array, a count or length field, a capability or model byte, a per-entry enable flag, a filter applied when the list is built, or suppression at draw time.

The answer is two gates, both closed for the hidden rows, and nothing else (static-inference over byte-read data):

1. Entry absent from the values array. Every labelled surface's values array names only five conversion-table rows, and rows 5 and 6 (PRO Neg Std/Hi) are named by no values array anywhere in the images, including the unlabelled nine-row list.
2. Suppression at draw time through the availability matrix. The list open path probes each named row, and the probe for property 0xfe consults the matrix `0x00795c98` by the row's col2 value and the current mode; a disallowed value is marked state 3, which removes it from the displayed count and the scroll.

There is no count or length field (both walks are terminator-terminated), no capability or model byte in this path (the matrix is keyed by the runtime mode cell `0x00956838`, not by model), and no static per-entry enable flag (the per-entry state is recomputed in RAM every time the list opens).

**Hardware-observed, 2026-10-09 (E-FILMSIM-INSTALL-150):** the pair of changes C was carried by the RC0 prefix container, installed through the SD card update, and the body's `SHOOTING MENU` to `FILM SIMULATION` shows seven entries including PRO Neg Std and PRO Neg Hi, so both gates are observed on the body for the owner's seven-column choice; the list order beyond the stock five is unreported. What the run shows about rendering is nothing: the entries exist and are selectable, their output is unmeasured.

## 2. Gate 1: the values arrays (byte-read)

A menu node in id4 data is `{id, type, X property, kind, 0, 0, f18, parent, values (+0x20), labels (+0x24), w28, w2c, …}`.
A values array is `{row, child}` 8-byte pairs until the pair `{0x7ffffffe, 0}`; `row` indexes the property's conversion table and `child` is a submenu pointer or 0.
The labels array is parallel: 16-byte `{preview icon, list icon, help, name}` string-id quadruples until `-1`.

The property-0xfe conversion table at `0x0077ac30` is 13 rows of `{col0, 0x7ffffffe, col2}` plus a `0xffff` terminator row (byte-read).
Rows 5 and 6 are `{9, -, 0xb}` and `{0xa, -, 0xc}` (byte-read), so the hidden sims are stock table rows already; no conversion-table change is needed for them.

| Surface | Node | Values array | Labels array | Rows named |
| --- | --- | --- | --- | --- |
| shooting menu FILM SIMULATION | `0x007823a4` (type 2, kind 4, parent `0x0078ea98`) | `0x0078319c` | `0x0078d4f8` | 0 3 1 7 0xc |
| Fn-assigned submenu | `0x00785d74` (kind 5, parent `0x0077f684`) | `0x0078319c` | `0x0078d4f8` | 0 3 1 7 0xc |
| unnamed text node | `0x0077d184` (type 9) | `0x0078319c` | `0x0078d4f8` | 0 3 1 7 0xc |
| Q menu cell | `0x0078b000` (type 0xb, parent `0x0078e4a0`) | `0x00782348` | `0x00789e2c` | 0 3 1 7 0xc |
| FILM 1 bracket child (property 0x100) | `0x00786774` | `0x0077ddf4` | `0x0077d4b4` | 0 3 1 7 0xc |
| FILM 2 bracket child (0x101) | `0x00786da8` | `0x00786724` | `0x0078dcb0` | 0 3 1 7 0xc |
| FILM 3 bracket child (0x102) | `0x007898d4` | `0x0078a0e8` | `0x00788ce4` | 0 3 1 7 0xc |
| unlabelled nine-row list | `0x0078d2d0` (type 8, parent `0x007910a4`) | `0x00780008` | `0x007879f4` (all blank `0x83c`) | 0 3 1 7 0xb 9 8 0xa 0xc |

All eight rows are byte-read from `id4-0061c400.bin` (the map in `private-data/analysis/x-m1-1.50/filmsim/nodes-id4.txt` agrees).
The shared array at `0x0078319c` holds the five pairs at `0x0078319c`, `0x007831a4`, `0x007831ac`, `0x007831b4`, `0x007831bc` and the terminator at `0x007831c4`; the next property's array begins at `0x007831cc`.
The shared labels at `0x0078d4f8` hold `{0x48a, 0x489, 0x48c, 0x48b}`, `{0x48e, 0x48d, 0x490, 0x48f}`, `{0x492, 0x491, 0x494, 0x493}`, `{0x49f, 0x49e, 0x4a2, 0x4a0}`, `{0x4b0, 0x4af, 0x4b2, 0x4b1}` and the 16-byte `0xff` terminator at `0x0078d548`; node `0x0078d558` follows (byte-read).

Rows 5 and 6 appear in none of these arrays (byte-read), and neither does any pair whose row is 5 or 6.
The nine-row list is the monochrome-variants list (col2 7, 8, 9, 0xa added); it does not carry the PRO Neg rows either.
The Q cell's labels are vector-glyph ids `0xff00077b`, `0xff00077c`, `0xff000773`, `0xff000777`, `0xff00077a` (byte-read), which is the closed glyph space of recipe-design P5.

There is no count field: the counter `FUN_00bf9794` (id5) walks the pairs until `{0x7ffffffe, 0}` and counts them, and the parallel label walk stops at `-1` (static-inference).
The list machinery reaches rows only through these arrays, so an unnamed row cannot be offered by any other mechanism.

## 3. Gate 2: draw-time suppression through the availability matrix

When a list opens, `FUN_00bfa800` (id5) walks the named rows and stores one state per entry at `DAT_009571f0`, computed by the render probe `FUN_00d52684(node.property, row)` (static-inference).
For property 0xfe the probe resolves the row's col2 with `FUN_0061d514(prop, row, 0, 3)` and then asks the availability query `FUN_0061d534(current mode, 0xfe, col2)`; a disallowed value yields state 3, and if fewer than two values are allowed the probe returns 3 for every entry (static-inference, decompiled case 0xfe of `FUN_00d52684`).
State 3 entries are subtracted from the displayed count (`FUN_00bf9794` mode 0x37), skipped by the row-to-index map `FUN_00bf9990` and skipped by the scroll `FUN_00bf9b1c`; state 2 means shown but not selectable (static-inference).
The state array is recomputed on every open and is not stored in the node data (static-inference), so it is not a static per-entry flag.

The queries sit behind four wrappers at `0x0061d534`/`0x0061d544`/`0x0061d554`/`0x0061d564`, thin thunks to `FUN_00631b74` (allowed), `FUN_00631d58` (mode default), `FUN_00631edc` (allowed count) and the value-or-default wrapper `FUN_0061d564` (static-inference).
The descriptor list at `0x0079944c` binds property 0xfe to the matrix at `0x00795c98` (byte-read).

The matrix is 13 rows of `{key, 22 cells}` at `0x00795c98`, keyed by col2 in row order `0, 3, 1, 0xb, 0xc, 4, 7, 9, 8, 0xa, 5, 2, 6` (row bases `0x00795c98` stepping `0x5c`), with the mode columns listed at `0x00792fac` in order `1, 0, 0x1f, 0x20, 0x24, 0x23, 0x22, 0x21, 5, 0x18, 8, 0xb, 0xc, 0xd, 0x12, 0x13, 0x11, 0x10, 0x16, 0x15, 0x17, 0x2c` and terminated by `0x2d`; a cell of 1 or 2 allows the value (byte-read rows, static-inference for the semantics).

| Key (col2) | Cells of 1 or 2 (mode columns) | Offered row that carries it |
| --- | --- | --- |
| 0 | 2 in all 22 (default) | row 0, PROVIA |
| 1, 3 | 1 in modes 0, 0x1f, 0x21, 0x22, 0x23, 0x24, 0x2c | rows 1, 3, ASTIA, Velvia |
| 4, 5 | 1 in those 7 plus mode 1 | rows 7, 0xc, B&W, SEPIA |
| 2, 6, 7, 8, 9, 0xa, 0xb, 0xc | none, all cells 0 | no offered row |

This closes the numbering question the unlock plan raised: the arrays index conversion-table rows while the matrix is keyed by col2, so the offered rows `{0, 3, 1, 7, 0xc}` carry col2 `{0, 3, 1, 4, 5}` and touch only the allowed keys.
Matrix keys 7 and 0xc are col2 values no offered row carries, and their zero rows keep nothing out of the five-item list.
For rows 5 and 6 the key rows at `0x00795dac` (key 0xb) and `0x00795e08` (key 0xc) are all zero (byte-read), so even a named row would draw hidden.

## 4. Candidate changes and what each yields

| Change | Bytes | Offered list after the change |
| --- | --- | --- |
| A. Matrix cells only | set chosen cells to 1 in the key-0xb row `0x00795dac` and key-0xc row `0x00795e08`, cell `+4·column` into the `0x792fac` list | unchanged, 5 entries on every surface: rows 5/6 are not named, so no entry exists to unhide. Side effect only: a stored col2 0xb/0xc value stops being replaced by the mode default in the C-record compare `FUN_00d4bbc4`, the one caller of `FUN_0061d564` |
| B. Values arrays only | append `{5, 0}` and `{6, 0}` pairs plus two label quadruples `{0x497, 0x496, 0x495, 0x498}` (PRO Neg Hi) and `{0x49b, 0x49a, 0x499, 0x49c}` (PRO Neg Std) | unchanged, 5 visible entries: the two new rows are named and enumerated but the probe still marks them state 3, so the count and scroll skip them |
| C. A and B together | A plus B | 7 entries on each extended surface: PROVIA, Velvia, ASTIA, B&W, SEPIA, then PRO Neg Std and PRO Neg Hi in appended order. The Q cell and the bracket lists stay stock unless their own arrays are extended too |
| D. Replace two rows in place | row word `0x007831b4` 7→5 and `0x007831bc` 0xc→6, label quadruples at `0x0078d528`/`0x0078d538` swapped, plus matrix cells | 5 entries: PROVIA, Velvia, ASTIA, PRO Neg Std, PRO Neg Hi, with B&W and SEPIA gone from that surface. Rejected: fewer words but it removes stock modes |
| E. Code patch to `FUN_00d52684` or `FUN_00631b74` | instruction edits in id5 | would bypass gate 2 for every property, not just 0xfe. Unnecessary and the only change class that risks the running image's control flow |

The arrays cannot be extended in place (byte-read): the values terminator at `0x007831c4` is followed by another property's array at `0x007831cc`, and the labels terminator at `0x0078d548` is followed by node `0x0078d558`.
An additive change therefore relocates the two arrays into free space and swaps the two pointer words per surface (node `+0x20` values, `+0x24` labels), for example the three words at `0x007823c4`/`0x007823c8`, `0x00785d94`/`0x00785d98`, `0x0077d1a4`/`0x0077d1a8` (static-inference from the node layout).

**Smallest and safest.** No single change reveals anything: gate 1 and gate 2 are both closed, and each single change leaves the other closed.
The smallest single write is the matrix cells of change A (one word per key per column) but it is inert alone.
The smallest change that actually reveals the hidden sims is the pair in change C, and the safest change is the same pair: both parts are data only, the stock five rows keep their bytes and order, the conversion table, the dispatchers `FUN_00d78418` and `FUN_00eaac74`, the metadata path and the code of the probe are untouched, and the matrix edits add allowance in two all-zero rows where no value can lose availability.
Smallest and safest are the same change.

The label quadruples referenced above already exist in every language slot (byte-read of the EN slot dump `strid-en.txt`, ids `0x495`-`0x49c`): preview icon, list icon, help line and name for both PRO Neg modes, all below the `0x83c` class boundary, so no new string id, no new label record and no language-slot work is needed (byte-read, inherited for the other 34 slots from the shared id space).
The Q cell would need glyph-space records for U+F201/F202 and that is P5's unbound residual, so the Q cell stays stock in a first change.

## 5. Downstream completeness per hidden code

The chain downstream of the menu is value-keyed: `FUN_00d78418` maps col2 to the 8-value develop code (0 from `{0, 2, default}`, 1 from 6, 2 from `{4, 7, 8, 9, 0xa}`, 3 from 5, 4 from 3, 5 from 1, 6 from 0xb, 7 from 0xc) and `FUN_00eaac74` maps col2 to a metadata tag (inherited, [film-simulation](film-simulation.md#processing-by-value)).
Each develop stage then selects parameter bytes from the body's adjustment area through u16 offset tables in id5: colour `0xd6891c`, chroma `0xd621a0`, hue `0xd62040`, tone families `0xd684a0`/`0xd683b0`, curve ids `0xd685f4` (byte-read of the tables, inherited for the stage semantics).
The body's bytes at every one of those offsets equal the firmware fallback defaults, including the whole extents of the nine develop tables and the tone region `0xcb77`-`0xccf9`, and moved nothing between the 1.00 and 1.50 captures (byte-read, E-ADJ-READ-150).

| col2 | Row / col0 | Name | Develop code | Tone family | Curve keys (level2 1-5) | Colour / chroma / tone / hue (row 0; tone also rows 1-2) | Processing verdict on this body |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0xb | 5 / 9 | PRO Neg Std (probably) | 6 | 4 | 0x11, 0x12, 0x13, 0x13, 0x13 | 4 / 4 / 4 / 3 | complete and distinct: own colour matrix 4, chroma 4, tone 4, hue range 3, family-4 curves. Family bytes `0xccf1`-`0xccf3` read 04 and curve-id block `0f 11 12 13 13 13` on the body (byte-read) |
| 0xc | 6 / 0xa | PRO Neg Hi (probably) | 7 | 5 | 0x16, 0x17, 0x18, 0x18, 0x18 | 5 / 5 / 5 / 3 | complete and distinct: colour 5, chroma 5, tone 5, hue 3, family-5 curves. Family bytes `0xccf7`-`0xccf9` read 05 and block `14 16 17 18 18 18` on the body (byte-read) |
| 2 | 2 / 2 | not named | 0 | 1 | 1, 1, 1, 1, 1 | 1 / 1 / 1 / 1 | complete but not distinct: PROVIA's parameters |
| 6 | 4 / 8 | not named | 1 | 1 | 1, 1, 1, 1, 1 | 1 / 1 / 1 / 1 | complete but not distinct: PROVIA's parameters |
| 7 | 11 / 7 | second monochrome (probably) | 2 | 1 | 1, 1, 1, 1, 1 | 1 / 1 / 1 / 1 | complete but not distinct: B&W output, and `FUN_00eaac74` tags it 0x300 like col2 4 |
| 8, 9, 0xa | 8, 9, 10 / 4, 5, 6 | filtered mono, probably +R/+Ye/+G | 2 | 1 | 1, 1, 1, 1, 1 | 1 / 1 / 1 / 1 | complete for code 2 but the filter branch is dead: the still-develop builders zero `+0x110` and `FUN_00d7851c` ignores the film value, so the filter matrices `0xd680f8` are unreachable and the output is plain B&W under filtered metadata 0x301-0x303 |

The tone half of the table is byte-read on this body: the family-offset table `0xd684a0` sends codes 6 and 7 at mode 0 to the adjustment offsets `0xccf2`/`0xccf1`/`0xccf1`/`0xccf1`/`0xccf3` and `0xccf8`/`0xccf7`/`0xccf7`/`0xccf7`/`0xccf9` (byte-read of id5), whose body bytes read 04 and 05 (byte-read, E-ADJ-READ-150), and the 37-record curve table at `0x824bc8` (count word `0x824bc4` = 0x25) carries keys 0x11-0x13 and 0x16-0x18 with real monotone curves ending 0x3fff, distinct from record 1 (byte-read).
The colour, chroma, tone and hue selector ids per code are the reviewed row-0 reading of [film-simulation](film-simulation.md#processing-by-value) over the fallback image, and the body holds those same bytes (byte-read, E-ADJ-READ-150). That reading is complete for colour, chroma and hue: the row term `rec+0x108` is written only 0, so those three selectors index row 0 only (byte-read, E-ADJ-READ-150 limits); the H/S-tone and `0xd6a5c4` selectors can also reach rows 1-2 through the `0x0073f940` row byte.

**Reachability verdict.** No hidden code is rejected by the processing chain: every col2 value from 0 to 0xc has a develop-code case and a metadata case, every family byte the body supplies is in range and resolves to a present curve record, and the colour ids validate 1 to 20 (inherited).
A menu entry for PRO Neg Std or PRO Neg Hi selects a code with complete, distinct processing data on this body; a menu entry for col2 2, 6, 7, 8, 9 or 0xa would select complete data that renders as PROVIA or B&W, which is why spec/10 keeps filtered B&W out of RC0 and why the offer list is the two PRO Neg modes.
The failure mode to avoid is a mapping error, not missing data: pointing a PRO Neg row at col2 0 or 1 would render PROVIA or ASTIA under a PRO Neg label (sibling failure modes 1 and 4 of recipe-design section 14), which is why change C leaves `FUN_00d78418`, the col2 values and the conversion table untouched.

Unproved residuals, kept out of the verdict: whether the H/S-tone and `0xd6a5c4` selectors' row 1-2 states are entered in an ordinary capture flow (colour, chroma and hue index row 0 only and that question is closed, E-ADJ-READ-150 limits); the hardware programming that follows the parameter block; any reference output for PRO Neg on this body; the live-view, playback and RAW-conversion render paths.

## 6. RAM or storage: the change is RAM-resident

Every deciding byte is loaded-firmware data: the nodes, values arrays, labels arrays and the conversion table are image-4 data at `0x0061c400`, the probe and the offset tables are image-5 data at `0x00bf1a00`, and the matrix `0x00795c98` is image-4 data (byte-read of file extents).
Image 4 is decompressed from internal storage at every boot (E-BOOT-LOADPATH, static-inference), and captured installed RAM equals the decoded images over their extents (E-DUMP-RAM-150, hardware-observed).
No persistent store carries a menu list: the ordinary settings block (image `0x10`-`0x127`, including the film-sim selection nibble at image `0x8a`) and the area-3 C records hold selections and presets only, and no mapped writer stores node, array or matrix data (inherited, [custom-settings](custom-settings.md), [recipe-design](recipe-design.md) P2/P10).

So a RAM patch to the arrays, the node pointers or the matrix cells survives only until power-off, and the next boot rebuilds the stock lists from firmware. On this body the lists now come from installed firmware instead: the RC0 prefix container carrying the pair was flashed on 2026-10-09 (E-FILMSIM-INSTALL-150), so a power cycle no longer reverts them.
That is the property a reversible demonstration needs: one power cycle removes the change with no restore step and no persistent trace.
Persisting the unlock needed exactly that firmware write, and it ran on 2026-10-09 under the practice decision recorded for this class (E-FILMSIM-INSTALL-150; RECOVERY-R4-PRACTICE-01, explicit informed acceptance, R4 itself still unclaimed, [spec/04 section 10.3](../spec/04-recovery.md#103-required-failure-classification)); nothing in the settings commit paths would persist a list patch (static-inference).

The service interface's RAM word-write primitive (whitelist value 0x24, `FUN_00dbc924`, one word per write inside `[0, 0x1fffffff]`) reaches every deciding byte named above, and each change-C edit is a single word write (inherited from [the adjacent service hazards](adjustment-and-service-interfaces.md#adjacent-service-hazards), promoted from private `private-data/analysis/x-m1-1.50/end-setting/followup/raw-media-read.md`; the primitive itself is unexercised, so its use stays unproved until a reviewed operation runs it).

## 7. Answers in one place

- Deciding mechanism: absence from the values arrays (rows 5/6 named by no array) plus draw-time suppression by the availability matrix `0x00795c98` through `FUN_00d52684` case 0xfe and `FUN_0061d534`. No count field, no capability byte, no static per-entry flag.
- Smallest change: matrix cells alone are the smallest write but reveal nothing; the smallest revealing change is the pair, relocated values/labels arrays with two pointer words per surface plus the matrix cells.
- Safest change: the same pair. Smallest and safest coincide once the two gates are both addressed; anything smaller leaves one gate closed, and code patches are the only class that risks more than data.
- Offered list after the pair on an extended surface: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi; a seven-entry list including both PRO Neg names is what the body showed after the 2026-10-09 install (E-FILMSIM-INSTALL-150), with the order beyond the stock five unreported; Q cell, bracket lists and the nine-row node unchanged unless separately extended.
- Downstream: PRO Neg Std (col2 0xb, code 6) and PRO Neg Hi (col2 0xc, code 7) have complete, distinct processing data on this body (byte-read curves and selector bytes, E-ADJ-READ-150); the other six hidden codes complete but indistinct, four of them plain B&W under filtered labels.
- Reversibility: RAM-resident and rebuilt from firmware at every boot, so a power cycle reverts a RAM patch; this body's list now comes from the installed RC0 container instead (E-FILMSIM-INSTALL-150), and returning it to the stock five would need another flash.

## Reproduction

All commands read host files only.

```sh
# values array 0x0078319c, labels 0x0078d4f8, node 0x007823a4, matrix 0x00795c98, mode list 0x00792fac
xxd -s $((0x78319c-0x61c400)) -l 64  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x78d4f8-0x61c400)) -l 96  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x792fac-0x61c400)) -l 96  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x795c98-0x61c400)) -l 1196 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# conversion table 0x0077ac30 and tone records 0x824bc8
xxd -s $((0x77ac30-0x61c400)) -l 176 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x824bc8-0x61c400)) -l 2664 -e -g4 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# selector offset tables in id5
xxd -s $((0xd6891c-0xbf1a00)) -l 32 -e -g2 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s $((0xd621a0-0xbf1a00)) -l 32 -e -g2 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s $((0xd62040-0xbf1a00)) -l 32 -e -g2 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s $((0xd684a0-0xbf1a00)) -l 80 -e -g2 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin

# decompiled list machinery and probe (private analysis artifacts)
# private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c: FUN_00bf8d20, FUN_00bf9794, FUN_00bf9990,
#   FUN_00bf9b1c, FUN_00bfa800, FUN_00631b74, FUN_00631d58, FUN_00631edc, FUN_0061d564
# private-data/analysis/x-m1-1.50/filmsim/fn_d52684.c: case 0xfe of FUN_00d52684
# private-data/analysis/x-m1-1.50/filmsim/nodes-id4.txt, filmsim/strid-en.txt
```
