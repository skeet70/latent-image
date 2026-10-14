# AUTO_ACT arming: the gate write, an armed boot, and what can be proposed

Host-only static work on the pinned official 1.50 images (id4 base `0x0061c400` SHA-256 `f2e2acab…ff16`, id5 base `0x00bf1a00` SHA-256 `3cd501c0…d892a`) and this body's three adjustment captures. No camera was enumerated, opened or commanded.

Labels: **byte-read** for bytes read from a pinned artifact, **static-inference** for a conclusion drawn from them, **hardware-observed** for captured camera state, **unproved** where the named fact is unavailable. Section 11 carries the reproduction commands.

## 1. Verdict

**The maintenance boot did create and enter `mujin`. The kernel delay argument is approximately 1 ms, not one 10 ms log tick. The wait-before-power-on run armed too late for the working ten-second model to survive; repeating that sequence is not the next experiment.**

A host config write of adjustment index `0xa2 = 2` lands in the RAM mirror of the adjustment area at `0x41acd8a2`, leaves storage untouched, and is cleared by the next power cycle or by the same write carrying `0x00` (byte-read: the write callback `0x00dc7564` stores through `0x0069fc94` and returns without a queue post for any start index other than `0x80`; the boot load `0x0069f204` refills the mirror from storage).

Hardware-observed: session `20261009T160839Z-arm-auto-act` confirmed `0xa2 = 2`, then observed no sender over 79 polls. Its baseline at `0x016777d4` already held `0x016777d8`, the receive-buffer pointer written by the task prologue before its first sleep. Static-inference: task absence and a wholesale module-address mistake do not explain this run. Section 13 binds the scheduler's units, the run's timings and the remaining task-state diagnostic.

Byte-read: the RAM-only write and its reversal store one mirror byte and post no storage message. The persistent form remains blocked on the separate `0x420000` whole-area commit's boundedness.

## 2. Scope: which boots run the check

**Maintenance: observed; ordinary: statically selected.** Byte-read: `FUN_00ed2b00`, called at `0x00d98660`, creates queue 35 with `{48 slots, 12-byte payload}` through `0x00724cdc`, then creates and activates `mujin` only on a zero queue-creation result. That condition tests allocation and kernel object creation, not application mode. Neither the creator nor the queue wrapper tests mode `0xb`. Hardware-observed: this run's `0x016777d4 = 0x016777d8` supplies the missing outcome evidence: the entry reached the prologue in a maintenance boot.

Once per boot: the entry sleeps 1000, tests `0xa2 == 2`, and on equality sleeps 4000 and calls the sender `FUN_00ed2c34`, which re-tests the byte for nonzero and then reads the RAM guard byte `0x1667048`; a set guard returns immediately, and the sender sets the guard after sending (byte-read). Nothing re-tests the byte inside the task's receive loop.

Byte-read: the delay wrapper `0x00723ce8` selects the loader's kernel service at interface `+0x4c`; that service adds the argument to the scheduler deadline clock, whose timer callback advances by 10 per interrupt. The same callback advances `0x400c0800` by one. Static-inference with the hardware-observed 99.88 Hz callback rate: an argument unit is about 1.001 ms, `1000` about 1.001 s and `4000` about 4.005 s, with timer quantization and scheduling latency. Section 13 gives the byte anchors.

Static-inference: if armed at the test, the task posts its two messages about five seconds after prologue entry, then waits approximately 100 ms before receiving them.

## 3. What the host write does

The write is the config opcode with function 3, the same request the End Setting protocol's step 3 uses at index `0xf7`: control data `d0 00 00 00 00 00 00 00 00 25 S_hi S_lo`, a 37-byte bulk OUT frame `02 00 1a 00` + twelve zero bytes + function `03 00 00 00` + mailbox `01 00 00 00` + page `00 00 00 00` + in-page offset `a2 00 00 00` + length `01 00 00 00` + the data byte, then `d1 … 00 80 S_hi S_lo` and a 128-byte bulk IN.

Path (byte-read): `0x00d801cc` admits function 3, `0x00d82568` reaches the callback `0x00dc7564`, whose byte loop runs through `0x0069fc94(index, byte)` before any other test; `FUN_0069fc94` stores to `*(0x0096ca50) + index`, and `FUN_0069f204` sets `0x0096ca50 = 0x41acd800`. So the byte lands at RAM `0x41acd8a2`, inside the 0x420000-byte mirror of the adjustment area.

