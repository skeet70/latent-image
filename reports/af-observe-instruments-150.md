# What carries autofocus state off the 1.50 body

Status: host-only analysis of the pinned 1.50 artifacts and of captures already on disk. No camera operation, no new byte, no authorization. Labels: **byte-read** for bytes in the pinned images or in a capture, **static-inference** for conclusions drawn from them, **hardware-observed** for a body observation already recorded, **unproved** where the named fact is unavailable.

This answers the instrument half of AF-OBSERVE-01: which of the instruments built since [the observation plan](af-observation-plan.md) was written can carry autofocus state off the body, what each would show, what the store trace holds about AF, and which question the first bench session should ask. The session itself is [the first-session script](af-first-session-script.md).

## 1. The AF lane's obstacle, restated

The static map binds the AF request path, the completion graph and the confirmation path in code, and [the observation plan](af-observation-plan.md) says what camera access reaches: visible indications, quality, cancellation and external intervals. Every internal event, the plan says, needs G4/G5 hooks.

The newer instruments change the *read* side only. None of them makes an ordinary shooting boot readable, because the vendor service session exists only in maintenance mode `0xb` and the log store is cleared at every boot. What they do give is an in-body read of a *maintenance* boot: its message trace with body ticks, its RAM state, and its adjustment area. The AF lane's question is therefore whether a maintenance boot carries the AF layer at all, which is what the first session tests.

## 2. The five instruments

| Instrument | What reaches the host | What it would show for AF | Bound today |
| --- | --- | --- | --- |
| Store trace, `read-log` | The whole 0x1fff4-byte log store at `0x400a0800`, 515 windows | The boot's message trace: every traced inter-task send with sender, receiver, message id and a tick, including the AF task's own sends | Reviewed operation; read works, but only in a maintenance boot |
| Debug RAM read, `probe-ram`, `dump-ram` | Named RAM windows or one of 21 fixed segments, over `0x200001` function 2 | The AF task's state cells, the AF request context and slot record, the lens position, the AF-frame and UI cells, the trace facility's filter bytes | Reviewed operation; reaches every listed segment |
| Config read, `read-config` | 256-byte windows of the adjustment area at `0x41acd800` plus the flash-log settings area | The stored FOCUS MODE and AF-area properties, and the three AF debug gate bytes at `0x887d`, `0x887e` and `0xe8` | Reviewed operation; it does not read arbitrary RAM, so the AF state cells are outside it |
| Service opcodes | `0x200001` functions 1 to 5 | Function 2 is the RAM read above; function 5 is the text/debug key used by `end-setting`. Functions 1 and 4 drain the console *text* ring at `*(0x409c1e2c)` and no built operation sends them; function 3 writes RAM byte by byte and is not a closed operation | Function 2 and 5 bound; 1, 3 and 4 not built |
| Imaging-job family | Whitelist values `0xa0`-`0xac` as a host config write, then `0x450006` ids `0x12`/`0x263` | The only mapped way to make a readable boot run recorder and capture machinery, and so the only candidate that could put an AF search into the trace | Not a closed operation; the exposure link inside mode `0xb` is unproved ([E-SERVICE-CAPTURE-PATH-150](../docs/evidence.jsonl)) |

- The store trace is the only instrument that carries *when* something happened in body time. A search driven inside a readable boot would appear as its message sequence with ticks, which is the §11.1 timeline at the message level, without hooks.
- The RAM read is the only instrument that carries AF *state*: the orchestration state, the search state, the in-flight and status fields, the accepted statistic, the request context and the lens position. It shows state, not time.
- The config read carries what the body *stored*, including the debug gates that decide whether the firmware's own AF printer can ever run. It cannot see live AF state.
- The service opcodes are the transport for both reads. Their text-ring functions would carry the firmware's own AF debug lines, and they are unreachable, see section 4.
- The imaging-job family is the only write-side candidate. Without it, nothing inside a readable boot asks the AF layer to do anything.

## 3. What the store carries about AF

### 3.1 Records

The store names itself `syslog Ver 2.3`, capacity 8188, and holds this boot's records from slot 0; loader A clears it in every boot (E-LOG-STORE-150, byte-read; the two jig captures share no non-zero slot, E-LOG-PERSIST-150, hardware-observed). Entry layout is 16 bytes: type at +0, the writing task's id at +1 (the `TPIDRURW` register, byte-read in the writer), a 16-bit counter delta at +2, then per type: 0x0f `{trace word +4, code<<16|param +8, tick +0xc}`, 0x72 `{tick +4, eight string bytes +8}`, 0x09 and 0x71 other fields.

Types seen in captures: 0x0f (the message and event trace), 0x09 (dispatch failure), 0x72 (eight-character boot and pipeline markers), 0x71 (ten records, unmapped).

### 3.2 The trace word

For type 0x0f the word at +4 is `{receiver u8, sender u8, message id u16}`, little-endian, and the record's task byte equals its sender. **Byte-read:** the writer `0x41c8c` in loader A is the only writer of type 0x0f, and its only caller is image 4's `0x00726478`, which the image-4 startup `0x0069d160` installs as a hook through `0x00724cbc` (that function is the one-instruction store `_DAT_4089ea94 = param_1`). So the trace covers every send the hook sees, not only UI events. Decoding the captured records gives triples such as saclac to lensdc `0x4200`-`0x4214`, lensdc to saclac `0x3b2f`/`0x3b30`/`0x3b32`, saclac to reccont `0x1394`-`0x139d`, and keytimer to usbcont `0x0000` for mode detents, which agrees with the input map's reading of the same records as emitter signatures.

### 3.3 Does the AF task publish to it

The AF orchestration task (`0x2b`) writes no record of its own: no function in image 5's AF range calls loader A's writers `0x41c8c`/`0x41a34`/`0x42188`, it owns no type 0x72 string, and no fixed-string 0x72 call site names an AF marker (the fixed sites are the loader, media, format, image-stability and image-pipeline markers; three sites pass computed pointers, so an unfixed string is not excluded). The lens side does publish strings: the helper `0x006c633c` calls the string writer with `IBW_S`/`IBWAIT`/`IBW_E`, and the capture below shows those records under the lensdc task byte. The AF task's *sends* are traced anyway, because the hook sits on the kernel's send path:

**Byte-read, hardware-observed** in the owner's act-logged boot `private-data/sessions/20261009T064945Z-read-log`, type 0x0f records by task: `0x2b` saclac 114, `0x32` lensdc 87, `0x62` lenscom 12, `0x63` lswatch 36, `0x64` lensdata 14, `0x65` 23, alongside `0x25` keytimer 60 and the rest of the system's traffic. Those 286 AF and lens records fall in ticks 23 to 1099, the boot's first eleven seconds (saclac 61 to 951, lensdc 62 to 951, lenscom 61 to 951, lswatch 32 to 951, lensdata 57 to 967, task `0x65` 23 to 1099), and their ids are the boot handshake: the lens API family `0x4200`-`0x4214`, the bridge family `0x1394`-`0x139d`/`0x13bc`/`0x13c4`-`0x13c8`, the `0x3f13`-`0x3f27` group to task `0x2f`, and lensdc's `0x720d`-`0x722b` to lenscom. The search start `0x3f2e`, the request `0x3b0d` and the result `0x13a5` are absent, so no search ran in that boot.

### 3.4 Filtering

**Byte-read:** `0x00726478` looks the id up in the 1716-entry sorted table at `0x81485c`; ids absent from the table get code 0 and are dropped, and `0x007262cc` initialises the filter enable `0x40a5b5e1` to 1, the threshold `0x40a5b5e2` to 1 and the trace buffer pointer `0x40a5b5ec` to `0x41ac9800`. The AF ids are in the table with code 1: `0x1394`, `0x13a5`, `0x1a0d`, `0x3b0d`, `0x3b32`, `0x3f0d`, `0x3f2e`, `0x420c` were each checked. A search's own messages are therefore inside the trace's allowlist, and the filter bytes are readable in a session if the allowlist's state ever needs checking.

