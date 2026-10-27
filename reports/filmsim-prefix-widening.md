# FILMSIM prefix widening: descriptor-aware ownership of the widened decode window (1.50)

Host-only ownership test of the window between the retained table content and the accepted decode prefix, run so that a combined film-simulation and recipe image (RC0 plus D3) has a home or is known not to. No camera was touched; every input is a pinned host artifact.
Evidence labels: **byte-read** for a direct read of a pinned artifact in this pass, **inherited** for a named record reused without re-derivation, **hardware-observed** for captured camera state, **static-inference** for a conclusion drawn from those bytes, **unproved** for a named missing fact.

## 1. The window and the corrected floor

**Byte-read, correction.** The retained table content ends at RAM `0x0061c1fb`, so the free window is `[0x0061c1fc, 0x0061c400)`, 516 bytes (`0x204`). The reported floor `0x0061c244` (E-RECIPE-PREFIX-TAIL-150, [the reach report](named-recipe-reach.md) section 3) is `0x48` too high, the payload-versus-media offset of the export it was read from.
The reading: `payload-complement.bin` maps RAM `0x0061c000` at file offset `0x140048`, and payload = media + `0x48`; the reach report's `od -j 0x140200` command therefore shows the terminator's *file* offset `0x140240`, whose RAM address is `0x0061c1f8`, not `0x0061c244`. The reach census itself prints the RAM addresses correctly (`ALIGNED_PAIR 0061c1a4 raw=4061c1b0,01100000` for the extent descriptor at RAM `0x0061c1a4`), and FILMSIM-PATCH-01 section 5.3 already records the last plain-extent terminator at `0x0061c1f8`.
The table structures over RAM `0x0061c000`-`0x0061c1fb`: header words at `0x0061c000`-`0x0061c00f`; eight 9-word segment records `0x0061c040`-`0x0061c15f`; the `0xffffffff` segment terminator at `0x0061c160`; the `0x1234` marker at `0x0061c184`; the extent descriptor `{0x4061c1b0, 0x01100000, 0x00360000}` at `0x0061c1a4`; three 6-word extent entries from `0x0061c1b0` (the last resource word `0x47` at `0x0061c1f4`); the `0xffffffff` extent terminator at `0x0061c1f8`. Nothing follows it.

| Reading | Artifact and range | Result |
| --- | --- | --- |
| Last nonzero byte of the table block | `payload-complement.bin` file `[0x140048,0x140400)` | RAM `0x0061c1fb` (the terminator word at `0x0061c1f8`); byte-read |
| Window at rest, payload export | `payload-complement.bin` file `[0x140244,0x140448)` = RAM `[0x61c1fc,0x61c400)` | 516 bytes, all `0x00`; byte-read |
| Window at rest, shipped container | `FWUP0002.DAT` raw `[0x140458,0x14065c)` = media `[0x1401fc,0x140400)` (file byte = complement of media byte) | 516 bytes, all raw `0xff`, i.e. media zeros; byte-read |
| Window in RAM | probe sessions `20261007T185012Z`/`20261007T193028Z` windows `0061c100`/`0061c200`/`0061c300` | `[0x61c1fc,0x61c400)` all zero in both boots; hardware-observed |

The ticket's window `[0x0061c244,0x0061c400)` is the top 444 bytes of this region, and its 68-byte neighbourhood below it is free on the same evidence.

## 2. Descriptor enumeration

The test is the accepted prefix's test: a candidate is rejected if any shipped base-and-size or base-and-end descriptor's span intersects it, whether or not either field names an address inside it. Every adjacent little-endian word pair of every shipped artifact is read as `{base,size}` and `{base,end}`, with bit-30 address aliases normalized and 64-bit span sums; consumer-bound descriptor families below have their fields bound by their consumers (FILMSIM-PATCH-01 sections 4.1-4.2, UPDATE-IMAGE-01 field contract, loaders notes).