No storage write: `0x00dc7564` posts the queue-7 message `0x1707` only when the request's start index is `0x80`, and returns 0 for every other index (byte-read). The storage writers and their complete caller sets are unchanged from the commit map: whole-area `0x0069fea8` at `0x00d5cf0c`, `0x00db932c` and `0x00dbc8e4`; reload `0x0069fe20` at `0x00db9228` and `0x00dbc8c4`; bounded `0x006a0028` at `0x0073d440` and `0x0073d458`, which writes offsets `0x400000` length `0xfd00` and byte `0x237` only (byte-read, whole-image scan; matches E-ENABLE-COMMIT-150).

What clears it: a power cycle, because the boot load refills the mirror from storage through `0x0069f204`; a reload (`0x0069fe20`) in place; and a second identical write carrying `0x00`. What does not clear it: nothing in the auto-act path writes the cache at all.

## 4. What the byte changes while set

Reading the byte by constant index through the accessor `0x69f8fc` across the three decoded images yields two sites, `0x00ed2b84` and `0x00ed45b0` (byte-read, whole-image BL scan with the index attributed at the setting instruction). The same scan finds fourteen sites for index `0xf7`, so the census resolves a known family.

- `0x00ed2b84` is `FUN_00ed2b78`, the gate function the `mujin` task calls. Its consumer is the equality test at `0x00ed2cb0` and the nonzero test inside `FUN_00ed2c34` (byte-read).
- `0x00ed45b0` is inside `FUN_00ed4570`, which packs ten cache bytes into a caller-supplied ten-byte struct; `0xa2` becomes byte 4 of it (byte-read). The struct's consumer is unproved, as is whether it reaches any persisted record.

Readers whose index arrives in a register are outside this census, and the generic cache accesses (boot load, reload, commit, media window) move the byte without reading it as a flag.

The second sender path is closed on this body. `FUN_00d0cd10`, whose only `bl` is at `0x00d0cd50`, calls `FUN_00ed2c34` from the application event pump `0x00c22f60` at `0x00c231fc` when event word `0x00944580` is 9 with a zero argument (byte-read). It is gated on `0x0096b477` and on the RAM guard `0x01467840`. `0x0096b477` is byte `0x77` of the block at `0x0096b400`, filled by `FUN_0072b8c4` from cache index `0x265`, called from `0x0073ad04` with that block as its argument (byte-read). Cache index `0x265` reads `0x00` in all three captures of this body (byte-read, windows `000200` offset `0x65`), so that path is dead as shipped and arming `0xa2` does not open it. Writing `0x265` would not merely arm the sender: the same flag gates the frontend's handling of event `0x91b` at `0x00d0c954` and the script branches at `0x00d3cbd4`, so it stays out of scope.

An arm written after the one-shot test changes no auto-act behavior in that boot. An arm written before it can select the sender chain described here.

## 5. The self-queue, and the correction it makes to the script map

The report's open question about the dispatch counterpart is answered by the addresses: the two messages go to the task's own queue. `FUN_00ed2b8c` and `FUN_00ed2be4` build a message buffer whose first halfword is `0x3300` and `0x3302`, whose byte 2 is the calling thread and whose byte 3 is `0x23`, and send it with destination `0x23` through `0x0072503c` (byte-read). The task's loop receives from `0x23` through `0x00725318` (byte-read: `FUN_00ed159c`). So `0x3300`, `0x3302` and the executor's `0x3305` are self-addressed, and the handshake needs no host or jig counterpart.

That also closes the second open question. After a load the loader sets the statement index to 0, sets the selector to 2 through `0x00ed15ec(2)` and sends `0x3305` itself; the dispatcher `0x00ed138c` maps `0x3305` to the code word of the current statement record at `0x166705c + index*0xe0`, so statement 0 dispatches without any `0x3301` message (byte-read). The guard flag is `0x1667048`, the selector word is `0x16777c0`, and the statement index is `0x1667054` (byte-read).

## 6. An armed boot, in the three states the owner would leave it in

Assumes the byte is 2 at the test instant, either from a same-boot RAM arm before the test or from storage.

