# X-M1 1.50 adjustment and service interfaces

## Scope and provenance

This report promotes original interface and layout findings from host-side research conducted on 2026-10-05 through 2026-10-09. It applies to the official X-M1 1.50 container identified in [the firmware manifest](../manifests/firmware.json), decoded image 4 at `0x0061c400`, image 5 at `0x00bf1a00`, the plain image at `0x01100000`, and the two supplied boot records. Extraction and load binding belong to [firmware extraction](firmware-extraction.md). Addresses are firmware-version-bound analysis results, not portable APIs. `FUN_` names in the underlying research are analysis labels.

The evidence IDs below identify the original analysis and observations, not new verification performed for publication. **Byte-read** means inspection of the named firmware or capture; **static-inference** means a conclusion from that inspection; **hardware-observed** applies only to the original owner-run captures. Negative searches remain bounded by their method. No new camera operation or firmware execution supports this publication.

Source notes are retained privately under the logical provenance root `analysis/x-m1-1.50/`. They are named below without public filesystem links. Firmware, extracted code/data, decompilations, disassemblies, calibration, raw logs, captured banks and photographs are not redistributed. The report contains layouts, recovered interfaces and original explanations, not copied instruction sequences or firmware tables.

## Adjustment read coverage and update stamps

Provenance: `adj-read-01/findings.md`, `adj-windows/findings.md` and `adj-commit/findings.md`; E-ADJ-READ-150, E-ADJ-MOVERS-150 and E-ADJ-COMMIT-MAP. Interface facts are **byte-read/static-inference**; comparisons against body captures are scoped to the original hardware evidence.

The adjustment mirror is based at `*(0x0096ca50) = 0x41acd800`. Distinguish three extents:

| Extent | Meaning |
| --- | --- |
| `[0,0x42000)` | Prefix loaded/refreshed by the boot and update configuration paths |
| `[0x400000,0x40fd00)` | Separately persisted AF parameter store, without a firmware fallback image |
| `[0,0x420000)` | Whole-area commit extent of `0x0069fea8` through `0x006a9fdc` |

The public `read-plan` generator owns reproducible plans, described in [readplan](../readplan/README.md). `rc0-bound` reads 14 windows covering the nine develop tables' row-zero resolutions and fixed landmarks; `rc0-extent` reads 19 windows covering their extents; `adjustment-42000` reads 1,309 windows, comprising 1,056 prefix windows and 253 AF-store windows, for 335,104 bytes. The last plan is not a complete `0x420000`-byte mirror, full flash image or restore backup. Row-zero and whole-extent coverage do not prove which rows runtime index expressions choose.

The product oracle is adjustment index `0x15c`, byte `0x5c` of window `0x100`; the signature is at `0x202..0x203`. Config opcode `0x1a0002`, function 2, mailbox 1 reads the mirror without the RAM-read gate. The [transport contract](../transport/README.md) owns framing and reply validation.

### Runtime selector bounds

