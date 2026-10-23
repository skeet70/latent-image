# H2-PLAN-01: bounded calibration-area and firmware read scope

Historical: the decisions in section 8 were answered and the capture completed (E-S03-FULL-CAPTURE). Section 7 describes the host machinery of that time (grants, lease, `clear-lease`), since replaced by `//transport:camera` and [the camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure). The firmware findings and read channel facts remain current; O3 completed 2026-10-08 (E-DUMP-RAM-150) and the 1.00-only list is moot since the upgrade.

All target statements are host-only static inference on the public 1.50 DAT, SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, using the plain image at `0x01100000` and decoded images 4/5 at `0x0061c400`/`0x00bf1a00` ([firmware extraction](firmware-extraction.md)). The owner's body runs BODY VERSION 1.00 and nothing here is attributed to its installed code. `FUN_` names are analysis labels. No X-M1 fact below is taken from a sibling model.

## 1. What the owner asked for and what each objective needs

| # | Objective | Data actually needed | Channel found in 1.50 | Gate |
| --- | --- | --- | --- | --- |
| O1 | PRO Neg calibration values for RC0 | The body's own adjustment-area bytes that develop codes 6 and 7 index: colour matrix and chroma data and their selectors between `0xacc3` and `0xb401`, the selectors at `0xc6bc` and `0xcb56`, the flag at `0xc043`, and the signature at `0x202` | Config-data read, D0 opcode `0x1a0002` function 2 | None in the mapped branch, with the length caveat in section 2.1 |
| O2 | The installed 1.00 image | Code bytes at the running load addresses, which 1.00 does not publish and no update file supplies ([no 1.00 DAT was ever released](firmware-extraction.md#preserved-input-and-framing)) | Debug RAM read, D0 opcode `0x200001` function 2, 256 bytes per request | Adjustment-cache byte `0xf7` must be nonzero, which requires a write into that cache |
| O3 | The decompression RAM check | RAM at the load ranges of the images decoded by the inferred grammar, on a body running 1.50 | Same as O2 | Same as O2, and it runs after the planned upgrade rather than before it (section 5) |

O1 is separable from O2 and O3, needs no camera-memory write in the mapped 1.50 branch as long as the host holds every request to 256 bytes, and costs under 5 KiB of reads. O2 and O3 share the persistence question in section 2.3. The three objectives also sit on opposite sides of the planned 1.50 upgrade: O2 is only available before it and O3 is only available after it, which makes the ordering in section 5 part of this proposal rather than a later detail.

## 2. The 1.50 service read and write map

This section is new work from this task and supersedes the general upstream reading in [the transport audit's effect matrix](source-and-transport-audit.md#readconfigdebug-packet-and-effect-matrix), which classified these operations from the pinned `ff80` client rather than from target code. It is author-only, one pass, from the private Ghidra export, and has not been re-derived from bytes by an independent reviewer (section 6, gap P3).

The D0 application dispatcher `0x00d80a80` admits opcodes `0x150000`, `0x1a0000`, `0x1a0001`, `0x1a0002`, `0x200001`, `0x450006`, `0x550001`, `0x580000`, `0x660002` and `0x760001`. Request bytes are read from the fixed request buffer `0x01469a9c`: function at `+0x10`, then four parameter words at `+0x14`, `+0x18`, `+0x1c`, `+0x20`, then appended data from `+0x24`. That layout matches the pinned client's `_mem_op` byte offsets 16, 20, 24, 28, 32 and 36 exactly, so the client's parameter names can be bound to target fields rather than assumed.

### 2.1 Config-data read, opcode `0x1a0002`

`0x00d801cc` accepts function 1, 2 or 3 with `mb` at `+0x14` below 2, and rejects everything else with error1 9 and error2 3. Function 2 passes `(mb, page*0x100 + offset, length, destination)` to `0x00d825f4`, which calls the service callback at `*(0x0146a0c4) + 4`. That table is copied by `0x00dc74b4` from `0x00dc7460`, whose four words read `00dc7564 00dc7664 00dc7848 00dc7874` in the image, so slot `+4` is `0x00dc7664` by bytes rather than by name.

**The firmware does not cap the length on this opcode.** `0x00d801cc` assembles the full 32-bit length at `+0x20` through `0x00d7f40c` and passes it on unchecked, where `0x00d803cc` would reject anything outside 1 to 256. The only bound is `0x00dc7664`'s `offset + length - 1 < 0x420000`, computed unsigned so a large length can also wrap back into range. `0x00d80a80` clears the reply region `0x0146989c`–`0x01469a98`, so a length beyond roughly `0x1d8` makes the firmware byte loop write past it into the request buffer at `0x01469a9c` and, further on, into the service globals that include `0x01469cc0` and the callback table pointer `0x0146a0c4`. The prepared available reply length `0x124` governs only how much the bulk source returns, not how much the handler copies. The 256-byte request cap in section 7 is therefore a host obligation and the single most important line in the adapter.

`0x00dc7664` is address-translated and bounds-checked in firmware, not in the host:

| Effective offset range | `mb` nonzero | `mb` zero |
| --- | --- | --- |
| below `0x420000` | Byte loop through `0x0069f8fc`, reading `*(0x0096ca50) + offset`, which is the live adjustment cache at `0x41acd800` | Copies from the two banks returned by `0x006a9e24`, split at `0x10000` |
| `0x420000` to `0x425fff` | Error `0x80000000` | Error `0x80000000` |
| `0x426000` to `0x42dfff` | Byte loop through `0x0072834c`, which reads one byte per call from flash-log area 0 through `0x00728318` and `0x006acda0` | Copies from the buffer returned by `0x006aaa7c` with its own length check |
| `0x42e000` and above | Error `0x80000000` | Error `0x80000000` |

No branch of `0x00dc7664` writes camera memory other than the caller-supplied destination, and no branch consults the `0xf7` byte. An out-of-range offset returns an error instead of reading; there is no path to an arbitrary source address through this opcode. With a 256-byte request the destination is inside the cleared reply region, which is what makes set A a pure read.

The reply contract follows from the same handler. On success `0x00d801cc` writes the page, offset and length back through `0x00d7f384` into the reply at `*(0x00d803c8)`, `+4` and `+8`, then sets the available length to `0x124`, so a correct reply echoes the three request parameters and carries up to 256 data bytes. Callback status 2 sets error1 1 and error2 `0xffff`; callback status 3, a function outside 1 to 3 and an `mb` above 1 all set error1 9 and error2 3; callback status 1 sets the transfer flag `0x01469cc0` and leaves the error words alone. Those four outcomes are the complete set the handler can produce, and any other combination is a stop.

#### C0 field and D1 length

The C0 suffix is not a D1 reply-size bound in the inspected 1.50 path. `0x00d7f56c` writes `00 00 01 00` at reply offsets 4–7 through the little-endian writer `0x00d7f384`; its units remain unnamed. `0x00d7f8a4` checks active session, matching SID and nonzero requested length, then stores that length at `0x01469868`. The non-streaming bulk source `0x00d7efd4` checks readiness and supplies `min(requested length at 0x01469868, available length at 0x0146986c)` from the prepared reply pointer; its other branch, selected by E1, is section 2.5. Neither inspected routine compares against the C0 suffix. Function 2's handler sets available length to `0x124` at `0x00d80334`–`0x00d80338`.

The pinned `fftlib.py:96–105` also sends the requested D1 length directly, independently of its big-endian `max_pkt_size` interpretation in `open_session`. `ffjlib.py:19–30,88–102` requests 36 prefix bytes plus the config payload. Upstream debug RAM code uses `max_pkt_size` for payload chunking; this does not establish firmware units or an exclusive firmware-write meaning.

The bounded-read host invariant is a 256-byte handler copy and an exact 292-byte D1 transfer, including the 36-byte prefix. C0 must transfer exactly eight bytes; only its little-endian SID is interpreted by this path, and all bytes remain journaled. The adapter does not derive transfer sizes, allocations or budgets from the suffix. Short replies, mismatched echoes, handler errors and failed cleanup still stop without retry.

S03's saved C0 `7c11000000000100` and S02's `710f000000000100` share the static suffix. E-S03-READ-C0-GUARD binds the stopped attempt; it dispatched no configuration command. Injecting those C0 bytes into a synthetic host pipeline can verify the corrected host invariant, not installed 1.00 opcode support, layout, calibration data or restoration.


### 2.2 Debug RAM read, opcode `0x200001`

`0x00d803cc` accepts functions 1 to 5 and rejects 6 and 7, which the pinned client still offers as `debug_cmdline` and `debug_cmdline_rsp`. It requires the length at `+0x20` to satisfy `length - 1 <= 0xff`, so a request reads 1 to 256 bytes and the client's `min(size, max_pkt_size)` chunking of 512 would be rejected with error1 9 and error2 3. The prepared available reply length is `0x11c`, which is the `0x1c`-byte prefix plus `0x100` data bytes.

| Function | Target routine | Effect in 1.50 |
| --- | --- | --- |
| 1 | `0x00647938` | Drains the `0x2000`-byte console ring at `*(0x409c1e2c)`, advances the read pointer and clears the overflow flag, so reading consumes the log |
| 2 | `0x00647b1c` | Byte-by-byte copy from the supplied address with no range check, inside the `0x007235d8`/`0x007236b0` critical section |
| 3 | `0x00647b78` | Byte-by-byte copy to the supplied address with no range check, same critical section |
| 4 | `0x00647938` | Same console drain as function 1 with a different parameter slot |
| 5 | `0x00647bd4` | Splits the appended text at the first space into two 256-byte buffers and invokes the console command callback `*(0x0089edd8)` |

Every one of those five routines returns without acting when `0x0069f8fc(0xf7)` is zero. Functions 1 to 4 return 2 at `0x00647938` and `0x00647b1c`, which `0x00d803cc` maps to `(1,0xffff)`; function 5 returns 4 at `0x00647c18`, which maps to `(1,3)` ([E-DEBUG-ERROR-MAP](../docs/evidence.jsonl)). Function 2 is the only gated read of arbitrary memory; the stock bulk path of section 2.5 reads arbitrary addresses without the gate. Functions 3 and 5 sit one parameter byte away from function 2: function 3 writes arbitrary memory and function 5 dispatches an arbitrary text command. Byte-sized reads of peripheral space can have device-side effects and can abort, and the copy runs with the scheduler lock held, so an address allowlist is a requirement of any use of function 2 rather than a precaution.

Function 5 is also the carrier of the separately recorded End Setting candidate, where the text `POWER OFF` feeds a stock orderly-exit sequence ([E-END-SETTING-CANDIDATE](../docs/evidence.jsonl)). Set B's gate write is what makes that path reachable at all, so a session that enables `0xf7` for reading also opens the text-command route, and the adapter must bind function 2 rather than treat the function byte as a parameter.

### 2.3 The gate byte is calibration data

`0x0069f8fc(index)` returns `*(*(0x0096ca50) + index)`, and `0x0069f204` sets that pointer to `0x41acd800`, the adjustment area loaded from body-specific class-2 flash blocks, `0x28` blocks from `0x3a9` per table `0x7c8cd8`. Index `0xf7` is therefore the byte at `0x41acd8f7` inside the body's own calibration image, not a separate debug register. The same area supplies the product string at `+0x15c` that the mapped tag-4 identity query returns, and the develop-stage selector bytes that RC0 needs.

Writing it uses the same config opcode: function 3 reaches `0x00dc7564`, which writes the cache through `0x0069fc94` for any index with `index + length - 1` below `0x420000` and only then checks the 144-record whitelist at `0x00dc7a84`, which gates nothing except posting queue-7 `0x1707` for starting index `0x80`. A write to `0xf7` therefore lands in the cache and returns success without posting a service command, and a write with `mb` zero still lands in the cache before returning `0x80000000`.

Two cache-to-storage writers exist: `0x0069fea8` writes the whole `0x420000`-byte area through `0x006a9fdc`, and `0x006a0028` writes a bounded sub-range through `0x006abfb4`. Their callers are now mapped and are listed in section 2.4. One of them is service-reachable, so the write to `0xf7` is temporary only while the session never posts the command that commits.

One further consequence of setting the byte is mapped: with tag `0xf7` nonzero, `0x00647398` on the alternative connect branch assigns the console receive buffer `0x41acb800`, zeroes `0x2000` bytes ending exactly at the adjustment cache base, and installs callback `0x0089edd8 = 0x00ed19e4`, and `0x00ecf8b8` changes which connect and disconnect events it emits. Enabling the gate is a behaviour change in the service lifecycle, not an inert flag.

### 2.4 What commits the cache, and what reaches it

A branch, literal and `movw`/`movt` scan of both decoded images, the plain image, both loaders and both boot records finds four A32 call sites for the two writers and two for the reload, and no aligned pointer word holding any of the three. The scan reads 4-byte units and matches A32 `B`/`BL` displacements, exact words and `movw` followed by `movt`, so it would miss a Thumb caller, a pointer carrying the Thumb bit, an unaligned pointer word, a `movt` emitted first, an address formed as a base plus an offset, and any reference in the regions it did not cover, including payload `0xca0048` and the extents at payload `0xe0048`–`0x111e08`. Thumb use in these images is still open ([firmware extraction](firmware-extraction.md)).

| Writer | Call site | Reachability |
| --- | --- | --- |
| `0x0069fea8`, whole `0x420000` area | `0x00db932c` in `0x00db91e0` | Firmware-update configuration completion, called from `0x00ec88c8` (section 5) |
| `0x0069fea8` | `0x00dbc8e4` in `0x00dbc8d8` | Service command value `0x22`, the chain below |
| `0x0069fea8` | `0x00d5cf0c` in `0x00d5cc08` | Exit events 6, `0x210`, `0xa02` and `0x603` of an on-camera byte-editor screen, gated on `0x0096b480 == 3`; the same screen writes single cache bytes through `0x0069fc94` |
| `0x006a0028`, bounded | `0x0073d440` and `0x0073d458` in `0x0073d3ec` | Init task family through `0x0073e834`, gated on cache bytes `0x238` and `0x237`; it writes only offsets `0x400000` length `0xfd00` and the byte `0x237`, never `0xf7` |
| `0x0069fe20`, reload from storage | `0x00db9228`, `0x00dbc8c4` | The same update completion, and service command value `0x21` |

The service chain is closed end to end: a config write of value `0x22` to index `0x80` lands in the cache, matches whitelist record 26 at `0x00dc7a84` whose `+4` word is 1, calls `0x00ed1300(0xff,0x22)` and posts queue-7 `0x1707` with that one byte; `0x00eca4c0` requires event `0x1707`, reads the payload byte and sets group-2 row 26; `0x00ec9b50` indexes matrix `0x00ece2b8` at width 7, where row 26 holds action `0x1d` in states 0, 1, 2, 3, 5 and 6 and nothing in state 4; action table `0x00ecdf7c` entry 29 is `0x00ecc66c`, which calls `0x00dbc8d8` and so the whole-area writer. Value `0x21` is the same chain to row 25, action `0x1c`, and the reload instead. This is the target-code form of the pinned client's `config save`, which sends app80 command `0x22`.

Two consequences for this proposal. The gate byte written for set B stays in RAM unless the session posts `0x22`, and a power cycle reloads the area from storage, so the write is temporary under a session that never touches index `0x80`. The adapter must refuse config index `0x80` absolutely, not merely avoid it, because the cache write in `0x00dc7564` happens before any check and a single wrong index byte is the whole distance between a read session and a calibration commit.

### 2.5 Stock bulk read, opcode `0x450006` tag `0x80` and transport command E1

**Byte-read in image 5.** The same transport carries a second arbitrary-address read that bypasses the 256-byte copy:

| Step | Routine | Effect |
| --- | --- | --- |
| D0 opcode `0x450006` | `0x00d80a80` case `0x00d80c54` → `0x00d806c8` | Reads the tag at request `+0x10` through `0x00d7f40c` and calls `0x00d8270c(tag, 0x01469878, 0x0146987c)`, which calls slot `+0xc` of the table at `*(0x0146a0c4)`, `0x00dc7874` in the `0x00dc7460` template. On return 0 it writes the capacity at reply `+0x14` through `0x00d7f384`, and the available reply length is 32 |
| Tag `0x80` | `0x00dc78bc`-`0x00dc78c4`, `0x00dc79c0` → `0x00dbd09c` | Stores pointer `0x4b2ece00` to `0x01469878` and capacity `0x100000` to `0x0146987c`, adding `0x8000` when `0x006fbaa4` (word `0x008fc864`) returns 1. `0x4b2ece00` is the [raw-media staging window](adjustment-and-service-interfaces.md#raw-media-read-interface) |
| E1 | `0x00d7fae8` → `0x00d7fa4c` | The dispatcher clears flag `0x01469c9c` on every command. E1 reads a big-endian offset at `+2`, big-endian length at `+6` and the SID at `+10`, refuses an inactive session (`0x01469898`), a SID mismatch (`0x0146989a`) and length 0, then sets `0x01469c9c = 1`, length `0x01469868` and offset `0x01469880`. The offset is not compared with anything |
| Bulk IN | `0x00d7efd4`, slot `+0x14` of table `0x00d7f314` | With `0x01469c9c` set it returns length `min([0x01469868], [0x0146987c])` and source `[0x01469878] + [0x01469880]` (`0x00d7f01c`-`0x00d7f044`), a 32-bit sum with no aperture check and without the D1 branch's readiness bytes |

An address `A` is therefore read with offset `(A - 0x4b2ece00) mod 2^32`, at most `0x100000` or `0x108000` bytes per E1. No routine listed reads cache byte `0xf7`. Only the `0x450006`, `0x550001` (`0x00d80844`) and `0x580000` (`0x00d80924`) handlers rewrite `0x01469878`/`0x0146987c`, so one tag-`0x80` query serves repeated E1 reads (static-inference). x70view's locator derives the same three addresses for `xm1-150` (`0x4b2ece00`, `0x00d7efd4`, `0x00d7fa4c`), and all 15 code checks of FujiMod's `xm1/1.50` bulk catalog hash-equal these bytes (E-BULK-READ-PATH-150, E-BULK-CATALOG-HASHES-150).

A whole-RAM read on this body needs a C0 session, one `0x450006` tag-`0x80` D0 request with its 32-byte D1 reply, then per chunk an E1 command and a bulk IN of up to the capacity, about 512 E1 exchanges for 512 MiB against roughly 2.2 million function-2 requests at 248 bytes. The transport has no E1 frame and admits bulk transfers of 1 to 292 bytes. Unproven on X-M1: the installed callback pointer in the session's mode (slot `+4` of the same table answers config reads on this body, E-ADJ-READ-150), whether the USB engine reads the source directly or through a CPU copy (the consumer of `0x00d7efd4`'s pointer is not traced), the safe address set (physical DRAM extent, bit-30 alias cache behaviour and MMIO side effects), and the rate. x70view's 512 MiB window at `0x40000000` and 12.66 MiB/s are X70 values, and the stock staging buffer's contents belong to the raw-media read and frame-buffer pool.

## 3. Proposed scope A: adjustment-area read for RC0

Nineteen aligned 256-byte windows, 4864 bytes total, all through opcode `0x1a0002` function 2 with `mb` fixed at 1 and length fixed at `0x100`. The list is resolved from nine develop-stage offset tables with a tool rather than transcribed (gap P4, closed below); the earlier transcribed list of eight was both short and wrong at the ends.

| Order | Windows | Why |
| ---: | --- | --- |
| 1 | `0x000000`–`0x0000ff` | Page 0, which holds the gate byte `0xf7`; set B must know its original value before writing it |
| 2 | `0x000100`–`0x0001ff` | The oracle window: `+0x15c` holds the 19 bytes the tag-4 identity reply copies |
| 3 | `0x000200`–`0x0002ff` | Signature `+0x202`, which `0x0069f204` compares against `0xaa55` to decide whether the body uses its own data or the firmware fallback. The adjacent `+0x204` and `+0x205` are stamped by the running firmware rather than body data, so they report the installed build and carry no weight for the offset hypothesis |
| 4–12 | `0x00ac00`–`0x00b4ff` | Colour matrix data from table `0x00d67f88`, its selectors at `0xae2b`, the filter selectors at `0xae30`, chroma matrix data from table `0x00d61e14`, chroma selectors at `0xb0c7`, the `0x00d61e04` selectors at `0xb0f4` and the hue-range selectors at `0xb3fd` |
| 13 | `0x00c000`–`0x00c0ff` | The flag at `0xc043` consumed by `0x00d5eed0`, which is an immediate in code rather than a table entry |
| 14–19 | `0x00c600`–`0x00cbff` | The `0x00d6a5c4` selectors at `0xc6bc` and the H/S tone selectors at `0xcb56`, with the span between them, which the whole extents of those two tables reach |

Windows run in ascending offset order, which the adapter requires and which rules out an overlap or a repeat. The oracle is checked when its own window returns, so in this scope exactly one window precedes the check. If the bytes at `+0x15c` do not match the product string bound into the adapter, the channel, the address translation or the 1.00 layout differs from the 1.50 map and the session stops with seventeen windows unexecuted. That test is the only cheap way to learn whether 1.00 implements this opcode at all, because nothing in the 1.50 image binds 1.00's dispatcher. The 14-window reduction puts the oracle first, since it does not include page 0.

Transfer accounting is exact: C0 open is 2 transfers, each window is 4, and C4 close is 2, for 80 transfers and 4864 read bytes. The tag-4 identity query stays in `identify-live` and adds 4 transfers in its own session, so the oracle comparison against a same-session query is an offline comparison of two records.

The scope is the upper bound on purpose. The lower bound, the row-0 entries each table binds for film codes 0 to 7, is 14 windows and 3584 bytes, and the difference is 5 windows and 20 transfers. The row counts the stage functions index are not bounded from code, because `0x00d671b4`, `0x00d60ea8`, `0x00d618a4`, `0x00d69df0` and `0x00d6a11c` are not in the decompilation export, so a lower-bound capture risks being short. Coming up short costs another jig entry and another user-configuration reset, or costs the data outright once the 1.50 upgrade lands. The owner may still choose the 14-window reduction.

Mapped camera effect of set A: protocol session state, the request and reply RAM fields already listed in [the service query map](service-query-and-jig-reset.md#exact-query-dispatch), and no camera-memory write. By the [H1 row](../spec/07-governance-and-handoff.md#32-operation-classes) that is a bounded read requiring no camera-memory write. The recommendation is still to request H2 coverage for it, for three reasons: the effect classification rests on 1.50 code and the body runs 1.00, the write function differs from the read function by one parameter byte on the same opcode, and the jig session that carries it has its own persistent cost (section 7).

What set A yields: the body's own PRO Neg selector inputs, which [the film-simulation map](film-simulation.md#processing-by-value) names as the missing piece of RC0 and which currently exist only as fallback-image values. What it does not yield: whether 1.00 implements PRO Neg processing at all, the hardware programming that follows the parameter block, or reachability behind the availability matrix. Interpreting the bytes by 1.50 offsets on a 1.00 body is a hypothesis under test, not a result; the signature in window 2 is the first check of it.

Set A also has a reason to run before the 1.50 upgrade rather than after it: it would be the only record the project holds of this body's adjustment data taken before a firmware write, and the update's effect on that area runs through a conditional path (section 5).

A third scope exists and the owner should price it. Capturing the first `0x42000` bytes of the adjustment area, the extent `0x0069f204` zeroes on signature failure and the extent the update completion reloads and restores, is 1056 windows, 4224 transfers and 270,336 bytes. That is a complete copy of the part of the body's calibration image those two paths touch, against the 4864 bytes of set A, and it is two orders of magnitude below set B's 43,700 requests. The `0xfd00` block at cache offset `0x400000` that `0x0073d3ec` also writes adds 253 windows. No body-specific calibration backup exists today ([principles](../docs/principles.md#scope-and-evidence) require two verified durable locations for one), and this session is the only mapped way to start one, so the choice is between 19 windows of RC0 inputs and 1309 windows of the data the project would want if a later write went wrong.

## 4. Proposed scope B: firmware read, conditional and not yet requested

Set B is dropped as of 2026-10-05 (decision 4) and nothing in the delivered tooling implements it. It stays described here because it is the only route to O2 and the only part of this work the planned upgrade destroys, so a later reversal has the shape and the cost in front of it. Section 2.4 settled its persistence question: the write to `0xf7` is temporary under a session that never writes config index `0x80`, and a power cycle reloads the area from storage.

Shape: write 1 to adjustment index `0xf7`, read 16 probe windows of 256 bytes at the 1.50 segment-table and load addresses `0x0061c000`, `0x0061c400`, `0x00bf1a00`, `0x01100000` and the loader bases `0x00040000`/`0x000d0000`, then restore the original byte at `0xf7`. Each probe is a hypothesis about where 1.00 loads its images, and the 1.50 addresses are the only candidates the project has; a miss is a result, not a failure to be worked around by scanning.

Four facts bound how far this can go. The gate write is reversible only while no commit command is posted, so the session must refuse index `0x80` rather than merely omit it. The 256-byte cap makes a full image capture about 11.2 MB, about 43,700 requests and about 175,000 transfers, using 1.50 sizes as the only available order of magnitude. The adapter's 1 MiB journal cap leaves about 24 bytes per request at that count, which is less than one journal record, so a full dump is excluded by the current host design independently of the camera. A RAM read returns loaded, decompressed images, not the stored compressed flash records, so its output is a comparison artifact and never a flashable 1.00 restore image.

## 5. Sequencing around the planned 1.50 upgrade

The upgrade is a planned eventual step, not a rejected one: patch work targets 1.50, the owner intends to get there, and staying on 1.00 is a delay held only for the value still extractable from the installed code. That makes the question an ordering question with one irreversible boundary, because no 1.00 update file was ever published and no mapped path reads the stored image, so nothing recovers 1.00 once it is overwritten.

What is available only before the upgrade: O2, and with it any installed-1.00 correspondence check for the 1.50-only [AF static map](autofocus-static-map.md), [service static map](service-query-and-jig-reset.md) and the film-simulation trace. Its value decays to near zero the moment the upgrade is decided, because 1.00 correspondence matters only while 1.00 runs, and because a RAM read cannot serve as a 1.00 restore image. The honest reading is that set B buys a historical cross-check of maps that will be re-checkable directly against the installed code afterwards.

What is available only after the upgrade: O3 as the real check, where decoded images and RAM come from the same firmware, which is the comparison [restart point 4](firmware-extraction.md#decoder-research-and-restart-points) specifies and the only one that can bind [E-LZ-GRAMMAR](../docs/evidence.jsonl) on hardware. Every other 1.50 static map also becomes directly verifiable, including the service read map in section 2, the AF graph, the settings-reset path and the G2 load addresses, each of which currently carries "installed correspondence unknown" as its standing limitation.

What is better after the upgrade: set A itself. Its window offsets are 1.50-derived, so reading them on 1.00 is a hypothesis test and reading them on 1.50 is a direct lookup. Set A keeps its pre-upgrade value because the update can rewrite the area it reads. The mapped writer block selection at `0x7c8c6c` covers logical blocks `[0,0x126)` by class below 2, while the adjustment area loads from class-2 blocks from `0x3a9`, so the main image write set excludes it. The configuration completion `0x00db91e0` is the path that reaches it, and it returns immediately unless `0x014e3578 & 3` is nonzero; the producer of that flag is untraced, so whether an ordinary update reaches the rewrite at all is unknown. When it does run it copies `0x42000` bytes of the live cache to scratch, reloads the same `0x42000` from storage through `0x0069fe20(0xff)`, restores only the ranges named by two lists carried in the update descriptor, bounded at `0x10000` for the 16-bit list and `0x42000` for the 32-bit list, and finally commits the whole `0x420000` area. Body data survives that in the normal branch because the reload comes from the body's own storage, and does not survive it if initialization took the signature-failure branch at `0x0069f204`, which zeroes `0x420000` bytes before the commit would write them back. The unresolved flag and the signature-failure case are the reason to read before the upgrade, not after.

Recommended order. Run set A on 1.00 as the pre-upgrade snapshot of body adjustment data, including the signature at `+0x202` that decides which initialization branch the body takes. Treat set B as optional and time-boxed: if the host work in section 7 closes while the owner still wants 1.00 correspondence, run it; otherwise drop it rather than delay the upgrade for it. Section 10 lists the 1.00-only questions and the condition that ends them; run O3 and the correspondence checks after the upgrade.

Two consequences of the upgrade belong in the decision. It writes firmware, so it is H3 and needs its own authorization and a calibration capture first, since no body-specific backup exists today; the exclusions [UPDATE-01](update-validation-and-boot.md) is establishing govern a project-modified image through that writer, not the vendor DAT the owner accepts as supported. The stock AF baselines from S01 and S02 were recorded on 1.00, so a G6 comparison must not straddle the version change and the baseline must be retaken after it.

## 6. What must close offline before any authorization request

| Gap | Target | Blocks |
| --- | --- | --- |
| ~~P1~~ | **Closed 2026-10-05.** Four A32 call sites for the two writers, two for the reload, no aligned pointer word, and the service-reachable chain bound to config index `0x80` value `0x22` (section 2.4) | Was set B and the `0xf7` write; now an adapter requirement instead of a research gap |
| ~~P2~~ | **Closed 2026-10-05.** The `0x426000` window is not the cache: with `mb` nonzero `0x0072834c` tail-calls `0x00728318`, which reads one byte per call from flash-log area 0 through `0x006acda0` and its geometry queries, and with `mb` zero `0x006aaa7c` returns a block-derived base and length from descriptors `0x007c8d08`–`0x007c8d18`, gated on `0x00828574 == 1` | Was the set A allowlist boundary; that window stays out, because a 256-byte read there is 256 storage reads rather than a RAM copy |
| ~~P3~~ | **Reviewed 2026-10-05** by an independent Opus 5 reader, after an Opus 5.5 request was stopped by the service `cyber_policy` classifier. It byte-anchored callback slot `+4`, found the missing length check in section 2.1, and scoped the commit-site completeness claim to A32 references | Was the whole proposal; the corrections are applied and the remaining exposure is that one reader worked from the same decompilation export |
| ~~P4~~ | **Closed 2026-10-05.** Window list generated from the nine develop-stage offset tables, extents bounded by the next function entry; 19 windows proposed with 14 as the reduction (section 3) | Was set A's window table; the transcribed list it replaces was short by five windows |
| ~~P5~~ | **Closed 2026-10-05.** Success echoes page, offset and length and advertises `0x124`; the handler produces exactly the error pairs 9/3 and 1/`0xffff` plus the status-1 transfer flag (section 2.1). The advertised length is not a copy bound | Was set A's stop conditions and the adapter's refusal tests, which can now be written against a closed set |
| P6 | Host work in section 7 | Both sets |
| P7 | **Bounded, not closed, 2026-10-05.** The update completion reloads `0x42000`, restores update-named ranges and commits `0x420000`, but it runs only when `0x014e3578 & 3` is nonzero and that flag's producer is untraced (section 5) | Was the set A ordering question; either answer gives the same order, which is to read before the upgrade |

No gap now blocks a decision. P6 is work rather than uncertainty, P7 moves only the strength of the ordering argument, and the proposal's remaining exposure is that sections 2.1 to 2.4 rest on one decompilation export that two readers have now used.

## 7. Host work, session shape and cost

The adapter could not carry either set before this work. `identity-experiment.schema.json` pins `operation_id` to `identify.product.v1`, `maxItems` to 1 and `operation_count` to 1, and the only live commands were `identify-live` and the cached collector. The generic `experiment.schema.json` already admitted `H2` and an operations array, so the work was a new specialization and a new command rather than a schema redesign.

Delivered on 2026-10-05 and passing in the 28-target suite: the three read schemas plus a read-adapter schema, a compiled `validate-read` that performs the product query's preflight class and owns the three invariants JSON Schema cannot express, `read-config` executing a plan's windows in one C0 session with no retries, the `ReadCompleteStockUnverified` lease phase through `clear-lease`, a journal cap of `65536 + 4096` bytes per planned window against an 8 MiB ceiling, an aggregate deadline of `transfer_timeout_ms * (8 + 4 * windows)` against a 7,200,000 ms ceiling, and the `readplan` package that resolves the window list from the images and compares captured bytes against the fallback adjustment image. The refusal of config index `0x80` and payload values `0x21` and `0x22` became structural rather than named: the read command builds its request frame from a window offset with function fixed at 2, so no manifest, plan or argument can reach an index or payload field, and a test sweeps every one of the 16,896 admissible offsets asserting function 2 and the absence of opcode `0x200001`. What remains before a session packet exists is the packet itself, which needs the owner's scope decision.

The lease decides the session shape, and it admits one camera-facing command per cycle. `withLeaseScope` accepts only `MetadataAwaitingQuery` with matching handoff and request digests, `identify-live` leaves `QueryCompleteStockUnverified`, and only an offline `clear-lease` against a verified-normal stock-after clears that, which in turn needs the power-off exit. So the identity query and the read cannot share one jig entry. The S03 packet under `private-data/sessions/S03/` implements two cycles: run 01 is identity and run 02 is the read, each with its own collection, its own jig entry and its own exit. Its rehearsal passes offline for all three scopes.

The read cycle alone is sufficient, and that is the cheaper shape. `read-config` checks the oracle against the product bytes compiled into the adapter rather than against a reply from the same session, so the identity cycle buys a same-body product string for an offline comparison and costs a second jig entry. Each cycle's power-off exit resets date, time, language and the C slot ([E-JIG-OBSERVED](../docs/evidence.jsonl)), and the 1.50 map shows a disconnect path reaching full settings reset with guarded persistence ([E-SETTINGS-RESET-MAP](../docs/evidence.jsonl)). The owner records those settings before each entry and restores them after. No settings-preserving exit is justified yet, and [SAFE-JIG-EXIT-01](https://github.com/skeet70/latent-image/issues/3) remains open.

The measured costs from the packet are 60 camera transfers for 14 windows, 80 for 19 and 5240 for 1309. The complete capture does not fit the 7,200,000 ms session ceiling at the 5000 ms per-transfer budget, which would need 26,220,000 ms; it is admissible only at a 1372 ms budget, and no measurement of this channel's actual per-transfer latency exists.

Stop conditions specific to these sets: the window 1 oracle mismatch, any nonzero error1, any reply whose echoed parameters differ from the request, any reply length other than the requested one, an unexpected disconnect, a PM or quirk state change at the recheck, and any unknown status. Each stops the session with the remaining windows unexecuted; none of them permits a retry, a second close attempt beyond the one the adapter already allows, or a substitute command.

## 8. Decisions, answered 2026-10-05

1. **Scope: the complete capture**, the 1309 windows covering the `0x42000` extent and the `0xfd00` block at `0x400000`, 335,104 bytes, conditional on it fitting the camera's battery window. That condition is measurable rather than assumable, so the read runs in two cycles: 19 windows first, then the complete capture with a per-transfer budget and aggregate deadline derived from the latency the first cycle's journal records. The two-hour figure in the packet is the arithmetic ceiling at the clamped 1372 ms budget, not an expected duration, and no measurement of this channel exists.
2. **The identity cycle runs.** A jig entry costs little when the settings are not being held constant; it costs most during before-and-after autofocus work, where each entry destroys the experiment's settings.
3. **H2**, which `read-experiment.schema.json` already fixes.
4. **Set B is dropped**, on the coordinator's call rather than the owner's. Its value is a historical cross-check of maps that the upgrade makes directly checkable, it cannot serve as a restore image, and it is the only part of this work that would write the adjustment cache. Section 10's exhaustion list therefore reduces to E1 and E2.
5. **Order stamped**: the capture before the upgrade, O3 and the correspondence checks after it.
6. **Grant**: the coordinator sets the window to the session day when the read cycle's documents are generated. It is a binding the validator checks, not a decision the owner makes.

## 9. Evidence status

Sections 2.1 to 2.3 are static inference on the 1.50 image, author-only, single pass, read from the private Ghidra export under `private-data/analysis/x-m1-1.50/af/functions/` plus the private service findings under `service-commands/` and `safe-jig-exit/`, recorded as E-SERVICE-READ-MAP. Section 2.4 is the same label and the same author, recorded as E-ADJ-COMMIT-MAP, from a branch and literal scan of both decoded images, the plain image, both loaders and both boot records, plus listings of the row mapper and a byte read of the whitelist table. Neither is independently reproduced. Neither establishes installed 1.00 behaviour, target execution, or that any of these opcodes exists in the 1.00 service.

Section 2.5 is static inference from a byte-read disassembly of the decoded 1.50 image 5, one pass, cross-checked against x70view's independent locator and FujiMod's code-check hashes (E-BULK-READ-PATH-150, E-BULK-CATALOG-HASHES-150). No E1 request has been sent to this body.

Sections 3 and 4 are proposals. No byte in them has been read from a camera. The window list in section 3 is resolved by tool from the develop-stage offset tables behind [E-FILMSIM-MAP](../docs/evidence.jsonl), with extents bounded by the next function entry and row counts beyond row 0 unbounded at the time (bounded since 2026-10-08: E-ADJ-READ-150 limits), so the 19-window list covers the extents while the 14-window reduction covers only what the trace binds. Section 5 reasons from the published-update record, the mapped updater block selection and the configuration completion read in this pass, whose entry condition is untraced; its ordering is a recommendation, not a finding. An independent Opus 5 review on 2026-10-05 corrected the length bound, the callback binding, the scan scope, the version bytes and the scope choice; its findings are applied above. No research gate, recovery level or authorization is advanced by this document.

## 10. The 1.00-only list and the exhaustion point

Two items need the body as it stands today, after set B was dropped. The other open work considered by this proposal is 1.50 static work or host work, and most of it gets cheaper after the upgrade rather than before it. Current open work is tracked in [GitHub Issues](https://github.com/skeet70/latent-image/issues).

| Item | Why only now | Value | Cost |
| --- | --- | --- | --- |
| E1 | The body's adjustment data must be captured before any firmware write, because the update's configuration completion can rewrite the area it lives in and the signature-failure branch at `0x0069f204` zeroes `0x420000` bytes | Irreplaceable. No body-specific calibration backup exists, [principles](../docs/principles.md#collaboration-and-records) require two verified durable locations for one, and RC0 needs the same bytes | One session at the scope chosen in decision 1, gated on the host work in section 7 |
| E2 | Whether 1.00 implements this read channel and whether the 1.50 offsets describe its layout can only be asked of 1.00 | Decides whether E1 produced data or noise | The oracle window of the same session, one request |
| ~~E3~~ | **Dropped 2026-10-05.** The installed 1.00 code exists in no published file and in no backup | Near zero. Every map it would check becomes directly checkable once the installed code is the mapped code, and it is not a restore image | Would have cost the `0xf7` write, adapter work for opcode `0x200001` and thousands of requests |

Four things are not on this list and should not hold the upgrade. AF timing work measured on 1.00 cannot be compared across the version change, so beyond the stock reference frames already recorded it is effort spent twice. The "installed 1.00 correspondence" limitation standing on SERVICE-STATIC-01, SAFE-JIG-EXIT-01, AF-STATIC-01, G2 and UPDATE-01 is resolved by the upgrade, not by waiting: those maps are of 1.50, so they become testable when 1.50 is what runs. RECIPE-00's menu record was taken on 2026-10-03 and wants a post-upgrade re-record as a diff. The G0 query and clearance binding and the G1 identity readback are version-independent and fold into whichever session happens next.

The exhaustion point is reached when both hold. E1 is captured, verified against the session journal and stored in two durable locations. E2 is answered in that session, either way. At that point none of this proposal's remaining work requires this firmware, and the upgrade is the next step rather than a cost.

The vendor update path is accepted as supported, so what the installed 1.00 updater does with the 1.50 DAT is not a research target: the answer stops mattering the moment the first upgrade completes, and no route returns the body to 1.00 to use it. That is what left E3 with nothing to buy. [UPDATE-01](update-validation-and-boot.md) stands as a map of the destination build, which is what the later patch work needs.