**No card, or no `DCAA\auto_act.scr`.** The loader runs its prologue, clears the `0x10764`-byte script area at `0x166704c`, builds `<volume>:\DCAA\auto_act.scr` from the template at `0x00ed3ac4` with the volume letter from `0x71f708`, and opens it through `0x0071c648`. The open fails into the callback `0x00ed1828` with a nonzero status, the loader restores the performance level and returns (byte-read). No read, no parse, no message: the camera boots and runs normally, with a cleared script area and the guard byte set. Nothing on screen.

**A card with the file, 1 MiB or less.** Size is taken from the open-info word at `+0x34` of the info struct (`0x0071eba0` fills it), the cap is compared against `0x100000`, the read targets the fixed buffer `0x43e6ff00` through `0x0071c6fc`, and the size is stored at `0x16777ac` only when the completion count equals the requested size (byte-read). Then the parser runs, the selector is set to 2 and the executor starts on the self-queue. A script that presses keys or calls the 17 `func` routines then acts on the camera with no host involvement.

**A file above 1 MiB.** The size test branches past the read to the close path; the parse then runs on a zero length, since the length word was cleared by the memset and never written. The loader still sets the selector and sends `0x3305`, whose dispatch resolves the zeroed statement 0 (code 0) so nothing executes. The camera behaves normally.

**An empty or unparsable file.** Whitespace-only input leaves the statement count at 0 and behaves like the oversized case. A file with more statements than the store's usable prefix writes into the script's own variable array at `0x16776dc` and label table at `0x1677744`, both of which start inside the cleared area, so the practical script bound is under about 300 lines rather than the store's 301 records; a record whose code matches no dispatch entry stops the table walk, queues nothing and leaves the loop blocked in receive. That is a quiet stall, not a write to camera state.

So a camera left armed with a card inserted is a camera that runs whatever that file says, once per boot, without a host.

## 7. The retained operation and its limits

The reviewed `arm-auto-act --wait-for-device` sequence is implemented and reached the camera. It confirmed the RAM write but did not fire the sender. Static-inference: it is not a justified firing proposal without a new task-relative timing bound; section 13.4 specifies a no-arm diagnostic instead. The sequence below describes the retained command, not authorization for another run.

Frames are the transport's existing shapes: C0 open, `d0`/`d1` per request, C4 close, with `S_hi S_lo` the session from the C0 reply and the C0 reply required to be `S_lo S_hi 00 00 00 00 01 00`.

1. **Wait and open.** With `--wait-for-device`, initialize USB and list at 50 ms spacing for at most 120 s, proceeding automatically on one supported `04cb:ff80`. Only no eligible device continues the wait; multiple devices, unsupported descriptors and provider failures refuse without retry. Open, claim and C0 remain the existing sequence.
2. **Check before arming.** Read config page 0 and require both `0xa2` and `0xf7` to be zero. Enable the reviewed `0xf7` gate and require its readback. Read the boot tick `0x400c0800`, then the two baseline witness windows and require the eleven decoded sender/loader cells to be zero. This does not require either whole window to be zero, and does not check the task's receive-buffer pointer.
3. **Arm and confirm.** Send the existing 37-byte, length-1 config function-3 frame at `0xa2` with value `0x02`, require the existing write reply, then the page-0 readback showing `0x02`. Separate captures of the precondition, gate readback and before witnesses are written after the arm. The raw replies and outgoing frames remain durable before checks and sends.
4. **Watch and end.** Watch the same witness cells for at most 90 one-second polls, capture final windows and read the end tick. Restore `0xf7 = 0`, require its readback and close C4 on every exit after gate opening. No `0xa2 = 0` write is sent; end by cutting the dummy-battery supply, not the POWER switch.

What the owner sees: the command's own output lines and nothing on the camera. The body is not left armed after the session ends, because the byte was never in storage.

## 8. What blocks the firing experiment

Byte-read: the boot load refills the mirror from storage. A same-boot early writer must therefore run after that load and before the one-shot test; a persistent byte would already be present at that test.

- Static-inference: the current USB arming sequence has no demonstrated reachable window. Its observed C0-to-arm path alone takes longer than the approximately one-second task delay. A faster unchecked arm is not proposed.
- The second sender path is gated on cache `0x265`, which reads `0x00` in all three captures, and the only writer that would reach it changes frontend error handling.
- Reaching storage with index `0xa2` requires the whole-area commit: request opcode `0x1a0002` function 3, start index `0x80`, value `0x22`, then the queue-7 chain `0x1707` → `0x00eca4c0` → `0x00ec9b50(2)` → `0x00ecc66c` → `0x00dbc8d8` → `0x0069fea8` → `0x006a9fdc(mirror, 0x420000)`, with the status stored to cache `0x81` (byte-read).