The later instruction-level analysis closes a gap that the original plans and readplan limits described as open; [the current limits](../readplan/README.md#limits) retain the original scope definitions and link this correction. Provenance is the corrective “Limits and deviations” section of `adj-read-01/findings.md`, also reflected in `adj-windows/findings.md`; E-ADJ-READ-150 records the correction. This is **byte-read** for the five index functions and **static-inference** for the bounded reachable rows.

The row term at record `+0x108` is written only zero by initializer `0x00d791b0` and setter `0x00d78508 -> 0x00d784ec`. Color selector `0x00d6891c`, chroma selector `0x00d621a0` and hue selector `0x00d62040` therefore use only row zero, entries 0 through 7. Filter selector `0x00d680f8` is not indexed because its record `+0x110` term is written only zero by `0x00d78538 -> 0x00d7851c`. `0x00d60dc0` indexes selector `0x00d61e04` by code alone, eight entries.

`0x00d69df0` and `0x00d6a11c` take a row from `0x0073f940`, whose mapped returns are only 0, 1 and 2. Selectors `0x00d6a708` and `0x00d6a5c4` thus reach entries 0 through 23. When its second byte is nonzero, `0x00d6a11c` instead selects `0x00d6a5f4`, entries 24 through 152.

For a future runtime-focused plan, `rc0-bound` lacks windows `0x00c700` and `0x00ca00`; `rc0-extent` includes extra develop-region windows `0x000000`, `0x00af00`, `0x00b100`, `0x00b200`, `0x00c800` and `0x00c900`. The complete 1,309-window capture already covers every one of the required windows, so the old capture remains usable. The original public scope names still mean row-zero and whole-extent coverage; these runtime bounds do not silently change their generated output.

Update completion `0x00db91e0` runs only if `0x014e3578 & 3` is nonzero. It snapshots the prefix, calls `0x0069fe20(0xff)`, restores selected ranges from the snapshot, then commits the whole `0x420000`-byte mirror. The first auxiliary list holds little-endian 16-bit offset/length pairs bounded by `offset + length <= 0x10000`; the second holds 32-bit pairs bounded by `0x42000`. The official container's lists contain 98 and 3 pairs respectively and cover 1,487 prefix bytes. These are selective retention lists, not a promise that every body-derived byte survives refresh.

`0x0069fe20(0xff)` reads the `0x42000`-byte template from raw storage offset zero, corresponding to payload offset `0xe0048`, then stamps cache `0x1e8/0x1e9`; every refresh form stamps `0x204/0x205`. On this 1.50 build both pairs are compiled as major byte 1 and minor byte 120. Boot loader `0x0069f204` rewrites only `0x204/0x205`, also rewrites `0x4069..0x406e` and `0xd8`, and leaves the older update stamp at `0x1e8/0x1e9` intact. A retained older update stamp alongside a current boot stamp is therefore possible without stale executable code. The executing updater's compiled stamp does not identify the newly installed image.

The prefix template and observed film-processing table extents agreed in the original 1.50 read. That establishes the recorded body's inputs, not equality for all bodies. Actual calibration values, identity bytes and between-session differences remain private.

## Autofocus parameter store

Provenance: `adj-read-01/block-400000-decode.md`; E-ADJ-BLOCK-400000-150. The following layouts and builder/consumer bindings are **byte-read/static-inference**. They extend the [AF static map](autofocus-static-map.md), not the film-simulation or recipe payload maps.

Initialization-family routine `0x0073d3ec` invokes `0x00772e70(1)`, persists offset `0x400000` length `0xfd00` through `0x006a0028`, sets and persists byte `0x237`, and finally invokes `0x00772e70(0)`. Its write guard is `cache[0x238] == 1 || (cache[0x238] != 2 && cache[0x237] == 0)`. The persisted result need not equal a reconstruction from capture-time cache cells.

All ranges in this table are inclusive cache indices. The table specifies storage shape, not calibration contents.

| Region | Shape | Builder | Source binding |
| --- | --- | --- | --- |
| `0x400000..0x4011c0`, flag `0x4011c1` | 3 groups of 15 records, stride `0x65` | `0x006e3b38` | s16 grid `0x3e3a55 + type*0xf0 + row*0x10`, cache `0x18f40/41/44/46` |
| `0x401200`, `0x4017eb`, `0x401dd6`, `0x402400`, `0x4029eb`, `0x402fd6` | Six `0x5eb` spans, each 15 records | `0x006e3eac` | One RAM record buffer built by `0x00745a88`, type from `0x3e3a53` |
| `0x403600..0x403857` | 6 records, stride `0x65` | `0x006e45d0` | Cache bases selected by firmware table `0x007d0890` |
| `0x403900..0x403eae`, flag `0x403eaf` | 15 records, stride `0x61` | `0x00745804` | Grid `0x3e3a55`, parameter `0x18f47`, retry cells `0x3e3d26/27` |
| `0x403f00..0x403f3b`, mirror `0x404500..0x40453b` | 15 pairs of u16 | `0x00745a88` | RAM record trailer fields `+0x84/+0x86` |
| `0x404b00..0x404d45` | 6 records, stride `0x61` | `0x00745584` | 8 u16 per row at `0x3420 - row*0x10` |
| `0x404e00..0x40890f` | 270 groups, stride `0x38`, 27 u16 per group | `0x00745d48` | 8 u8 per group at `0x3e3170 + block*0x2d0 + mid*0x78 + row*8` |
| `0x408a00..0x40aacf` | 75 groups, stride `0x70`, two curves of 28 halfwords | `0x007450b0` | Two 8-byte input sets at `0x3e3dd0/0x3e3dd8 + (block*15 + row)*0x10` |
| `0x40ab00..0x40f9e7` | 100 rows of 101 u16 | `0x0074c5e8(1)` | Ramp `0x9526`, per-row `0x3e2643` and `0x3eb100`, axis count `0x3e0f80` |
| `0x40fa00..0x40fc57` | 100 rows of six bytes | `0x00749668(1)` | Three minimum/maximum column pairs over axis count `0x3e0f82` |

The `0x65` and `0x61` records both start with 31 little-endian 24-bit entries at `+0x00..+0x5c`, followed by u16 fields at `+0x5d/+0x5f`. Only the `0x65` form adds u16 fields at `+0x61/+0x63`. Their RAM-side representation is `0x8c` bytes with 31 u32 entries and u16 fields at `+0x7c` through `+0x88`. The stored entries retain the low 24 bits. Reader `0x00da194c` expands the records for window builder `0x00e57d10`, whose 31-word configuration copy is at `0x01570600`; `0x00edc2f0` and `0x00edbf0c` feed the shooting-state evaluation configuration. Validator `0x00eddaec` checks flag `0x403eaf` and a bit at `0x3e3a50 + index/8`.

The six `0x5eb` copies come from one buffer in one writer loop. Their equality is forced by `0x006e3eac`, not independent evidence about calibration or a redundancy-check scheme. Mapped consumers select one copy and do not compare copies. A factory/service producer outside this builder set is not excluded.

For `0x404e00`, the input transform maps zero to `0x800` and otherwise computes `0x7f800 / input`, then interpolates/extrapolates. For `0x408a00`, the control-point values are `input << 1`, interpolated by thirds and clamped to `[-0x200,0x1ff]`. Consumer `0x00daa540` indexes the first table by `block*0x13b0 + mid*0x348 + row*0x38`; consumer `0x00d697f0` indexes the second by `(block*15 + row)*0x70` and supplies 56 halfwords to reply constructor `0x00d5d294` at reply `+0x1a6`.

The original complete read did not include source cells in the `0x3e0000` band, including grid counts and the source grids. It cannot reproduce the stored block from observed inputs or distinguish an old persisted build from a current rebuild. Physical meanings of the 31 entries and trailer fields, selected-area-to-request binding, and shoot-time record selection remain unproved. The layout above counts `0x408a00` as 75 stride-`0x70` groups, or 150 individual 28-halfword curves; the private note's phrase “150 groups” conflated these units.

## Opaque adjustment families

Provenance: `adj-consumers/findings.md` and the mover attribution in `adj-read-01/findings.md`; E-ADJ-MOVERS-150 covers the preceding census. This is a **static-inference** from bounded negative searches, not proof that no runtime reader exists.

The six template-derived families examined were `0x8fa0..0x8ff0`, the record runs in `0xf300..0x104ff`, the units around `0x10166`, `0x185be..0x185c1`, `0x1e300..0x1edff` and `0x20e00..0x22dff`. None is named by a resolved constant argument to the inspected adjustment-cache readers. The reader interface map is:

| Accessor | Result |
| --- | --- |
| `0x0069f8fc` | u8 |
| `0x0069fec8`, `0x0069ff7c`, `0x0069fffc` | u16, s16, s16 respectively |
| `0x0069ff68`, `0x0069ffe8` | s8 |
| `0x0069fef0` | u24 |
| `0x0069ff24`, `0x0069ffa4` | u32 |
| `0x006a0010`, `0x0073ed54` | Pointer at cache base plus supplied offset |
| `0x0069fcb8`, `0x0069fcfc` | Cache base or fixed-region pointers |

The original census examined 9,816 exported call sites and 8,574 disassembly reader sites across image 4, image 5 and plain code; the loaders and boot records had no direct references to the accessors. Literal, immediate-pair, pointer and referenced-index-table searches found no resolved family reader. However, 1,333 of the 8,429 image-4/image-5 disassembly sites had no resolvable constant argument. Computed-index readers, code outside decoded extents, service-side consumers, unobserved 1.00 code and undecoded image-2 data remain outside the negative conclusion. The private note's opening “no CPU-side reader exists” is stronger than its evidence and is not adopted here.

The original body/template comparison found these families equal to the 1.50 template and outside the updater's restore coverage. The `0x20e00..0x22dff` comparison to the older capture was explained by swapping every adjacent pair of 64-byte blocks. That is a scoped transformation observation, not a description of all 1.00 firmware. No family is identified as a film-simulation, recipe or AF parameter. Adjacency to known AF inputs does not establish a consumer.

## Raw media read interface

Provenance: `end-setting/followup/raw-media-read.md`, corrected by `read-media/findings.md`; E-RAW-MEDIA-READ-150 and E-RAW-MEDIA-NUMBERING-150. This section records the **byte-read/static-inference** interface, not permission to issue arbitrary service commands. The supported closed operation and current refusal policy belong to [transport](../transport/README.md).

Config opcode `0x1a0002`, function 3 reaches `0x00dc7564`. It writes cache bytes through `0x0069fc94` before scanning any whitelist. Only a request whose start index is `0x80` scans the 144 records of stride 12 at `0x00dc7a84`. A repeated value and even a zero-length request at that start index rescan the current byte. A matching enabled record posts queue-7 event `0x1707` through `0x00ed1300(0xff,value)`.

Value `0x35` selects group-2 row 34 in receiver `0x00eca4c0`. Dispatcher `0x00ec9b50(2)` indexes the width-7 matrix at `0x00ece2b8`; row and state cells are `0x015c20bc` and `0x015c20b0`. Row 34 selects action 37 through table `0x00ecdf7c` in states 0, 1, 2, 3, 5 and 6; state 4 selects inert action 3. The read chain is `0x00ecc72c -> 0x00dbcf80 -> 0x006fba08 -> 0x006fed00`.

| Request field | Binding |
| --- | --- |
| Spare selector | `cache[0x81]`, accepted below 2; supported operation requires 0 |
| Raw block | `cache[0x82] << 3` |
| Block count | 8 |
| Destination | `0x4b2ece00`, an imaging-pool buffer |
| Result | `cache[0x83]`: 0 success, 1 range/flag failure, 2 other failure |
| Timeout argument | 2000, units not established here |

The action opens the driver through `0x006faf00` at `0x00dbcfe0`, selecting device record 3, and closes it through `0x006faf40` at `0x00dbd030`. Record 3 at `0x0082988c` specifies `0x400` blocks, `0x800` page bytes and `0x40` pages per block. The bounds check is `block + 8 <= 0x400`, so selectors through `0x7f` are accepted. With spare selector zero, the staging extent is `0x100000` bytes. The read performs no bad-block remap. Closing has no refcount/already-open ownership guard; the staging buffer must be idle and a power cycle is required before further ordinary storage use.

For area 2, encoded descriptor bases `0x3d6/0x3d7` are not raw block numbers. The live map pointer is at `0x00919a20`; its start word is at map `+0`, and u16 logical entries `0x56/0x57` are at `map + 0x10 + 2*index`. For each bank derive `raw = start + entry`, selector `raw >> 3`, and staging offset `(raw & 7)*0x20000`. Refuse invalid/unmapped entries and unexpected map geometry. The two banks need not occupy the same eight-block read. Read these runtime values per session, never infer an identity map from the descriptors.

A terminal result proves completion only after a nonterminal sentinel has been written. The closed operation writes `cache[0x83] = 0xff`, triggers, then polls for 0/1/2. Only success permits staging reads. Data moves page by page before completion; reading early can mix old and new bytes. Disarm value `0x33` at `0x80` has its whitelist flag clear. The operation disarms before setup and after the read, restores the captured control bytes and gate, and deliberately leaves `0x80` disarmed rather than retriggering the saved value.

### Adjacent service hazards

The safety boundary is the closed operation's allowed values, not the shared dispatcher. State 4 also disables the intended read, so it cannot protect a working read from adjacent commands.

| Value at `0x80` | Row/action | Effect |
| --- | --- | --- |
| `0x06` | 6/9 | Settings-image service `0x00dbaa50`, retained-header mutation and defaults/write-capable cases |
| `0x20` | 24/27 | `0x00dbc858 -> 0x00db8ab0`, S-record install from `C:\IMFIDX10\LX30A.S3`, trailer erase and firmware programming |
| `0x21` | 25/28 | `0x00dbc8b4 -> 0x0069fe20`, reloads and destroys unsaved mirror state |
| `0x22` | 26/29 | `0x00dbc8d8 -> 0x0069fea8`, commits the whole `0x420000`-byte mirror |
| `0x24` | 28/31 | `0x00dbc924`, one-word RAM read/write service |
| `0x28` | 29/32 | `0x00dbcaa4`, erase/area-operation dispatcher |
| `0xc3` | 109/110 | `0x00dc558c -> 0x006c17fc`, storage-write-capable closure; inert in state 6 as well as state 4 |

For the word service, cache `0x81` selects direction, `0x83..0x86` form a big-endian address and `0x87..0x8a` carry the word. The address must be word-aligned and inside `[0,0x1fffffff]` or `[0x40000000,0x5fffffff]`; status is written to `0x82`. Direction zero copies RAM into cache; direction one copies cache into RAM. This is an interface finding, not a supported unrestricted write command.

Cache byte `0x81` is also the raw-read spare selector and erase-dispatch case selector. Zero keeps the erase dispatcher's selector outside cases 1 through 5 and leaves the word service in RAM-to-cache direction. Setting it to one both requests spares and arms destructive alternatives. No single overload makes the surrounding dispatcher safe.

## Ordinary settings record layout

Provenance: `read-media/findings.md` and `read-media/mapping-instrument.md`; E-AREA2-BANKS-150. Layout and integrity are **byte-read/static-inference**, with the original raw-bank observations limited to their captured sessions. The current field names and act bindings belong to [the ordinary settings byte map](settings-byte-map-150.md).

Area 2 uses two mapped `0x20000`-byte banks with 31 records of `0x200` bytes each. They hold consecutive generations of an append chain, not mirrored or alternating records. Only `0x3e00` bytes of each bank are record slots. Sequence `0xffff` marks an unused slot; accepted sequence values use 15 bits and ordering must account for wrap.

| Record offset | Layout |
| --- | --- |
| `+0x00..+0x01` | Sequence halfword |
| `+0x02` / `+0x03` | Flag byte / language byte |
| `+0x04..+0x23` | Four eight-byte entries, each three halfwords plus a terminator |
| `+0x24..+0x27` | DCF folder/file counter pair |
| `+0x28..+0x13f` | `0x118`-byte ordinary-settings body |
| `+0x100..+0x11c` | Last-shot path field within that body |
| `+0x140..+0x1e7` | Fill in the original captures |
| `+0x1e8..+0x1e9` | Rebuild stamp; 1.50 rebuild `0x006a8074` emits major/minor bytes 1/80 |
| `+0x1ea..+0x1eb` | Additional halfword, `0xffff` in the original captures |
| `+0x1ec..+0x1fb` | 128-bit word-parity vector, one XOR-of-32-bits bit per data word |
| `+0x1fc..+0x1ff` | Column parity, XOR across the data words |

`0x006a6b74` computes the parity over the first `0x1ec` bytes, 123 words; `0x006a6c54` validates/corrects it. There is no additional `0xaa` marker or additive checksum in this record. Those belong to the retained header. One older stored record lacked the 1.50 rebuild stamp, so consumers must not reject historical records merely for differing from that stamp without a code-bound predicate.

A RAM snapshot can be rebuilt from live state after the stored record was read. It is not necessarily the stored bytes, even if it retains that record's sequence. Raw-bank comparison is required before attributing path, counter or settings changes to a persistent append. Record diffs without a controlled act identify changed bits, not user-visible field meanings. [The console trace instrument](console-trace-instrument.md) separately owns event-ring interpretation.

### Record acceptance and custom continuation

Provenance: `end-setting/followup/area2-invalidation.md` sections 2.2 and 2.4; E-AREA2-PROBE-150 and E-AREA2-VALIDATOR-NATIVE-150 bound the original capture and native callback evidence respectively.

Newest-record reader `0x006ac4c8` first requires ready flag `0x0089f8b0`, an active high handle and a lower-object pointer; rejection returns `-14`. Lower-object decoding through `0x006ad5cc` can return `-30`. Buffer selector `0x006ad2f8` uses `0x006acfa8` to find the last non-`0xffff` slot and invokes the validation callback held at lower-object `+0x30`. Exact parity accepts; a one-bit syndrome in each expected dimension can be corrected in RAM without writing the correction back to storage. On validation failure the reader walks older slots; reaching slot zero without an accepted record clears the current buffer and returns `-255`. Across banks, 15-bit sequence ordering uses a 62-record window around the `0x8000` wrap.

Initializer `0x006a75a4` treats every nonzero read result as failure and fills snapshot `0x409c2d54` with a synthetic failure image whose sequence remains `0xffff`. A successful read copies the record into that snapshot; both branches copy it to pending mirror `0x409c2b54`. The initializer has no separate version/magic/payload-checksum test. Acceptance also requires the independently stored retained-header marker and additive checksum.

After area-2 handling, `0x006a8abc` opens logical area 3 and reads its `0x34`-byte custom record. A read failure fills `0x0089f838` with `0xff`, constructs and commits defaults through `0x006a877c/0x006a86c4`, and invokes `0x006a6fd8(0)`. Independently, an area-2/header rejection sets one-shot flag `0x0089f834`; area-3 initialization consumes it, reconstructs/attempts to commit custom defaults, then clears it. A single area-2 rejection can therefore default both ordinary and custom settings. This does not bind the physical cause of that rejection.

## Retained header and reset predicates

Provenance: `end-setting/followup/header-byte-differential.md`, especially its corrective section 9, and `cycle-d-round.md`; E-JIG-HEADER-DIFF-150 and E-JIG-CYCLE-D-150. These are **byte-read/static-inference** interface findings. [The service/reset report](service-query-and-jig-reset.md) owns lifecycle routes and the settings-loss conclusion.

The header is 16 byte-wide retained registers `0x50..0x5f`. Application index table `0x007cb7d4` selects descriptors in `0x007c9b54`; the live header is at `0x409c281c`, application snapshot at `0x409c29b8`, and companion snapshot at `0x400a00d0`. Boot-record reader `0x00013244` sets validity word `0x400a00cc` after checking byte 14 equals `0xaa` and the 16-byte sum modulo 256 is zero. Writer `0x00013338` recomputes byte 15 and writes the registers through encoded descriptors.

Application writes converge on `0x006a6f3c -> 0x006a6ddc`: copy into the live record, recompute byte 15, compare against the snapshot, and write changed bytes in ascending order with the checksum last. The retained header is not an atomic transaction across power loss.

Initializer `0x006a75a4` selects defaults only for an area-2 read failure, bad marker byte or bad additive checksum. It does not test header bytes 6, 10 or 11 as a defaults predicate. Byte 6 bit 0 set causes `0x0073cbfc` to post debug event `0x91c`; its clear branch does not invoke a defaults writer. Completed maintenance startup itself sets the bit through `0x0073ccb8 -> 0x00639b1c -> 0x00637478 -> 0x00643a08(1)`. A capture boot therefore changes this witness before later observation.

The companion byte-11 predicate at `0x0001386c` feeds `0x00013b80`, but that callee ignores the predicate argument and returns zero on every mapped path; its stored result at `0x400a0144` does not preserve the predicate. Header byte 10 bit 0 has live accessors `0x006a7de0/0x006a7e68`: image-5 `0x00f3af14` maps state 3/4 to clear/set, and `0x00f3be00` maps the bit back to state 3/4. Earlier claims that these accessors had no callers are superseded.

Defaults producer `0x006a6fd8`, modes other than 2, copies the header template from `0x007cb7f4` and ordinary-body template from `0x4089e8e0`, overlays adjustment-cache fields, restores the counter bytes and pushes a recomputed checksum. Mode 1 also constructs and commits custom defaults through `0x006a877c/0x006a86c4`; mode 2 is a different partial-default path. A valid template-shaped header is an output signature, not evidence of invalid header input or proof of which caller ran. The original controlled observations place the recorded settings loss before the subsequent ordinary boot; the precise switch-ending carrier remains unproved. Body-specific header values, sequence histories and shot paths are retained privately.

## Color space setting and metadata

Provenance: `color-space/findings.md`; E-COLOR-SPACE-150 and E-COLOR-SPACE-OWNER-150. The setting search is **byte-read/static-inference** on 1.50. Photographic metadata cited by that research came from recorded 1.00 frames and does not prove the 1.50 capture producer.

The language vocabulary contains label IDs `0x647/0x648/0x649` for COLOR SPACE, sRGB and Adobe RGB. The inspected image-4 menu-node/label map contains no corresponding node or values array. The conversion-pointer array at `0x0081b09c` spans 645 property slots and yielded 289 tables, none identified as a color-space property. The complete 57-field custom-payload map contains no identified color-space field. These findings support “no color-space setting identified on the mapped 1.50 surfaces”, not a universal proof that every opaque field has another meaning.

The playback/info routine `0x01118a00` displays the word at `+0x184` behind `*(0x009737ac)`, interpreting 0 as sRGB, 1 as AdobeRGB and other values as blank. The file-parser summary separately holds an EXIF halfword at `+0x4ca`: initializer `0x011c2ecc` sets 1, and parser `0x011c36f8` can replace it from EXIF tag `0xa001`. These are different encodings in different records. An EXIF parser's sRGB default does not establish the capture encoder's emitted value.

The capture-time producer of the metadata word and emitted tag remains unmapped by this research. A public reproduction needs an owner-supplied 1.50 frame and an explicit metadata inspection, not the private 1.00 photograph or an assumption that its result transfers across versions. No private image, embedded metadata or proprietary language table is bundled here.

## Reproduction prerequisites and retained-private boundary

For static reproduction, obtain the exact official X-M1 1.50 container independently, check it against [the firmware manifest](../manifests/firmware.json), and use [the documented extraction flow](firmware-extraction.md) and [recovery-kit extraction commands](../recovery/README.md). Address-based inspection uses `file_offset = address - image_load_base`. The public read-plan generator regenerates scopes from those owner-supplied images. No proprietary image is required in a public checkout merely to read these findings.

The original accessor censuses depended on a private decompiler export and local argument-reconstruction helpers. Reproducing their exact counts requires independently recreating that export and the stated constant/literal/immediate/table searches; there is no bundled public one-command equivalent. The AF-store shape is reproducible from the listed builder/consumer addresses, but reproducing a body's values additionally requires its source cache bands and persisted store. The originally captured ranges omit several of those sources. Record-layout hardware corroboration requires owner-supplied bank captures; no synthetic fixture is presented as a captured bank.

The private `read-plans/` JSON files are generated request descriptions, not firmware or calibration; their reusable scopes are public through [readplan](../readplan/README.md) and [the marker-0 route map](marker0-read-plan.md). The old session/protocol documents mix reusable addresses with local acts and obsolete pre-run status; their current interfaces and caveats are distilled here and in the linked reports rather than copied as instructions to repeat a historical experiment.

The private analysis categories already represented by dedicated public reports are firmware segmentation/loaders/decompression/checksums, boot-ROM and SD entry/recovery, AF/menu/lens dispatch, service/reset routing, film-simulation processing and patch placement, recipe storage/UI/runtime, native-case emulation, updater/native-writer flow, language access, settings act maps and console tracing. Their generated firmware extracts, decompiler exports, captured native-case memories, local review transcripts and build products remain private inputs or development archaeology. Rendering research contributes experimental methods and scoped conclusions through the evidence ledger; source photographs, calibration and local frame-by-frame comparisons stay private. This classification does not make an original reusable implementation private merely because it sits beside those inputs.