### 3.5 Tick resolution

**Byte-read:** the tick at +0xc is the 32-bit cell `0x400c0800`, which is `[0xFFF91000]` divided by 1875 (E-LOG-TICK-SOURCE-150); the second field is a 16-bit delta of a different counter, not a wrap of the tick. **Hardware-observed:** `read-log` samples the cell at three points in one session, and the 2026-10-09 session's samples span 14,958 ticks over 149.759 s, so one tick is 10.01 ms and the rate is 99.88 ticks/s. The 32-bit cell does not wrap inside a plausible session. The resolution is thus about 10 ms for any sequence read back from the store, which is finer than an external camera at 60 fps and coarser than the CPU.

## 4. Where the wall is

- The store is cleared by loader A at every boot and has been read only in maintenance boots.
- In maintenance mode `0xb` the keys take no shot, no menu is dispatched and no preview starts (E-SERVICE-SHOOT-150, byte-read), so no AF request and no search happen there.
- Therefore no reviewed operation today can carry an AF *search* off the body: the boots whose trace can be read are boots in which AF does not run.

The firmware's own AF debug printer is a second candidate for AF internals and it is inert here. **Byte-read:** `0x00f26d78` formats a line naming the search state (`DIRTION`, `PEK_SRH`, `PEAK`, `FOCUS`, `FCS_DRI`, `WOB`), the signed position from `0x016b66e8` and the focus value from `0x016b66e0`, and calls `0x0064744c`, which writes only when `cache[0xf7] == 1` and the text ring at `0x41acb800` is installed. The printer's own gates are `cache[0x887d] != 0`, with `cache[0x887e]` as its counter bound, and `cache[0xe8]` bit 7 deciding the emit; its state argument comes from the setter `0x00f26ff0`, which stores its argument to `0x016b66f4` and calls the printer with 1, and from `0x00f26fbc`, which calls it with 0. **Hardware-observed in the 2026-10-08 config capture:** the body stores `0x887d` = `0x00`, `0x887e` = `0x0a` and `0xe8` = `0x00`, so the printer is off in the stored configuration; and `0x0089edd4`, the flag set when the connect edge installs the text ring, reads 0 in the captured jig boots, so the ring was never installed there. Reaching the printer would need a committed gate byte (the persistent `f7` route the standing rules forbid) and an AF run in a jig boot, which the mode does not provide.

## 5. What the newer work settles in the observation plan

- The plan's G4/G5 table lists internal bindings as hook-only. The store trace and the RAM read make a *maintenance* boot readable at the state and message level, so part of that can be observed without hooks, in that boot only.
- The plan's camera-access rows stand unchanged. Nothing here dates an internal event of an ordinary session, so the request-proxy-to-visible-indication latency, the quality sweep and the cancellation sequence keep their external form, and the AF debug text route does not add an internal endpoint.
- The plan's "any service command needs a reviewed byte plan" no longer gates the first step: the store read, the RAM read and the config read are reviewed operations, so the first session needs no new bytes.
- The plan's settling-versus-confirmation block needs a validated continuous measure; unchanged.

## 6. The first question

**Does a maintenance boot carry the autofocus layer at all: do the AF task's own state cells and message trace read back in one jig boot, and does the physical shutter press move either?**

Four properties put it first.

- It gates the only internal instrument that needs no hook. Every later AF design that wants in-body timing or in-body state in a readable boot depends on this answer.
- It costs one jig entry, uses four reviewed read operations and a few key taps, and writes nothing beyond the gate byte every read operation already writes and restores.
- Both answers are actionable. Positive: the AF-layer cells become the project's idle baseline, and the next step is a reviewed operation that drives one recorder job inside a readable boot, the only mapped agency, with those cells as the predeclared control. Negative: the jig-session route is closed for AF and the lane keeps to the external kit and G4/G5.
- The peripheral parts are already answered, so the session tests the remaining unknown. The AF and lens tasks do emit traced sends in a jig boot (section 3.3), the lens position cell `0x00942b84` reads `0x36`, `0x20` and `0` in three jig captures, and the text-ring install flag reads 0.

This question is first in the lane's order; [the plan](af-observation-plan.md) states the second and third questions and answers the trace-versus-search tension this survey exposes.

## 7. What the first session would settle

| Read | Addresses | Expected result | What the other outcome means |
| --- | --- | --- | --- |
| AF state machine | `0x0169d4b4` state vector, `0x0169d468` event vector, `0x0169d4e2` orchestration state | Nonzero, defined values: the AF task brought its state machine up in the maintenance boot | All zero or the loader's fill: the AF layer is not running despite the task existing |
| Search and evaluation fields | `0x016c0fbc`/`0x016c0fc0`, `0x016c0fe2`/`0x016c0fe4`/`0x016c0fe5`, `0x016c11fc`, `0x016c1160`-`0x016c1178`, `0x016c1f10`/`0x016c1f14`/`0x016c1f74`, `0x016c2200`/`0x016c2204`/`0x016c2208`, `0x016c21e4`/`0x016c21e6` | Idle values that repeat across a boot: the control a later drive session is read against | Movement between the two reads with no act directed at AF, which would itself be the finding |
| AF request context | `0x01529fb8`, `0x01529fbc`, the slot base `0x015b73b8` at stride `0x17d0` | A live slot index or pointer binds the AF-area-to-request derivation, an open item of the static map, to live RAM | Zero or the `-1` sentinel: the context is not bound before a request, which is what the static map's "target selected" point needs to be read against |
| Lens layer | `0x00942b84` position, `0x00942b88`, the protocol flags `0x0095b394`/`0x0095b398` | A plausible position with the flags set: the lens layer is live in mode `0xb` | The `-0x8000` query-invalid sentinel: the lens is not queried in a maintenance boot, and no jig-session instrument can move it |
| AF-frame and UI cells | `0x00950544` queue, `0x00951424`/`0x00951428`/`0x0095142c`, `0x0094f64c`-`0x0094f65c`, `0x0094f888`-`0x0094f894`, `0x0094f650`/`0x0094f654` | Zero, matching the 2026-10-08 dumps; any nonzero value after the shutter probe is new evidence about S1 in maintenance mode | Zero both times confirms the presentation path is untouched without a shooting mode |
| Text ring | `0x0089edd4` install flag, `0x0089edd8` callback slot | Zero, as in the captured boots: the AF debug text route stays inert under the current procedure | Nonzero would show the facility arming in-session and would reopen the text route |
| Trace facility | `0x40a5b5e1` enable, `0x40a5b5e2` threshold, `0x40a5b5e4` offset, `0x40a5b5ec` pointer | The initialiser's values (1, 1, 8, `0x41ac9800`): the trace is filtered by the allowlist and the buffer is where the code says | Different values would bound how complete the trace is, which matters before any AF timeline is read from it |
| Store trace | the whole store | The AF and lens send set up to the read, with ticks, and the acts' records for tick alignment | Any `0x3f2e`, `0x3b0d` or `0x13a5` record after the shutter acts would show the press reaching the AF layer in maintenance mode, which the mode's key dispatch says it should not |

## 8. Reproduction

The census that produced section 3.3 and the triples (host-only, reads session files):

```sh
devenv shell -- runghc private-data/analysis/x-m1-1.50/af-observe/TraceCensus.hs \
  private-data/sessions/20261009T064945Z-read-log 2b 32 62 63 64 65
```

The trace allowlist and the trace hook call sites (read-only over the pinned analysis):

```sh
xxd -s $((0x81485c-0x61c400)) -l 64 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
awk '/^\/\/ =====/ {fn=$3} /FUN_00041c8c\(/ {print fn}' private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c | sort -u
awk 'NR>=67091 && NR<=67135' private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c
```