The property that fails is boundedness. That request writes `0x420000` bytes from the RAM mirror at `0x41acd800`. The area's stored content is captured to `0x409a00` on this body under 1.50 and to the same offset under 1.00 (byte-read, window listings of `20261008T175358Z-read-config` and `S03/read-03`, 1309 windows each), so the target content is not the unknown; what is unestablished is which spans of the mirror have been loaded from storage at commit time and which still hold boot-time RAM. Comparison of the mirror against the capture exists only for its first `0x400` bytes (inherited: `header-byte-differential.md`, zero differing bytes; cycle-D captures, byte-identical adjustment window).

Reliability of the reversal is equal to reliability of the set, since both are the same function driven by a different mirror byte, and both can be confirmed or refuted by the same config readback. The stopping rule for unbounded effects and the standing prohibition on that request in the End Setting live protocol apply unchanged.

Three things would have to be established first, all host-side:

1. The mirror's storage-derived extent over the whole `0x420000` span, from a read of `0x41acd800..0x41aed800` and a comparison against the existing captures, so the commit's difference from storage is stated rather than assumed.
2. Whether the on-camera byte editor `0x00d5cc08` can exit with `0x0096b480 == 3`, which commits the whole area from outside the config path and without a queue-7 trigger. Its entry conditions and that byte's meaning are unproved, and the same block's flag `0x77` is `0x00` on this body.
3. A bounded persistent writer for a single cache byte, or a statement of what recovery means for the `0x420000` span if one does not exist. The 1.00 area capture and the 1.50 captures make a content restoration arguable; no restore route is demonstrated.

## 9. The observation plan for a load, if a firing run ever happens

The module calls no syslog writer on its success path (byte-read: the loader, the dispatcher and the statement handlers call the file API, the kernel wrappers, `0x0064982c`, `0x00ed1660` and each other). The ring is therefore a failure detector and the RAM words are the instrument.

Ring, read with `read-log` in the same boot: the file layer's own records through `0x00726c74` carry codes `0x50`, `0x52`, `0x53` and `0x55` from `0x0071ddf0` when the volume or file system is not ready (static-inference for the code meanings); the kernel wrappers log type `0x09` records through the `0x723a14` family with codes `0x1c` and `0x1f` when a send or a receive fails, and the destination byte of a failed auto-act send is `0x23` (static-inference). A successful load leaves nothing. `WT_LOG` writes a `.msg`-suffixed file through the file API from the template at `0x00ed3ab0`, which is the one card-side artifact a script can leave (name construction unproved).

RAM, read through the `0xf7`-gated `0x200001` function 2 with two 128-byte requests per 256-byte window, two windows:

- `0x1667000`: the guard byte `0x1667048` (0 before, 1 after the send, so a dark run separates "the byte was not 2 at the test" from "the send happened"), the `-1` word `0x1667050`, the statement index `0x1667054`, the statement count `0x1667058`, statement 0's code at `0x166705c` and its keyword text at `0x1667060`.
- `0x1677700`: the loader's async state `0x16777b0` (in flight), `0x16777b4` (last status), `0x16777b8` (open result), `0x16777bc` (file-completion count), input length `0x16777ac`, selector `0x16777c0` and receive-buffer pointer `0x16777d4`. Byte-read: the prologue writes `0x16777d8` to the last cell before sleeping.

Static-inference: guard 1 witnesses reaching the sender's store after its send calls, not successful queue delivery by itself. Nonzero input length witnesses a complete file read; selector 2 with records is statement mode; selector 1 with index `0xffffffff` and records is `END`. Guard 0 with the eleven decoded cells zero establishes no observed sender, not task absence, a zero whole module, or a measured gate-test instant.

## 10. State-changing bytes

New to this design, and the whole of what an independent review would check:

| Step | Bytes | Effect |
| --- | --- | --- |
| Arm | config `0x1a0002` function 3, mb 1, page 0, offset `0xa2`, length 1, data `0x02`, D0 length `0x25` | RAM `0x41acd8a2 = 2`; no storage, no queue post |
| Confirm | config function 2 at the same location | RAM read of the mirror byte |
| Gate restore | config function 3 at offset `0xf7`, length 1, data `0x00` | RAM `0x41acd8f7 = 0`; no `0xa2` disarm frame is sent |

