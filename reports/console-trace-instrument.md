# The console trace instrument: format, version-bound findings and private inputs

Status: host-only consolidation of already recorded evidence, one full read of the two private decoders and of the private notes that hold the record-layout derivation, plus the decoder correction and delegated synthetic CLI verification in section 1.2. A host-only follow-up binds type `0x71`'s tick at `+4` from its pinned writer `0x0004208c` and aligns both decoders, the synthetic fixture and the smoke expectations with that binding (section 1.2). The [same-boot carry closure](af-observe-instruments-150.md#9-same-boot-carry-closure-over-the-pinned-bytes) adds pinned-byte writer analysis for retained capacity, the first-failure latch and type `0x71` in section 2. No camera or USB device was touched and no capture was moved. Labels: **byte-read** for bytes in the pinned X-M1 1.50 artifacts or in a capture, **static-inference** for conclusions drawn from them, **hardware-observed** for recorded body observations, **reported** for the owner's correspondence, **unproved** where the named fact is unavailable.

The X70 emulator's author wants to record autofocus traces on a camera and replay them into his emulator (**reported**, owner's 2026-10-09 correspondence). This project's console trace instrument records the kernel's message traffic, including the AF and lens tasks' sends, which speaks to that goal directly. This report separates what a third party would need to use it on their own body into three parts, gives a verdict on the two decoders, describes the store in a form that can be implemented from, and lists which files to consider publishing and which must stay private. Every firmware-layout value below is scoped to X-M1 1.50 and is loader or image layout rather than silicon fact; a different firmware re-derives them, and a different unit running the same firmware holds the same values. This report carries no unit-specific value, no serial and no capture content.

## 1. The three parts

### 1.1 The operation that reads the store: published

`read-log` in `//transport:camera` is the operation. `transport/FujiAF/Log.hs` builds every byte, `transport/LogTest.hs` replays the transfer plan against synthetic replies and checks the decoding, and [the transport README](../transport/README.md) publishes the wire protocol (**byte-read**). It opens the vendor service session, enables the adjustment-cache gate byte every debug read requires and confirms the readback, reads the store whole as 512 fixed 256-byte windows of two 128-byte `0x200001` function-2 debug reads each, samples the tick cell twice before and once after the store, restores the gate, confirms the restore and closes. Every window is hashed into the journal and the window bytes are judged host-side, so no in-session check is an oracle.

What a third party takes from this: the request shape, the fixed window list, the gate discipline and the stop rules are published and reimplementable. The wire bytes are X-M1 1.50's service surface, so whether another body's firmware exposes the same opcodes is **unproved**; a different body needs its own equivalent read channel, and the store format of section 2 is what that read must return.

Ordinary-boot `read-ram`/`read-bulk` are now a separate reviewed extraction route ([bulk-read report](ff80-bulk-read.md), E-BULK-READ-RUN1-150 through E-BULK-READ-RUN3-150). This supersedes the maintenance-only access premise, not the decoder's coherence limits. The [AF observation plan](af-observation-plan.md) still requires actual stock AF under USB and same-boot extraction to be demonstrated.

### 1.2 The format knowledge: what the decoders are

The store geometry, per-type layouts and tick derivation are implemented in [the public shared decoder](../console-trace/FujiAF/ConsoleStore.hs). Section 2 retains the version-bound format and evidence.

The original `RingCheck.hs` and `TraceCensus.hs` analysis helpers are now public as `//console-trace:ring-check` and `//console-trace:trace-census`. They contain no capture data, private identity or body calibration. RingCheck's task IDs and release-bit convention remain X-M1 1.50 annotations. Both use one shared format decoder.

The public decoder selects ticks by type: `0x0f` at `+0xc`, `0x72` at `+4`, `0x09` at `+8`, and `0x71` at `+4`. Unknown types have no decoded tick. Failure deltas and retained-record payload words remain uninterpreted. Counts include all records; tick bounds include only mapped records.

Run `devenv shell -- bazel run //console-trace:ring-check -- store STORE.bin` or `devenv shell -- bazel run //console-trace:trace-census -- store STORE.bin 2b` for a raw console-store file. Both also accept `high-known FILE` (store at file offset `0x800`) and `windows DIRECTORY` (consecutive 256-byte files named by addresses beginning `400a0800.bin`). Inputs must provide the header and complete record prefix; capacity must be 8188. Census thread selectors are hexadecimal bytes. Window sets and live captures can be non-atomic; slot order is not a proven chronological order after wrap.