The AF debug gates in the stored configuration (bytes read from the 2026-10-08 session):

```sh
xxd -s 0x7d -l 4 private-data/sessions/20261008T175358Z-read-config/windows/008800.bin
xxd -s 0xe8 -l 2 private-data/sessions/20261008T175358Z-read-config/windows/000000.bin
```

The lens position and the text-ring flag in the existing jig dumps:

```sh
xxd -s $((0x00942b84-0x0089ec00)) -l 4 private-data/sessions/20261008T181841Z-dump-ram-id4-bss-0/id4-bss-0.bin
xxd -s $((0x0089edd4-0x0089ec00)) -l 8 private-data/sessions/20261008T181841Z-dump-ram-id4-bss-0/id4-bss-0.bin
```

## 9. Same-boot carry closure over the pinned bytes

This is **source-inspected** and **static-inference**, host-only. It adds no camera observation and no executable recorder. The owner approved building a same-boot recorder, not installing one. The examined stock routines establish a card-file route and real allocation APIs, but do not yet establish a bounded recorder activation through those APIs. A live, non-atomic RAM-to-card capture remains a viable design direction; the coherent-timeline residual below is not a reason by itself to reject it.

### 9.1 Inputs and ownership

All addresses below are X-M1 1.50 bindings. The decoded input identities are:

| Input | SHA-256 |
| --- | --- |
| Loader A, `decode/loader-40000.bin` | `21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882` |
| Loader B, `decode/loader-d0000.bin` | `8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec` |
| Image 4, `lz/id4-0061c400.bin` | `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16` |
| Image 5, `lz/id5-00bf1a00.bin` | `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a` |

Paths in that table are relative to `private-data/analysis/x-m1-1.50/`. The per-function decompilations under its `af/functions/` directory supply the caller chains; the loader disassembly and the byte reads below distinguish the control words and writer order.

The installed RC0 allocation occupies `[0x0061c340, 0x0061c400)` in the owned 516-byte prefix. The remaining `[0x0061c1fc, 0x0061c340)` is 324 bytes, shared by any recorder code, path, state and literals, not 324 bytes plus unowned scratch RAM ([prefix ownership](filmsim-prefix-widening.md)). No complete recorder has been sized against that budget. A zero-filled region, the stock diagnostic scratch at `0x01677840`, and the shared trace store are not new recorder-owned allocations. No RC0b space or installed bytes are changed by this analysis.

### 9.2 Writer order, controls and coherence

Loader A's message writer `0x00041c8c` masks its executing core's IRQs through `0x00041140`, checks the word `0x4004f25c`, and, if it is 1, writes at the header's current next index. It writes the type first, then the remaining fields and tick, calls the registered callback through `0x00041568`, advances/wraps the header next index, clears all sixteen bytes of the new next slot, and restores its local IRQ state through `0x00041164`. The callback therefore runs before advancement, with local IRQs masked. It is not a safe place for heap or file operations that can wait.

Loader B's matching writer at `0x000d1c8c` uses the same entry base `0x400a0800` and tick cell `0x400c0800`, but checks its separate enable word `0x400df21c` and uses its own IRQ helpers. A's setter `0x000423f4` writes `(arg == 1)` to `0x4004f25c`; B's `0x000d23f4` writes the same predicate to `0x400df21c`. Neither setter masks or drains the peer. Neither writer acquires the shared lock at `0x400c0804`. A writer which passed its gate can continue after another core stores zero to that gate. These bytes prove the distinct controls and the absence of a peer acknowledgment in these routines, not that every possible peer writer ran in a captured ordinary boot.

The image-4 critical pair `0x007235d8` / `0x007236b0` masks local IRQs and uses that shared lock, but its protection does not extend to the loader writers. The send hook `0x00726478` calls `0x00041c8c` before taking the shared lock for its separate payload buffer. Its `0x40a5b5e1` flag controls filtering: zero bypasses the allowlist check, rather than disabling trace writes. Its threshold at `0x40a5b5e2` filters send records, not the direct loader marker/failure writers. Suppressing this hook alone is not a whole-store freeze.

The stock snapshot helper `0x000423d4` copies `0x1fff4` bytes through `0x00041320`, a byte loop, with no writer lock. This matches the existing read instrument's live, non-atomic character. Equal next-index values before and after copying are not a consistency witness: an in-progress record precedes advancement, and wrap can revisit the same index. The type byte is not a commit marker.

The new-next clearing means 8188 physical slots retain at most 8187 records once full. Before wrap, the populated prefix ends immediately before the next index. After wrap, chronological traversal starts after the cleared next slot and ends immediately before it. The header has no observed generation or wrap counter, so a decoder cannot infer the lifetime record count from that index alone. The header's `+0x24` record is the first type-`0x09` failure latched by `0x00041a34` while its type byte is zero, not a permanent mirror of slot 0.

Two capture claims must remain separate. A raw carry can explicitly describe itself as live/non-atomic, carry the exact requested and completed byte lengths and its read interval, and make no coherent ordering claim for overlapping appends. A coherent timeline additionally needs a proven writer-quiescence or commit protocol. This pass has not established the latter; it does not impose it on raw capture.

### 9.3 What the stock card exporter actually writes

The mujin `WT_LOG` handler `0x00ed28b4` calls `0x00ed1850(0xfff)`. That exporter does not export `0x400a0800`. It requests `0x101f` bytes from `*(0x40a5b5ec) - 0x20`; initialization at `0x007262cc` makes that source `0x41ac97e0`. The separate send-payload buffer begins at `0x41ac9800` and has a `0x2000`-byte circular body.

The observed exporter uses the following asynchronous wrappers:

| Operation | Wrapper | Forwarded routine | Relevant argument registers |
| --- | --- | --- | --- |
| Open | `0x0071c648` | `0x0071c2b0` | `r0` callback, `r1` path, `r2` mode |
| Write | `0x0071c75c` | `0x0071bfe0` | `r0` callback, `r1` handle, `r2` length, `r3` source |
| Close | `0x0071c7bc` | `0x0071c134` | `r0` callback, `r1` handle, subsequent arguments as used by the caller |

The open callback `0x00ed1828` stores status and handle; write callback `0x00ed17b0` stores status and completed count; close callback `0x00ed1800` stores status and clears the handle. The stock `WT_LOG` executor proceeds through open/write/close without testing those stored statuses or the write count. It is not a success/failure contract to copy. Its `0x00ed1704` helper waits while the in-flight word is 1 with no deadline. Its filename builder `0x0070fbfc` also advances stock DCF bookkeeping, so borrowing it would have a photography-visible effect beyond trace export.

At the request boundary, `0x0071bfe0` retains the source pointer in the queued write request; it does not snapshot the caller's bytes. `0x0071fde4` sends the request to task `0x65`; `0x007211d4` forwards to task `0x1c`, with `0x00721578` and the handle-tracking helpers mediating requests. A heap snapshot must therefore outlive actual write completion, not just the wrapper call. A live-source design can retain the stock ring's boot lifetime and avoid that allocation, while still labeling concurrent mutation. Neither design may treat a queued request or an existing pathname as proof of a completed file.

Even the asynchronous open wrapper is not a proven bounded call: `0x0071c2b0` calls handle allocator `0x0071ef18`, whose first operation is `0x00724b68(0x3c, 0xffffffff)`. The asynchronous wrapper does not replace this semaphore wait with a poll or deadline. The allocator returns zero on wait failure or when its sixteen handle slots are occupied. This is the earliest byte-grounded bounded-I/O gap for a proposed callback-driven, live-ring carry. Calling it from the IRQ-masked trace callback is unsafe; merely changing the exporter to asynchronous callbacks does not close it. A task-only route needs a proven acceptable lock/liveness contract or a separately bounded handle-allocation path, plus completion and cleanup behavior. No such new path is emitted here.

### 9.4 Allocation and ordinary activation

