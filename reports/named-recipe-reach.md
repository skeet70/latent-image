# Named custom recipes on X-M1 1.50: what the body has, what naming needs, what fits

Host-only assessment on the pinned official 1.50 images and this body's captured adjustment data. No camera was enumerated, opened or commanded; no patch bytes are produced and no storage is written. It answers [spec/10 section 19.3](../spec/10-film-recipes.md#193-tiers) tier RC1's reach question after FILMSIM-PATCH-01: [the recipe design](recipe-design.md) fixes the data structures and behaviour, and this report fixes what of it has a home on this body.

Evidence labels: **byte-read** for bytes read from a pinned artifact in this pass, **inherited** for a named record reused without re-derivation, **hardware-observed** for captured camera state, **static-inference** for a conclusion drawn from those bytes, **unproved** for a named missing fact.
Markdown paragraphs are one line each.

## 1. What the body has today

### 1.1 The one stored preset

The C position's record is [the custom-settings map](custom-settings.md) as recorded in E-CUSTOM-SETTING-MAP-150: logical log area 3, record size `0x34`, count `0xfe`, opened by `0x006a8abc` through `0x006ac328(3, 0x34, 0xfe, 0, ...)`, with layout log sequence halfword at `+0x00`, two compared bytes at `+0x02`, the `0x28`-byte payload at `+0x04`, word parity at `+0x2c` and bit-plane parity at `+0x30` over the parity span `[0x00, 0x2c)` (inherited, E-CUSTOM-SETTING-MAP-150).
The area's two banks hold 127 records each (descriptors `0x007c8d38` = `{1, 0x3d8, 3}` and `0x007c8d44` = `{1, 0x3d9, 3}`), and the read path takes the newest record by sequence: `0x006a8abc` reads through `0x006a865c` into the live copy `0x0089f838`, copies the payload to the settings image at `0x409c295c` (mirror offset `0x140`) and mirrors the whole `0x34` bytes to `0x0089f874` (inherited, byte-read sites in E-CUSTOM-SETTING-MAP-150).
The record is therefore a log of one logical setting, not a slot table: the 254 record positions are append history, and nothing in the image selects an older record.

### 1.2 How many slots

The property layer's addressing admits three slot records per bank image: `0x0096cc64 + 0x1c2*bank + 0x140 + 0x28*slot`, whose `+0x140` is payload byte 0 (inherited, E-CUSTOM-SETTING-MAP-150 section 3), and property `0x16d`'s conversion table at `0x0077532c` has three rows with stored codes `0`, `1`, `2` (byte-read: rows `{00000000}`, `{00000001}`, `{00000002}`, each with col2 `0x7ffffffe`, then a terminator row), which is the same three.
The bank image is `0x1c2` bytes, so three `0x28` records at `0x140`, `0x168` and `0x190` fit before its end at `0x1b8` (arithmetic over the inherited bank layout).

Shipped behaviour uses one of the three.
The save action `FUN_00d4e2a0` converts the source and destination rows through property `0x16d`, returns unless the destination code is 0 (`0xd4e2e4 cmp r4, #0`, `0xd4e2e8 popne`, inherited), and its only store is `FUN_0072b3d0({&0x0096cda4 + 0*0x28, 0x28, 0x140})`, the mirror slot 0 (inherited).
The boot read fills slot 0 only (`FUN_0061d994(&0x0096cda4, 0)`), the CUSTOM RESET action rewrites slot 0 only (E-CUSTOM-RESET-150), and the record-target store a property write never issues (P16, hardware-observed E-CPOS-EDIT-VOLATILITY-150).
Slots 1 and 2 have no writer, no reader, no menu row and no persistence path in the mapped code; their payload semantics beyond the addressing arithmetic are **unproved**.

### 1.3 How a slot is selected

The mode dial's `C` detent closes one input-map line, key code `0x317`, named `KEY_C1_MODE` in the key table at `0x007a01b0`; state `0x317` maps to class 3, and `FUN_006439e0` returns slot 0 while the mode-class cell `0x00951564` reads 3 (byte-read, P15).
The property layer's current values are then the stored record's fields, which is the applied preset; a setting changed at C survives moving the dial away and back and is lost at a power off unless the record was registered (hardware-observed, E-CPOS-EDIT-VOLATILITY-150).
Registration is the SET-UP item `CUSTOM SET` (property `0x16d`), which copies the current values into slot 0 and commits area 3 on the spot; `CUSTOM RESET` rewrites slot 0 from the per-slot defaults block `0x007cb804` and commits the same way (inherited, E-CUSTOM-RESET-150).
The ordinary (non-C) settings block at image `0x10`-`0x127` persists separately through area 2 at an orderly power-off (P10); the C record does not use that route.

### 1.4 What a slot stores, and what it does not

The payload's bound fields are `custom-settings.fields.json` plus the recipe-design prerequisite pass.
The nine fields a recipe model needs are all present and byte-read in the payload: film simulation `0xfe` at `+0x24` bits 0-3, dynamic range `0x104` at `+0x05` bits 6-7, highlight tone `0x110` at `+0x17` bits 5-7, shadow tone `0x111` at `+0x09` bits 5-7, colour `0x10e` at `+0x07` bits 5-7, sharpness `0x10f` at `+0x08` bits 5-7, noise reduction `0xef` at `+0x10` bits 5-7, white-balance mode `0xed` at `+0x21` bits 0-3 and ISO `0x141` at `+0x20` bits 0-4.
Beside them the payload carries image quality `0xfa`, image size `0xf6`, the aspect group `0xf9`, focus mode `0x129`, photometry `0x1c2`, the AE-bracket step `0x139`, the bracketing/drive selector `0x1b1`, the movie frame rate `0x214`, the display-information setting `0x1d8`, the white-balance fine shift `0xee`, about sixty further bound properties and the recorded mode `0xe9`.

Not covered, with the reason:
- a name. No payload field is a text, index or label field; the payload's unbound space is four bytes plus five bits, measured in section 2.1.
- a second and third preset. The format and the property layer address three slot records, and no shipped path writes, reads, selects or persists any but slot 0 (section 1.2).
- the two-axis white-balance shift. Properties `0x113`/`0x114` have no payload field, and the one payload scalar `0xee` is a single 31-row axis (P13).
- long-exposure NR. Offered as OFF/ON, with no mapped payload field (P8 residual).
- any menu identity. The record stores values; nothing stores which preset produced them, and no menu surface shows a preset name.

### 1.5 The one menu surface a recipe could use

Property `0xfe` drives eight list surfaces, three of which share the values array `0x0078319c` and the labels array `0x0078d4f8`: the shooting-menu node `0x007823a4`, the Fn-assigned node `0x00785d74` and the type-9 node `0x0077d184` (inherited, FILMSIM-OFFER-01 section 2).
A row is a `{row, child}` pair in the values array plus a parallel 16-byte `{preview icon, list icon, help, name}` quadruple of string ids in the labels array, terminated by `-1`; the drawing of those quadruples is what makes this list the only text-bearing preset surface.
The Q cell `0x0078b000` names its rows with vector-glyph ids in the closed `0xff000000 | n`, `n <= 0x8c5` space, and the RAW-conversion node `0x0078bd5c` has its own values and code domain; neither takes a recipe name (P5, FILMSIM-PATCH-01 section 1).