Reused from reviewed operations, only if a RAM read is included: index `0xf7 = 1` and its restore to `0x00`, plus opcode `0x200001` function 2 reads at the two addresses above.

Not to be sent, in any value: a config write whose start index is `0x80` (the store precedes the whitelist check, and `0x22` commits the whole area while `0x21` reloads it); any write at index `0x265`; any index in `0x80`..`0x8a` outside the reviewed `read-media` set.

Writes the body itself performs when the loader runs, listed so an armed-boot run is reviewed as a whole rather than as the config request alone: the `0x10764`-byte memset at `0x166704c`; the async state words `0x16777b0..0x16777bc`; the input length word `0x16777ac`; the guard `0x1667048`; the selector `0x16777c0`; the performance-level entry `0x409c1e54` through `0x0064982c(7,8,0)` and its restore `(7,0,0)`; the file read into `0x43e6ff00` capped at `0x100000` bytes; parse output over `0x166704c` and past it for an oversized file; whatever a loaded script's own statements then do.

## 11. Reproduction

All commands read host files only.

```sh
# call-site scans (prints the eight words before each match)
cc -O2 -o /tmp/blsite private-data/analysis/x-m1-1.50/adj-consumers/blsite.c
R=private-data/analysis/x-m1-1.50/lz
/tmp/blsite $R/id5-00bf1a00.bin bf1a00 69f8fc a2      # the two 0xa2 readers
/tmp/blsite $R/id4-0061c400.bin 61c400 69f8fc f7      # the 0xf7 family, for contrast
/tmp/blsite $R/id5-00bf1a00.bin bf1a00 ed2b00 -      # one call site: 0x00d98660
/tmp/blsite $R/id5-00bf1a00.bin bf1a00 ed2c34 -      # 0x00d0cd50 and 0x00ed2cc4
/tmp/blsite $R/id5-00bf1a00.bin bf1a00 69fea8 -      # the whole-area commit's three sites
/tmp/blsite $R/id4-0061c400.bin 61c400 69f8fc 265    # 0x0072bd20, the 0x265 reader

# the task, the loader and the senders
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=0x2e10f0 --stop-address=0x2e12e0 $R/id5.elf      # 0xed2b00, 0xed2b78, 0xed2b8c, 0xed2be4, 0xed2c34, 0xed2c90
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=0x2e03cc --stop-address=0x2e0590 $R/id5.elf      # 0xed1de0 loader

# the body's two gate bytes in all three captures
for s in 20261008T175358Z-read-config 20261007T060812Z-read-config S03/read-03; do
  od -A n -t x1 -j 0xa2 -N 1 private-data/sessions/$s/windows/000000.bin   # 0xa2
  od -A n -t x1 -j 0x65 -N 1 private-data/sessions/$s/windows/000200.bin   # 0x265
done
```

## 12. What this changes in the script map

- The dispatch counterpart is the task's own queue 35, not an unidentified external link.
- The executor's first statement dispatches from the loader's own `0x3305`, without a `0x3301`.
- The gate byte's readers by constant index are two, `0x00ed2b84` and `0x00ed45b0`.
- The second sender path exists and is dead on this body through cache `0x265`.
- The loader's size read is info `+0x34` (the report's `stack_info+0x54` is the same field, the struct base being `r11-0x88`), and its clear is `0x10764` bytes, with the length word at `0x16777ac`.
- An armed boot's card behaviour is stated for the four file states in section 6.

## 13. The delay unit, the host-command latency, and the window verdict (2026-10-09)

### 13.1 What the owner-run session contains

Hardware-observed: `20261009T160839Z-arm-auto-act` waited for the supported device, opened session `0x017c`, required page-0 `0xa2 = 0` and `0xf7 = 0`, enabled and confirmed `0xf7 = 1`, sampled boot tick 426 (4.265 s at 99.88 Hz), read baseline windows, sent `0xa2 = 2` and confirmed 2. Cleanup restored `0xf7 = 0` and completed C4, release, close and exit. The last record is `session-uncertain: user interrupt`, not a completed outcome report. The owner reports no new `.msg` file; this is consistent with no observed sender.

Hardware-observed: the full 256-byte baseline windows are not all zero:

