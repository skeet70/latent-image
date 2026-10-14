# X-M1 1.50 custom setting: the C slot record, its save and apply paths

## Scope and labels

Host-only static analysis of the pinned official 1.50 payload and the private menu mapping already in this tree. No camera was enumerated, opened or commanded in this pass; no camera operation is proposed. Everything below is about the exact 1.50 images at their load addresses, not about the owner's 1.00 body.

Evidence labels follow [the principles](../docs/principles.md):

- **byte-read**: an instruction or data byte read from a pinned artifact in this pass, with the address given;
- **decompiled**: a statement taken from the private Ghidra listing `private-data/analysis/x-m1-1.50/af/functions/*.c`, which was generated from the same pinned bytes; the addresses cited from it were spot-checked in disassembly wherever this report quotes an instruction address;
- **static-inference**: a conclusion drawn from those bytes, with the reasoning stated;
- **inherited**: reused from a named report or private note without re-deriving it;
- **lead**: a private analysis file in this tree that was not re-derived here;
- **unproved**: named missing fact.

Companion artifacts: [the field map](custom-settings.fields.json) holds the machine-readable layout of the 0x28-byte payload. The scanners used here are the private `private-data/analysis/x-m1-1.50/custom/GetterFields.hs` (property-to-bit-field extraction from the getter's own dispatch table) and `WordFind.hs` (exact-word table search); section 9 has the commands.

## Inputs

| Artifact | Base | SHA-256 |
| --- | ---: | --- |
| `private-data/firmware/x-m1-1.50/FWUP0002.DAT` | n/a | `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490` |
| `private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin` (image 4) | `0x0061c400` | `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16` |
| `private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin` (image 5) | `0x00bf1a00` | `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a` |
| `private-data/analysis/x-m1-1.50/lz/plain-01100000.bin` (plain image) | `0x01100000` | `ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d` |
| `private-data/analysis/x-m1-1.50/decode/loader-40000.bin` | `0x00040000` | `21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882` |

Private leads used without re-derivation: `private-data/analysis/x-m1-1.50/filmsim/slot-fields.txt` (payload field map), `private-data/analysis/x-m1-1.50/filmsim/nodes-id4.txt` (menu node headers and labels), `private-data/analysis/x-m1-1.50/af/functions/*.c` (Ghidra listing over the same bytes). The owner's 1.00 body record `docs/camera-menus-human-recorded.txt` is used only to confirm menu option lists.

## 1. What the C position is

The mode dial's C position is a UI state of the shared application layer. Three byte-level landmarks bind it:

- Mode code `0x2c` is the C code. `FUN_0063b4c8` (`0x0063b4c8`, byte-read: dispatch `sub r0, r0, #0x304; cmp r0, #25; ldrls pc, [pc, r0, lsl #2]` with the table at `0x0063b4e0`, whose entry for state `0x314` at `0x0063b614` is `mov r0, #44`) maps state `0x314` to `0x2c`; `FUN_00d20098` (`0x00d20098`, image 5, byte-read: at `0x00d20230`/`0x00d20234` it does `mov r0, #44; bl 0x006337d8`, so the mode cell `0x00956838` holds `0x2c`) is the C-position entry and runs about forty per-mode setup helpers each passed `0x2c`, including the value-resolution helpers `FUN_0063ec1c` (image quality), `FUN_0063c108` (dynamic range), `FUN_0063ea80` and `FUN_0063ead0` (decompiled).
- `FUN_00d20098` has one caller, `0x00c2de74` (by the decompiled-call scan), which is handler slot `0x46` of the image-5 handler table at `0x00d01000` (`0x00d01118` holds `0x00c2de74`, exact-word table search).
- The C record's own mode is stored in the record: `FUN_00643a38(0x317)` returns property `0xe9` converted from encoding 1 to 3 (decompiled; the function's dispatch `sub r0, r0, #0x302; cmp r0, #26; ldrls pc, [pc, r0, lsl #2]` with a 27-entry table at `0x00643a50` is byte-read), and property `0xe9` is what the custom-set routine writes into the destination slot (section 4). Property `0x16d`, the CUSTOM SET menu node's property (`0x00784040`, node header `X=16d`), has three table rows with stored codes `0, 1, 2` (table `0x0077532c`, byte-read), matching the three slot records the bank carries.

**static-inference**: the custom setting is one of three slot records; the X-M1's single C position uses slot 0, and the record keeps the shooting mode it was saved from as property `0xe9`.

## 2. The stored record

### 2.1 Area and geometry

`0x006a8abc` (byte-read) opens the log area and reads the record:

```text
0x006a8ac8-0x006a8b0c  FUN_006ac328(3, 0x34, 0xfe, 0, FUN_006a8484, &0x00828570,
                                    FUN_006a84d4, &0x00828570, &0x0089f86c)
```

so logical area id 3, record size `0x34`, record count `0xfe`, write callback `0x006a8484`, read-validation callback `0x006a84d4` and handle cell `0x0089f86c`.

- Area 3's descriptors are rows 10 and 11 of the twenty `{count, encoded-base, flags}` rows at `0x007c8cc0`: `0x007c8d38` = `{1, 0x3d8, 3}` and `0x007c8d44` = `{1, 0x3d9, 3}` (byte-read from the image; inherited from the private area-2 invalidation note `private-data/analysis/x-m1-1.50/end-setting/followup/area2-invalidation.md` section 3 for the row layout; public record context is in [the ordinary settings layout](adjustment-and-service-interfaces.md#ordinary-settings-record-layout)).
- `FUN_006ab254` (`0x006ab254`, byte-read) is area 3's geometry mapper and selects bank 0 through `0x007c8d38`/`0x007c8d3c` and bank 1 through `0x007c8d44`/`0x007c8d48`. Two banks of 127 records follow.
- `FUN_006adbe4` case 3 (`0x006adbe4`, byte-read) selects the fixed record buffer `0x0089fa80`, and `FUN_006addf0` (`0x006addf0`) dispatches that buffer to the native writer `FUN_006ab2f4` with `DAT_00917fcc`/`DAT_00917fd4` and size `0x600`.