Real stock heap allocation is available: graphics' callback table binds allocator `0x00890ef8` to `0x0089cda4` and free `0x00890ee8` to `0x0089cdc4`. Those wrappers forward through the reentrancy pointer at `0x0089c970` to allocator `0x0089cde4` and free `0x0089e560`. Overflow and exhaustion return null; successful allocation returns the chunk payload after unlocking. Free accepts null. Backing growth `0x0072b110` refuses growth past `0x00bf1a00` with its `0x20`-byte margin. These boundaries establish conditional allocation ownership, not that a particular boot has `0x1fff4` bytes available.

Heap lock `0x0072b164` requires a nonzero task id, waits on semaphore `0x3f` with `0xffffffff` when another task owns it, and hard-loops on invalid task context or wait failure. Its paired unlock is `0x0072b1cc`. An allocator-owned snapshot is preferable to claiming a free static RAM gap, but this API cannot be called in the trace callback or described as bounded under arbitrary contention. Allocation before disabling tracing would avoid holding trace controls across heap or file waits; it would not by itself make a later copy coherent.

The stock mujin entry `0x00ed2ce0` reaches `0x00ed2c90`, which delays, reads its boot gate through `0x00ed2b78`, and only invokes the script start `0x00ed2c34` when that value is 2. The dormant script route does not establish ordinary-photography activation for a recorder. The ordinary AF success callback `0x00defa30` sends `0x13a5` with a sixteen-byte result through `0x00dcb950`; its failure branch calls `0x00dcd010`. That is a byte-grounded event candidate, not a selected hook: its caller context, preserved stock result delivery, reentrancy and complete file-request ownership would need closure before adding recorder work there. Recording only that success branch would not capture an AF refusal or cancellation by construction.

The tempting diagnostic route is also unsuitable. `0x00ed485c` and `0x00ed468c` disable A's trace gate and use the table at `0x00ed4b68`, whose first row requests a `0x20000`-byte diagnostic block using snapshot `0x000423d4`. The snapshot itself supplies only `0x1fff4` bytes. Their erase helper `0x006ab854` and program helper `0x006ab794` address internal raw-media blocks, not an SD file. Invoking this route would consume the diagnostic once flag and perform persistent internal writes. Neither its scratch allocation nor its diagnostic activation is adopted.

### 9.5 Closure and remaining prerequisite

Reachable closure consists of the two loader gate bindings and writer ordering, the raw-versus-coherent capture distinction, the real heap ownership/failure boundary, the existing card open/write/close ABI and callbacks, the dormant mujin gate and ordinary AF result candidate, the rejection of internal diagnostic writes, and the remaining 324-byte RC0 prefix budget. There is no new hook, patch manifest, payload, container, camera command, install plan or success claim.

The next design review must close a task-context, bounded file-handle acquisition and callback/cleanup sequence, choose ordinary-boot activation that preserves all stock AF outcomes, and size the complete payload and retained state in proven owned storage. If it chooses a live raw carry, peer quiescence is a documented limitation rather than its blocker. If it claims a coherent timeline, the A/B acknowledgment gap also remains. Stock semaphore liveness, callback execution context, cancellation and media-consumer buffer release have not been proved by this pass. The current native harness's user-mode ARM boundary does not exercise peer-core scheduling, IRQ masking or the relevant kernel synchronization, so mocked success there would not settle them.

Sections 9.6 to 9.8 carry that closure forward: 9.6 binds the callback-form file ABI and its completion and error semantics, 9.7 places handle acquisition in the media task and names the two activation sites with their current words, and 9.8 states the placement arithmetic, the candidate conflicts and the items that remain unproven.

Host-only byte-read reproduction, without a build or camera access:

```sh
sha256sum private-data/analysis/x-m1-1.50/decode/loader-40000.bin private-data/analysis/x-m1-1.50/decode/loader-d0000.bin private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -g 4 -s 0x1c8c -l 260 private-data/analysis/x-m1-1.50/decode/loader-40000.bin
xxd -g 4 -s 0x1c8c -l 260 private-data/analysis/x-m1-1.50/decode/loader-d0000.bin
xxd -g 4 -s 0x23f4 -l 40 private-data/analysis/x-m1-1.50/decode/loader-d0000.bin
xxd -g 4 -s 0x2dfe50 -l 216 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -g 4 -s 0x100248 -l 40 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -g 4 -s 0x10035c -l 36 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -g 4 -s 0x2809a4 -l 80 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
```

Those commands inspect firmware already held under `private-data/`; they do not emit a recorder. No build, test, formatter or native smoke ran in this closure pass.

### 9.6 The stock card-file client ABI

This subsection closes the file-API part of sections 9.3 and 9.5. It is **byte-read** from the pinned images and from the extracted function set under `af/functions/`, host-only. All addresses are X-M1 1.50 bindings.

The wrappers live in image 4. Each builds a 12-byte record `{u16 id, u8 source, u8 destination, u32 length, u32 payload}` and posts it through `0x0071fde4` -> `0x0072503c(0x65, record)`. The source byte is the calling task from `0x00723cb8`, the destination byte is `0x65`, and the kernel copies the payload into the receiver's message with `0x00722054` before queueing, so the caller's frame does not need to outlive the call; only the pointers inside the payload do.

| Op | Callback form | No-callback form | Client impl | Request id | No-callback id |
| --- | --- | --- | --- | --- | --- |
| open | `0x0071c648(cb, path, mode)` | `0x0071c670(path, mode)` | `0x0071c2b0` | `0x2c17` | `0x2c2d` |
| read | `0x0071c6fc(cb, handle, len, buf)` | `0x0071c720(handle, len, buf)` | `0x0071be74` | `0x2c18` | `0x2c2e` |
| write | `0x0071c75c(cb, handle, len, src)` | `0x0071c780(handle, len, src)` | `0x0071bfe0` | `0x2c19` | `0x2c2f` |
| close | `0x0071c7bc(cb, handle, 0, 0)` | `0x0071c7e0(handle, p3, p4)` | `0x0071c134` | `0x2c1a` | `0x2c30` |

The open `mode` is an index 0..3. The media-side worker `0x0071f2a8` switches on it at `0x0071f380` (payload word `0xe`) and calls the file-system open `0x00701634` with the mode characters `r`, `w`, `a`, `s` for 0, 1, 2, 3. `WT_LOG` opens with mode 1, and its suffix template at `0x00ed3ab0` reads `.msg`.

Handle. `0x0071ef18` allocates a tag from a sixteen-entry table at `0x9101c0` under semaphore `0x3c` (`0x00724b68(0x3c, 0xffffffff)`), and the open wrapper returns the tag in `r0`. `0x0071eb28` maps a tag to the service's file index through the sixteen-entry table at `0x910140`. The release routine is `0x0071eff4` (returns immediately on a zero tag, then takes semaphore `0x3c` and clears the tag); the close worker `0x0071f1d4` loads the tag from the request's completion cell, calls `0x0071eff4`, then `0x0071eb28` and `0x0071eaac`, the read and write helpers `0x0071f078` and `0x0071f134` release the same way, and the open failure path calls it at `0x71f3e0`. Sixteen is therefore the simultaneous allocation limit, not a per-boot limit, a closed file releases its tag, and every error path that acquired a tag must close it; the emitted recorder closes on a write failure for that reason. The client-side helper `0x0071bc2c` (semaphore `0x3a`, current task id into `0x910134`) belongs to the no-callback forms, which refuse to run in task `0x1c` or task 0 and report status 9 (guard at the head of `0x0071c2b0` and inside `0x0071bfe0`).