| Window | Nonzero content | Remaining bytes |
| --- | --- | --- |
| `0x01667000` | little-endian word `0x02063800` at `+0x40`; nonzero bytes `+0x41=38`, `+0x42=06`, `+0x43=02` | zero |
| `0x01677700` | little-endian word `0x016777d8` at `+0xd4`; bytes `d8 77 67 01` | zero |

Unproved: the meaning of the word at `0x01667040`. Hardware-observed: it reads the same `0x02063800` in the 2026-10-08 dump of the same segment, where the bytes around it are uninitialized-RAM fill `0xc6` while here they are zero, so the twelve bytes before the array are not the module's own state. Byte-read: instructions `0x00ed2c98`-`0x00ed2ca4` form selector base `0x016777c0`, add `0x18`, and store that pointer at base `+0x14`, yielding `[0x016777d4] = 0x016777d8` before `sleep(1000)`. Static-inference from this hardware-observed value: `mujin` entered, and the second state window is at the intended module address. The task-absence candidate is refuted for this maintenance boot.

Hardware-observed: all 79 decoded polls, numbered 0 through 78, show eleven zero sender/loader cells and empty statement text. The journal contains 81 RAM replies for each polled half: two baseline halves plus 79 poll replies. Every captured half is byte-identical to its corresponding baseline half, including the two nonzero words. The actual host interval between the first and last poll records is 98.694854268 s; the 78 nominal one-second sleeps exclude USB and journal costs.

Hardware-observed: interruption occurred before final capture. This session has no `after-*.bin` windows and no end tick. The last poll supplies only `0x01667000..0x0166707f` and `0x01677780..0x016777ff`, not two final full 256-byte windows. No claim about uncaptured final halves follows.

Hardware-observed: the gate held for the whole watch. Every one of the 158 RAM-read requests after the gate enable was granted, and the RAM-read service checks `FUN_0069f8fc(0xf7)` on this body, so the mirror still held the host's `0xf7 = 1` at each poll. A mirror refill from storage (`0x0069f204`) would have reverted both bytes to their stored `0x00`, so no refill happened between the arm and the interruption. The absent send cannot be a lost byte: the same accessor the task's test uses demonstrably saw the host's writes.

### 13.2 Binding the kernel sleep argument

Byte-read: `0x00ed1668` calls `0x00723ce8`. The wrapper selects the current kernel through interface `+0x1618` and calls service `+0x4c` with the nonzero argument unchanged. `0x00725a1c`-`0x00725a30` publishes the service interfaces from loader header words `0x00040010` and `0x000d0010`, which hold `0x4004dae0` and `0x400ddaa0`.

Byte-read: loader A initialization `0x00040524`-`0x0004052c` installs `0x00046e48` into service `+0x4c`. That function saves the argument in `r8` (`0x46e50`), marks the current task delayed (`0x46f44`-`0x46f48`), reads the scheduler clock through descriptor `+0x40`, adds the argument (`0x46f84`-`0x46f90`) and inserts the wake deadline through `0x44c60`. It does not multiply by ten.

Byte-read: `0x44a50` initializes scheduler current time `0x4005476c = 0` and next time `0x40054770 = 10`, and publishes those two addresses at `+0x28`/`+0x2c` of the record it loads from `[0x400546d4]`. `0x43ac0` copies the per-class records at `0x400c0818 + n*0x70` into the records at `0x400545cc + n*0x84`, whose `+0x3c` and `+0x40` words the scheduler's expiry loop and the delay read as pointers. Timer service `0x44d8c`, installed at interface `+0x104`, expires due deadlines against next time, copies next time to current time at `0x44eec`, then advances next time by 10 at `0x44ef0`.

Byte-read: callback `0x40b24` calls that timer service at `0x40b34`-`0x40b38` and then increments `0x400c0800` by one at `0x40b50`. Startup `0x40b5c` registers the callback as interrupt 32, writes timer control `0xfff91008` and reload `0xfff91004 = 30000`; the initial phase uses counter `0xfff91000 / 1875`. Loader B installs delay service `0x000d6e08` at `0xd0524`-`0xd052c`; its deadline addition is `0xd6f44`-`0xd6f54`, and its timer clock advances by the same 10 at `0xd4eb0`. This is a firmware-bound ten-to-one clock relationship, not an assumed relationship between two counters. The two kernel images' timer blocks at file offsets `0xbb0`-`0xc4c` are identical apart from base immediates, including the `/1875` reciprocal multiply and `reload = 30000 = 16 * 1875`, so the ten-to-one relationship is the same code on both cores.