`devenv shell -- bazel run //console-trace:log-diff-check -- BANK0.bin BANK1.bin` compares two owner-supplied complete 128-KiB area-2 banks. It requires a nonempty, unique, contiguous 15-bit sequence chain across the first 31 record slots of each bank. It reports record/body differences and occupancy; it does not validate firmware parity, prove persistence, or authorize a camera operation. `//console-trace:decoder-tests` uses synthetic records and banks.

Synthetic CLI verification (**host-only**, E-CONSOLE-TRACE-TICK-DECODE-150): a delegated runtime worker ran the hand-authored fixture generator and before/after smoke commands under `devenv`, comparing full stdout against independent literal expectations. Both accepted file forms were exercised: a high-known segment with a zero prefix and a two-file window set carrying the same synthetic store. The nine records include input press/release and a general message, an eight-byte marker, two dispatch failures whose tick/delta pairs are `900/3` and `7/4294967295`, a `0x71` record whose decoded tick is `42` and whose payload words stay uninterpreted, and an unmapped `0x55` record. The corrected tools passed all eight CLI cases, including both census thread-selection modes and the unchanged quiet input output; the saved originals failed six cases and passed the two quiet input cases, their census bounds consuming the failure deltas and unmapped words (`2c` `3..4294967294`, `44` `4294967295..4294967295`, `55` `9..9`) where the corrected tools bound the mapped records only (`2c` `42..900`, `44` `7..7`, `55` `tunknown`). The original RingCheck verbose output did not expose non-message ticks. The regenerated fixture files match the SHA-256 values in the provenance file. Fixture bytes, literal expected outputs, provenance and runtime results remain under ignored `private-data/analysis/x-m1-1.50/trace-tick-smoke/`. This verifies synthetic decoding and output, not camera timing, event absence, trace completeness or the meaning of `0x71`'s payload words.

### 1.3 Version-bound findings and unit-specific inputs

Task/message IDs, firmware layouts, input codes and recovered interfaces are publishable technical findings when bound to their version and evidence scope. [The AF instrument report](af-observe-instruments-150.md) and [physical input map](input-map-150.md) own those bindings. A different version or body needs independent applicability evidence. Raw captured bytes, private identities and owner material remain private; a distilled traffic count, tick interval or control-flow conclusion is not private merely because a capture supplied its evidence.

## 2. The instrument

### 2.1 Location and geometry