## 2. What naming would take

### 2.1 Storage for a name

Three measured candidates exist, and none of them holds a name.

1. The payload, `0x28` bytes. The property field map binds bytes `+0x04` through `+0x27`; bytes `+0x00`-`+0x03` carry no field in the getter's 645-entry dispatch table at `0x0061e12c`, and of `+0x27` only bit 0 (property `0x1e8`) and bits 1-2 (property `0x1a`) are bound (byte-read, `custom-settings.fields.json` and its open item 2). Spare: four bytes plus five bits, which is under one three-character UTF-16 name.
2. The record's two unnamed bytes at `+0x02`/`+0x03`, which the commit routine `0x006a86c4` compares and the parity covers, with no writer in image 4 (inherited, E-CUSTOM-SETTING-MAP-150). Two bytes.
3. The settings image's ordinary block at image `0x10`-`0x127`, whose tail bytes `0x124`, `0x125` and `0x127` are `0x00` in the image-resident ordinary defaults block `0x0089e8e0` and unclaimed by any property (inherited, recipe-design P1/P2). Three bytes.

A name needs its own storage, and the only patch-owned RAM this body has is section 3's decode tail.

### 2.2 Text source

The firmware's text source is the per-language string table, and it is closed.
Each language slot is loaded at `0x0039c000` from flash `0x1100000 + lang·0x20000` (one 128 KiB block per language, 35 languages), and the table is `0x83c` entries of 8 bytes `{w, attr, pad, lineptr}` whose line pointer leads to a 12-byte line record and then to UTF-16 text (inherited, film-simulation report section "UI strings"; the loader is `FUN_006aa63c`).
The count `0x83c` is hard-coded, and every id at or above it leaves the ordinary path (section 2.3).
Three special ids exist above it: `0x83c`, `0x83d` and `0x83e`, served from 66-byte records at image-2 RAM `0x37b784` whose first 56 bytes are an inline UTF-16 buffer and whose five layout halfwords sit at `+0x38`-`+0x40` (byte-read: the layout accessors compute `base + 66*(id-0x83c)` and load `[+56]`, `[+58]`, `[+60]`, `[+62]`, `[+64]`).
Those three records are byte-read in this pass: `0x83c` holds `? A M ?` with layout `{1, 1, 0x1e, 0x24, 0x30}`, `0x83d` holds 24 glyphs in the `0xe1xx` code range with layout `{0x14, 0x14, 0x1e, 0x24, 0x30}`, and `0x83e` is 66 zero bytes.
No node label in the 1.50 node map references `0x83d` or `0x83e` (byte-read: zero matches for `#83d` and `#83e` over the 1048-node listing); `0x83c` is the blank id used throughout.
A whole-image reference census for the two ids is **unproved**.

One text-entry surface exists on this body and it is not a naming surface for settings.
The SET-UP `WIRELESS SETTINGS` item `0x787018` (`kind 6`, property `0xb5`) has `NAME` as value 6 `0` with child `0x7894e0`, and the PC AUTO SAVE item `0x787b4c` (property `0xa6`) reaches the same child; the labels are string ids 406 and 407, both `"NAME"` (byte-read, node map and English string dump).
A child address with no node header means an action-driven editor, so a text-entry widget for the camera's wireless name exists in the 1.50 build; its implementation, its storage field and the cost of reusing it are **unproved**.
There is no menu item anywhere in the 1.50 node map that names a setting, a preset or the C record, and the owner's 1.00 and 1.50 menu records list none (owner report, E-RECIPE-150-OWNER).

### 2.3 Drawing path

The list draw path renders stored strings, and that half is already there.
The film-simulation rows the owner sees are drawn from the four string ids of a labels-array quadruple: row 1 is `{0x48a, 0x489, 0x48c, 0x48b}` and resolves to `<f06a>` icons, the help line `Ideal for a wide` and the name `<f06f>/STANDARD` (byte-read, FILMSIM-OFFER-01 section 2 and recipe-design section 4).
Text is resolved at draw time through the resident `FONT_LATIN.FDL`, whose cmap covers every printable ASCII code, with UI-pixel metrics and measured row boxes of 440 px for a name and 400 px for a help line (P6).
So the drawing path needs no change for a name; what it needs is an id that resolves to the name's text.

Resolution of new ids is the expensive half, and its bounds are byte-read.
The entry accessors `0x0117de18` (attribute byte), `0x0117de64` (width halfword) and `0x0117deb0` (entry pointer) split at `id <= 0x83b`, the first two guarding with `id-1 <= 0x83d` and the third with `id <= 0x83d` (byte-read: `0x0117de24`-`0x0117de30`, `0x0117de70`-`0x0117de7c`, `0x0117deb8`-`0x0117dec0`); for ids above the split the attribute returns `1`, the width returns `50`, and the entry pointer returns `base + id*8` for `id <= 0x83d` and `0` for `0x83e`, so for `0x83c` and `0x83d` it indexes just past the `0x83c`-entry language table (byte-read: `0x0117de2c`-`0x0117de50`, `0x0117de78`-`0x0117de9c`, `0x0117debc`-`0x0117ded0` in the plain image).
The locator `0x0117dfd8` is the one accessor that returns the special record base for `0x83c`-`0x83e`; its only direct caller is `0x01179b20`, while the draw-family functions `0x0117dee0` and `0x0117df70` call the entry-pointer accessor, and `0x0117dee0` writes the constant `100` for an id above `0x83b` instead of reading metrics (byte-read, plain-image disassembly).
Whether a film-sim *list* row whose label id is `0x83d` or `0x83e` draws its inline text is therefore **unproved**, and it is the one question that decides whether a name can come from spare stock storage instead of from patch-owned ids and accessor hooks.
It is cheap to settle offline: the project's native harness already executes the film-sim list open and its draw-time check (FILMSIM-EMUL-01), so a case over the text-resolution call with a label id of `0x83e` costs one fixture, no hardware and no new admission.

### 2.4 Answer to the three parts

Storage for a name does not exist: the record, the payload and the settings image together hold under ten unclaimed bytes, and a name is at least tens. Any naming design must supply storage.
A text source exists but is closed: one string table per language with a hard-coded count, three special ids above it, and one wireless-name editor that proves a text-entry widget exists without giving any setting a user-chosen name.
The drawing path exists and takes a stored string: names and help lines render through string ids today, and a patch does not have to touch the renderer.
Naming on this body is therefore a storage-and-id problem, not a renderer problem, and the storage it needs is measured next.

## 3. The patch-owned space this body has