### 2.2 Record layout

| Record offset | Bytes | Contents | Basis |
| ---: | ---: | --- | --- |
| `0x00` | 2 | log sequence halfword, 15 bits; `0xffff` is an empty slot | byte-read: `FUN_006ad3f4` stamps `(latest + 1) & 0x7fff` |
| `0x02` | 2 | two bytes compared by the commit routine and covered by the parity; no writer found in image 4 | byte-read: `0x006a86f0-0x006a86fc`, `0x006a8714-0x006a8728` |
| `0x04` | `0x28` | payload, the custom setting itself | byte-read: `0x006a8b94-0x006a8ba0` copies `0x28` bytes between record `+4` and the settings image |
| `0x2c` | 4 | per-32-bit-word parity over bytes `0x00-0x2b` | byte-read: `0x006a8484` zeroes `+0x2c`; `0x006a83f4` fills it |
| `0x30` | 4 | per-bit-plane parity over bytes `0x00-0x2b` | byte-read: `0x006a8484` zeroes `+0x30`; `0x006a83f4` fills it |

The parity source length is the word at `0x00828570`, which is `0x2c` in the image (`0x00828570` holds `2c 00 00 00`, byte-read at image offset `0x20c170`); the area-2 pair at `0x00828568` holds `0x1ec`, the same scheme one area over.

Write path: `0x006a8484` zeroes both regions and calls `FUN_006a83f4(record, [0x00828570], region_at_0x30, region_at_0x2c)`. `0x006a83f4` (`0x006a83fc` reads its length, `lsrs r9, r1, #2`) sets one bit of the `+0x2c` region per source word and XORs each source bit into the `+0x30` region's matching bit plane.

Read path: `0x006a84d4` recomputes both regions into stack copies, compares each 4-byte region with `memcmp`, and on a difference walks the two syndromes to flip one byte in the RAM record, returning 0; an uncorrectable pattern returns 1. **byte-read** for both callbacks and `0x006a83f4`; the one-byte correction is inherited as the area-2 callback's behaviour and re-read here for the area-3 pair ([public record acceptance findings](adjustment-and-service-interfaces.md#record-acceptance-and-custom-continuation)).

### 2.3 How the record reaches RAM

- `0x006a8abc` reads through `0x006a865c` into `0x0089f838` (the live copy) and then copies the payload to the settings image at `0x409c295c` (`0x006a8b94-0x006a8ba0`: source `0x0089f83c`, destination `0x409c295c`, length `0x28`). It also mirrors the whole `0x34` bytes to the snapshot `0x0089f874`.
- `0x00630a34` (`0x00630a34`, sole caller the wrapper `0x0061c8cc` by the decompiled-call scan; the body is decompiled) reloads the three bank images `0x0096cc64`, `0x0096ce26` and `0x0089ec00` from the settings image, and for the slot records calls `FUN_0061d994(&0x0096cda4, 0)`, which reads `0x28` bytes from settings-image offset `0x140` into bank 0's slot 0 (`0x0061d994`, byte-read: `local_14 = param_2 * 0x28 + 0x140`, `local_18 = 0x28`).

**static-inference**: after a boot the stored C record sits in three RAM places with the same payload layout: the area-3 log record image in the settings image (`0x409c281c + 0x140`), bank 0's slot 0 (`0x0096cc64 + 0x140`), and the live/snapshot record copies `0x0089f838`/`0x0089f874`.

## 3. The payload field map

The payload's fields are the property layer's own fields: `FUN_0061e110` (`0x0061e110`) dispatches on `property - 1` through the 645-entry absolute table at `0x0061e12c` (`0x0061e11c sub r0, r0, #1`; `0x0061e120 cmp r0, #644`; `0x0061e124 ldrls pc, [pc, r0, lsl #2]`, so indices 0 through 644 are admitted and the table ends at `0x0061eb3c`; byte-read), and each property block reads one byte with one of three bases:

- slot records: `0x0096cc64 + 0x1c2*bank + 0x28*slot` (its `+0x140` is payload byte 0),
- per-slot defaults: `0x007cb804 + 0x28*slot` (the same offsets as a slot record),
- the live values: the ordinary block of `0x0096cc64 + 0x1c2*bank` at each property's own ordinary offset.

So a property's payload offset is the same in a slot record, in the defaults block, and in the settings image's slot area. [The field map](custom-settings.fields.json) records offset, bit range, property number and the menu meaning where the menu data binds one.

Three fields carry a checked menu meaning end to end:

- film simulation: property `0xfe` at payload `+0x24` bits 0-3 (`0x620d44 ldrb r0, [r3, #0x24]; 0x620d48 and r0, r0, #15`, byte-read);
- image quality: property `0xfa` at payload `+0x0d` bits 5-7 (`0x620bc4 ldrb r0, [r0, #0x14d]; 0x620bc8 lsr r0, r0, #5`, byte-read);
- image size: property `0xf6` at payload `+0x1a` bits 5-6 (`0x620a6c ldrb r0, [r3, #0x1a]; 0x620a70 ubfx r0, r0, #5, #2`, byte-read).