Byte-read: the id the task is created under carries its class in the high halfword of the service argument (`0x00725318` reads the class from the table `0x007cad9c` at `id - 1` and shifts it up; entry 35 reads 1), and the kernel resolves that class to the record `0x400545cc` (class 1; class 2 is `+0x84`, `0x40054650`). The mujin task is created by image 5's own init through the app's wrappers, and those wrappers dispatch to the calling core's kernel, the kernel whose log carries the app's own message records (E-AF-LENS-TRAFFIC-150), so the task's delay is this kernel's `+0x4c` entry against the `0x4005476c`/`0x40054770` pair.

Static-inference with the hardware-observed 99.88 Hz interrupt/log-tick rate: ten scheduler units occupy approximately 10.012 ms, so one delay argument unit occupies approximately 1.0012 ms. `1000` is approximately 1.001 s and `4000` approximately 4.005 s. Expiry is quantized to interrupts; task resumption can add scheduling latency. A perfect SI millisecond and a hard maximum wake latency are unproved because the timer's physical input clock and a scheduler response bound are not established from these bytes. Neither uncertainty supports the former ten-second delay.

### 13.3 The reachable host timing

Hardware-observed journal timings:

| Event | Monotonic nanoseconds | Relation to first tick record |
| --- | ---: | ---: |
| Successful final wait-list completes | `2042185994096056` | `-0.604766 s` |
| C0 control dispatch begins | `2042186040551286` | `-0.558310 s` |
| First tick record, 426 | `2042186598861741` | `0` |
| Arm bulk OUT dispatch begins | `2042187146451023` | `+0.547589 s` |
| Arm confirmed | `2042187433655927` | `+0.834794 s` |

Static-inference: the observed first C0 is approximately boot uptime 3.71 s, the arm approximately 4.81 s and confirm approximately 5.10 s, using the tick record as an anchor. These are interpolated host/camera times, not camera ticks captured at those events. The path from C0 dispatch to arm dispatch is 1.105899737 s, and from successful selection to arm is 1.152354967 s. The prior run's 1.181 s prepared-to-arm interval likewise exceeded the whole corrected task delay; its claimed 8.82 s margin is withdrawn.

Static-inference: the arm reached the camera after the one-shot test had run. The chain closes on hardware-observed facts: the prologue store was present at the baseline read, so `mujin` ran in this boot; the mirror still held the host's bytes at every granted poll, so the arm was live and never reverted; and the guard, the loader's async words and the statement area all stayed zero, so no sender ran. With the measured unit the test instant is 1.0 to 1.33 s after task start, and the arm landed at about 4.81 s uptime (confirmed about 5.10 s), so the task started no later than about 3.5 to 3.8 s. The host's wait list first saw the device at about 3.66 s uptime, so the window had already closed or was closing at the instant the host could first act: this run missed it by a margin the journal cannot resolve, and the current sequence cannot be justified by promptness. Unproved: the task's first-sleep tick, the earliest possible service-ready tick across boots, and whether a materially different host sequence could land inside the window. Boot uptime 4.265 s alone is not a task-start measurement; section 13.4 measures the window's position instead.

### 13.4 Next step and alternatives

**Do not repeat the RAM-arm firing run.** The cheap existence diagnostic has already happened: a four-byte RAM read of `0x016777d4` returning `0x016777d8` proves prologue entry without writing `0xa2`. Static-inference: it is unnecessary to spend another camera session merely to settle task absence.

If the owner wants the remaining timing question settled on the body, propose one closed **no-arm window measurement**, starting before power-on: C0, page-0 `0xa2 = 0` and `0xf7 = 0`, the existing reviewed gate enable and confirm, then fixed RAM reads - boot tick `0x400c0800`; the scheduler cells `0x4005476c` and `0x40054770`, twice each, host-timed; the class-1 record's pointer words `0x40054608` and `0x4005460c`; the task-35 record `0x4006067c` through `0x4006068b`; and its descriptor's first word `0x4005c3fc` - then gate restore and C4. Byte-read: the task's record is `0x4005ff0c + 34*0x38 = 0x4006067c`, its descriptor pointer at `0x40060684` and its state byte at `0x40060688`; the descriptor's own first word at `0x4005c3fc` should carry the task number 35. The service the receive wrapper calls, kernel `+0xc8` = `0x4a56c`, writes `0x12` at the current record's `+0x3` when a task blocks waiting for a message, and the timed delay writes `0x2` at `+0xc` (`0x46f48`). With no arm the task can only be inside its first sleep, with the test still ahead, or blocked in its receive, with the test already run, so `+0x3 = 0x12` closes the window for that boot and `+0xc = 2` with `+0x3` not `0x12` leaves the clock read to bound the time remaining.