Open entry points. `0x0071c648` is the stock card-file writer's callback open (WT_LOG `0x00ed1850`, the auto-act loader `0x00ed1de0` and the S-record loader all call it): its fourth argument is 0 and its fifth argument is 1. The fourth argument is the client-side bookkeeping selector (nonzero resolves the volume context through `0x0071bc2c` and appends the `{1, slot, slot+4}` triple), and the guard refuses any call whose fourth argument is nonzero when the current task is `0x1c` or 0, which is what the no-callback twins `0x0071c670` and `0x0071c6c8` pass; a service-task caller must use a callback form. The fifth argument is stored at payload `+0x4c` and reaches the file-system open `0x00701634` as its fourth argument through the open worker `0x0071f2a8`; every observed caller of the callback form passes 1, and the twins `0x0071c6a4` and `0x0071c6c8` have no call site in the extracted set. The path check at `0x71c388`-`0x71c3ec` requires `path[1] == ':'` and compares `0x0070ec84(letter)` against the current volume index at `*(0x910204 + 0x114)`; a letter with no registered volume returns status 5. The volume registry words at `0x814730` and `0x814738` are `0x00000143` and `0x00010142`, registering `C` for volume 0 and `B` for volume 1, so `A:` is not a usable drive on this body. The auto-act loader resolves the letter at run time: it calls `0x0071f708` and stores the result over its template's leading letter, so a literal letter in a path is a per-body choice; the emitted recorder hard-codes `C`, this body's volume-0 letter, and another body should resolve through `0x0071f708`. Both stock call sites also fix the wrapper argument order as length before buffer: the loader's read passes the length in `r2` and the buffer in `r3`, and the `WT_LOG` write passes the length in `r2` and the source pointer in `r3` (its length argument is `param_1 + 0x20`, its source `*(0x40a5b5ec) - 0x20`).

Service side. Image-4 init `0x0071fd40` attaches id `0x1c` and creates the media task (entry `0x0071fe7c`, stack `0xc00`, priority 10). Its loop receives on id `0x1c` with an infinite timeout at `0x0071feb4` (`bl 0x0071fe2c`, which calls `0x00725318(0x1c, buf, -1)`) and dispatches from the eight-byte record table at `0x814740`:

| id | handler | role |
| --- | --- | --- |
| `0x2c17` | `0x007115a4` | open |
| `0x2c18` | `0x0071edbc` | read |
| `0x2c19` | `0x007116e4` | write |
| `0x2c1a` | `0x00711768` | close |

The wrappers post to receiver id 101, and the forwarding routine `0x007211d4` rewrites the destination byte to `0x1c` and re-posts through `0x0072503c(0x1c, ...)`; id 101 is the `mediamsg` task created at `0x00721834` (name `0x782008`, entry `0x00721744`, priority 2, stack `0xc00`, queue attached as `0x00724cdc(0x65, {0x30, 0x58})`), whose loop receives on id `0x65` into `0x91021c` and forwards through `0x00721578` / `0x007211d4`, so the client-to-handler chain is named end to end. The two stock clients of this family on the pinned images are the script loader (open `A:\DCAA\auto_act.scr`, read through `0x0071c6fc`) and the `WT_LOG` exporter `0x00ed1850` (open mode 1, write, close).

Completion. The media task calls the request's callback itself, in its own context, with `(status, count)`. `0x0071ed34` stores the bytes written by that call into the request's length word before calling back (`puVar2[2] = uVar1`), and the script loader accepts a read only when the count equals the requested size. Status values pass through the mapper `0x0071d450`. The no-callback forms append a `{1, slot, slot+4}` triple; the handler stores status and count through those pointers and releases semaphore `0x3b` (`0x0071c634`). A callback-form client needs no semaphore; the stock exporter waits by sleeping two milliseconds while its own in-flight word is 1 and its callbacks clear that word (`0x00ed1704` / `0x00ed178c`). Open completions take one more hop: `0x0072129c` stashes the client callback at `0x91028c`, replaces the request's callback word with the relay `0x007217f0`, and the relay stores status at `0x910280` and the handle-side value at `0x910284`, calls the stashed callback, clears `0x91028c` and calls `0x724144(101)`. Those are single-slot cells, so a callback-form open is single-flight; the recorder's one open per boot stays inside that bound, and any client that opens concurrently would overwrite the stash.

Write route. `0x007116e4` -> `0x0071ed34` -> `0x006ff000(file index, src, len, &status)` -> `0x00703eb4` (file context), mode check (`'r'` gives `0x24`), `0x0070600c` (prepare), `0x007069b0` (cluster write, returns the count), `0x00703f28` (commit, failure gives `0xd`). The handler reads the caller's source buffer at request time; nothing snapshots it. A live ring carry therefore needs no allocation and no copy, and the ring's boot lifetime covers the request.

Ready gate. `0x0071ddf0` returns 0 when a volume is mounted, 1 after logging `0x50`, 5 after `0x53` or `0x55`, and 2 after `0x52`; the wrappers deliver such a status synchronously through the callback, with no service round trip. The sixteen-byte media state copy at `0x007111c4` (source `0x8fc8a4`) drives the ready branches in the wrappers and handlers.

Read route note. `0x0071edbc` streams through the unit-addressed primitive `0x006aa4ec` / `0x006aa52c` (three-entry descriptor table at `0x7c8f78`, 128 KiB units, DMA bit `0x40000000` in `0x006a8f1c`), not through `0x006ff000`. The recorder does not use the read path; the ids, payloads and handler addresses above are byte-read, and the substrate of that primitive is not established here.

### 9.7 The smallest ordinary-boot carry path over this ABI

Shape: one trigger at the ordinary AF result, one worker step in the media task, and a callback chain. No allocation, no snapshot, no new task, and no wait in the AF callback; the allocator's null-on-exhaustion boundary in section 9.4 does not gate this design.

Trigger. The current source resets its scan index to zero on every media-loop invocation; it has no retained cursor. It searches 16-byte slots from `0x400a0834` up to the live next-index halfword at `0x400a081e` for type `0x0f` and signature `0x13a52b03` (receiver 3, sender `0x2b`, message `0x13a5`). This is success-only collection. Its growing prefix-rescan cost and moving per-invocation bound remain unmeasured; failure/cancellation retention and wrap coverage need a separate contract.

Worker. The media loop's receive call at `0x0071feb4` (stock word `0xebffffdc`, `bl 0x0071fe2c`) is retargeted to the hook (`0xebfbf0d0`, `bl 0x0061c1fc` on the emitted build). The hook restores the caller's message pointer into `r0` and tail-calls `0x0071fe2c`, so stock receive and dispatch continue unchanged, and on a hit it posts the stock writer's callback-form open `0x0071c648(cb_open, path, 1)`, whose fourth argument is 0 as the guard requires from task `0x1c`. The hit block sits at the end of the scan path, so the open wrapper's return flows into the receive block rather than back into another scan iteration with the wrapper's clobbered registers, and the short-write continuation re-posts with the length in `r2` and the advanced source in `r3`, the order both stock call sites fix. The handle allocation runs in the media task, which is the file task's own context: not the trace callback, not an IRQ-masked section, not the AF callback. The hook touches only r0-r3, r12 and lr, because the loop keeps r4-r7 and r11 live across the receive call.

Chain. `cb_open(status, handle)` accepts status 0 and a nonzero tag before posting a write. A status-0/tag-0 reply ends the chain, but the stock client can queue an open after zero tag allocation, so this branch does not prove no filesystem object exists. Success advances the source and decrements remaining bytes; the error path bounds and accounts the completed count, preserves the first error and calls close. A ready-gate rejection can invoke the close callback synchronously without reaching the release worker. Thus calling close does not prove tag/index cleanup. `cb_close` leaves the fired byte at 1. These ownership outcomes remain open under FILE-FAILURE-OWNERSHIP-01.