The film-sim work established the only placement that survived an ownership test, and its scale: a decode prefix in front of image 4 ([filmsim-patch-01](filmsim-patch-01.md) section 5).
This pass measured its remaining extent from the same artifact the accepted run used.

Byte-read, `payload-complement.bin` mapped at RAM `0x0061c000` (offset `0x140048`): the boot-copied table block holds eight segment records at RAM `0x0061c088`-`0x0061c1a7` (ids 0-7, image 4's being `{4, 0x0061c400, 0x00282828, 0x0089ec00, 0x00352e00, 0x00160000, 0x00160000, 0x20, 1}`) and plain-extent records running to a `0xffffffff` word at RAM `0x0061c240`; RAM `0x0061c244` through `0x0061c3ff` is zero in that artifact.
The zero tail is `0x1bc` = 444 bytes.
FILMSIM-PATCH-01 section 5.3 puts the retained objects below `0x0061c200`; the byte-read boundary is `0x0061c244`, because the plain-extent records extend past `0x0061c200`. The accepted window `[0x0061c340,0x0061c400)` sits above them either way, so the accepted prefix is unaffected, and a widened prefix has a hard floor at `0x0061c244`.
**Correction 2026-10-09** (E-FILMSIM-PREFIX-WIDENING-150, [the widening test](filmsim-prefix-widening.md)): the terminator word is at RAM `0x0061c1f8`, not `0x0061c240`, so the floor is `0x0061c1fc` and the free tail is `0x204` = 516 bytes. This section's od commands print *file* offsets in the payload export, whose bytes sit `0x48` below their media offsets (payload = media + `0x48`); the RAM addresses above were read off those file offsets. The accepted window and every other conclusion in this report are unaffected.

The accepted prefix is `0xc0` = 192 bytes of that tail, leaving `0xfc` = 252 bytes below it.
This pass ran the patch's own descriptor census (`extension-census.c`) over the wider window `[0x0061c244, 0x0061c400)` with the accepted run's eleven inputs, writing `private-data/analysis/x-m1-1.50/recipes/prefix-tail-census.txt` (SHA-256 `837885d3f5584d55bd78155fcfbb9c787161a8c5f5664ea210d25c5e86406676`, byte-read).
The wider window adds no direct address literal and no resolved `movw`/`movt` pair: the only two direct hits are the unaligned instruction fragments `0x0082d5f2` reading `0x0061c352` and `0x00f4ba7e` reading `0x0061c3a0`, both inside the already-accepted window, and the six wide raw `{base, size}` pairs that sweep the window are the same six the accepted run reports and left unclassified.
The 252 bytes below the accepted prefix therefore carry the census profile of the accepted 192, and no ownership acceptance covers them; extending the accepted argument to them is review work, not new evidence.

The other loaded images have no such axis (static-inference from the segment records, byte-read): image 5 loads at `0x00bf1a00`, which is image 4's bss end, so a prefix on image 5 would overwrite image 4's bss; image 4's own tail cannot grow because the bss-clear base is fixed at `0x0089ec00` and tail writers sit at its start ([filmsim-patch-01](filmsim-patch-01.md) section 5.4).
The plain image's predecessor extent is not established here: segment entries 5 and 6 carry the same load `0x00bf1a00` and size `0x004f1e60` with different flash bases, so the record that bounds the plain image's lower neighbour is unproved.
Patch-owned RAM on this body is the 444-byte image-4 decode tail, of which 192 bytes are spent.

## 4. Designs, sizes and risks

### 4.1 Item costs

Measured or computed from the byte-read structures above: a values-array row 8 bytes, a label quadruple 16 bytes, a conversion-table row 12 bytes, a special-class label record 66 bytes per id (56 bytes of it inline text), one UTF-16 character 2 bytes.
The RC1 table entry is 8 bytes of metadata plus the `0x28`-byte payload (recipe-design section 7); a compact form that carries only the nine stored codes needs 5 bytes (film 4 bits, dynamic range 2, three tone fields 3 each, colour 3, sharpness 3, noise reduction 3, white-balance mode 4, ISO 5 = 33 bits) and 6 bytes with alignment.
The schema's rendering bounds cap a name at 12 characters and a help line at 18, so a full name plus help is at most 60 bytes of text.

### 4.2 The designs

**D0, stock C slot. 0 bytes, no code.** Set the nine values by hand and run `CUSTOM SET`; recall at the dial's C position. One preset, no name, no menu entry, no second preset. This is what the body does today.

**D1, the film-sim unlock (RC0), built and reviewed. 192 prefix bytes plus 14 matrix cells and six pointer words, 68 words in all, all data.** It adds two stock-named modes (PRO Neg Std, PRO Neg Hi) to three list surfaces. It is not a recipe: it names processing, not presets, and it is the base-mode set any recipe design inherits. Placement accepted by static inference only, the prefix's runtime writer absence is unproved, and its install decision is the owner's.

**D2, one compiled recipe as the C-slot default. 40 in-place bytes, no code, no relocation.** Replace the per-slot defaults block at `0x007cb804` (image 4, inside the checksummed range) with the recipe's payload; the stock `CUSTOM RESET` item then rewrites slot 0 from it and commits area 3 through the mapped chain (E-CUSTOM-RESET-150). The owner gets one compiled look that a stock menu action registers into the C position and that survives as the C record, with the film field free to name a PRO Neg code once D1 ships.
Risks: it redefines what `CUSTOM RESET` means for the C record, and the same block feeds the getter's defaults branch and the jig-cycle constructor `0x006a877c`, so after a settings-loss event the C record comes up as the recipe rather than as PROVIA. It gives no name, no menu row and no second preset, and changing the recipe needs a rebuild.

**D3, compiled recipe rows on the unlock's arrays. About 154 added bytes and one code hook, for two named rows.** Extend the two relocated arrays by two values rows and two label quadruples (+48 bytes: the arrays go from the current 192 bytes to 240), rewrite two inert conversion-table rows in place with the free stored codes `0xd` and `0xe` and the base modes' col2 values (+24 bytes), store the two compact parameter blocks (+12 bytes), and add one hook at the film-sim commit case that applies the eight parameter values for a selected recipe row (a 4-byte site patch in image 5 plus a body estimated at 40-80 bytes in the prefix). Names come from the two spare special ids `0x83d`/`0x83e`, whose inline UTF-16 buffers are rewritten in place in image 2 (0 bytes of prefix, and at most 28 characters each).
The two buffers and their container offsets are byte-read: id `0x83d`'s text sits at DAT file offset `0x00eeba22` and id `0x83e`'s at `0x00eeba64`, each 56 bytes stored complemented (RAM is the complement of the file byte, the image-2 extent mapping RAM `0x00130000` through file `0x00ca025c`), with id `0x83e`'s layout words all zero at `0x00eeba9c` and id `0x83d`'s already set; both buffers lie inside the main checksummed range, so the edit recomputes the main checksum word at raw DAT `0x20c` and needs a container write operation for image 2, which `rebuild-update replace-record4` does not provide.
Cumulative: 192 + 154 = about 346 bytes of the 444-byte tail.
Why it works with stock code: a distinct stored code per recipe means the menu's own code-to-row resolution highlights the recipe row, the availability matrix is keyed by col2 and the two base col2 values are already allowed, and the develop and metadata dispatchers are value-keyed on col2, so the recipes render through their base modes' processing with no change to those tables (FILMSIM-OFFER-01 sections 3-5).
Risks: the two names depend on the special-id resolution question of section 2.3, which is unproved and decides whether D3 is named or merely numbered; a third free code `0xf` gives a third row but only two names exist, so a third row shows a stock or blank label; the rewritten rows stop naming the hidden codes they carry today, which nothing selects on this body's stock paths but which a record written elsewhere could carry; the hook is the first *code* change in this lane, so it needs the patch review and an emulation case of its own; and the hook's body competes for the same 98 bytes that remain after D3's data. If the names do not resolve, D3 degrades to numbered rows at the same cost.

**D4, RC1 as designed (arbitrary named recipes with patch-owned ids and text). About 1.05 KB in image 4, 660 bytes in image 2 and four code hooks, for five recipes.** Fixed cost 328 bytes (values and labels arrays 48 + 96, conversion-table copy 168, table header 16) plus 144 bytes per recipe (8 values, 16 labels, 12 conversion row, 48 table entry, up to 60 text), plus 132 bytes of special-class label records per recipe in image 2, plus the accessor hooks and the commit, index and display hooks of recipe-design sections 4-6.
It does not fit. The fixed 328 bytes alone exceed the 252 unaccepted bytes and the 444-byte tail must also hold the 192 bytes D1 already claims; the per-recipe cost is larger than D3's whole added footprint; and the code hooks have no accepted space in any image. RC1 remains a valid specification of the tier and has no home on this body at the current ownership state.

**D5, user-named recipes (the owner typing a name on the body).** Needs storage for the name (none, section 2.1), a naming surface (none for settings, section 2.2) and a write path from that surface into the storage (none). The wireless `NAME` editor is the only text-entry UI in the build and its storage holds one wireless string for another feature. Out of reach, and no design in this report's budget reaches it.

### 4.3 Ranked

| Rank | Design | Added bytes | Code changes | Value to the owner | Principal risk |
| ---: | --- | ---: | --- | --- | --- |
| 1 | D0 today | 0 | none | one unnamed preset, by hand | none; the second priority stays unmet |
| 2 | D2 C-default recipe | 40 | none | one compiled preset registered by a stock menu action, C-dial recall | redefines `CUSTOM RESET`; the recipe becomes the post-reset default |
| 3 | D3 two recipe rows | ~154 over D1 | one commit hook | two named rows in the film-sim list, each applying nine fields, persistent, riding the unlock's arrays | name resolution unproved; the first code patch in the lane; 98 bytes of tail left |
| 4 | D4 RC1 naming | ~1.7 KB | four or more hooks | arbitrary named recipes with patch-owned labels | no accepted home in image 4, image 2 or the code images |
| 5 | D5 user-named | not reachable | not reachable | the owner typing names | no storage, no prompt, no write path |

Ranking rule: what the owner gets against bytes spent and risk taken. D1 is not in the table because it is already built and is a prerequisite of every recipe design that uses PRO Neg as a base; it should be built together with D3 if D3 is built at all, since both live in the same prefix tail and D3 reuses D1's arrays.

## 5. Recommendation

Named recipes are not reachable on this body as the owner described them, which is a set of presets he can name and recall. The 2026-10-09 direction places that bank on the SD card (section 6), so the verdict below holds for internal storage only. User-chosen names are out of reach in every design: the record has no name field, the settings image has three free bytes, the string table's count is fixed in code, and the only text-entry surface edits the wireless name.
Compiled names are reachable in two places.

First, one compiled recipe with no name, at 40 bytes and no code, through the C slot's defaults block. It costs one in-place array edit, needs no placement argument, and gives the owner a look that `CUSTOM RESET` registers and the C dial recalls.
Second, two compiled named rows at about 154 bytes on top of the film-sim unlock and one commit hook. This is the largest useful resident change the 444-byte tail holds, and it is two checks away from being buildable rather than four: the special-id label resolution (offline, in the existing harness) and the ownership acceptance for widening the prefix from 192 to about 384 bytes (the census is equal to the accepted window's; the retained-content floor is now measured at `0x0061c244`).
Both checks are host-only and neither needs a camera.

Order of work:
1. Settle `0x83d`/`0x83e` as list labels with a native-harness case over the text-resolution call. If they resolve, D3 is named; if not, D3 is numbered and the two names need D4's id machinery, which has no room.
2. Take the widened prefix through the same review the 192-byte window had, using `prefix-tail-census.txt` and the `0x0061c244` floor. Without it, D3 cannot be placed and only D2 remains.
3. If both pass, build D1 and D3 as one image: D3 reuses D1's arrays and both share the tail, and two separate patches would waste it.
4. Keep RC1-DESIGN-01 as the tier's specification and treat RC1-PATCH-01 as blocked on a placement mechanism for about 1 KB plus code space in three images, which the current firmware image does not have. A persistent firmware install does not change this: the constraint is RAM ownership at load, and the prefix tail is the only accepted axis.
5. If the owner wants a compiled preset before either check lands, D2 is available now and its semantic cost is stated in one sentence: `CUSTOM RESET` would restore the recipe instead of the factory default.

Nothing here changes the install decision. D2 is an in-place data edit inside image 4 and D3 rides the same carry as D1; both are H3-class firmware changes with the owner's named go, one independent review of the diff, and the recovery level already recorded for this lane (R1 accepted, R2 unavailable, RECOVERY-R4-PRACTICE-01).

## 6. The editable named bank (owner direction 2026-10-09)

The owner then asked for many editable named slots with on-camera `Save Current Settings` and `Select Custom Setting` (recall), the newer-camera custom-bank UX ([E-RECIPE-OWNER-PRIORITY-150](../docs/evidence.jsonl)), kept alongside D3 and its Q exposure. Seven named slots is the UX target, not a capacity claim; stock C stays as it is; there is no auto-update; the slots carry the X-M1's image parameters only. This section is the storage half of that bank: what persistence this body offers, the typed file, the save and recall semantics, and where the kernel and its RAM image can live. The menu half is [the recipe design](recipe-design.md) sections 15-19.

### 6.1 Storage options and the decision

| Option | Persistence | Capacity and names | Byte state |
| --- | --- | --- | --- |
| internal multi-custom remnant | none for slots 1/2 | two RAM scratch records, no name space | the area-3 boot path `0x006a8abc` restores one logical record to mirror offset `0x140` = slot 0 only (byte-read this pass: `FUN_006ac328(3, 0x34, 0xfe, 0, ...)` then one read into `0x0089f838`), so a slot-1/2 commit through `FUN_0072b3d0` would be loaded as the C record at the next boot; the remnant is scratch only |
| new internal log area | card-independent | a record per slot with name and payload | not evaluated: no byte evidence that log blocks past `0x3d9` are free and unclaimed, and internal flash allocation is the owner's to grant, so no permission question is raised on speculative blocks |
| SD bank files | on card | seven named slots trivially | file read and write are byte-proven through the stock media task (section 6.2), zero internal persistent writes, stock C and area 3 untouched |

The SD bank is the conservative implementation choice: the file APIs are byte-proven (section 6.2) and the internal options have neither a restore path nor free-block evidence. Its tradeoff is card dependence: a recall needs the card, a save on a read-only or full card is refused with the stock error and the previous bank stays, and the bank lives and dies with the card. The internal option becomes a candidate only after a log-layout read proves free blocks and the owner grants the allocation.

### 6.2 The stock file-task ABI (byte-read unless noted)

| Binding | Value | Source |
| --- | --- | --- |
| open | `0x71c648(cb, path, mode) -> handle tag`; mode `0` = `'r'`, `1` = `'w'` create/truncate, `2` = append, `3` = `'s'`; the mode parser at `0x701634` folds A-Z and compares `114/119/97/115` | byte-read; the write mode is the WT_LOG exporter's call in `0xed1850`, the read mode the auto-act loader's in `0xed1de0` |
| read | `0x71c6fc(cb, handle, length, buffer)` | byte-read in `0xed1de0` (`r2` = length, `r3` = buffer) |
| write | `0x71c75c(cb, handle, length, source)` | the WT_LOG call site in `0xed1850` passes `(param1 + 0x20, [0x40a5b5ec] - 0x20)` in `r2`/`r3` and the handler chain `0x71ed34 -> 0x6ff000(idx, src, len, &status)` reads the request's `payload[2]` as length and `payload[3]` as source, so `r2` is the length and `r3` the source (AfRecorder pass, reconciled against the call site; a first reading of the call site as `(source, length)` is superseded) |
| close | `0x71c7bc(cb, handle, 0, 0)` | byte-read in `0xed1850` and `0xed1de0` |
| request routing | the wrappers post request ids `0x2c17` open, `0x2c18` read, `0x2c19` write, `0x2c1a` close to media task `0x1c`; the dispatch table at `0x814740` reads `0x2c17->0x7115a4`, `0x2c18->0x71edbc`, `0x2c19->0x7116e4`, `0x2c1a->0x711768` | byte-read |
| completion | the wrappers are non-waiting (`0x71c6fc`/`0x71c75c`/`0x71c7bc` pass wait 0, variant 0 to `0x71be74`/`0x71bfe0`/`0x71c134`); the kernel calls those base functions with wait 1 and variant 0 and consumes the callback's status and count, one request at a time | the completion code is `0x71d450`'s: `0` ok, `3` prepare (raw `0x20`), `0xe` commit (raw `0xd`), `8` the raw `0x24` class, `6` missing file (raw `7`/`8`); a read is accepted on count == requested (`0xed1ecc`) |
| handle lifetime | the completed callback's handle is the handle; a failed open's tag is already released by the open worker (`0x71f3cc`-`0x71f3e8` sets the callback handle to zero) and is never closed by the kernel, while a successful one is closed on every path (`0x711768` -> `0x71f1d4` -> `0x71eff4`) | closing a released tag would resolve to index zero and could close another client's file (ReviewRecipeStorage) |
| caller context | the poster `0x71c2b0` refuses calling task `0x1c` and task `0` on the guarded form (callback status `9`); the blocking forms refuse the same tasks through `0x71bc2c` | byte-read; the bank hooks run from the menu action path, a normal task client like the WT_LOG exporter and the auto-act loader |
| path letter | paths carry the active volume letter, resolved at runtime: the auto-act loader calls `0x71f708` and writes the result over its template letter; the poster maps the letter through the two-slot registry at `0x814730`/`0x814738` (`0x70ec84`, unregistered -> error `8`) and requires the request context's drive index `[ctx+0x114]` to match (else error `5`) | byte-read; the literal `A:` in stock name templates is a placeholder, not the SD identity. This body's registry registers `C` for volume 0 (AfRecorder pass with Sol's review and a fixture over the real `0x70ec84` mapper), so the card files are addressed `C:\RECIPEA.BIN` and `C:\RECIPEB.BIN` here and the kernel keeps the runtime accessor for portability |
| card state | `0x71ddf0(0)` returns `0` ready, `1`, `5`, `2` not ready; write failures are `0x24` read-only mode, `0x20` prepare, `0xd` commit | the not-ready and error code sets inherited from the AfRecorder pass and read in the open path's card branch (`0x71c35c`-`0x71c384`, byte-read) |

The stock clients prove the shape end to end: the mujin WT_LOG exporter `0xed1850` opens with mode `1`, writes and closes, and the auto-act loader opens with mode `0`, reads and closes (both byte-read this pass). A bank save is the exporter's sequence over one 512-byte image; a recall is the loader's sequence.

### 6.3 The bank file

Two files per card, `RECIPEA.BIN` and `RECIPEB.BIN` at the active volume root, each 512 bytes. The header is 32 typed bytes, then the 462-byte blob (seven `0x28` payloads back to back, then seven 26-byte names), then zero fill.

| Offset | Field | Value or rule |
| --- | --- | --- |
| `0x00` | magic | `FJRB` |
| `0x04` | format version | `1` |
| `0x06` | header length | `32` |
| `0x08` | firmware tag | `150` |
| `0x0a` | slot count | `7` |
| `0x0c` | name units | `12` |
| `0x0e` | payload length | `0x28` |
| `0x10` | generation | `1` + the highest valid generation on the card |
| `0x14` | blob length | `462` |
| `0x18` | checksum | the container main-range convention (E-CHECKSUM): complement of the sum of the eight-bit complemented bytes, over the whole 512 bytes with the checksum field zeroed, so generation and the slot mask are covered |
| `0x1c` | slot-valid mask | bit per saved slot |
| `0x1e` | reserved | `0` |

Write discipline, no retries: the write targets the file not holding the current highest valid generation (file A when neither is valid), so a write that dies leaves the previous bank loadable in the other file; after close the file is read back and kept only when the read back is byte-equal to the image written. Load precedence is the valid file with the higher generation, a tie goes to A, and generation wrap is refused before any truncate. Validation is whole-file: header fields, the whole-file checksum (which covers generation and the slot mask), the exact 512-byte length with zero trailing padding, every name and every walked field's stored code; a file that fails any check applies nothing. A missing file (completion 6) is a positively identified absence and only an operational read or close failure aborts target selection. An absent card is a clean refusal before any state changes.

### 6.4 Record semantics

Save Current and Recall are whole-record operations over the `0x28` payload through a per-field getter/setter walk over the 81 bound fields of [the field map](custom-settings.fields.json) plus the two halfword properties `0x1cb`/`0x1cc` at payload `+0x00`/`+0x02`, which the map omits. The halfwords' access path is raw at both selectors: `ldrh`/`strh` with no mask and no range compare (byte-read arms `0x623130`/`0x623198`, record `+0x00`/`+0x02`, live branches at the ordinary block head, defaults zero), so they are carried at full width and no narrower consumer domain is derived for them (residual, section 7). The stock walk `FUN_00630b14` is not used: its tail stores every destination except selector 1 through `FUN_0072b3d0`, and the settings writer `0x006a6f3c` rejects the scratch record's store (end `0x1b8` past its `0x19a` limit), so a copy to the scratch record raises the stock error handler `0x633594` and notification `0xa81` (ReviewRecipeStorage). The scratch record is the inert bank-0 row 2 at `0x0096cdf4` (getter slot `2`, record image `+0x190`; getter slot `1` is the live selector and `+0x168` is addressed by no slot value); the live selector comes from `FUN_006439e0()` (0 at C = the slot-0 record, 1 = the ordinary block). The only unbound payload bits are `+0x27` bits 3-7: writes zero them and reads require them zero.

- Save Current: a per-field getter walk from the live selector into the staging payload, with no persistence step (at C this reads the C record and writes only the staging image), then the name and the two-generation write (section 6.3).
- Recall: the file validates whole, then a per-field setter walk writes the payload to selector 1 and `FUN_0061d8dc(0x0096cc74)` stores the ordinary block back, so the selection persists at the next orderly power-off like any setting (P10). No bank operation writes the C record: at the C dial position the apply target resolves to the slot-0 record, so recall is refused with the typed status `RB_AT_C` (8) rather than applied, and the menu grays it out there. No partial application: validation precedes every apply, and it covers the header fields, the whole-file checksum, the 512-byte length and zero padding, every name and every walked field's stored code against its conversion table wherever the table pointer resolves.
- Staged edit: `rb_stage_load` copies the committed slot's payload and name into the kernel's own staging buffers (the stock record at `0x0096cdf4` aliases live settings and is never written), or the per-slot defaults row `0x007cb804` for an unset slot; `rb_stage_field` is a validated pure bit placement into the staged payload and `rb_stage_read` extracts from it; the OK dialog commits (`rb_stage_commit`), CANCEL discards (`rb_stage_cancel`), and a same-slot rename follows into the staged name only after its write-back succeeds. Item edits therefore never touch the bank file or any stock record until commit.
- Reset clears the slot's valid bit, name and payload and writes the file back. There is no auto-update: only Save Current and commit write, and only those two change the bank.
- Media coherence: every load and every write-back samples the media epoch witness at operation start and verifies it unchanged before publishing or committing, refusing with `RB_MEDIA_CHANGED` (13) otherwise; a refusal publishes nothing, preserves the resident bank and fails every cached availability verdict. The witness moves only on the attach/detach worker entry shims `0x0071fa9c`/`0x0071f7dc`.

Unrelated persistent writes: none. The bank never writes area 2 or area 3, the adjustment cache or the C record; Recall's ordinary-block store-back is the settings change it applies, the same persistence any menu edit has.

### 6.5 Field conversions, names and glyphs

The payload carries stored codes (the conversion tables' col0 values) at the bound bit fields of [the field map](custom-settings.fields.json); property value, payload code and stored code are one thing, so recall and snapshot need no per-field conversion. The nine editable fields (recipe-design section 2) bind `(property, byte, mask, shift, table)` for the Edit item and for validation: `0x104 (0x05, 0xc0, 6, 0x00775428)`, `0x110 (0x17, 0xe0, 5, 0x00775e24)`, `0x111 (0x09, 0xe0, 5, 0x0077643c)`, `0x10e (0x07, 0xe0, 5, 0x0077a810)`, `0x10f (0x08, 0xe0, 5, 0x00778620)`, `0xef (0x10, 0xe0, 5, 0x00779f10)`, `0xed (0x21, 0x0f, 0, 0x0077a60c)`, `0x141 (0x20, 0x1f, 0, 0x0077af48)`, `0xfe (0x24, 0x0f, 0, 0x0077ac30)`; the table pointer for every property sits at `*(0x0081b09c + 4·property)` and rows are `{col0, 0x7ffffffe, col2}` to a `0xffff` col0 (P7). Menu rows still cross through `FUN_0061d514` and the stock tables; the module `FujiAF.RecipeBank` is the only host place that crosses the two directions.

A name is 26 bytes: 12 UTF-16LE units of printable ASCII (`0x20`-`0x7e`), then the zero terminator unit at bytes 24-25 and zero fill; both validators require the terminator unit zero, and the editor glue converts up to 12 ASCII bytes and refuses longer input (RecipeNameEditor pass). `FONT_LATIN.FDL` covers the whole set (P6), the film-sim name box holds 440 px of right-aligned text at the bound metrics, and all languages show the same names, matching the stock precedent of Latin-script film-sim names. The name record the list draws from is the stock 66-byte special-record shape (56 inline bytes plus the layout halfwords `0x14, 0x14, 0x1e, 0x24, 0x30`), refreshed after a save or rename.

### 6.6 Placement of the kernel and its RAM

The bank needs one carried blob: the storage kernel (`menu/recipe_bank_storage.c`), the menu arms and hook, the seven name records (7 x 66 bytes at `0x011de4a0` in the menu plan, which retargets the record-base pairs there), the resident and staging file images (2 x 512 bytes) and their state, about 3 KB in total. The image-4 decode tail cannot hold it: the head window `[0x0061c1fc, 0x0061c340)` is 324 bytes and the AfRecorder carry claims it whole, while the Q arrays of FILMSIM-PATCH-02 claim `[0x61c240, 0x61c340)` of the same window, so three candidates compete for one head and the bank is not among them.

The grounded axis is the plain image's loaded window: the id-6 loader copies flash `0x5a0000` for `(flash_len + 0x1ffff) >> 17` blocks to `0x01100000` (byte-read `0x6a987c`-`0x6a98b0`), and its three inter-extent gaps (80,480 / 13,152 / 88,608 bytes at `0x011ac5a0`, `0x011dcca0`, `0x0144a5e0`) carry no resolved reference in the eleven-input census. Execution there is proven by the plain image executing under MPU region 0, the same region that covers the data writes this kernel already depends on (AfRecorder pass; the region words' bit semantics stay undecoded). The carrier landed (`rebuild-update extract-plain STOCK_DAT OUT_PLAIN` and `rebuild-update replace-plain --flash-offset 0xHEX SPAN OUTPUT_DAT`, PlainExtentCarrier): in-place same-size window change, same emitter gates as `replace-image2`, span bounds from the stored words, main checksum recomputed and a read-back proving the source unchanged outside the span and the checksum word, image 4 untouched, so D3 and Q stay intact by construction. ReviewPlainOwnership rejects every gap ownership acceptance pending resource, writer and callback bounds: the parent descriptor covers the gaps for protection and early callbacks run before total copy completion, so the census's no-resolved-reference reading narrows the residual only, and OwnershipScout continues consumer-derived bounds. Link and native proof may use gap 2 (`0x011dcca0`) provisionally for host-only work; no firmware live offer exists on this placement. Until a gap is accepted, placement is the bank's one unclosed prerequisite.

### 6.7 What exists, and what is not established

Exist: the storage kernel `menu/recipe_bank_storage.c` (candidate bytes, non-installable) with a typed status enum: `0` ok, `1` not-ready, `2` read-only (the completion-8 class, ambiguous), `3` full, `4` io, `5` invalid-bank, `6` unset-slot, `7` range, `8` refused-at-C, `9` no-bank (rb_load only), `10` generation exhausted, `11` task refused, `12` missing file (internal to discovery); the file codec and validators in `host/FujiAF/RecipeBank.hs` (`//host:recipe-bank`), which preserve the bound halfword fields, zero only the unbound bits on encode and refuse them nonzero on decode, and compute the file checksum over the whole 512 bytes with the checksum field zeroed; the menu lane's arms and hook calling `rb_recall`, `rb_save_current`, `rb_reset`, `rb_readback`, `rb_stage_commit`, `rb_stage_load`, `rb_stage_field`, `rb_stage_read`, `rb_stage_name_set`, `rb_stage_cancel`; and the plain-extent carrier. RuntimeRecipeCandidate owns the link and the runtime exercise.

Not established: gap ownership and writer bounds; a bounded stock queue, scheduler and callback ownership contract; writable-filesystem close/flush and late-completion cleanup; serialization of usable-state publication with media transitions; the menu task id of the hook callers (the kernel refuses tasks `0x1c` and `0`); and the meaning of the two halfword properties `0x1cb`/`0x1cc`, whose table pointers are null so they are carried at full width. The native storage and producer evidence, with its model and ABI-adapter boundaries, is in [section 6.8](#68-file-worker-failure-boundary). Physical recipe-write acceptance is unestablished. Installation is not approved and these candidate bytes remain non-installable.

### 6.8 File-worker failure boundary

The X-M1 1.50 recipe producer rejects a zero allocated tag before queue dispatch. At the media-worker boundary, the recipe guard rejects both a changed accepted identity and a null active-volume pointer before the stock outer open handler. The null-volume branch of `0x007115a4` otherwise returns status 5 with the saved tag still zero and skips the worker that releases the allocated request tag. The recorder still uses the unchecked stock producer; its separate [ownership gaps](af-observe-instruments-150.md#99-recorder-revision-2-bounded-scan-attempt-and-result-coverage-moved-placement) remain open.

Publication requires close completion and released tag ownership. A successful callback with its tag still allocated returns `RB_IO`, and write-back preserves a close error instead of replacing it with `RB_FULL`. A written generation can remain on card after refusal; a later complete scan must establish whether it is usable.

The `RB_VOLATILE_BANK=1` build loads and writes the resident RAM bank without submitting filesystem requests. Its producer wrapper excludes the zero-tag completion arm; the three producer hooks and three worker guards remain published. Builds with `RB_VOLATILE_BANK` undefined or zero retain the SD-backed completion handling.

The existing `recipe-bank-cases` harness reproduces the close false success through public `rb_save_current`: `h-write-close-deferred-flush` returns `0` with the original storage source and `RB_IO` (`4`) with the ownership check. Both executions retain written generation 8 beside prior generation 7 and leave live settings unchanged. All 17 `h-*` behavioral cases pass, including success and error completions that withhold tag release. These executions model synchronous foreign-media completion; they do not execute the stock queue, scheduler or writable filesystem.

All 17 `r-media-*` stock producer/relay adapter cases pass their declared behavioral outputs. Wrapper `0x0071c648` forces asynchronous operation; synchronous cases call stock `0x0071c2b0` with its six AAPCS arguments. Completion-slot semaphore `0x3a` rejection returns `0` with callback `(9,0)` and no open; synchronous missing-file completion returns `0` with callback `(6,0)` and one open. Rejected dispatch followed by failed completion semaphore `0x3b` wait instead returns allocated tag `1`, with no callback or open: the producer ignores that wait failure. Sixteen rejected stock posts retain their tags and the seventeenth allocation-zero call still attempts a post; the recipe shim returns `RB_IO` without that seventeenth post. A same-card recipe load reaches two missing-file opens and returns `RB_NO_BANK`. Producer cases preserve card and live bytes.

The behavioral verdicts compare declared public output spans. Raw runner strict verdicts still report incidental stub-event order and whole-region private residue mismatches; those checks are not counted as passing. Queue dispatch, semaphore wait/release, task wake and filesystem-open boundaries remain explicit adapters. No writable close, filesystem flush or bounded scheduler behavior is established by these 17 cases.

Actual bounded completion requires a queue and scheduler contract, not a larger native-case instruction budget:

- `0x0071ef18` and `0x0071eff4` acquire semaphore `0x3c` with timeout `0xffffffff`; synchronous clients acquire `0x3a` at `0x0071bc2c` and wait on `0x3b` at `0x0071bc90` with the same timeout.
- Producers ignore the result of queue post `0x0071fde4`, and synchronous producers ignore completion-wait failure. An absent callback is not evidence that delivery was canceled.
- Queue primitive `0x0072503c` dispatches through the selected engine in `0x0096bd20`, including queue-block allocation at vector `+0x170`, enqueue at `+0xac`/`+0xb0` and failed-enqueue block release at `+0xf8`. A failed-enqueue result needs a proven no-later-delivery guarantee before ownership can be reused.
- The X-M1 Run 3 capture ([`E-BULK-READ-RUN3-150`](../docs/evidence.jsonl)) binds the primary vector to `0x4004dae0`: allocation `0x000411cc`, enqueue `0x00049308`/`0x00049528` and block release `0x0004bab8`. Caller `0x0072503c` selects enqueue through `+0x148`, cell `0x4004dc28`, whose target `0x0004cbd8` returns whether the 32-bit selector at `0x400545a4` is nonzero. Zero chooses `+0xac`/`0x00049308`; nonzero chooses `+0xb0`/`0x00049528`. Separate `+0x14c` targets `0x0004cc00`, which reads CPSR.I and is not this caller's selected probe. The ring-full branch at `0x00048edc` returns without inserting the block, and enqueue maps that refusal to `-50`. Allocation and enqueue retry their engine spinlock without a deadline. The non-atomic capture binds these methods, not a bounded scheduling contract.
- The [closed selected-queue experiment](../emulation/README.md#bounded-selected-queue-experiment), [`E-XM1-QUEUE-CALLER-BOUNDARY-150`](../docs/evidence.jsonl), executes original `0x0072503c` from entry through allocation and enqueue. Its observed caller, original nonblocking consumer `0x0004a2f0` and original allocator results are:

  | Initial SVC CPSR / selector | Initial ring | Caller return | Drained block pointers | Next allocation return / pointer |
  | --- | --- | --- | --- | --- |
  | `0x13` / 0 | full | `-50` | `0x01010100`, `0x01010180` | `0` / `0x01010200` |
  | `0x13` / 0 | one free | `0` | `0x01010100`, `0x01010200` | `-50` / none |
  | `0x93` / 0 | full | `-50` | `0x01010100`, `0x01010180` | `-50` / none |
  | `0x93` / 1 | full | `-50` | `0x01010100`, `0x01010180` | `-50` / none |

  All four return `-50` on explicit empty probes after drain and after allocation, with query CPSR `0x13` and selector 0 distinct from caller state. No extra release of drained blocks precedes the allocator probe. Selector-zero enqueue preserves initial I; in both masked cases failed-enqueue release `0x0004bab8` refuses before modifying the pool. The 21 memory/state controls and refusals also complete. These are capacity-two/one-block fixtures with empty wait lists, uncontended locks, disabled logging, no errno match and synthetic CP14 identity `0x4003`; CPU1 stays in a masked-SVC ROM loop. They establish neither stock occupancy, waiting-receiver scheduling, cancellation/tag ownership nor a universal no-later-delivery guarantee. The file-worker and AF lifetime requirements remain open.
- The open relay saves its callback in global `0x0091028c`. The captured primary suspend method `0x00045cd4` consumes one pending wake from task byte `+0xe`, bit 1; wake `0x00046004` sets that bit when the task is not waiting. A second pending wake returns `-43`. Both methods retry an engine lock without a deadline and reject disallowed context with `-25`. Failed wait and late completion still need execution against these methods before callback-slot reuse is established.
- The stock tag sequence at `0x00910200` wraps to 1 and skips currently allocated tags. A deferred numeric tag does not identify an allocation lifetime after release and reuse. Cleanup needs a terminal completion or proven cancellation before retrying close against that tag.
- Writable close and flush require a file opened through the stock filesystem. Close `0x006ffff4` resolves its file table through `0x008fc880` and capacity `0x00944468`, then uses the volume table at `0x008fc87c` and metadata/media paths. A missing-file open fixture creates none of that state.
- The final media-epoch check and resident copy run in the client task. Atomic publication against attach/detach needs serialization with the media task or an established exclusion contract.

## 7. Residuals

- Whether the film-sim list's drawer resolves a special id (`0x83c`-`0x83e`) to the inline text of its image-2 record. Decides D3's names. Cheapest settlement: one native-harness case, no hardware (section 2.3).
- Whether any consumer outside the 1.50 node map references `0x83d` or `0x83e`. A whole-image reference census would close it; the node-map census found none.
- Whether slots 1 and 2 of the bank's slot area have any consumer at all. Their addressing exists; nothing mapped writes, reads or selects them.
- The wireless `NAME` editor's implementation and storage. It is the only text-entry UI in the build and a candidate host for one user-entered string; its reuse cost is unmeasured.
- The prefix's runtime writer absence remains unproved for the accepted window (FILMSIM-PATCH-01 section 5.3), and it is unproved for the 252 bytes below it for the same reason: no load-path write trace exists.
- The stock status a missing bank file produces: the kernel treats any non-card-state open failure as "no candidate file" beside card absence (section 6.2).
- The menu task id behind the bank hooks: the file guards refuse tasks `0x1c` and `0`, and the hooks must run as a normal task client (section 6.2).
- Plain-extent gap ownership (OwnershipScout) and the carrier's landing (PlainExtentCarrier): until both, the bank's placement is open (section 6.6).

## 8. Reproduction

All commands read host files only.

```sh
R=private-data/analysis/x-m1-1.50

# the three spare special label records (image 2), 3 x 0x42 bytes at RAM 0x37b784
od -A x -t x1 -v -j $((0x24b784)) -N $((0xc6)) "$R/filmsim/id2-00130000.bin"

# the retained table extent and the zero tail (payload artifact mapped at RAM 0x0061c000)
od -A x -t x4 -v -j $((0x140200)) -N $((0x140)) private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin
od -A x -t x4 -v -j $((0x1402c0)) -N $((0x40))  private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin

# the special-class layout accessors and the locator (plain image, offsets are file offsets)
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --start-address=0x7dbf0 --stop-address=0x7df10 "$R/filmsim/plain.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --start-address=0x7df70 --stop-address=0x7e070 "$R/filmsim/plain.elf"

# the payload's bound and unbound bytes
od -A x -t x1 -v -j $((0x7cb804-0x61c400)) -N $((0x28)) "$R/lz/id4-0061c400.bin"

# property 0x16d's three rows
od -A x -t x4 -v -w12 -j $((0x77532c-0x61c400)) -N 60 "$R/lz/id4-0061c400.bin"

# the widened-window descriptor census
devenv shell -- cc -std=c11 -O2 -Wall -Wextra -Werror "$R/filmsim/patch-01/extension-census.c" -o /tmp/filmsim-extension-census
FILMSIM_CENSUS_OUTPUT=/tmp/prefix-tail-census.txt /tmp/filmsim-extension-census 0061c244 0061c400 \
  "$R/lz/id4-0061c400.bin" 0061c400 "$R/lz/id5-00bf1a00.bin" 00bf1a00 "$R/lz/plain-01100000.bin" 01100000 \
  "$R/decode/loader-40000.bin" 00040000 "$R/decode/loader-d0000.bin" 000d0000 \
  "$R/decode/boot-40048.bin" 00000000 "$R/decode/boot-50048.bin" 00010000 \
  "$R/filmsim/lang-en-0039c000.bin" 0039c000 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 00130000:00ca025c:0024bac8:1 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 003bc000:0106025c:0007cfd4:1 \
  private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin 0061c000:00140048:00000200:0

# the node-map census for the two spare ids (no output means no reference)
awk '/#83d|#83e/' "$R/filmsim/nodes-id4.txt"

# the stock file-task ABI (section 6.2): dispatch table, wrappers, mode parser
od -A x -t x4 -v -j $((0x814740-0x61c400)) -N 32 "$R/lz/id4-0061c400.bin"
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x71c648 0x71c800
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x701634 0x701760

# the stock write client (WT_LOG exporter) and read client (auto-act loader)
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id5.dis" 0xed1850 0xed1920
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id5.dis" 0xed1de0 0xed1f40

# handle allocation and release, the poster's task and drive guards, the
# area-3 boot read (one record restored at mirror offset 0x140)
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x71ef18 0x71f040
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x711768 0x7117f0
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x71f1d4 0x71f260
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x71c2b0 0x71c430
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x70ec84 0x70ed04
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x6a8abc 0x6a8b80
devenv shell -- "$R/update/natw-tool" slice "$R/lz/id4.dis" 0x630b14 0x630c40
```