The private lead `private-data/analysis/x-m1-1.50/filmsim/slot-fields.txt` lists the full map; every offset in it that this pass re-derived from the dispatch table and the block reads agrees, for about thirty properties including all the neighbours named in section 7. Its entries for payload bytes `+0x25` and `+0x26` were the only ones it stated ambiguously, and both bytes are re-derived here from their property blocks: `+0x25` holds `0x214` at bits 0-3, `0x163` at bits 4-5 and `0x161` at bits 6-7; `+0x26` holds `0x183` at bits 0-1, `0x107` at bits 2-3, `0x106` at bits 4-5, `0x148` at bit 6 and `0x142` at bit 7 (**byte-read**: `ubfx`/`lsr` forms at `0x623e30`, `0x621c08`, `0x621b20`, `0x621534`, `0x620ed0`, `0x620f44`, `0x61faec`, `0x61f918`). Both bytes are fully partitioned by those nine fields. The absence claims in this map are a census of the getter's dispatch table and the two private leads, not an exclusion of register-indirect or out-of-image access.

Neighbour field positions re-derived from the same table (payload offset, bits, property): DR `0x104` at `+0x05` bits 6-7; colour `0x10e` at `+0x07` bits 5-7; sharpness `0x10f` at `+0x08` bits 5-7; shadow tone `0x111` at `+0x09` bits 5-7; highlight tone `0x110` at `+0x17` bits 5-7; WB fine shift `0xee` at `+0x17` bits 0-4; WB mode `0xed` at `+0x21` bits 0-3; ISO `0x141` at `+0x20` bits 0-4; noise reduction `0xef` at `+0x10` bits 5-7; focus mode `0x129` at `+0x18` bits 5-7; photometry `0x1c2` at `+0x1b` bits 5-6; AE bracket step `0x139` at `+0x1c` bits 5-6; movie frame rate `0x214` at `+0x25` bits 0-3; display-information custom setting `0x1d8` at `+0x24` bits 4-7.

## 4. Save path

The CUSTOM SET menu node (`0x00784040`, `X=16d`, `kind=6`) acts through property `0x16d`. The action routine is `FUN_00d4e2a0` (`0x00d4e2a0`, image 5, decompiled):

1. `uVar1 = FUN_0061d514(0x16d, param_1, 0, 1)` and `uVar2 = FUN_0061d514(0x16d, param_2, 0, 1)`: convert the source and destination rows through property `0x16d`'s table to its stored codes, which are the slot indices.
2. `if (param_2 != 0) return;`: only slot 0 is a legal destination.
3. `FUN_0061c92c(0xe9, value, 0, uVar2)` writes property `0xe9` for slot 0: `params_3 == 0` takes the current mode cell `0x00956838` converted from encoding 3 to 1, otherwise the source slot's own `0xe9`.
4. `FUN_0061c9bc(param_1, 0, param_3)` → `FUN_00630b14(row_a, 0, flag)`.

`FUN_00630b14` (`0x00630b14`, decompiled) walks about ninety properties and for each one does `FUN_00624694(prop, FUN_0061e110(prop, 0, flag, source_slot), 0, 0)`, then:

- destination slot 1: `FUN_0061d92c()`, which writes the ordinary block from `*(0x0061d940)` through `FUN_0061d8dc` (`0x0061d8dc`: length `0x118`, settings-image offset `0x10`);
- destination slot 0 (the C case): `FUN_0072b3d0` with `{source = &0x0096cda4 + 0*0x28, length = 0x28, offset = 0x140}`.

`FUN_0072b3d0` → `FUN_006a6f3c` (`0x006a6f3c`, byte-read) copies `0x28` bytes to `0x409c281c + 0x140`, calls `0x006a6ddc` (the 16-byte retained-header write-through) and, because `offset + length >= 0x141`, calls `FUN_006a86c4`.

`FUN_006a86c4` (`0x006a86c4`, byte-read) is the compare-before-commit:

1. `0x006a86d0-0x006a86e4`: `memcpy(0x0089f878, 0x409c295c, 40)` copies the settings-image payload into snapshot `+4`.
2. `0x006a86f0-0x006a8728`: compare live record byte 2 with snapshot byte 2, then live bytes 3 through `0x2b` with the snapshot's; equal bytes end the routine without a write.
3. On a difference: `FUN_006ac7d4(*(0x0089f86c), 0, 0x0089f874)` appends the snapshot to area 3, `0x006a865c` re-reads the live record, and `0x006a875c-0x006a8768` copies the new sequence halfword back to the snapshot.

`FUN_006ac7d4` (`0x006ac7d4`, byte-read) resolves the newest sequence, appends `(sequence + 1) & 0x7fff` through `FUN_006ad3f4`, and on an append failure switches bank; a second failure erases through `FUN_006ad51c` and retries. `FUN_006ad3f4` (`0x006ad3f4`) stamps the sequence, invokes the write callback (`param_1[10]`, for area 3 `0x006a8484`) and calls `FUN_006addf0`, which routes the `0x0089fa80` buffer to `FUN_006ab2f4`. Below that, the native program path is the shared one: `FUN_006ab2f4` → descriptor rows `0x3d8`/`0x3d9`.

The compared range is record `[2, 0x2c)`: byte `+0x02` is compared before the loop, and the loop at `0x006a8714` pre-increments before each load and ends when the pointer reaches `live + 0x2b` (`0x006a8710 add r12, r12, #43`), so `+0x03` through `+0x2b` are compared. The payload is record `[4, 0x2c)`, so its last byte `+0x27` is record `+0x2b` and lies inside the comparison; no payload byte escapes it. The parity source span is record `[0, 0x2c)`, a superset. **byte-read**.