The store is a fixed loader-owned RAM region at `0x400a0800`, `0x1fff4` bytes: a `0x34`-byte header and 8188 entry slots of 16 bytes from `+0x34` to `+0x1fff3`, so the last slot ends at `0x400c07f3` (**byte-read**, E-LOG-STORE-150). The header halfword at `+0x1c` is the capacity `0x1ffc`, 8188 slots, and the halfword at `+0x1e` is the next index, which wraps to zero at capacity. The writers clear all sixteen bytes of the new next slot after advancement, so the store retains at most 8187 records once full (**byte-read** for advancement and clearing, **static-inference** for retained capacity; [same-boot carry closure §9.2](af-observe-instruments-150.md#92-writer-order-controls-and-coherence)). No capture has wrapped.

Header fields (**byte-read**): the ASCII magic `syslog Ver 2.3` at `+0x00`; a constant halfword at `+0x14`; two bytes at `+0x16`/`+0x17`; a halfword at `+0x18`; a halfword at `+0x1a` that a second initialiser call rewrites from the low half of the timer block's `+0x004` register (**static-inference**); the word 2 at `+0x20`; and a sixteen-byte first-failure latch at `+0x24`. Writer `0x00041a34` copies its type-`0x09` record there only while the latch's type byte is zero; it is not a mirror of slot 0 ([same-boot carry closure §9.2](af-observe-instruments-150.md#92-writer-order-controls-and-coherence)).

The tick cell sits at `0x400c0800`, twelve bytes past the store end, and the twelve bytes between read zero in every capture (**hardware-observed**). The loader initialises a second store at `0x40063460` with the same header layout, capacity and mirrored writer set; no session has captured it and its contents are **unproved** (**byte-read** for its existence). An instrument that reads only `0x400a0800` covers one store of two.

### 2.2 Record layout

Four loader writers supply the captured record types, and the layouts differ after the first two bytes (**byte-read** from the writer code; the original three layouts are in `private-data/analysis/x-m1-1.50/dump/2026-10-08-syslog.md`, and the fourth is bound by `af/functions/0004208c.c` under the same pinned analysis root).

| Type | Record | `+1` | `+2` | `+4` | `+8` | `+0xc` |
| --- | --- | --- | --- | --- | --- | --- |
| `0x0f` | message and event trace | writing task's id | timer delta | receiver, sender, message id | code and param | tick |
| `0x72` | marker string | writing task's id | timer delta | tick | at most eight string bytes | |
| `0x09` | dispatch failure | arg0 | arg1 | task id and arg2 packed | tick | timer delta |
| `0x71` | two-word record, payload meanings unproved | writing task's id | timer delta | tick | arg0 | arg1 |

All fields little-endian and the type byte is at `+0`. For `0x0f`, `0x72` and `0x71` the byte at `+1` is the low byte of the writing task's `TPIDRURW`, with zero used if its sign bit is set (**byte-read**). The `0x72` writer copies at most eight bytes of a NUL-terminated string. The `0x09` writer packs `(TPIDRURW & 0x7fffffff) << 16 | arg2` at `+4` and gates itself per id through a byte table, so a zero table byte suppresses the record (**byte-read**).

For type `0x0f` the word at `+4` is `{receiver u8, sender u8, message id u16}` little-endian (**byte-read** from the single writer and its single caller, E-AF-TRACE-MECH-150). The word at `+8` is a code and param pair whose meaning follows the emitter family: for input publications the upper halfword is the input code and the lower halfword the parameter, zero in every capture examined (**byte-read**); for other sends its meaning is **unproved** beyond the filter role of section 2.5. A decoder therefore selects on the type byte, then on the thread byte and the `+4` signature, before reading `+8` (**static-inference**).

### 2.3 Tick source and rate

The tick word of a record is the 32-bit cell at `0x400c0800`, read at record-write time (**byte-read**). Loader A manages the cell in three places, mirrored by the second loader: an initialiser stores zero and registers an increment function as kernel timer callback id 32, the same code's other branch computes the timer block's counter register `0xFFF91000` divided by 1875 and stores the quotient in the cell, and a per-core check taken in the initialiser and again in the callback leaves one core to maintain it (**byte-read**, E-LOG-TICK-SOURCE-150). The code states the division in its own constants: `30000`, written to `0xFFF91004` in the same block, is `16 × 1875`. A debug read of the cell copies bytes and latches or clears nothing (**byte-read**).

Rate: 99.88 ticks per second, one tick 10.01 ms, measured against the host clock from three cell samples spanning 149.759 s in one session (**hardware-observed**). The cell is 32-bit and does not wrap inside a plausible session (**static-inference**).

The halfword at entry `+2` of `0x0f` and `0x72` records is `[0xFFF91004] - [0xFFF91000]`, a difference of two registers of the same timer block (**byte-read**). Its span over one fixed interval differs across boots where the tick span does not (**hardware-observed**), so it orders records within a boot only and the tick word is the comparable time.

### 2.4 Clearing behaviour

The store is per boot. The loader's initialiser writes the magic block, sets capacity `0x1ffc` and index 0 and clears all 8188 entry slots, and the loader's boot path calls it on both branches of its per-core test, so every boot that runs loader A clears the store; the second loader holds the same code at the same offsets (**static-inference**, E-LOG-STORE-150). **Hardware-observed:** captures of different boots share no non-zero slot, and every capture's store begins with the same boot-start pattern with its clock-derived fields freshly written (E-LOG-PERSIST-150). The bytes cannot separate a programmed clear from RAM failing to retain the store across a power cycle; the programmed clear is the operative fact either way.

Consequences for use: acts and read must share one boot, so nothing carries across boots and an ordinary boot's trace is unreadable after the next boot. Events before the clear are lost while held states published after it are present. At capacity the append overwrites the oldest entries, so a busy boot can lose its own early records before any read (**static-inference**).

### 2.5 The multi-source structure

This is the part an earlier reading missed. An earlier pass read the ring through one emitter's view and treated the type `0x0f` stream as an input-event log; it then classified shutter, POWER and entry-combo events as release-only because their press forms live under other threads. The correction is recorded in [the physical input map](input-map-150.md) section 2, whose title names it: the ring has several input sources (**byte-read**, **hardware-observed**). The structural facts a decoder needs:

- Type `0x0f` has one writer, and that writer's only caller is a hook the image-4 startup installs on the kernel's send path (**byte-read**, E-AF-TRACE-MECH-150). The stream is a trace of sends: every traced send is recorded whoever the sender, tasks that write no records of their own appear through their sends, and the record's task byte names the sender rather than a writer of the moment. The AF and lens tasks appear this way on this body (**hardware-observed**, E-AF-LENS-TRAFFIC-150), and which task and message ids they use is findings, not format.
- The same record type carries input publications, which are sends from the input tasks (**byte-read**). They are distinguished by thread byte and `+4` signature, not by type, and press and release forms are split across emitters: one press publishes under one thread while its release appears under another and its held state under a third, and mode changes publish twice under different threads. Absence from one emitter is not absence from the ring, and a state publication is not proof of a fresh physical edge (**static-inference**, input map section 2).
- The store is not a pure message log: `0x72` marker strings and `0x09` dispatch-failure records come from other writers with other layouts, and the `0x71` record's two payload words are uninterpreted (section 2.2).

Completeness is bounded by an allowlist. The hook looks each message id up in a 1716-entry sorted table at `0x81485c` and drops an id whose code falls below the threshold; the filter's mode byte, enable, threshold, write offset and trace buffer pointer live in readable RAM cells whose initialiser values are known (**byte-read**, E-AF-TRACE-MECH-150). A trace of any id family is complete only when that family's ids are admitted, which must be checked per firmware.

### 2.6 Limits

- The trace is allowlist-filtered, not a complete send log (section 2.5).
- A record proves a message was sent, not that the receiver's handler acted on it, and a task's internal state never appears (**static-inference**, E-AF-LENS-TRAFFIC-150).
- Per-boot clearing and capacity overwrite bound what any one capture can hold (section 2.4).
- The read is not atomic: `read-log` walks 512 windows over minutes while the ring keeps appending, so the header's next index can move during the read and a decode must expect the header index and the occupied slots to differ, with the acts preceding the read (**hardware-observed**, **static-inference**, input map section 1).
- Tick resolution is about 10 ms and the `+2` delta is not comparable across boots (section 2.3).
- Type `0x71`'s two payload words, the type `0x09` thread fields, the type `0x0f` `+8` pair for non-input sends, the `+0x24` first-record copy and the second store's contents are **unproved**.
- Ordinary-boot RAM access is hardware-observed, but an ordinary AF search and its useful trace with USB attached remain unproved. Maintenance-mode key behavior does not constrain this new route; a RAM-to-card recorder is no longer an access prerequisite.
- The console text ring that service opcode `0x200001` functions 1 and 4 drain is a different facility, an 8 KiB ring at `0x41acb800` installed at the connect edge; whether those drains reach this store is **unproved**, and the operation's own module comment records the same open question.

## 3. What a third party needs, and what is unavailable to them

| Need | State today |
| --- | --- |
| A read channel into the store's RAM on their body | The operation and its bytes are published, bound to X-M1 1.50's service surface; another body needs its own channel, same opcodes **unproved** |
| The store format | Published here (section 2); before this report it was split across published prose and private notes |
| Working decoders | Public Haskell/Bazel targets in [console-trace](../console-trace/BUILD.bazel); commands and input requirements below |
| Id and name bindings for their firmware | Public version-bound findings in the AF and input maps; another firmware must establish its own applicability |
| A boot whose trace can be read while AF runs | Ordinary-boot reads are demonstrated; simultaneous AF and useful same-boot search observations remain to establish |

## 4. Publication boundary

[Current principles](../docs/principles.md#publication-and-asset-boundaries) own the boundary, and [licensing](../LICENSING.md) owns redistribution obligations. Original software is AGPL-3.0-only; upstream components retain their own terms.

Publish original decoders, record layouts, task/message bindings, control-flow findings and methods, including exact X-M1 1.50 addresses and bytes needed to describe the protocol. Separate format parsing from version-specific annotation when generalizing a tool; both can remain public with scoped provenance.

Do not redistribute firmware binaries, extracted proprietary code/data, body dumps, raw logs, calibration, private identities or unrelated photographs. Review copied instruction sequences and captured fixtures individually for redistribution rights. Retain owner-supplied inputs privately and document the required identity, extraction and capture interfaces rather than bundling them.

Section 2 is the authoritative store-format description. Historical private mixed notes contain both technical derivations and raw/unit-specific data; publish their distilled findings, not the notes blindly. Derivation from firmware alone is not a privacy rule.

## 5. Limits of this report

The original format claims inherit the labels of the ledger rows and notes they cite (E-LOG-STORE-150, E-LOG-TICK-SOURCE-150, E-LOG-PERSIST-150, E-AF-TRACE-MECH-150, E-AF-LENS-TRAFFIC-150, E-PRESS-MAP-150, the dump note and the mapping note). The same-boot carry closure adds host-only byte and source inspection of the pinned writers; it adds no capture. Section 1.2's host-synthetic CLI checks cover the corrected decoder output only. Both decoders now decode type `0x71`'s tick at `+4`; its two payload words remain uninterpreted and no record type outside the four mapped ones contributes a tick. The decoder verdict is a content reading of the two files as they stand and would change if they change. Firmware addresses and geometry are X-M1 1.50 bindings; the tick rate is measured on one body. None is claimed for another firmware, another body or the X70, and the X70 author's trace-and-replay goal is **reported** context rather than an observed property of that project. Host-only throughout: no camera or USB device was touched.

## 6. The carry path a third party needs for an ordinary boot

This section retains the earlier stock card-file-client binding as **host-only, byte-read** research. Ordinary-boot RAM reads remove the former need for this carry on the X-M1; [recorder work #50](https://github.com/skeet70/latent-image/issues/50) is parked, non-installable and not an AF prerequisite. The full derivation, activation sites and placement arithmetic remain in [the instrument report](af-observe-instruments-150.md#96-the-stock-card-file-client-abi) sections 9.6 to 9.8. Another body must establish its own extraction route.

The card-file client in image 4, callback forms:

| Op | Call | Request id |
| --- | --- | --- |
| open | `0x0071c648(cb, path, mode)` | `0x2c17` |
| read | `0x0071c6fc(cb, handle, len, buf)` | `0x2c18` |
| write | `0x0071c75c(cb, handle, len, src)` | `0x2c19` |
| close | `0x0071c7bc(cb, handle, 0, 0)` | `0x2c1a` |

The open mode is an index 0..3 that the service maps to `r`, `w`, `a`, `s`. The open wrapper returns a handle tag from a sixteen-entry table (semaphore `0x3c`), and the media task (receiver id `0x1c`, dispatch table `0x814740`; a request posts first to the `mediamsg` task on id `0x65`, which forwards it) performs the operation and calls the request's callback in its own context with `(status, count)`. The write stores the bytes written by that call into the request before calling back, so a client can loop on the count. The request's payload is copied into the message; the pointers inside it and the source buffer must stay valid until completion, which the store satisfies because it lives for the boot.

A smallest carry over that client, and the one emitted for this body: a hook on the media loop's receive call at `0x71feb4` scans the store's slots for the AF result record's exact signature (`{receiver 3, sender 0x2b, id 0x13a5}` in the record's `+4` word) and, on the first hit, runs the stock open/write/close chain once: open `C:\DCAA\AFTRC.BIN` mode 1 through `0x71c648`, the stock card-file writer's entry (`C` is the registered SD volume letter; a literal `A:` fails the qualified-path check), write the whole store (`0x400a0800`, `0x1fff4` bytes), close, and close again on a write failure so the handle tag is released. All of it runs in the media task's context, no allocation and no snapshot are needed, and the result is a live, non-atomic dump of the store, labelled as such, in the same class as the non-atomic `read-log` of section 2.6. The blob is 324 bytes at `0x61c1fc`, filling the free RC0 window exactly, with one retargeted `bl` word; the payload, the candidate manifest and the reusable container commands are in [the instrument report](af-observe-instruments-150.md#97-the-smallest-ordinary-boot-carry-path-over-this-abi) sections 9.7 and 9.8.

What a third party re-derives for their own body: the request ids and handler table, the mode mapping, the handle table and its tag-to-index map, the callback contract, and a store-side trigger or wake that their own boot can reach without touching the AF path's waits. The emitted bytes are a candidate, not an installed artifact: a verified non-installable container now exists for this body (instrument report section 9.8), and it needs independent review and the owner's go.