| Descriptor family and source | Fields | Span | Covers `[0x61c1fc,0x61c400)`? |
| --- | --- | --- | --- |
| Boot table-block copy, boot record 0 `0x1f5c` -> `0x1dc4` -> `0x218c`: media `0x140000` -> RAM `0x0061c000`, one 128 KiB unit | src `0x140000`, dst `0x61c000`, 1 | `[0x61c000,0x63c000)` | **Yes**, as a writer; the single mapped copy of the block, ordered before the image-4 decode (section 4) |
| Segment record 0, RAM `0x61c040` / media `0x140040` | load `0x40000`, size `0xdae0`; flash `0x80000`/`0x20000` | `[0x40000,0x4dae0)`; `[0x80000,0xa0000)` | No |
| Segment record 1 | load `0xd0000`, size `0xdaa0`; flash `0xa0000`/`0x20000` | `[0xd0000,0xddaa0)`; `[0xa0000,0xc0000)` | No |
| Segment record 2 | load `0x130000`, size `0x24bac8`; flash `0xca0000`/`0x260000` | `[0x130000,0x37bac8)`; `[0xca0000,0xf00000)` | No |
| Segment record 3 | load `0x3bc000`, size `0x7cfd4`; flash `0x1060000`/`0x80000` | `[0x3bc000,0x438fd4)`; `[0x1060000,0x10e0000)` | No |
| Segment record 4 | load `0x61c400`, size `0x282828`; bss `0x89ec00`/`0x352e00`; flash `0x160000`/`0x160000` | `[0x61c400,0x89ec28)`; `[0x89ec00,0xbf1a00)`; `[0x160000,0x2c0000)` | No; load begins at the window's end |
| Segment record 5 | load `0xbf1a00`, size `0x4f1e60`; bss `0x1460000`/`0x2bbe40`; flash `0x2c0000`/`0x2e0000` | `[0xbf1a00,0x10e3860)`; `[0x1460000,0x171be40)`; `[0x2c0000,0x5a0000)` | No |
| Segment record 6 | load/size repeat record 5's (unread by the id-6 loader); flash `0x5a0000`/`0x360000` | as record 5; `[0x5a0000,0x900000)` | No |
| Segment record 7 | load/size zero; flash `0xb40000`/`0x20000` | empty; `[0xb40000,0xb60000)` | No |
| Extent descriptor at RAM `0x61c1a4` | `{0x4061c1b0, 0x01100000, 0x00360000}` | pointer `0x61c1b0`; destination `0x01100000`; length `0x360000` -> `[0x1100000,0x1460000)` | No under the consumer-bound reading; the `{0x61c1b0,0x1100000}` base/size reading covers but its second word is a destination the loader reads at `0x6a9890` (consumer-excluded, section 3) |
| Extent entries at `0x61c1b0`, `0x61c1c8`, `0x61c1e0` | base/size `0x01100000`/`0xac5a0`, `0x011c0000`/`0x1cca0`, `0x011e0000`/`0x26a5e0` | `[0x1100000,0x11ac5a0)`, `[0x11c0000,0x11dcca0)`, `[0x11e0000,0x144a5e0)` | No |
| Language records, table at `0x7c8db4` and second table at `0x7c8f9c` | 12-byte records carrying a block count, a destination and a source; destinations `0x5bc000` (three records, count 3), `0x39c000` (count 1 each), `0x45c000` (counts 1-0xb), `0x3bc000` (count 0xb) | largest ends `0x5bc000+3*0x20000 = 0x61c000`, `0x45c000+0xb*0x20000 = 0x5bc000`, `0x3bc000+0xb*0x20000 = 0x51c000` | No; every destination span ends at or below `0x61c000` |
| Resource registry at `0x7dceb0` and the 16-byte records at `0x810f38` (29 confirmed pairs, filmsim-patch-01 section 4.2) | base/size and base/end fields bound by `FUN_006ed0d8`/`FUN_006ed150` and the queue family | smallest base `0x7d2180`, largest end `0x80f3d8` | No; every span lies far above the window |
| Record-4 header word 0 (decoded size), media `0x160000` | `0x00282800` at base `0x61c400` | `[0x61c400,0x89ec00)` | No; begins at the window's end |
| Trailer words 5/6, raw `0x24a0014`/`0x24a0018` | `{0x0061c400, 0x00282828}` | `[0x61c400,0x89ec28)` | No |
| Restore lists, raw `0x24a027c`/`0x24a0404` | `(offset,length)` pairs into the `0x42000`-byte adjustment cache | adjustment-cache offsets, not this RAM space | No |
| Image loads on the boot path: ids 0, 1 to `0x40000`/`0xd0000`; image 4 to `0x4061c400`; image 5 to `0x40bf1a00`; plain image to `0x41100000` and sub-extents `0x11c0000`/`0x11e0000`; language blocks to `0x39c000`/`0x5bc000` | load/size as above | none reaches `0x61c1fc`-`0x61c400` | No |