Values. Path `C:\AF.BIN` as a literal, so the DCF builder `0x0070fbfc` and its bookkeeping stay untouched; `C` is the registered SD volume letter from the registry words at `0x814730`/`0x814738` (section 9.6), where a literal `A:` would fail the qualified-path check with status 5. The short root-level name is a space decision, not a functional one: restoring the nonzero-handle guard after review needed eight bytes, and the guard plus the path string must fit one 324-byte window, so the DCAA-qualified `AFTRC.BIN` name was shortened to `AF.BIN` at the volume root. Source `0x400a0800`, length `0x1fff4`. The file is a live, non-atomic capture of the store: the A and B writers keep appending during the write, and the dump claims no ordering for records appended while it runs. The store's per-boot clear bounds the content to this boot.

Alternative activation, not used by the emitted candidate. The AF success site in image 5 (`0x00defa64`, stock word `0xebff6fb9`, the `bl 0x00dcb950` that sends `0x13a5`) can instead carry a stub that stores an armed byte and posts one message to the file service's front task `mediamsg` (receiver `101`), whose loop at `0x00721744` receives on id `0x65` and forwards every request it does not handle through `0x00721578` and `0x007211d4` to the media task, rewriting the destination byte to `0x1c`. An id absent from both switches (`0x2c27`) forwards unchanged, and the media dispatch table at `0x814740` could carry an added entry for it. That variant gives a deterministic wake with no dependence on capture traffic, at the cost of one image-5 word, one added table entry and the `mediamsg` continuation state (`0x91027c`) as a small conditional risk; the emitted candidate avoids all three.

### 9.8 Placement, limits and what stays open

Placement. The installed RC0 occupies `[0x0061c340, 0x0061c400)`; the free window is `[0x0061c1fc, 0x0061c340)`, 324 bytes. The emitted payload fills it exactly: 324 bytes from `host/firmware/af-recorder.S` and `host/firmware/af-recorder.ld`, SHA-256 `bd7f2b20c4f7a2eb1a581fe264d5668aca1e97568164f4b860f1167003adab02`, with the linker asserting `SIZEOF(.text) <= 0x144` and the hook at the window floor `0x0061c1fc`. The schema-3 applier requires `extension length == decode_base - output_base`, so the floor base and the 324-byte extension are the pair that spends the whole window; the earlier 316-byte revision sat at `0x0061c204` and left eight bytes unused, and the defect-fix revision needs the full window. The RC0b candidate (FILMSIM-PATCH-02) additionally claims `[0x0061c240, 0x0061c340)`, so the two candidates are mutually exclusive in this window; an RC0b install would need another home for the recorder.