### 4.1 The CUSTOM RESET action

The SET-UP menu's `CUSTOM RESET` item is item 13 of the setup root node `0x00789228` (`kind=7`, property 5); its values entry at `0x007886e0` is `{0x91, 0}` with no child, so the item carries no node and no property, and its label record at `0x00786688` is `{0x1a4, 0, 0, 0}` (**byte-read**). String id `0x1a4` is `<f136>CUSTOM RESET` in the English slot (`filmsim/strid-en.txt` id 420, a lead; the same label is in the private node map `af/menus/menu-tree.txt` line 1300), the label quadruple shape is [the offer mechanism's](filmsim-offer-mechanism.md) node layout, and the owner reports the item on the body's SET-UP 2 page (owner-reported, 2026-10-09).

The action is `FUN_00c32320` (`0x00c32320`, image 5, **byte-read**): it reads the framework's action parameter cells `0x0094f650` and `0x0094f654` (`movw r2, #0xb0d0` and `#0xb0d4` from base `0x00944580`; the block `0x0094f64c`-`0x0094f65c` is written by `FUN_0063270c`, id4) and calls the reset dispatcher `FUN_00d4e4d4` with that pair. The dispatcher's first branch fires when the first value is `0x91`, the item's id in the values array, and the handler's word is `0x00c32320` at pointer `0xd04764` in the image-5 pointer run that also holds the CUSTOM SET action `0x00c74e5c` at `0xd04214` (byte-read; that function's tail calls `FUN_00d4e2a0(4,0,0)`, the copy of the current settings into slot 0 that section 4 describes). Which framework call site fills `0x0094f650` with `0x91` is not traced, so the item-to-handler binding rests on the id in the handler's keyed branch and on the handler's position in that run (**static-inference**).

What the branch does (decompiled, with the byte-read instruction sites in section 9):

1. `*(0x00954294) = 0`, and `FUN_00638220(3)` stores 3 into `0x00944564`, a reset-class cell whose only reader found here compares it with 2 at `0x00c83e10`; a class-3 consumer is not mapped.
2. `FUN_00d4e2a0(row, row, 1)`, both rows taken from `0x0094f654`. The routine converts both rows through property `0x16d` to slot codes and returns unless the destination row is 0 (**byte-read**: `0xd4e2e4 cmp r4, #0`, `0xd4e2e8 popne`), so the action can only reach slot row 0. With the reset flag 1 it writes property `0xe9` for slot 0 from the getter's default branch (`FUN_0061c95c` → `FUN_0061e110(0xe9, 0, 1, 0)`, whose value is `0x007cb80e >> 5` = 3) and calls `FUN_0061c9bc(row, 0, 1)` → `FUN_00630b14(row, 0, 1)`.
3. `FUN_00630b14(src, 0, 1)` walks the same about-ninety-property list as the CUSTOM SET copy, but each value comes from `FUN_0061e110(prop, 0, 1, srcSlot)`, the getter's *defaults* branch. For slot 0 that branch reads through the pointer at image-4 `0x0061f7d8`/`0x006212c8`, both `0x007cb804`, at `0x28·slot + payload offset` (**byte-read**: the default arms of properties `0xe9`, `0xed`, `0xee`, `0xfe` and `0x104`, and the two pointer words), so the payload written is the 40-byte block of section 6, the same one `0x006a877c` copies after a jig cycle. Each value goes into the destination slot code 0 through `FUN_00624694`.
4. The destination slot code is not 1, so the copy takes the explicit store path: `FUN_0072b3d0({&0x0096cda4 + 0x28·slot, 0x28, 0x140 + 0x28·slot})` → `FUN_006a6f3c` → `FUN_006a86c4`, which compares the record's bytes `[2, 0x2c)` with the snapshot and appends the record to area 3 through `FUN_006ac7d4` on any difference. This is the CUSTOM SET copy's store and it commits on the spot (section 4).
5. The dispatcher's tail calls `0x00d466c8`, the settings-change notify.

**Scope:** slot row 0. The destination check refuses every other row before any write, so slot rows 1 and 2 and the ordinary settings block are untouched, and there is no all-slots loop. The ordinary block's store branch (`iVar2 == 1` → `FUN_0061d92c`) is not taken. The result is in the settings-image mirror and in area 3 as soon as the action returns, whereas a property edit leaves the record in the bank's RAM: the record reads as the per-slot defaults, film simulation PROVIA, image quality FINE, the payload's free index byte `+0x27` zero (`0x007cb804` holds `0x00` there), and the record's mode field `0xe9` at the defaults value 3.

**Relation to the three routes.** The action is the CUSTOM SET copy `FUN_00630b14` run with the defaults source (`flag 1`) instead of the current-or-slot source (`flag 0`), with the same store and area-3 commit; it is not the defaults constructor `0x006a6fd8`, which resets the ordinary block and the adjustment-cache fields at boot after an area-2 failure (section 6), and not the mirror reload `FUN_00630a34` through `FUN_0061d994`, which runs the opposite direction (image into RAM) at boot init and in the UI session reset. It is therefore a third stock writer of the mirror at `0x140` and of area 3, beside the CUSTOM SET copy and the boot record read `0x006a8abc`. The earlier note that those two were the mirror's only writers (E-CPOS-EDIT-VOLATILITY-150) is corrected by this section.

The UI session reset is not this item's path. `FUN_00d3c20c`, whose first call is `FUN_0061c8cc` → `FUN_00630a34`, has one caller, `FUN_00d3c3b4` at its `switch(*(0x009500f0))` case `0xc` (**byte-read**: the 5-entry jump table at `0x00d3c3f4` and the `bl` at `0x00d3c804`), and `0x009500f0` is the previous-UI-mode cell (current `0x009500ec`, previous `0x009500f0`, pending `0x009500f8`; [the mode word](service-shoot-feasibility.md#the-mode-word-and-the-mode-change)). So the reset runs when the UI leaves mode `0xc`, a shutdown-class mode the mode requester `0x00d350c4` writes for the power, ring, barrier, adapter and cradle inputs, and the CUSTOM RESET chain writes no mode cell. The reset's reachability in ordinary shooting is a property of that mode machine, not of this item: no shooting action, menu commit or dial move calls it, and this item does not either.

## 5. Apply path

### 5.1 Boot read and bank reload

`0x006a8abc` runs during settings initialization (called from `0x006a75a4` at `0x006a7834`, byte-read) and fills the settings image's slot 0. `0x00630a34` propagates it into bank 0's slot record. Both are in section 2.

### 5.2 Which slot the property layer calls "current"

Every "current value" access in the property layer resolves its slot through `FUN_006439e0` (`0x006439e0`, byte-read):

```text
0x006439e0: ldr r0, [0x00643a04]   ; 0x00643a04 holds 0x00951480
0x006439e0: ldr r0, [r0, #0xe4]    ; the mode-class cell 0x00951564
0x006439e0: cmp r0, #3
0x006439e0: return r0 != 3         ; 0 -> slot 0 (the stored record), 1 -> slot 1 (live)
```

- Read: `FUN_0061c93c(prop, 0)` → `FUN_00624620` (`0x00624620`, decompiled: `FUN_0061e110(prop, param_2, 0, FUN_006439e0())`).
- Write: `FUN_0061c91c(prop, value, 0)` → `FUN_006309c4` (`0x006309c4`, byte-read: `0x6309d8 bl 0x006439e0` into `r7`; with the third argument zero it reloads the ordinary block from `*(0x00630a30)` at `0x6309ec`, calls `0x00624694(prop, value, 0, r7)` at `0x630a00`, and when that returns 1 writes the ordinary block back at `0x630a10`).

Slot 0 is bank 0's slot record, slot 1 is the ordinary block; section 3's bases carry the same distinction.

**static-inference**: when the mode-class cell is 3, the property layer's current values are the stored C record's fields, and menu writes while that class is active land in the same slot record.

`0x00951564` is written by `FUN_00636230` (`0x00636230`, decompiled: `old = current; current = param_1`). The class is applied from the UI state inside `0x00639b1c` (decompiled), which resolves the active state, maps it to a mode and a class and calls `FUN_006439ac(mode, class)`; `FUN_006439ac` calls `FUN_00636230(class)`. `FUN_0063b728` (`0x0063b728`, byte-read: dispatch `sub r0, r0, #0x304; cmp r0, #24; ldrls pc, [pc, r0, lsl #2]` with the 25-entry table at `0x0063b740`) maps state `0x317` to class 3 (table entry at `0x0063b7ac` is `mov r0, #3`) and states `0x318`/`0x319`/`0x31a` to classes 4/5/6, `0x304`/`0x305` to 1/2, `0x311` to 7, `0x31b` to 8, `0x31c` to 9, everything else to 0.

### 5.3 The C position entry

`FUN_00d20098` (image 5, sole caller `0x00c2de74` at handler slot `0x46` of `0x00d01000`) sets the mode cell to `0x2c` and resolves the mode-dependent properties for C. It does not set the class cell; the class comes from the state machine's `(mode, class)` pair for the active state (section 5.2).

### 5.4 What is not closed

The dial position C is one UI state; two candidates carry C's mode code and its record class:

- state `0x317`: class 3 through `FUN_0063b728`, and mode through property `0xe9` (`FUN_00643a38(0x317)`), which is the mode the record itself carries. `0x317` also sits before `0x314` in the active-state list at `0x007748f8`, and `FUN_0063a3c4` returns the first active state in list order.
- state `0x314`: mode code `0x2c` through `FUN_0063b4c8` and class 0 through `FUN_0063b728`, which would make the property layer read the live block rather than the record.

The mechanism is byte-read: class 3 selects slot 0 (section 5.2). Which UI state the physical C position latches is **unproved**, because the state conditions are per-state entries of the hardware input map and `FUN_00632950` tests the current input bitfield (`0x00944580 + (row + 0x2cce)*4`, `FUN_00632920`/`FUN_00632934`) without exposing which physical line the dial closes; the state table at `0x007748f8` lists both candidates.

One capture holds both cells, without dial provenance. `private-data/sessions/20261008T045420Z-dump-ram-id4-bss-0/id4-bss-0.bin` covers `[0x0089ec00, 0x0099ec00)`; at file offset `0x0b2964` the cell `0x00951564` reads `0x00000000` and at offset `0x0b7c38` the cell `0x00956838` reads `0x00000024`. **byte-read**. The session is a service-time RAM dump: its journal's `prepared` record reads `dump-ram;segment=id4-bss-0;requests=4229;gate-index=247`, its 248-byte reads return status `(0,0)` from `0x0089ec00` upward, and its last record is `dump-complete-stock-unverified` with `4229 requests` (**byte-read** in the journal). It shows a plain mode code `0x24` with class 0, so `FUN_006439e0` returned 1 and the property layer read the ordinary block at that moment; the dial position was not recorded independently, so the pair neither establishes nor excludes the C behaviour. It does match the `FUN_0063b728` table, which maps the plain-mode states outside its case list to class 0.

Because no bulk slot-to-live copy exists in images 4, 5 or the plain image (section 4 lists the only slot-to-slot copy and it targets slot 0), the physical-C behaviour is conditional: if the C position latches class 3, the property layer's current values are the stored record's fields, and if it latches class 0, the record would only be read through explicit slot-0 accesses, which the searched shooting paths do not contain. A C-position-correlated RAM window or the input-map binding for the C line settles it.

## 6. Custom defaults after a jig cycle

`FUN_006a877c` (`0x006a877c`, byte-read) is the custom default constructor. Its steps:

1. `0x006a8784-0x006a8794`: `memcpy(0x409c295c, 0x007cb804, 40)`, the default payload.
2. Six fields are then overwritten from the adjustment cache (`FUN_0069f8fc` = byte read at `0x41acd800 + index`, with `*(0x0096ca50) = 0x41acd800` set by `FUN_0069f204`):

| Cache index | Payload field | Property | Meaning |
| --- | --- | --- | --- |
| `0x217` | `+0x1a` bits 5-6 | `0xf6` | IMAGE SIZE |
| `0x218` | `+0x0d` bits 5-7 | `0xfa` | IMAGE QUALITY |
| `0x219` | `+0x1e` bits 5-6 | `0xf9` | not bound |
| `0x208` | `+0x19` bits 0-4 | `0x15b` | not bound |
| `0x209` | `+0x18` bits 0-4 | `0x15c` | not bound |
| `0x20a` | `+0x1a` bits 0-4 | `0x15d` | not bound |

Instruction sites: `0x006a8798-0x006a87d4` (`0x217`), `0x006a87e8-0x006a8808` (`0x218`), `0x006a880c-0x006a8848` (`0x219`), `0x006a884c-0x006a8914` (`0x208`), `0x006a8918-0x006a89e0` (`0x209`), `0x006a89e4-0x006a8aac` (`0x20a`). Cache `0x218` maps to `0` when it reads 1 and to `1` otherwise; `0x217` maps values 0-3 to 2, 4-6 to 1, `0x81` to 1 and `0x82` to 2; `0x219` maps 1/2/3 to 1/2/3; the three `0x208`/`0x209`/`0x20a` sites share one switch whose outputs are 7, 8, 11, 14, 17, 20, 23, 25 for cache values 0-10 and 1, 2, 3, 4 for `0x85`-`0x88`.

3. The whole payload is committed by `FUN_006a86c4` (section 4), so the comparison runs against the live record and the default record is appended unless the live record already equals it.

The default block's bytes at `0x007cb804` are

```text
00 00 00 00 7f 22 22 09 49 49 69 49 49 09 09 09
49 29 09 69 09 09 a9 40 43 22 03 02 82 82 8f 8f
88 30 63 60 30 07 40 00
```

so the reset payload holds, among others: film simulation `0` (byte `+0x24` = `0x30`, low nibble PROVIA), image quality `0` (byte `+0x0d` = `0x09`, bits 5-7 FINE), image size `0` (byte `+0x1a` = `0x03`, bits 5-6). **byte-read**.

**static-inference**: after a jig cycle the C payload is rebuilt as the default block with image size, image quality and the four unbound fields carried over from the body's persistent adjustment area; every other C-payload field takes the default block, so film simulation returns to PROVIA. This bounds the C payload only. The date, time and language symptoms of the same cycle come from the ordinary reset, which `0x006a6fd8` performs from the ordinary default block with its own cache inputs (`0x215`, `0x216`, `0x20b` beside the six above) and which is recorded in [the retained-header and reset findings](adjustment-and-service-interfaces.md#retained-header-and-reset-predicates) and E-RESET-PROBE-POST-BOOT-150; nothing here establishes the ordinary payload beyond that.

The same six cache entries feed the ordinary defaults: `FUN_006a6fd8` (`0x006a6fd8`, byte-read) writes `0x007cb7f4` (16 bytes) and `0x4089e8e0` (`0x118` bytes) into the settings image and then applies `0x215`, `0x216`, `0x20b`, `0x217`, `0x218`, `0x219`, `0x208`/`0x209`/`0x20a` at ordinary offsets. The two records therefore share those six values; the ordinary record's remaining fields come from its own defaults and cache inputs, not from the C payload.

### 6.1 The three carried fields: per-shooting-mode ISO

**Byte-read, property layer:** `0x15b`, `0x15c` and `0x15d` are ordinary five-bit properties at payload `+0x19`, `+0x18` and `+0x1a`, read through `FUN_0061e110`'s cases and written through `FUN_00624694`'s cases, which replace the low five bits of the addressed slot record byte and preserve the top three. The write sites are `0x0096cdbd` (`0x15b`, payload `+0x19`), `0x0096cdbc` (`0x15c`, `+0x18`) and `0x0096cdbe` (`0x15d`, `+0x1a`) plus `bank*0x1c2 + slot*0x28` over the bank at `0x0096cc64`, whose `+0x140` is payload byte 0; the per-slot defaults branch of the same three cases substitutes 2, 3 and 3 when the addressed slot is 1 and the caller asks for defaults. The C-copy walk `FUN_00630b14` carries all three from the source slot to the destination slot.

**Byte-read, ISO family:** the three properties are the ISO values the camera remembers per shooting mode. The readers are `FUN_006444a0` (slot 0, the stored record) and `FUN_00644674` (slot 1, the live values), both returning `FUN_0061d514(0x141, value, 1, 3)`; the writer is `FUN_00644848`, which stores `FUN_0061d514(0x141, DAT_009568b4, 3, 1)` where `DAT_009568b4` is the live ISO cell. All three switch on `DAT_00956838`, the shooting-mode cell written by `FUN_006337d8`: case 0 stores property `0x157`, cases 2, 3 and 4 store `0x15b`, `0x15c` and `0x15d`, case `0x1e` stores `0x160`, `0x1f` stores `0x15e`, `0x20` stores `0x15f`, and cases `0x21`-`0x25` store the ordinary ISO property `0x141` itself. `FUN_0063c51c` compares the live ISO cell with the per-mode value returned for the current mode.

**Static-inference:** each of the three fields is the ISO remembered for one shooting-mode code, in the same code space as the ISO property `0x141` and reachable only through that property's conversion table (`FUN_0061d514(0x141, …, 1, 3)` and `(…, 3, 1)`). The mode cell's exposure modes are bound by `FUN_0063b4c8` (`0x30e` P → `0x21`, `0x30f` S → `0x22`, `0x310` A → `0x23`, `0x312` M → `0x24`, `0x314` C → `0x2c`, `0x306` SR+ → 1, `0x307` Portrait → 5, `0x308` Landscape → 8, `0x309` Action → `0xb`), and those modes share `0x141` rather than a dedicated field, so the three payload fields cover mode codes 2, 3 and 4, which this dial's state table never produces. Mode codes 2, 3 and 4 also appear in the advanced-filter list (node `79100c`, property `0xe6`, values `3 4 0 2 c 5 d 6 9 b 7 8 a`), but that list is the only place they are named and their dial identity on this body is **unproved**.

**Byte-read, jig-cycle carriage:** the body's cache bytes at `0x208`, `0x209` and `0x20a` read `0x86`, `0x87` and `0x87` in the 2026-10-08 capture, and the shared switch of `0x006a877c` maps cache values `0x85`-`0x88` to 1, 2, 3 and 4. The live slot record's payload bytes `+0x18`, `+0x19` and `+0x1a` read `0x43`, `0x22` and `0x03`, that is 3, 2 and 3 in the low five bits, matching the cache-derived values for `0x15c`, `0x15b` and `0x15d`. So the carried values are ISO codes, and a jig cycle restores the per-mode ISO memory from the body's calibration rather than from the default block.

## 7. Film simulation and image quality against the menus

Film simulation (property `0xfe`, payload `+0x24` bits 0-3, live field settings-image byte `0x8a` bits 4-7 per `0x620d84-0x620d98`):

| Menu row | Menu label | Payload value (table col0) | Table col2 |
| ---: | --- | ---: | ---: |
| 0 | PROVIA | 0 | 0 |
| 3 | Velvia | 3 | 3 |
| 1 | ASTIA | 1 | 1 |
| 7 | B&W | 0xb | 4 |
| 0xc | SEPIA | 0xc | 5 |

The rows offered by the shooting menu (`0x007823a4`), the Q menu (`0x0078b000`), the bracket nodes and the Fn node are `{0, 3, 1, 7, 0xc}` and the table at `0x0077ac30` has thirteen rows; both are [inherited](film-simulation.md) and the table bytes are byte-read here. The record stores the col0 code, which is the value `FUN_0061e110` returns for the slot; the menu layer converts it to a row.

Image quality (property `0xfa`, payload `+0x0d` bits 5-7):

| Menu row | Menu label | Payload value (table col0) | Table col2 |
| ---: | --- | ---: | ---: |
| 0 | FINE | 0 | 1 |
| 1 | NORMAL | 1 | 0 |
| 2 | FINE+RAW | 2 | 1 |
| 3 | NORMAL+RAW | 3 | 0 |
| 4 | RAW | 4 | 1 |

The table is at `0x0077a084` (five rows, then the `0xffff` terminator) and the row order is the value list of node `0x00784cb4` (`V 0 1 2 3 4`, labels `FINE`, `NORMAL`, `FINE+RAW`, `NORMAL+RAW`, `RAW`). The same five labels and order appear in the owner's 1.00 record. **byte-read** for the table and the node; the label text is a lead from the private node listing.

Image size (property `0xf6`, payload `+0x1a` bits 5-6, two bits): the table at `0x0077aa38` has twelve rows whose col0 groups as `0,0,0,0,1,1,1,1,2,2,2,2`, and node `0x00784014` offers nine rows (L/M/S at 3:2, 16:9 and 1:1, matching the owner's quick-menu list of nine). The stored field therefore carries the L/M/S size and not the aspect ratio. Where the aspect ratio is carried is **unproved**; the defaults path (`0x006a877c`) writes only this two-bit field.

## 8. Open items

1. Which UI state the physical C position latches, and therefore whether the mode-class cell is 3 while the dial is at C (section 5.4). The single capture of both cells holds class 0 with mode code `0x24` and carries no dial provenance, so the C-position behaviour stays conditional; a C-position-correlated RAM window or the input-map binding for the C line decides it.
2. Payload bytes `+0x00`-`+0x03` carry no field in the getter's dispatch table at `0x0061e12c` or in the private lead, so their meaning is unbound; that census does not exclude a register-indirect or out-of-image reader.
3. Field `0xf9` is carried from the adjustment cache across a jig cycle but has no menu node in the id4 node listing, so its user-visible meaning stays unbound, and cache indices `0x217` and `0x219` likewise have no name in this tree. `0x15b`, `0x15c` and `0x15d` are bound as the per-shooting-mode ISO memory (section 6.1) with cache indices `0x208`, `0x209` and `0x20a`; what stays unbound there is the dial identity of mode codes 2, 3 and 4.
4. Where the image-size aspect ratio is stored, given the two-bit payload field (section 7).
5. The area-3 erase and wrap paths (the area-2 counterparts are mapped in the private invalidation note `private-data/analysis/x-m1-1.50/end-setting/followup/area2-invalidation.md` section 4.2; public record context is in [the ordinary settings layout](adjustment-and-service-interfaces.md#ordinary-settings-record-layout)) are not re-derived for area 3 here.
6. The two compared bytes at record `+0x02` have no writer in the seven decoded artifacts, so their meaning stays unbound.

## 9. Reproduction

All commands read host files only.

```sh
R=private-data/analysis/x-m1-1.50

sha256sum private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  "$R/lz/id4-0061c400.bin" "$R/lz/id5-00bf1a00.bin" \
  "$R/lz/plain-01100000.bin" "$R/decode/loader-40000.bin"

# area-3 open, read, defaults, compare-before-commit and the settings-image copy
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 \
  --start-address=$((0x6a83f4-0x61c400)) --stop-address=$((0x6a8bc0-0x61c400)) "$R/lz/id4.elf"

# getter dispatch table (645 absolute entries, index = property - 1)
od -A x -t x4 -v -j $((0x61e12c-0x61c400)) -N 2580 "$R/lz/id4-0061c400.bin"

# the captured mode-class and mode cells in the reviewed id4 BSS dump
xxd -s 0xb2964 -l 4 private-data/sessions/20261008T045420Z-dump-ram-id4-bss-0/id4-bss-0.bin
xxd -s 0xb7c38 -l 4 private-data/sessions/20261008T045420Z-dump-ram-id4-bss-0/id4-bss-0.bin

# SHA-256 of the 40-byte default payload block
xxd -s $((0x7cb804-0x61c400)) -l 40 -p "$R/lz/id4-0061c400.bin" | xxd -r -p | sha256sum

# the three payload-bearing tables
od -A x -t x4 -v -w12 -j $((0x77a084-0x61c400)) -N 96  "$R/lz/id4-0061c400.bin"  # quality
od -A x -t x4 -v -w12 -j $((0x77aa38-0x61c400)) -N 144 "$R/lz/id4-0061c400.bin"  # size
od -A x -t x4 -v -w12 -j $((0x77ac30-0x61c400)) -N 180 "$R/lz/id4-0061c400.bin"  # film sim

# C record's own mode property table (property 0x16d rows 0/1/2)
od -A x -t x4 -v -j $((0x81b650-0x61c400)) -N 8 "$R/lz/id4-0061c400.bin"
od -A x -t x4 -v -w12 -j $((0x77532c-0x61c400)) -N 48 "$R/lz/id4-0061c400.bin"

# default payload block and the parity length words
od -A x -t x1 -v -j $((0x7cb804-0x61c400)) -N 40 "$R/lz/id4-0061c400.bin"
od -A x -t x4 -v -j $((0x828568-0x61c400)) -N 16 "$R/lz/id4-0061c400.bin"

# the CUSTOM RESET item, its handler word, its branch and the defaults pointer
od -A x -t x4 -v -j $((0x7886e0-0x61c400)) -N 8  "$R/lz/id4-0061c400.bin"   # values entry {0x91, 0}
od -A x -t x4 -v -j $((0x786688-0x61c400)) -N 16 "$R/lz/id4-0061c400.bin"   # label {0x1a4, 0, 0, 0}
od -A x -t x4 -v -j $((0xd04764-0xbf1a00)) -N 4  "$R/lz/id5-00bf1a00.bin"   # handler word 0x00c32320
od -A x -t x4 -v -j $((0x61f7d8-0x61c400)) -N 4  "$R/lz/id4-0061c400.bin"   # slot-0 defaults pointer 0x7cb804
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=$((0xc32320-0xbf1a00)) --stop-address=$((0xc32348-0xbf1a00)) "$R/lz/id5.elf"
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=$((0xd4e2a0-0xbf1a00)) --stop-address=$((0xd4e4d4-0xbf1a00)) "$R/lz/id5.elf"  # destination-row check
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=$((0xd3c3b4-0xbf1a00)) --stop-address=$((0xd3c3f4-0xbf1a00)) "$R/lz/id5.elf"  # the case-0xc table
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x61c400 \
  --start-address=$((0x638220-0x61c400)) --stop-address=$((0x638240-0x61c400)) "$R/lz/id4.elf"  # class-3 store

# property-to-bit-field extraction and exact-word table search (private scanners)
devenv shell -- runghc "$R/custom/GetterFields.hs" "$R/lz/id4-0061c400.bin"
devenv shell -- runghc "$R/custom/WordFind.hs" "$R/lz/id5-00bf1a00.bin" bf1a00 c2de74 c2dfb4

# mode and class maps, the current-slot resolver (all image 4)
for a in 0063b4c8 0063b728 006439e0 006439ac; do
  llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x61c400 \
    --start-address=$((0x$a-0x61c400)) --stop-address=$((0x$a+0x100-0x61c400)) "$R/lz/id4.elf"
done

# the C-position entry and its handler table (image 5)
llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=$((0xd20098-0xbf1a00)) --stop-address=$((0xd20260-0xbf1a00)) "$R/lz/id5.elf"
od -A x -t x4 -v -j $((0xd010e0-0xbf1a00)) -N 96 "$R/lz/id5-00bf1a00.bin"
```

The scanners print about 5000 lines; `GetterFields.hs` bounds each property by the next case block, and a property whose case shares a tail with its neighbour can print that neighbour's reads, which is why each offset quoted above was also read from the disassembly listing.