No consumer-bound descriptor covers the window. The boot table-block copy does, and it is the reason the region is reachable rather than free: the copy is the block's only mapped writer and it completes before the image-4 decode, which the prefix decode joins (section 4).

## 3. Raw pair census and the six pairs anchored in the table block

**Byte-read** of the new census `private-data/analysis/x-m1-1.50/filmsim/patch-01/widened-window-coverage.txt` (all eleven of the accepted run's inputs, extended to the corrected window; the tool is `window-coverage.c` beside it).
1,401,242 raw pair interpretations across the eleven inputs intersect `[0x61c1fc,0x61c400)` under the `{base,size}` or `{base,end}` reading (the per-input `pairs=` counts); 321,050 of them are resource-shaped (base at least `0x10000`, span end at most `0x2000000`) and cover it, and 3,005 `ANCHORED_PAIR` lines have a base in the block's neighbourhood `[0x600000,0x640000)`. A raw pair is a lead, not ownership evidence; the six anchored pairs whose base lies inside the table block `[0x61c000,0x61c400)` are the set to classify, and here they are with verdicts:

| Pair site | Raw words | Span reading | Verdict |
| --- | --- | --- | --- |
| id4 RAM `0x0082d5f2` (file `0x2111f2`) | `0x0061c352`, `0x0000da00` | `[0x61c352,0x629d52)` | unaligned straddle of the instructions `cmpgt r2,#0` at `0x82d5f0` and `ble` at `0x82d5f4`; no reference |
| id5 RAM `0x00f4ba7e` (file `0x35a07e`) | `0x0061c3a0`, `0x8830da00` | `[0x61c3a0,0xe9929a9e)` | unaligned straddle of `movgt r0,#0` at `0xf4ba7c` and `ble` at `0xf4ba80`; no reference |
| id5 RAM `0x0101808b` (file `0x4d668b`) | `0x0061c200`, `0x0061c800` | `[0x61c200,0xc38a00)` | unaligned straddle of two entries of the ascending offset table around `0x01018060` (aligned words `0x000061bd`/`0x000061c2`/`0x000061c8`, values in the `0x5cc8`-`0x6707` range); no reference. The only new hit the widening finds (section 5.2) |
| id5 RAM `0x010c94f1` (file `0x4d7af1`) | `0x0061c055`, `0xad871f7c` | `[0x61c055,0xae48dfd1)` | unaligned straddle inside a high-entropy data region (aligned words `0x61c055ab`, `0x871f7c00`); no reference |
| payload RAM `0x0061c000`, aligned | `0x4061c040`, `0x4061c1a4` | size reading `[0x61c040,0x40c381e4)` | the two header pointers (table base, extent-list head); consumers read them as pointers (`0x6571a0` returns `[0x61c004]`), never as a size; consumer-excluded |
| payload RAM `0x0061c1a4`, aligned | `0x4061c1b0`, `0x01100000` | end reading `[0x61c1b0,0x1100000)` | the extent descriptor's pointer and plain-image destination; `0x6a9890` reads `[+4]` as the destination, so the second word is not a size or end; consumer-excluded |

## 4. References into the window

**Byte-read** over the eleven inputs (image 4, image 5, the plain image, both loader images, both supplied boot records, the EN language slot, the complemented id2 and id3 ranges of the pinned DAT, and the payload table block).

| Hit class | Result |
| --- | --- |
| Aligned word equal to an address in `[0x61c1fc,0x61c400)` | 0 in every input |
| Adjacent resolved `movw`/`movt` pair forming an address in the window | 0 in every input |
| Unaligned word equal to an address in the window | 3, all straddles: id4 `0x0082d5f2` (`0x61c352`), id5 `0x00f4ba7e` (`0x61c3a0`), id5 `0x0101808b` (`0x61c200`); the first two lie inside the accepted prefix, the third in the newly measured lower section |

The third hit is the widening's only new reference finding, and the first two match the accepted run's two unaligned instruction fragments. No input contains an aligned literal or a formed constant naming any byte of the window.

## 5. Runtime writes and the restated falsifier

### 5.1 The one mapped writer

**Byte-read and static-inference.** The boot record at flash `0x40000` copies the table block with a single call chain: `0x1f5c bl 0x1dc4` -> `0x1dcc`/`0x1dd4` form the source `0x140000` and destination `0x0061c000` with one `mov r2,#1` unit -> `0x1de0 bl 0x218c` -> `0x213c`. The boot's block addressing converts an address to a 128 KiB unit with `lsr r0,#17` and advances a multi-unit copy by `#131072` (`0x21b8`-`0x2244`), so one requested block covers `[0x61c000,0x63c000)`, including the whole window.
No other mapped artifact copies into the block: `bl 0x1dc4` appears once in `boot-40048.dis` and nowhere in `boot-50048.dis`, `loader-40000.dis` or `loader-d0000.dis`; the only other formations of `0x0061c000`/`0x0061c400` in the pinned images are reads (boot `0x1f60`-`0x1f78`, `0x1f9c`-`0x1fac`, `0x1dac`, `0x417c`; second boot record `0x1fa4`, `0x357c`; image 4's accessor family rooted at `FUN_006570f4`, which returns `0x0061c000` for the read-only getters `FUN_00657114`/`FUN_006571a0`/`FUN_00657184`/`FUN_006571bc`).
The kernel-level order is fixed by the boot's own sequence: `0x1f5c` copies the block, `0x1f60`-`0x1f78` publish the skip-list count at `0x61c188`, `0x1f7c` checks the trailer (`0x1cc0` returns 1 only on the halt path, so the good path continues at `0x1fb4`), and `0x1fc8` calls `FUN_00001d08`, whose only image-4 call site is `0x1d5c bl 0x2250`. The copy therefore precedes the image-4 decompression to `0x4061c400` in every path, and the prefix decode writes last inside that order, exactly as the accepted prefix does.

### 5.2 Observed runtime writes inside the block

**Hardware-observed, inherited.** Three boots' captures (`20261007T185012Z`, `20261007T193028Z`, the `20261008T045*` dump set) show five differing bytes in `0x61c000`-`0x61c1ff` against the payload artifact: RAM `0x61c188` = `0x00004310` (the boot's skip-list publication, writer `0x1f60`-`0x1f78`) and RAM `0x61c1c0`, `0x61c1d8`, `0x61c1f0` = `0x00000001` (the extent done words). The done-word writer is `FUN_0073bc54` (`*(entry + 0x10) = 1`, reached through `FUN_006571bc(id)`), whose entry walk advances by six words and stops on the `0xffffffff` first word at `0x61c1f8`; `FUN_0073b924` walks the same list and stops the same way. Every known runtime write in the block lies at or below `0x61c1f0`, below the corrected floor.

### 5.3 The restated falsifier

The accepted window's falsifier extends unchanged to the larger region: a later reload of the table block after decompression, or any runtime writer reaching `[0x61c1fc,0x61c400)`, would overwrite the widened prefix, and would overwrite the accepted arrays with it. No such later reload is mapped in the pinned artifacts (one copy call site, section 5.1), and the observed RAM over the window is zero in three captures. Those observations do not exclude value-identical writes or writes between captures. What stays **unproved** is the general absence of an indirect or runtime-created writer: section 5.4 records a software request/error path, while section 5.5 observes successful stock and RC0 device-modelled early loading through original close. The latter ends before the primary rendezvous store and does not cover application runtime. A newly discovered covering consumer, a parent/index path into the tail, or a target-build change falsifies the placement the same way.

### 5.4 The historical software request-path trace

**Emulated, recovered historical observation.** `//emulation:load-window-cases` emits six cases. The 2026-10-09 refused run remains in `private-data/analysis/x-m1-1.50/native-case/load-window/`: `load-window-copy` stopped as `unsupported_instruction` at `bfi r3,r2,#12,#2`, PC `0x2b5c`, after 302 instructions and 142 provenance records (result SHA-256 `ed014a813d7a076dab1b6d5ea84dbcfe3024e092682977549a71cfe6a871169a`). No historical receipt is replaced.

The recovered held receipt in `private-data/analysis/x-m1-1.50/native-case/load-window-complete/` records the later sentinel return below. The directory name does not establish copy success. These are retained observations, not runs performed for this correction; the current source expectations now require the observed error-valued return as well as the request stores.

| Case | Executed | Result (emulated) |
| --- | --- | --- |
| `load-init-storage` | stock storage initialization `FUN_0000325c` with its closure | `returned`, 223 instructions, 123 provenance records; writes confined to RAM `0x42xx`-`0x4axx` and the declared controller windows; none in the table block |
| `load-window-copy` | the table-block request chain `0x1dc4` -> `0x218c` -> `0x213c` -> `0x30d8` -> `0x35c4` -> `0x2fc8`/`0x3408` -> `0x3528` -> `0x2ab8`, started from the state `load-init-storage` produced | `returned` at sentinel `0x3c000000`, **r0 = `0xffffff01` (error-valued)**, exactly 488 instructions, 241 provenance records and 128 writes; none in `[0x61c000,0x61c400)` |
| `load-segment-walk-0/1/4` | stock segment-record walk `FUN_00004174` over the installed container's table block, read through `0x4061c000` | `returned` `0x4061c040`, `0x4061c064`, `0x4061c0d0`; stack frame only, no write in the block |
| `load-segment-walk-9` | the same walk for an absent id | `returned` `0`, after the `0xffffffff` terminator at `0x61c160`; no write in the block |

The recovered copy result has SHA-256 `3ee65a7a1c0e348b257725dcdfaee708ee5dbafeac08d341672aaa40643cb470`. Both recorded copy wires have SHA-256 `7413090ee46863b194741b5aa56e5ba6aebb923476f4cd9164dea8f756e2bcab`. Equal wire hashes establish equal declared case inputs, not that instruction admission was the runner's sole change.

The declared inputs beyond carried state are an empty media skip list at `0x4310` (pointer cell `0x4804`) and ready read-status `0xfff52404 = 0x0000ffff`. The 128 writes comprise 20 in RAM globals below `0x4b00`, 84 on stack frames at `0x3b0003xx` and 24 in the media-controller window. The executed request stores include descriptor `0xfff528a0 = 0x00301000` at `0x2b78`, destination `0xfff528a4 = 0x0061c000` at `0x2b88`, count `0xfff5201c = 0x00300100` at `0x2ba4` (256 sectors, 128 KiB), block offset `0xfff52060`/`0xfff52024 = 0x280` at `0x2a90`/`0x2aa8`, and start `0xfff52884 = 0x00010001` at `0x2b9c`, cleared to `0x00010000` at `0x2c50`.

The windows are plain writable memory, not device models. No media payload movement executes. The retained zero ECC words take the firmware error path: `FUN_00003508` treats byte `+4 != 0xff` as failure, `FUN_000035c4` maps transfer result 1 to 3, and `FUN_000030d8` maps that to `0xffffff01`. The void wrappers return to the sentinel; native status `returned` is control flow, not successful media copy. This bounds the zero software-write finding to the observed request/error path. It does not cover the successful copy continuation, hardware decompressor, fill engine, second record, both loaders or execution after the loaded-image handoff. The boot record and media-table slice remain pinned by the case source.

### 5.5 Early-load prefix write audit

**Emulated, 2026-10-27.** The existing `recovery-bootstrap` executed stock/media/none and RC0/media/none with complete prefix write observation, strict decompressor reset/LZ semantics and read-only media. Both controls returned `model_matched`, engine `complete` at `0x1ff8` in 58,387 instructions after original close, with all seven source-bound full snapshot comparisons matching.

The source correction adds engine report `schema_version = 2` with `prefix_writes` for canonical address `0x0061c1fc`, 516 bytes. A CPU event is a **prewrite attempt**, not proof of a committed store; a device event records a **successful committed RAM write**, attributed by the `BbRamWriter` passed through the actual device write callback. Origins distinguish `cpu_attempt`, `media_dma`, `sd_dma`, `fill` and `lz`. Equal old/new bytes within the clipped prefix overlap still produce an event with `overlap_unchanged = true`; this field does not compare the rest of the original write interval.

Only mapped RAM ranges `[0,RAM_SIZE)` and `[0x40000000,0x40000000+RAM_SIZE)` are aliases of shared storage. Overlap is computed with widened interval ends, including writes starting below the prefix and crossing its floor. Each event retains the original address and full write length plus canonical `overlap_address` and `overlap_bytes`, source PC and phase (`reset`, `rom_load`, `native_reset`, `native_load`, `rollback` or `close`). The report distinguishes CPU-attempt and device-commit counts and marks entry to load/decode and successful load return with `load_entered`, `decode_entered` and `load_returned`.

Each event also carries stage `before_load`, `pre_decode`, `image_decode` or `after_load`, derived from the load/decode/return markers. `pre_decode` covers the whole interval from load entry at `0x1e4c` to validated decode entry at `0x2250`, not only table-copy writes. The decode marker follows the existing `poison_bss` source, phase and opcode checks; the load-return marker follows successful ABI and full-snapshot checks at `0x1fdc`. Phase continues to identify the engine execution interval.

Evidence is bounded to 512 intersecting events. Overflow or a relevant unsupported CPU width sets `complete = false` and `engine_error`; incomplete evidence cannot establish write absence. `complete` describes audit capture, not successful loading or lifetime coverage. The existing instruction/wall limits, refusal behavior, strict decoder, BSS checks and full snapshot comparisons remain required.

| Control | Committed prefix-overlapping writes in order | Observed result |
| --- | --- | --- |
| Stock 1.50 (`43d67afa...`) | One `media_dma` write at PC `0x2bc8`, destination `0x61c000`, full length 131,072 bytes, stage `pre_decode`; overlap `[0x61c1fc,0x61c400)` is 516 bytes and `overlap_unchanged = true` | Zero CPU attempts; no LZ prefix overlap; 2,631,680 decoded bytes at `0x61c400`; all load markers true; complete evidence |
| Installed RC0 (`acb0fbbb...`) | The same predecode table transfer, then one `lz` write at PC `0x3cd4`, destination alias `0x4061c340`, full length 315,426 bytes, stage `image_decode`; canonical overlap `[0x61c340,0x61c400)` is 192 bytes and `overlap_unchanged = false` | Zero CPU attempts; 2,631,872 decoded bytes at `0x61c340`; all load markers true; complete evidence |

Both controls cleared and compared the full 3,485,184-byte poisoned BSS. Neither audit contains another overlapping device write through original close. The unchanged table-transfer observation concerns only its 516-byte prefix overlap, not all 131,072 transferred bytes. The seven actual-CLI synthetic boundary cases also pass, including value-identical CPU/fill writes, mapped aliases, interval crossings and capacity failure.

Evidence `E-PREFIX-EARLY-LOAD-STOCK-150` and `E-PREFIX-EARLY-LOAD-RC0-150` retain the qualified receipts and source identities. These are host-model observations under the existing logical-media, timing, cache and primary-only assumptions.

This completes the measured early-load write audit, not #43's whole-lifetime acceptance criterion. Later runtime-issued CPU/device programs remain a host-code question: carry complete overlap observation into application execution and bind each reachable CPU/device submission path, then establish whether any can overwrite the prefix after decode. A further finite trace alone does not prove absence across all runtime paths. No SD-card operation or camera input is required for that host work.

### 5.6 Finite later-runtime source cuts

**Source-inspected and static-inference.** The per-input segment records and qualified early-load snapshots distinguish the first decoded image from its declared record span:

| Input | First-image load | Declared segment bytes / end | Decoded bytes / end |
| --- | --- | --- | --- |
| Stock | `0x0061c400` | `0x282828` / `0x0089ec28` | `0x282800` / `0x0089ec00` |
| Installed RC0 | `0x0061c340` | `0x2828e8` / `0x0089ec28` | `0x2828c0` / `0x0089ec00` |

RC0 shifts the load down 192 bytes and adds 192 decoded bytes. The `0x28` excess in each declared record is not evidence of an additional decoded payload write.

Both inputs retain the separate plain-media descriptor `P={0x4061c1b0,0x01100000,0x00360000}`. `FUN_006a9844` uses `P[1]` as the destination, giving `[0x01100000,0x01460000)`, not either first-image span. The first plain extent is `[0x01100000,0x011ac5a0)`; the other two begin at `0x011c0000` and `0x011e0000`.

With the three extent entries and terminator retained, the direct `*(entry+0x10)=1` in `FUN_0073bc54` can target only canonical words `0x61c1c0`, `0x61c1d8` and `0x61c1f0`, below the prefix. This excludes that store, not the worker's transitive effects. Its `FUN_0073badc` calls configure MPU ranges through CP15 c6 operations, not RAM movement; `0x4d978`/`0x4da6c` provide division/divmod arithmetic, not checksum verification.

The stock plain bytes supply finite constructor seeds: one target at the first extent, an empty list at the second, and fourteen targets at the third, each zero-terminated. The worker also has 32 completion-callback slots and deferred handlers. Constructor effects, spawned tasks and callback effects remain unresolved; these are not a closed three-writer set.

- **LZ:** the segment-5 loader supplies decoded base `0x40bf1a00` to `FUN_006ae360`; `0x43e40000` is compressed staging. The decoded-destination store is at `0xfff43048`. A running decoded offset is added to the base; a nonnegative unsigned offset does not prove nonwrapping addition. Its actual stream/offset bound and the ordinary submitter set remain unproved.
- **SD:** the second argument of `FUN_006f3268` reaches the RAM-base store at `0xfff50000` through thin wrappers. The decompiler's zero-argument wrapper does not erase that A32 register flow. The 32-bit store at `0xfff5000c` is command/control submission, not a RAM destination. Native halfword stores at `+8`/`+0xa` and combined 32-bit `+0xc` submission are not qualified by the early ROM model's narrower width grammar. Ordinary caller buffer bounds remain unresolved.
- **Media and fill:** the modeled media RAM destination is `0xfff528a4`; `0xfff52020`/`0xfff52024` are NAND column/row. The prefix address fits 24 bits and has bit 30 clear, so neither property excludes it. The later-runtime media RAM-destination programming edge and fill destination writer remain unresolved, not absent.

Evidence `E-PREFIX-RUNTIME-SOURCE-CUT-150` retains source identities and independent byte checks. No ordinary reachable post-decode prefix overwrite was established, and #43 remains open. The remaining host task is to bind ordinary CPU/device writer reachability, destination ownership, maximum lengths, nonwrapping arithmetic and initialization/lifecycle state. A source-bound ordinary runtime overwrite after the prefix becomes live would be a valid negative placement result for #17; an arbitrary service RAM write, reinstall or reboot is not substituted for that counterexample. No camera or SD-card input is needed for these source cuts.

## 6. Combined prefix geometry

**Static-inference** over the accepted contract (FILMSIM-PATCH-01 sections 5.1-5.3). The invariant is unchanged for every size: `load + decoded_size = 0x0089ec00` (stock bss start) and `load + segment_size = 0x0089ec28`; the segment size keeps the stock `0x28`-byte excess over the decoded size; entry 4's bss words, image 5's load, the plain image's extents, the entry point and the preserved 32-byte embedded header do not move.
D3 adds about 154 bytes on top of RC0's 192-byte prefix and reuses RC0's shared arrays, so the combined image is one prefix whose top stays where RC0's arrays are pinned (`0x61c340`/`0x61c380`) or grows them in place.

| Base (new load) | Size | Record decoded size | Segment 4 size | Bytes below `0x61c340` | Note |
| --- | --- | --- | --- | --- | --- |
| `0x0061c240` | `0x1c0` = 448 | `0x2829c0` | `0x2829e8` | 256 | 64-byte aligned; the widest aligned base once the corrected floor is used |
| `0x0061c244` | `0x1bc` = 444 | `0x2829bc` | `0x2829e4` | 252 | the floor as earlier reported; 4-byte aligned |
| `0x0061c280` | `0x180` = 384 | `0x282980` | `0x2829a8` | 192 | 64-byte aligned with the earlier reported floor; the "about 384" shape |
| `0x0061c1fc` | `0x204` = 516 | `0x282a04` | `0x282a2c` | 324 | the corrected floor, unaligned |

Every row satisfies the bss relation: `0x61c240 + 0x2829c0 = 0x61c244 + 0x2829bc = 0x61c280 + 0x282980 = 0x61c1fc + 0x282a04 = 0x0089ec00`, the stock bss start, and every segment end lands on the stock `0x0089ec28`. The recorded values to propagate on a build are the two table words (load and size), trailer words 5/6, record header words 0-2 and the main checksum, exactly the accepted chain.
D3's data need is about 100-140 bytes (two arrays grown by 48, two 6-byte parameter records, and a 40-80-byte hook body), which fits the 192-byte-or-more lower section of every row, with the two names still in place in image 2. The row to build is a choice for the design, not a constraint from ownership: `0x0061c240` if the 64-byte base convention is kept, `0x0061c1fc` if it is not.

## 7. Verdict

The widened window is **accepted by the same descriptor-aware test the 192-byte prefix passed**, with the floor corrected to `0x0061c1fc`. No consumer-bound descriptor covers it, no aligned literal or formed constant names it, no confirmed reference points into it, the observed application writes inside the block stop below the floor, and the region is zero in the shipped container, in the payload export and in three boots of captured RAM. The boundary is the terminator word at `0x61c1f8`: everything below is retained table content. The successful stock/RC0 early-load audits observe the table transfer before decode and RC0's expected decode-prefix write, including the table transfer's value-identical prefix overlap, with no additional overlapping writer through original close. Absence of an indirect or later runtime-issued writer remains unproved for both the accepted prefix and added bytes.
This is not installation authorization for the widened window: the 2026-10-09 install used the accepted 192-byte prefix at `0x0061c340` (E-FILMSIM-INSTALL-150), and any use of `[0x0061c1fc,0x0061c400)` beyond it is a separate decision. The arrays being live in an ordinary menu narrows effective-overwrite concerns on that observed boot path only; it does not exclude a value-identical write or a write between observations.

## 8. Reproduction

All commands read host files only.

```sh
R=private-data/analysis/x-m1-1.50

# the corrected floor, both representations
od -An -v -tx1 -j $((0x140048)) -N $((0x3b8)) \
  private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin |
  tr -s ' \n' '\n' | awk 'NF{n++; if($1!="00") last=n} END{print n, last}'
od -An -v -tx1 -j $((0x140458)) -N $((0x204)) private-data/firmware/x-m1-1.50/FWUP0002.DAT |
  tr -s ' \n' '\n' | awk 'NF{n++; if($1!="ff") nz++} END{print n, nz}'

# captured RAM over the window
od -A x -t x1 -v -j $((0xfc)) -N 4 private-data/sessions/20261007T185012Z-probe-ram/probes/0061c100.bin
od -A x -t x1 -v -N 16 private-data/sessions/20261007T193028Z-probe-ram/probes/0061c300.bin

# the widened-window census: anchored pairs printed, intersect/cover counts in the per-input SUMMARY lines
devenv shell -- cc -std=c11 -O2 -Wall -Wextra -Werror "$R/filmsim/patch-01/window-coverage.c" -o /tmp/window-coverage
WINDOW_COVERAGE_OUTPUT="$R/filmsim/patch-01/widened-window-coverage.txt" /tmp/window-coverage 61c1fc 61c400 \
  "$R/lz/id4-0061c400.bin" 0061c400 "$R/lz/id5-00bf1a00.bin" 00bf1a00 "$R/lz/plain-01100000.bin" 01100000 \
  "$R/decode/loader-40000.bin" 00040000 "$R/decode/loader-d0000.bin" 000d0000 \
  "$R/decode/boot-40048.bin" 00000000 "$R/decode/boot-50048.bin" 00010000 \
  "$R/filmsim/lang-en-0039c000.bin" 0039c000 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 00130000:00ca025c:0024bac8:1 \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT 003bc000:0106025c:0007cfd4:1 \
  private-data/firmware/x-m1-1.50/export-20260928/payload-complement.bin 0061c000:00140048:00000400:0

# the boot copy and the block's address formations (checked-in listing, byte-read)
awk '$0 ~ /^ +(1dcc|1dd4|1dd8|1ddc|1de0|1f5c|1f60|1f64|1f68|1f78):/' "$R/decode/boot-40048.dis"
awk '/bl[[:space:]]+0x1dc4/' "$R/decode/boot-40048.dis" "$R/decode/boot-50048.dis" "$R/decode/loader-40000.dis" "$R/decode/loader-d0000.dis"

# the request/error-path cases (section 5.4), using a fresh output directory
devenv shell -- bazel build //emulation:load-window-cases //emulation:native-runner
devenv shell -- bazel-bin/emulation/load-window-cases bazel-bin/emulation/native-runner \
  private-data/analysis/x-m1-1.50/native-case/load-window-correction
```

## 9. Residuals

- The historical software trace observes the request/error path with r0 `0xffffff01`. Section 5.5 establishes only the measured stock/RC0 early-load/close interval; section 5.6 adds finite source cuts, not closed transitive writer effects. #43 stays open until ordinary application CPU/device paths either supply a reachable prefix overwrite or are covered by lifetime evidence, including value-identical writes. The remaining work is host reachability and ownership analysis, not SD-card preparation or a camera session.
- The extent-list walk terminates on its first word, so no mapped reader of it reads at or past `0x61c1fc`; consumers outside the mapped set are not excluded.
- The 68 bytes `[0x61c1fc,0x61c240)` are free on this evidence but were never part of any accepted argument; using them is what the corrected floor buys, and the ticket's 444-byte window is accepted without them.
- The D3 name mechanism is the separate special-id question (E-RECIPE-LABEL-SPARE-150), untouched here.