The scheduler steps `0x4005476c` and `0x40054770` by ten per timer interrupt, so reading that pair twice against a host timestamp measures the delay unit on the body in seconds, independent of this report's code reading. About 1000 steps per second confirms the corrected unit and about 100 would restore the former one, and the class-1 record's `+0x3c`/`+0x40` words reading back the same pair's addresses binds the delay's own clock to it. The state byte then says whether an arming write can ever land inside the window on this body, which is the decision this route is waiting on.

Static-inference: these targets are RAM, not MMIO, and require only fixed function-2 reads. A 128-byte read beginning at `0x4006067c` contains the whole record. This read set is a proposal, not an implemented or authorized command. The session still needs the existing volatile `0xf7` write to read RAM; it is not an entirely write-free USB operation. Risk is the existing maintenance/gate lifecycle risk, with the supply-cut ending, but no AUTO_ACT arm or card script execution. Any new state-changing bytes require independent review before a first run; the owner alone decides and runs camera operations.

| Alternative | What it would require | Status and risk |
| --- | --- | --- |
| Ordinary boot plus the same USB arm | A service/config writer available in ordinary mode before the test | Static-inference: ordinary boot still creates the task and uses the same early test; the mapped vendor transport is activated only by maintenance entry. Ordinary USB does not supply the reviewed arm operation. No ordinary-boot run is proposed. |
| Persistent `0xa2 = 2` | Store the byte before the next boot | Byte-read: the mapped route commits the full `0x420000` mirror, not one byte. Blocked on mirror provenance and restore/recovery bounds. Risks adjustment/calibration damage and executing any card script on subsequent boots. Not justified for this experiment on the owner's only body. |
| Early transient writer | After mirror load, before the task test, with return to stock boot | Unproved: an accepted SD-boot/R2 hook or equivalent writer at that point. An earlier store is overwritten by mirror initialization. No available closed route establishes this timing or continuation. |
| Call the second sender later | Open cache `0x265` and reach its frontend event path, or introduce a direct sender hook | Byte-read: `0x265` is zero on this body and also changes frontend error handling. No reviewed trigger exposes a sender call. This is a different experiment with new writes and broader risk, not a timing fix. |

Static-inference: there is no demonstrated low-risk early writer on this body. Keep the USB promptness route stopped unless the no-arm state diagnostic supplies a reachable margin; if it confirms the task already receiving at first contact, close this route rather than commit calibration state or patch startup merely to obtain a `.msg` witness. If the state instead shows the first sleep still pending, the clock gives the window that remains, and a later arming attempt should arm only while a repeat of the same read still shows it, so that a closed window costs a refusal rather than a session.

### 13.5 Host reproduction

```sh
xxd private-data/sessions/20261009T160839Z-arm-auto-act/witness/before-01667000.bin
xxd private-data/sessions/20261009T160839Z-arm-auto-act/witness/before-01677700.bin
# the reply census, the two nonzero words and the poll-byte comparison
cc -O2 -Wall -Wextra private-data/analysis/x-m1-1.50/autoact-session-inspect.c -o /tmp/fuji-autoact-session-inspect && /tmp/fuji-autoact-session-inspect
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --adjust-vma=0xbf1a00 \
  --start-address=0x2e1100 --stop-address=0x2e1300 private-data/analysis/x-m1-1.50/lz/id5.elf
# Kernel byte anchors are in the pinned loader listings:
# loader-40000.dis: 0x40524..0x40530, 0x40b24..0x40c4c,
# 0x43ac0..0x43c20, 0x44a50..0x44abc, 0x44d8c..0x44f2c,
# 0x46e48..0x46fcc.
# loader-d0000.dis: 0xd0524..0xd0530, 0xd4e98..0xd4eb8,
# 0xd6f44..0xd6f58.
```