The priority D3 prefix `[0x0061c240,0x0061c400)` overlaps this recorder by 256 bytes. This RC0-only recorder is not additive to D3 or the bank bootstrap. It remains non-installable pending a separate placement and the file/scan/trace contracts tracked in [GitHub Issues](https://github.com/skeet70/latent-image/issues).

Executability of the window is a shared-region argument (**static-inference** over **byte-read** register programming). The boot ROM programs five PMSA regions at `0x80000040-0x800000cc` (region 0 base 0, size field 63, access `0x1304`; region 1 base 0 size 59 access `0x30c`; region 2 base `0x40000000` size 59 access `0x130c`; region 3 base `0x80000000` size 27 access `0x308`; region 4 base `0xfff00000` size 23 access `0x30c`), but those are not the final state: the payload boot record at `0x0019e4`-`0x0019f8` first calls the MPU helper `0x001a84` for region `r1 = 0` with base `0`, access word `0x303` (loaded from `0x001adc`) and size field `0x3a`, which the helper finishes to `0x3b` (1 GiB), and only then visits regions 1 to 4. Region 0 is therefore a 1 GiB, access-`0x303` region, not the ROM's 4 GiB `0x1304` region whose bit 12 carries execute-never. Image 4 installs only regions 5, 6 and 7, through its setter `0x0061c700` and helper `0x0073ba50`, with bases from its own heap allocator at `0x007235d8`; none of those covers the window. Image 4's own code at `0x61c400` lies inside the reprogrammed region 0 and executes there, and the payload window `[0x0061c1fc, 0x0061c340)` lies in the same region and the same subregion, so it carries the permissions under which image 4 executes. The single-image candidate needs no image-5 patch: the container route carries only segment 4, and the manifest names one `image.segment_id`.

Limits. The close worker releases tags when reached, but synchronous client rejection and allocation exhaustion are not covered by the modeled ownership proof. The trigger retains successful searches only. Card-full/read-only status cases do not establish scheduler or removal behavior. The handle allocator waits on semaphore `0x3c` with `0xffffffff`; no bounded wait or lock-liveness proof follows from media-task placement. `C:\AF.BIN` rejects the inline DCF boolean-open branch by its fixed pathname, not by the nonzero-tag guard. Ordinary-boot trace content, post-move statistic freshness and complete attempt timelines remain unobserved.

Emitted. The payload source and linker script are in `host/firmware/` with a `//host/firmware:af-recorder` genrule that mirrors the diagnostic-halt one; the commands below were run directly (not through Bazel), and the blob's out-of-blob branches were verified numerically from the emitted words: `0x61c25c` to `0x71c648` (open, the stock card-file writer entry), `0x61c264` to `0x71fe2c` (receive), `0x61c298` and `0x61c2d0` to `0x71c75c` (write), `0x61c300` to `0x71c7bc` (close). The candidate manifest is `private-data/analysis/x-m1-1.50/af-recorder/patch-set.json` (SHA-256 `6fd0446843ff776dd0779f250e56fc20431e466f0ae3f4d44f82f4b2d8148392`): schema 3, one 324-byte prefix-extension blob filling the window (orders 1 to 81, current bytes the zero creation template) and one word write (order 82, `0x0071feb4`, `0xebffffdc` to `0xebfbf0d0`, `bl 0x0061c1fc`). The container chain ran on the prebuilt host tools, all host-only:

| Artefact | Bytes | SHA-256 |
| --- | --- | --- |
| patched decode `af-recorder/id4-recorder.bin` | 2,632,196 (`0x282a04`) | `241d75c014cd2cab94ff81755c96bf728dd9f3b67ae9b695b550a0f21c66c069` |
| record `af-recorder/record4.bin` | 1,337,092 (`0x146704`) | `ac9510b707c993e8840123f1f2617c63d1ac229aaf9aaf6b963dfa26f15257c9` |
| container `af-recorder/FWUP0002-recorder.DAT` | 38,405,148 (stock size) | `e9f1ed59b21171a9227ea0f0fefd99c14160cf5b1c1ba53cc69c026e48429544` |

`apply-patch-set` reported 82 ordered writes over `0x0..0x144` and `0x103cb8..0x103cbc`. `replace-record4` reported image-4 load `0x0061c1fc`, decoded size `0x00282a04`, segment size `0x00282a2c`, the stage header preserved at decode offset 516, the stored record length `0x00146704 = 0x14 + 0x001466f0`, `fflzd` round trips at both the record and rebuilt-stream gates, and every structural invariant passing; the container's word at `0x0071feb4` reads back as `0xebfbf0d0`. `verify` passes 41 of 41 contract fields (including the trailer image words `0x0061c1fc`/`0x00282a2c`), and `diff` against the installed RC0 container changes only the image-4 header (4 bytes), the segment table (4), the trailer (4), the image-4 stream (1,281,066) and the window-tail slack (258); the carried parts are untouched. The candidate is non-installable: ReviewAfCarry accepted this revision at source and byte scope with no open findings, RuntimeAfCarry's ten cases and the container reproduction are green, and installation remains a separate, unapproved decision.

The runtime record spans revisions and must be read that way. Against the first 316-byte revision at `0x0061c204` ([evidence](local://af-carry-runtime-evidence.md)), RuntimeAfCarry reported all six changed-path cases passing with no mismatches (valid, where the captured file equals the store bytes byte-for-byte with cb `0x61c278`, mode 1, tag 1, fifth argument 1, length `0x1fff4`, source `0x400a0800`, first error 0; short; `werr`, where status `0xd` is kept and the close is still posted; openfail; noaf, where the receive path is preserved every iteration; and wrap), with three blob mutations each failing a case (restoring the `r1` pop reproduces the memory-guard fault inside `0x71fe3c`, the `A:` path reproduces the drive refusal with `r0 = 5` and no file, and calling `0x71c6a4` changes the observed fifth argument to 0), and with the stock closure executing real bytes (wrappers, clients, the drive mapper `0x70ec84`, the ready gate, the handle allocator, the path copy, the post builder `0x71fde4` and the receive helper `0x71fe2c`) while the media dispatch, the kernel post/receive primitives and the byte copy are declared model code. The intermediate 324-byte revision then failed two cases: the hit path fell through from the open post into the next scan iteration with the wrapper's clobbered registers (the run died at `0x0061c224` with `r2 = 0`, `memory_guard`, after 574 instructions), and the short-write continuation passed the length and source in the wrong registers. Both are fixed in the current revision, whose open return flows into the receive block, whose short-write continuation re-posts with length in `r2` and source in `r3`, whose scan requires the type byte before accepting the `+4` word, whose failure path accounts the bounded completed count, and whose callback refuses a zero handle. Against the frozen revision RuntimeAfCarry reports every gate green ([evidence](local://af-carry-runtime-evidence.md)): all ten changed-path cases pass with no mismatches (valid, where the 131060-byte captured file equals the store bytes byte-for-byte; short; `werr`; `werrpart`; `werrfull`; openfail; `zerohandle`, where the restored guard ends a zero-handle success with no write and no close; `type72`, where the type-`0x72` tick collision does not fire and the appended real type-`0x0f` record does; noaf; wrap), the container chain reproduced independently with the same hashes and a byte-identical `fflzd` decode, and the three mutations of the blob still fail as intended (`pop {r1,lr}` reproduces the `memory_guard` inside `0x71fe3c`, the `A:` path reproduces the drive refusal with `r0 = 5` and no file, and `0x71c6a4` flips the observed fifth argument). The fixture pins the `C:\AF.BIN` path through the blob's own bytes, so no expectation rests on the earlier name. ReviewAfCarry accepted this revision for source and byte inspection with no open findings, carrying the residuals rather than approving runtime behaviour: ordinary-boot trace availability, wrap and no-next-message coverage, semaphore liveness, live non-atomic mutation, the independently modeled kernel and media boundaries, and the explicit RC0b placement conflict. Installation remains unapproved and the candidate non-installable.

Reproduction for sections 9.6 and 9.7 (host-only):

```sh
xxd -g 4 -s $((0x814740-0x61c400)) -l 160 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0x782008-0x61c400)) -l 16 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -g 4 -s $((0x7c8f78-0x61c400)) -l 48  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
xxd -s $((0xed3ab0-0xbf1a00)) -l 48 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
```

The function bodies named above sit under `private-data/analysis/x-m1-1.50/af/functions/`.

Payload and candidate container, host-only (all of it ran in this pass with the prebuilt tools; nothing was installed):

```sh
NIX_HARDENING_ENABLE= clang --target=armv7r-none-eabi -march=armv7-r -marm -nostdlib -static -fuse-ld=lld \
  -Wl,--no-dynamic-linker -Wl,--build-id=none -Wl,-T,host/firmware/af-recorder.ld \
  host/firmware/af-recorder.S -o af-recorder.elf && llvm-objcopy -O binary af-recorder.elf af-recorder.bin
llvm-objdump -d --triple=armv7-none-eabi af-recorder.elf
bazel-bin/host/firmware/apply-patch-set private-data/analysis/x-m1-1.50/af-recorder/patch-set.json \
  private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin private-data/analysis/x-m1-1.50/af-recorder/id4-recorder.bin
bazel-bin/lz/lz-encode 0x160000 private-data/analysis/x-m1-1.50/af-recorder/id4-recorder.bin \
  private-data/analysis/x-m1-1.50/af-recorder/record4.bin
bazel-bin/host/firmware/rebuild-update replace-record4 --decoder bazel-bin/lz/fflzd --load-address 0x0061c1fc \
  private-data/firmware/x-m1-1.50/FWUP0002.DAT private-data/analysis/x-m1-1.50/af-recorder/record4.bin \
  private-data/analysis/x-m1-1.50/af-recorder/id4-recorder.bin private-data/analysis/x-m1-1.50/af-recorder/FWUP0002-recorder.DAT
bazel-bin/host/firmware/rebuild-update verify private-data/analysis/x-m1-1.50/af-recorder/FWUP0002-recorder.DAT
```

The first two lines need `af-recorder.elf`/`af-recorder.bin` written somewhere writable; the manifest pins the blob bytes themselves, so the build directory does not matter.

### 9.9 Recorder revision 2: bounded scan, attempt and result coverage, moved placement

**Source-inspected and static-inference, host-only; no build and no camera ran in this pass.** Sections 9.7 and 9.8 above are the revision-1 record (324 bytes at `0x0061c1fc`); the D3 prefix owns that window, so revision 2 in `host/firmware/af-recorder.S` and `host/firmware/af-recorder.ld` moves the blob out of it.

- **Placement.** The blob sits in the plain window's tail padding at RAM `0x0144a5e0` (flash media offset `0x8ea5e0`), inside the id-6 loader's raw-copy window `[0x01100000, 0x01460000)` (reports/recipe-design.md section 18). It claims `[0x0144a5e0, 0x0144ade0)` and touches no byte of the D3 prefix `[0x0061c1fc, 0x0061c400)`, of the installed RC0 prefix, or of the bank's gap-1/gap-2 blocks. The image-4 write set shrinks to one word: `0x0071feb4` `0xebffffdc` -> `0xeb34a9c9` (`bl 0x0144a5e0`), and the image-4 decode keeps its RC0 geometry. The region is loaded-but-unowned padding on the same census the bank carrier rests on; that status, not an ownership proof, is what the manifest records. Residency is a gating prerequisite: the id-6 load is an application-init flashloader request (command `0x4c03`) in the same init flow that creates the media task, so the site patch must not be live before it completes; the first media-loop iteration against the load completion is unproven, with the settlement routes named in the manifest's `free_space.residency_prerequisite`.
- **Bounded scan.** `rec_state` retains a next-slot cursor; one invocation examines at most 16 slots and stops at the first trigger, and a caught-up invocation costs one halfword read of the store's next index. The slot budget lives in one register that the attempt and search handlers leave alone (their scratch is `r12`). A next index below the last observed index latches the wrapped flag and counts the crossing once, before any trigger can exit; a next index below the cursor drains the scan to the store end and resumes at slot 0 without counting again, so a tail-drain trigger reports one wrap for one writer crossing. A full lap between two invocations needs 8188 traced sends between them, so the count is a lower bound and the aliasing stays a recorded coverage limit.
- **Attempt identity and result coverage.** A type-0x0f record with id `0x3b0d` or `0x3f2e` opens an attempt; `0x3f2e` sets the search flag; a second `0x3b0d` closes the previous attempt with no recorded result; the exact success word `0x13a52b03` fires class 1; an attempt open past 1500 ticks (about 15 s) fires class 3 only once the scan is caught up, so a pending result record is always examined before a timeout is committed. The failure arm `0x00dcd010` emits a UI event through `0x00dcb730` and, in the mapped evidence, no AF result send, so a failed attempt is covered as an attempt without a recorded `0x13a5` record, not as a failure record; whether the UI event itself carries a trace record depends on its id in the trace allowlist and is not claimed. Statistic, AF-frame and lens freshness are state and are not carried: the state read reaches lens position `0x00942b84` and the AF-frame cells, and post-move statistic freshness needs a hook on the intake or cadence path (`0x00f40944`, `0x00f44fec`, `0x00ff6b4c`), as does completion timing (`0x00ff5d80`, `0x00ff9178`).
- **Trailer.** The capture is the live store followed by a 40-byte trailer ("AFRC", version 2, trigger class and status, attempt/result/no-result counts, flags, trigger tick, attempt ordinal, wrap count, next index, store length, retained attempt tick). A file of any other length is a short capture.
- **File ownership and lock liveness.** One carry per boot opens at most one file; the recorder owns the tag from open to close and posts close on every path with a nonzero tag. A nonzero open status ends the chain with no close for either delivery: the synchronous ready-gate rejection `0x0071ddf0` before any request (no object), or a post-dispatch failure whose worker releases the tag at `0x71f3cc`-`0x71f3e8`; a zero-handle open leaves no closable identity and sets the NOHANDLE flag, one bounded orphan per boot; a synchronous close rejection leaves one allocated tag for the boot and is not retried. A close callback status is not release evidence: a pre-dispatch no-op can report 0 with the tag untouched, so release is read from the tag table at `0x009101c0` (rule and fixtures from the recipe-bank lane's handoff, D2StateApply 2026-10-11). The recorder adds no lock and never waits; the only waits are the stock semaphore `0x3c` in the open allocator `0x0071ef18` and the release routine `0x0071eff4`, inherited by every stock file client and not bounded from the client side.
- **Service-AF job map.** The manifest's `service_job_map` binds, per whitelist value `0xa0`-`0xac`, the queue-7/config prerequisites, the job's trigger and terminal posts, its internal result slot or host-pair publication, and what the recorder can carry for it; `0xa3` and `0xab` have no handler bound in the mapped record and no coverage claim. No value's path to the AF request/search layer is bound, so the map states per value whether the store capture dates the outcome or only the trigger messages.

The revision-2 manifest and emission implemented this design and were the installed failed-boot candidate. Full-system emulation later reproduced the residency violation: the media task reached `0x0071feb4` while the 1,896-byte target was still zero (E-AF-RECORDER-RESIDENCY-EMU-150).

Revision 3 keeps `0x0071feb4` stock in the emitted image and changes the id-6 case-3 completion call at `0x0073b564` from `0xebffa12a` (`bl 0x00723a14`) to `0xeb343d0d` (`bl 0x0144a9a0`). The pinned 52-byte `rec_stage` trampoline at blob offset `0x3c0` publishes the hook word, performs data/instruction cache maintenance, restores the call ABI and tail-calls the displaced helper. Its exact emitted DAT, SHA-256 `426ee4ec5cf83fef492e719bbfae94fa247549cc02d1eda099af55c44fd4d0e5`, passes 42/42 container checks. In the exact-candidate ROM run, the early tap remained stock while the payload was zero; the later stage call saw the payload resident; the trampoline tail saw `0xeb34a9c9` published with `r0-r12` and the completion-call link register preserved. No later media receive occurred before the 100,000,000-instruction bound. The candidate remains non-installable because failed-copy traversal into the unstaged trampoline, padding ownership, later media-loop liveness and physical recovery remain open.

### 9.10 Service-AF job map: prerequisites and terminal outcomes per job

**Static-inference over byte-read**, inherited from E-SERVICE-CAPTURE-PATH-150, E-SERVICE-CAPTURE-CASES-150 and E-SERVICE-JOB-RETURNS-150; host-only, no camera. The family is the queue-7 imaging jobs behind the config-function-3 handler `FUN_00dc7564` (queue-7 `0x1707` through `FUN_00ed1300`), dispatched by the group-2 row / width-7 matrix `0x00ece2b8` and action table `0x00ecdf7c` through the state cell `0x015c20b0`; whitelist values `0xa0`-`0xac` are rows 90-102 of the 144 records at `0x00dc7a84`. `FUN_00dc25a4` returns literal zero, so no mode or state test blocks the jobs; the `0x87` shooting command is selected for shooting states `0xce`/`0xcf` (`0x88` for `0xca`/`0xcb`). Common prerequisites: a host config write of the whitelist value into adjustment cache `0x80` (the live selector `FUN_0073ac60` = `FUN_0069f8fc(0x80)` at completion), a vendor service session (mode `0xb`) to carry it, the job's own trigger records on the queue-3 `0x200`-`0x209` family and, for frame jobs, the machine-`0x14` action `FUN_00e14914` with the frame module `FUN_00e676ec` (message `0x1349`, receiver `FUN_00e8659c`). The exposure/preview path inside mode `0xb` is unproved, so production is static-inference.

| Value | Prerequisites beyond the common set | Terminal outcomes | Recorder coverage |
| --- | --- | --- | --- |
| `0xa0` | parameter block built at `0x014e4384` from cache `0xc1`, `0xc2` and `0x960c`-`0x960f` | queue-3 `0x200` through `FUN_00db3eec`; the block is written; slot `_DAT_01526acc` is not published | the `0x200` post is a traced send; the block write is not carried |
| `0xa1` | none | queue-3 `0x201` through `FUN_00db3f64` plus a queue-2 envelope through `FUN_00db4af4` | the posts are traced sends |
| `0xa2` | 1000-byte block copy `0x014e4410` to `0x014e5f30`; `FUN_00dc3208` or `FUN_00dc327c`; the host pair is cleared first through `FUN_00e67a40`; waits on `0x901`, `0x90a`, `0x842` and `0x816` | `0x204` then `0x207` or `0x206` selected by cache `0x81`, then `0x209` through `FUN_00db4254` into queue-3 slot `0x01529964`, read by `FUN_00dce40c(0x209, 0x87, 0)`; no publication on the host pair | the posts are traced sends; the slot is not carried, and whether the machine-`0x14` action runs is unproved |
| `0xa3` | no handler bound in the mapped record | unstated | none claimed |
| `0xa4` | the trigger `FUN_00dc327c` clears the host pair first (destructive) | `0x204` and `0x206` posts; no publication | the posts are traced sends |
| `0xa5` | per-slot record from `FUN_00e87ad4` (`+0x188`) | host pair `0x015967a8`/`0x015967ac` published from record `+0x188` with lengths from `+0x198`/`+0x190` | the completion handler's own posts are traced; the pair is internal |
| `0xa6` | `FUN_00dc2f68` reads cache[value] and enters the frame module directly through `FUN_00e66e50` (message `0x1338`, callback `FUN_00dc25f4`, semaphore `0x10` wait) without the queue-3 trigger | host pair published from record `+0x8c` with `+0x90` | the `0x1338` send is traced; the pair is internal |
| `0xa7` | per-slot record from `FUN_00e87ad4` (`+0x188`) | host pair published as for `0xa5` | as `0xa5` |
| `0xa8` | the `0x203` stop `FUN_00dc33c0` carries `{0xab, v, 0xac, 0}` | `0x203` plus the host pair from record `+0x2928` with `+0x292c` | the `0x203` post is traced; the pair is internal |
| `0xa9` | the `FUN_00e883f0(0, 0x01583e74)` object | host pair through `FUN_00e6f9a0` from object fields `+4` and `+8` | internal pair; the surrounding posts are traced |
| `0xaa` | parameter block `0x014e5b44` with word 0 = `0x2c` and a duration from cache words | `0x20c`, `0x204` and `0x20b` through `FUN_00db42c0` into slot `_DAT_015299d0`, read by the job-record family and not published | the posts are traced sends; the slot is not carried |
| `0xab` | no handler bound in the mapped record | unstated | none claimed |
| `0xac` | none | queue-`0x2b` requests `0x3b03`, `0x3b29` and `0x3b02` through `FUN_00ef8ed0`, `FUN_00ef98e4` and `FUN_00ef8e7c`; side effect only | the request posts are traced sends |

No value's path to the AF request/search layer (`0x3b0d`, `0x3f2e`, `0x13a5`) is bound, so the recorder captures an attempt and its result when a job drives one and, for values whose only outcome is a frame or an internal slot, dates only the trigger messages. Statistic, frame and lens freshness stay outside a message carry and need the state read or a hook ([the observation plan's G4/G5 table](af-observation-plan.md)).
