# X-M1 1.50 timer and interrupt-controller laws (#156 gate 1)

Scope: every register the stock X-M1 1.50 kernel touches in the timer block `0xfff91000..0xfff9103f` and the two interrupt-controller banks `0xfffb3000` and `0xfffb4000`, the read/write sequences around them, what X-M1 evidence grounds, what the pinned emulator models and how, and what is unknown. Each unknown carries the least-risk experiment that would close it. This is the device-law input for the embassy time driver and interrupt glue named in [rtos-platform-options.md §4.10](rtos-platform-options.md#410-recommendation). Host-only; no guest or camera run was made for this report. Ledger: `E-XM1-TIMER-IRQ-LAWS-150`.

Evidence labels follow the ledger: **byte-read** (instruction or data bytes of the pinned 1.50 images), **static-inference** (a conclusion over byte-read facts), **hardware-observed** (captured from XM1-body-01), **source-inspected** (emulator source), **emulated** (an earlier guest run, cited by ledger id). **[INFERENCE]** marks a hypothesis without X-M1 evidence.

Listings: loader A `private-data/analysis/x-m1-1.50/decode/loader-40000.dis` (base `0x40000`), loader B `loader-d0000.dis` (base `0xd0000`; below A `0x43934` the code is A's at `+0x90000` with its own data addresses, above it B's addresses shift, for example `+0x104` A `0x44d8c`, B `0xd4d4c`), boot record 1 `boot-40048.dis`, boot record 2 `boot-50048.dis` (listing offsets), image 4 `lz/id4.dis`. Emulator: FujiMod `3d90035d` as built by `build-support/fujimod.nix`. None of the seven `build-support/fujimod-*.patch` files (the five in `fujimod.nix` plus enable-readback and scoped-watch) touches `exr2/irq.rs`, `emulator-exr2-system/src/timer.rs` or the core-release model, so the device laws below are upstream FujiMod; the run document is `emulation/fujimod-xm1/data/xm1-150.json`.

## 1. Results that decide the embassy glue

1. **The kernel timer counts up.** Within one tick the stock log field `[0xfff91004] - [0xfff91000]` falls in all 4,004 same-tick adjacent record pairs of two hardware-captured console stores from two boots, and never rises (section 6.3, **hardware-observed** bytes, **static-inference** reading). With `0xfff91004` holding the written reload 30000, `0xfff91000` rises through each 10 ms tick. Unpatched FujiMod `3d90035d` counts this mode down (section 7), so its sub-tick values are mirrored; the default FujiMod build counts up since 2026-11-03 ([adoption](xm1-timer-upcount-adoption.md)).
2. **Rate.** One reload period of 30000 counts is one kernel tick; the tick cell advances at 99.88 Hz on hardware (E-LOG-TICK-SOURCE-150, console-trace-instrument.md §2.3). One count is therefore about 0.334 µs (2.996 MHz). Four firmware-authored constants agree with a power-of-two prescaler in control bits 4..6 over one input clock (section 6.4).
3. **The kernel has no compare/alarm register in use.** The only timer interrupt the kernel uses is line 32, priority 1, which on hardware advances the tick cell once per 30000-count period without lagging the counter wrap (section 6.3, **static-inference**; U3 keeps the physical timing open). Sub-tick deadlines in the stock kernel do not exist: the kernel clock advances by 10 per tick (`+0x104`), and the µs busy-wait `0x723ffc` polls the counter.
4. **Stock IRQ handling is serialized across both cores** by one spin lock at `0x400c0804`, shared by the loader A and loader B entries, held from entry to the outermost exit and re-entrant per core (section 3). Handlers run with CPSR.I clear, so nesting is gated only by the controller's priority logic, which is not grounded on hardware.
5. **Bank selection is by executing core**, not by loader copy: CP14 `c2,c0,0` = `0x4003` selects `0xfffb3000`, anything else `0xfffb4000`. Both banks are addressable from either core: boot record 1 initialises both, record 2 programs line 15 on `0xfffb3000`, and the notifiers write a bank chosen by their argument (section 4.7).

Consequence for phase 1 (embassy executor inside a stock task, stock vectors retained): the time driver can be built on grounded laws plus the exact counter range (U1 in section 8). `now()` = 30000 × (software tick epoch) + `[0xfff91000]`, with the epoch advanced from the stock line-32 path and wrap detected by the counter falling within one epoch; alarms resolve at tick granularity (10 ms) plus optional counter polling. The interrupt glue needs no new device law if owned sources are registered through the stock `+0x160` define and `+0x158` enable slots and run under the stock entry, ack and EOI. A hardware software-pend for an interrupt-mode executor, a dedicated alarm channel, or retiring the stock vector each needs one of the unknowns in section 8.

## 2. Exception vectors

**Byte-read.** Loader A `0x40020-0x40084` (B `0xd0020-0xd0084`) stores the seven handler words of a literal table into vector-base `+0x24..+0x3c`. The vector base is 0 when `0x427b8` returns 0 (CP14 identity `0x4003`) and `0xffff0000` otherwise (literal `0x400dc`). Handlers: undefined `0x40100`, SVC `0x401b4`, prefetch abort `0x40128`, data abort `0x40148`, reserved `0x40168`, IRQ `0x40204`, FIQ `0x40184` (B: the same at `0xd0xxx`). The loader writes only the literal slots; the instruction words at `+0x00..+0x1c` come from earlier boot stages and are not re-read here.

The FIQ handler saves the frame and calls `0x40d70`, which calls the hook word `0x4004f110` if nonzero. No controller register is touched on the FIQ path, and the slot define path and both loader-internal registrations write 0 into the `+0x60` bit (section 4.3), so no stock source is shown to route to FIQ.

## 3. IRQ entry and exit (`0x40204`, B `0xd0204`)

**Byte-read**, loader A addresses; loader B is identical with nesting counter `0x400e4564`, dispatch flag `0x400e4698`, and the same lock word `0x400c0804` and IRQ stack `0x70002000`.

| Step | Sites | Effect |
| --- | --- | --- |
| 1 | `0x40204-0x40214` | CPSR := `0xd3` (SVC, I and F masked); push `{r0-r3,r12,lr,pc}` on the SVC stack; switch to IRQ mode `0xd2` |
| 2 | `0x40218-0x4022c` | Return address `lr_irq-4` and `spsr_irq` read; back to SVC with I masked and F copied from SPSR; return address stored into the frame's pc slot; SPSR pushed |
| 3 | `0x40234-0x40254` | Nesting counter `0x400545a4` incremented; on 0→1, SP switches to TCM IRQ stack `0x70002000` with the task SP pushed there |
| 4 | `0x40258-0x402bc` | Spin lock `0x400c0804`: if zero, LDREX/STREX store 17 (identity `0x4003`) or 18 (other core) via `0x427d4`, retrying on failure; if nonzero and equal to the own value, proceed (re-entry on the same core); otherwise spin with IRQ masked |
| 5 | `0x402c0-0x402c4` | `0x40d50` → `0x430a8` acknowledge (section 4.5); returns the vector or 0 |
| 6 | `0x402c8-0x402dc` | CPSR.I cleared; if the vector is nonzero it is called |
| 7 | `0x402e0-0x402f4` | I masked again; `0x40d60` → `0x431d8` end of interrupt (section 4.6) |
| 8 | `0x402f8-0x4030c` | If the counter is 1 (outermost), lock released by `0x427fc` (LDREX nonzero, STREX 0, DMB `c7,c10,5`) |
| 9 | `0x40310-0x4034c` | Counter decremented; at 0 the task SP is restored and dispatch flag `0x400546d8` is tested; if set it is cleared and control branches to scheduler `0x4375c` |
| 10 | `0x4034c-0x40364` | Saved SPSR restored with its F bit replaced by the current F; `ldm sp!,{r0-r3,r12,lr,pc}^` |

Consequences, **static-inference**: an IRQ on one core waits at step 4 while the other core is inside any IRQ handler, so stock IRQ latency on one core includes the other core's handler time. Nesting depth is bounded only by the controller and the 16 per-priority save slots at `0x40054500` (section 4.5). The context word that `+0x158/+0x15c` callers see as "IRQ context" is this counter (E-RTOS-SLOT-MANIFEST-V5-150).

## 4. Interrupt-controller banks

### 4.1 Register map as the stock code uses it

`g` = line / 32, `bit` = 1 << (line % 32), `h` = line / 16, `s` = 2 × (line % 16). All accesses are 32-bit. "Model" is FujiMod `crates/emulator-devices/src/exr2/irq.rs`, one instance per bank (`irq0` core 0 at `0xfffb3000`, `irq1` core 1 at `0xfffb4000`, both `active-bitmap` arbitration).

| Offset | Stock accesses (byte-read) | Model |
| --- | --- | --- |
| `+0x040+4g` | record 2 `0x1e68` and image 4 `0x6c3bfc` read `0xfffb3040`; image 4 tests bit 15 (line 15) | unmodeled: a read refuses |
| `+0x060+4g` | enable RMW of `bit` from table field `+4` (`0x42bd0-0x42bfc`); disable clears `bit` (`0x42f4c`); boot init writes 0; record 2 `0x1e1c` clears bank-0 bit 15 | stored configuration only |
| `+0x080+4g` | enable reads the group word, later writes `saved \| bit` (`0x42bb8`, `0x42d64`); disable writes `saved & ~bit` (`0x42fe0`); standby reads `0xfffb4080..8c`, writes `0x01000000` to `0xfffb408c` before WFI (`0x69cdc8`), restores the four words; record 1 writes `0x01000000` to `0xfffb408c` | write ORs into enable; read returns enable |
| `+0x0a0+4g` | `0xffffffff` at the start of enable and disable (`0x42bcc`, `0x42f48`), boot init and standby; `0x01000000` to `0xfffb40ac` by record 1 and after standby WFI | write clears enable bits |
| `+0x0c0+4g` | `bit` written to the bank chosen by the caller (`0x433e4`/B `0xd33e4`, from notifiers A `0x44018` and B `0xd3fd4` with line 120); record 1 writes `0x01000000` to `0xfffb40cc` | write ORs pending; on `irq1` line 120 also latches the core-1 release pulse |
| `+0x0e0+4g` | `bit` to the own bank by `0x43444` (line-120 handler) and disable; `0xffffffff` boot; `0x01000000` to `0xfffb40ec` by record 1 | write clears pending |
| `+0x100+4g` | enable RMW of `bit` from table field `+0xc`; disable clears; boot 0; record 2 `0x1e28` sets bank-0 bit 15 | stored configuration |
| `+0x120+4g` | `bit` written by EOI (`0x4327c`), enable (`0x42d4c`) and disable (`0x42f8c`); `0xffffffff` boot; record 2 `0x1e64` writes `0x8000` to bank 0 | write clears pending |
| `+0x140+4h` | 2-bit field `s`: table `+0x10` when `+0xc` = 1, else cleared; disable clears; boot 0; record 2 `0x1e40` sets bank-0 line 15's field to 2 | stored configuration |
| `+0x180+4h` | 2-bit field `s`: table `+0x10` when `+0xc` = 0, else cleared; disable clears; boot 0; record 2 `0x1e34` clears bank-0 line 15's field | stored configuration |
| `+0x1c0` | boot writes `0xffff` first; acknowledge reads it after `+0x200` and saves it per priority | read: top active line or `0xffff`; write other than `0xffff` refuses |
| `+0x1c4` | boot writes `0xffff` last | read `0xffff`; write other than `0xffff` refuses |
| `+0x1c8` | boot writes 1 | stored; other values refuse |
| `+0x200` | acknowledge reads (`0x430cc`); EOI writes 0 (`0x432f0`) | read: selects the best pending∧enabled non-active line below the active priority ceiling, pushes it active and returns its vector; with nothing selectable it returns the top active line's vector without a push, and 0 only when nothing is active; write 0 pops the top active line and re-latches asserted levels into pending |
| `+0x230+4g` | acknowledge and EOI scan groups 0..3 and take the highest set bit of the first nonzero word (`0x4310c-0x43138`, `0x43208-0x43234`) | only the top active line's bit |
| `+0x400+4·line` | enable writes the table handler (`0x42d30`); disable writes the empty handler `0x42840` (`0x42fcc`); boot writes 0; record 2 `0x1e54` writes 0 for bank-0 line 15 | stored; returned by acknowledge |
| `+0x800+4·line` | enable writes the table priority (`0x42d40`); boot writes 15; record 2 `0x1e5c` writes 12 for bank-0 line 15 | stored; values above 15 refuse; lower value = higher priority |
| `+0xc00` | boot writes 0 | stored; other values refuse |

ROM `0x80000304` would write `+0x8c/+0xac/+0x10c/+0x12c/+0x15c` from the config block, but this body's config skips that sequence (bootrom-window.md §3, `+0x54` = `0x001a0000`).

### 4.2 Bank initialisation (boot record 1 `0x23bc`, record 2 `0x1fec`)

**Byte-read.** `0x23bc(sel)` selects `0xfffb3000` for `sel = 0`, else `0xfffb4000`; record 1 calls it with 0 and then 1 (`0x26f8`, `0x2700`), so core 0 initialises both banks. Record 2's `0x1fec` has the same body and is called twice (`0x2328`, `0x2330`). Order: `+0x1c0 = 0xffff`; per group `+0xa0 = ~0`, `+0x60 = 0`, `+0xe0 = ~0`, `+0x100 = 0`, `+0x120 = ~0`; per half-group `+0x140 = 0`, `+0x180 = 0`; `+0x1c8 = 1`; per line `+0x400 = 0`, `+0x800 = 15`; `+0xc00 = 0`; `+0x1c4 = 0xffff`.

Record 2 `0x1e18-0x1e68` then programs line 15 on `0xfffb3000` without enabling it: `+0x60` bit 15 cleared, `+0x100` bit 15 set, `+0x180` field cleared, `+0x140` field := 2, vector `+0x43c` := 0, priority `+0x83c` := 12, `+0x120` := `0x8000`, then reads `+0x40`. Image 4 `0x6c3bfc` later tests bit 15 of the same `+0x40` word, which suggests `+0x40` exposes a raw or pending state of a polled source **[INFERENCE]**.

### 4.3 Handler table and define (`+0x160` → `0x4d8fc` → `0x429ac`)

**Byte-read.** The loader's table is 128 entries of 24 bytes at `0x400538fc` (B `0x400e38bc`): `+0` handler, `+4` the `+0x60` bit (0..1), `+8` priority (0..15), `+0xc` the `+0x100` bit (0..1), `+0x10` a 2-bit mode (0..3). `0x4d8fc(id, pk)` builds `{pk[1], 0, pk[2], pk[3], pk[4]}`, so the `+0x60` bit is always 0 through the slot; a NULL packet installs `{0x42840, 0, 15, 0, 1}`. Define writes no controller register.

Stock registrations seen in the loaders: line 32 `{0x40b24, 0, 1, 1, 1}` (`0x40b5c`); line 120 `{0x40b00, 0, 0, 0, 1}` (`0x40d98`, internal define). Image 4's channel driver maps channel `n` to logical id `[0x81c604 + 4n]` = 5..8, and its logical-id table `0x7c9124` gives ids 5..8 lines 32..35 with priority 12 and mode 3; the SD source line 78 is programmed with priority 12 and tuple `(0,0,0,2)` (full-system-emulation.md).

### 4.4 Enable (`+0x158` → `0x41108` → `0x42b00`) and disable (`+0x15c` → `0x41124` → `0x42eb0`)

**Byte-read**, both on the executing core's bank with CPSR.I set by `0x42574` (E-XM1-IRQ-GROUP-MASK-WINDOW-150):

- Enable: return -1 if the line is above 127 or its table handler is `0x42840`. Read `+0x80+4g`; write `~0` to `+0xa0+4g`; RMW `+0x60`, `+0x100`, `+0x180`/`+0x140`; write `+0x400+4·line` and `+0x800+4·line`; write `bit` to `+0x120+4g`; write `saved | bit` to `+0x80+4g`. The slot wrapper maps nonzero to -5.
- Disable: read `+0x80+4g`; write `~0` to `+0xa0+4g`; clear the `+0x60` bit; write `bit` to `+0xe0+4g`; clear the `+0x100` bit; write `bit` to `+0x120+4g`; clear both 2-bit fields; vector := `0x42840`; write `saved & ~bit` to `+0x80+4g`.

Both sequences disable the whole 32-line group for the duration, then restore it. They are correct under either reading of `+0x80` (write-one-to-set or plain read/write), so they do not decide which it is.

### 4.5 Acknowledge (`0x430a8`)

**Byte-read.** Read `+0x200` → `v`. If `v` is 0 or `0x42840`: increment `0x400544fc`, log through `0x41668(6)`, return 0. Otherwise scan `+0x230..+0x23c` for the line (highest set bit of the first nonzero group); if one is found, set TPIDRURW bit 31, and store `{[+0x1c0], line}` at `0x40054500 + 8·priority` (priority from the table). Return `v`. The stock code does not clear pending at acknowledge.

### 4.6 End of interrupt (`0x431d8`)

**Byte-read.** Scan `+0x230` again for the line; if found: write `bit` to `+0x120+4g`; if the nesting counter is 1, clear TPIDRURW bit 31; clear the priority slot to `{0, -1}`. Always write 0 to `+0x200`.

### 4.7 Software pend and cross-core notification

**Byte-read.** `0x433e4(sel, line)` (B `0xd33e4`) writes `bit` to `+0xc0+4g` of bank `sel ? 0xfffb4000 : 0xfffb3000`. Loader A's notifier `0x44018(x)` returns 1 for `x ≤ 1` and otherwise calls `0x433e4(x >> 1, 120)`, so it can only pend line 120 on `0xfffb4000`. Loader B's notifier `0xd3fd4(x)` (33 callers, first `0xd5030`) returns 1 for `x` = 0 or 2 and otherwise calls `0xd33e4(x >> 1, 120)`, so `x` = 1 pends line 120 on `0xfffb3000` and `x` = 3 on `0xfffb4000`. The line-120 handler `0x40b00` clears its own pending bit through `0x43444(120)` (`+0xe0`) and sets dispatch flag `0x400546d8`, which the outermost IRQ exit turns into a scheduler entry (section 3 step 9). Record 1's release (`0x1ebc-0x1ed0`) writes `0x01000000` to `0xfffb408c`, `+0xcc`, `+0xac`, `+0xec`: enable, pend, disable and clear line 120 on bank 1.

### 4.8 Standby (image 4 `0x69cd58`, task B93)

**Byte-read** (inventory §8.17): after its flag wait returns 1, the task masks IRQ (`0x42574`), cleans/invalidates and disables both caches, reads `0xfffb4080..8c`, writes `~0` to `0xfffb40a0..ac`, writes `0x01000000` to `0xfffb408c` (line 120 only), executes WFI `0x69cdcc` with CPSR.I still set, writes `0x01000000` to `0xfffb40ac`, restores the four words to `0xfffb4080..8c`, restores the IRQ bit and enables both caches. The bank is fixed to `0xfffb4000`, not selected by identity. The design relies on WFI ending while IRQ is masked and line 120 is the only enabled source; neither is grounded on hardware.

## 5. Tick registration and periodic dispatch

**Byte-read** unless marked.

- Timer start `0x40b5c` (both loaders, run once per core): zero the tick cell `0x400c0800`; define line 32 with `{_, 0x40b24, 1, 1, 1}`; on identity `0x4003` only, run the timer switch of section 6.2; then enable line 32 on the executing core's bank (`0x41108(32)`). Both cores therefore enable line 32 on their own bank (E-LOG-TICK-SOURCE-150).
- Tick callback `0x40b24` (B `0xd0b24`): calls its domain root `+0x104` (A `0x44d8c`, B `0xd4d4c`), then increments `0x400c0800` on identity `0x4003` only. `+0x104` holds the tick-root lock (A `0x400545c8`) with IRQ masked across timeout-heap work, advances the domain clock by 10, and fires expiries (manifest v3).
- Cyclic expiry `0x4beb0`: re-arms the entry at last deadline + period, releases the tick-root lock, clears CPSR.I, calls the entry's handler with its extended info, masks IRQ and re-takes the lock. Image 4's trampoline `0x72337c(exinf)` calls `[0x409c3500 + 4·exinf]`. The retained root104 run observed `0x69cc18` dispatched this way with `exinf` 64 inside a primary IRQ on TCM stack `0x700021c4` (**emulated**, E-ROOT-TASK-RETAINED-REFUSALS-150). Periodic callbacks therefore run in IRQ context with IRQ unmasked, inside the global IRQ lock of section 3.

## 6. Timer block `0xfff91000`

### 6.1 Registers the stock code touches

| Register | Accesses (byte-read) |
| --- | --- |
| `+0x00` counter | write 0 by record 1 `0x26d8` and record 2 `0x2308`; read by `0x40bb4`/`0x40be8` (÷1875), loader elapsed `0x41adc`, log writers (`0x422d8` family), image 4 `0x724044` (busy-wait), `0x7258b8`, `0x728774` |
| `+0x04` reload | write `0xffffffff` by both records, `30000 − 16·(count mod 1875)` then 30000 by `0x40c0c`/`0x40c44`; read by `0x41ad8`, log writers, `0x724010`, `0x725894`, `0x728548` |
| `+0x08` control | write `0x63` by both records (`0x26e4`, `0x2314`); 0 then `0x23` by `0x40c08`/`0x40c3c` |
| `+0x10·n + 0..0xc` | record 2 `0x270c` (n = 1 or 2): reload `~0`, control `0x53`, poll counter up to `375·units`, control 0, reload 0, write 1 to `+0xc`; image 4 `0x655e..` (n from its caller, table covers 0..3): counter 0, reload, optional IRQ registration (line `32 + n`), control `3 \| p<<4`; stop writes control 0 and reload 0 |

The channel record 2 uses is `1` on identity `0x4003` and `2` otherwise (`0x35d8`), so channels 1 and 2 are per-core polled delays.

### 6.2 Sequences

- Boot (records 1 and 2): counter 0, reload `0xffffffff`, control `0x63`. The counter then runs from zero; `0x40b5c` treats `count/1875` as elapsed ticks since boot.
- Kernel switch (`0x40bb0-0x40c44`, identity `0x4003` only): read the counter at `0x40bb4` for the remainder `r = count mod 1875`; read it again at `0x40be8` and store `count/1875` in the tick cell; control := 0; reload := `30000 − 16·r`; 100-iteration delay loop; control := `0x23`; reload := 30000. **Static-inference:** the first period is the remainder of the boot-time 10 ms grid, so tick boundaries continue the boot count's phase. Whether the counter restarts at 0 on the `0x23` write and when a reload write takes effect are not shown by the bytes (section 8, U2).

### 6.3 Counting direction (hardware-observed, new)

The console store's `+2` halfword of record types `0x0f`, `0x72`, `0x71` and `0x81` is the low 16 bits of `[0xfff91004] − [0xfff91000]` at record time, and the tick word is `[0x400c0800]` (console-trace-instrument.md §2.2-2.3, byte-read). `private-data/scratch/DeviceLaws/subtick-direction.sh` pairs adjacent occupied slots, breaking pairs at type `0x09`, zero slots and the header's next index, and counts how the halfword moves. Two independent reviews found no required change; the outputs below are its single run per capture (`subtick-<session>.txt` beside the script). The first reviewer's own check of the earlier revision, written only to `/tmp`, gave the same first-capture counts.

| Capture (hardware-observed, read-log windows) | Records | Ticks | Range of `+2` | Same-tick pairs up / down | Same-thread subset up / down | Next-tick pairs up / down |
| --- | --- | --- | --- | --- | --- | --- |
| `20261009T064945Z-read-log` | 1215 | 21..74361 | 6..29881 | 0 / 942 | 0 / 416 | 110 / 12 |
| `20261008T024241Z-read-log` | 4069 | 21..12288 | 1..29924 | 0 / 3062 | 0 / 1262 | 515 / 91 |

No value exceeds 30000, so none comes from the boot `0x63` mode with reload `0xffffffff`. No same-tick pair rises, and none rises by 15000 or more, so the tick cell is never seen lagging a counter wrap. **Static-inference:** `[0xfff91004] − [0xfff91000]` falls through each tick; with `+0x04` reading back the written 30000, the counter rises from near 0 towards 30000 within a tick, wraps at the tick boundary, and the line-32 IRQ that advances the tick cell is taken before any later record is written. Four stock readers depend on the direction:

| Reader | Treats `reload − count` as | Under the hardware direction |
| --- | --- | --- |
| `0x723ffc` µs busy-wait (59 call sites, e.g. `0x69e23c`) | falling: progress = previous − current, wrap adds `reload + 1` | correct |
| record 2 `0x270c` channel delay (`0x53`) | waits while `target > count` after reload `~0` | terminates only on a rising counter; the hardware direction is observed on the kernel channel in mode `0x23`, so channels 1/2 in `0x53` rising is **static-inference** from this loop |
| loader `0x41ad4` diagnostic word, log `+2`, `0x728770` (`30000 − count`) | stored values only | they record time remaining in the tick |
| `0x72585c` (one caller `0x8911e0`) | `10·clock + (reload − count)·10/reload` | the sub-tick term falls within a tick [INFERENCE: intent unknown] |

### 6.4 Rate and prescaler

**Hardware-observed:** the tick cell advances at 99.88 Hz (14,958 ticks in 149.759 s). **Static-inference:** if one line-32 IRQ ends each 30000-count period, the counter runs at about 2.996 MHz. Firmware constants that are consistent with one input clock divided by `2^((control >> 4) & 7)`: `0x63` (÷64) with 1875 counts per 10 ms; `0x23` (÷4) with 30000 counts per 10 ms (`30000 = 16 × 1875`, and 16 = 2^(6−2)); `0x53` (÷32) with 375 counts per unit at `0x270c`; `0x723ffc` converting µs with `10,000,000 / reload` (3 counts per µs). These imply a 12 MHz input and 1 ms units at `0x270c` **[INFERENCE]**: only the 0x23 rate is hardware-observed, and only through the tick rate.

## 7. Emulator models (source-inspected)

| Law | FujiMod `3d90035d` | Against X-M1 evidence |
| --- | --- | --- |
| Bank instances | two controllers, one per core, 4 KiB windows, any core may access either | consistent with sections 4.2 and 4.7 |
| Enable/pending aliases | `+0x80` OR, `+0xa0` clear, `+0xc0` OR, `+0xe0` and `+0x120` both clear | consistent with every stock sequence; not grounded |
| Acknowledge | `+0x200` read selects, pushes active and returns the vector; with nothing selectable it returns the top active vector without a push; pending is untouched | consistent with stock clearing pending at EOI; not grounded |
| EOI | `+0x200` write 0 pops one active level and re-latches levels | consistent; not grounded |
| Arbitration | lower priority value wins; ties by descending bit in ascending group (`active-bitmap`); nesting only strictly above the active ceiling; at most 16 levels | not grounded; stock gives the tick priority 1 and line 120 priority 0 under a default of 15, which fits lower-is-higher [INFERENCE] |
| `+0x40` family | unmodeled | record 2 `0x1e68` and image 4 `0x6c3bfc` read it; those paths would refuse |
| `+0x1c0` | top active line | stock saves it per priority; one Run-3 hardware slot conflicts with the model ([audit §2.2](emulator-device-fidelity-audit.md#22-interrupt-controller-irq0-0xfffb3000-irq1-0xfffb4000)) |
| Source kinds | edge pulse or level; level re-latched at EOI | stock mode fields `+0x100/+0x140/+0x180` are stored but drive nothing |
| CPU IRQ pin and WFI | pin = selection result; WFI ends when the pin rises, whatever CPSR.I | matches what standby needs; not grounded |
| Release | `irq1` line 120 software write latches the core-1 release pulse | declared model of record 1's write; E-PEER-MODEL-OBSERVATION-SOURCE-150 |
| Kernel timer mode `0x63` | counts up modulo `reload + 1`, no IRQ | direction consistent with `count/1875`; IRQ behaviour unknown |
| Kernel timer mode `0x23` | unpatched: **counts down** from the loaded value to 1 (0 is never read), then reloads; default since 2026-11-03: the register reads `reload − remaining`; edge on line 32 to both cores per expiry | unpatched **direction contradicts section 6.3**, default agrees; period and fan-out unproved |
| Divider | `2^((control >> 4) & 7)` virtual ticks per count | consistent with section 6.4 constants |
| Channels `+0x10..+0x2c` | channels 1 and 2: polled up-counters (`0x23`/`0x53` after reload `~0`) or finite-reload application mode raising line `0x20 + n` on cores whose bank enables it; acknowledge `+0xc` | consistent with `0x270c`; not grounded |
| Channel `+0x30..+0x38` | application timer only; a `+0x3c` write refuses | image 4's driver would program channel 3 like the others; not traced |

Effects of the direction mismatch on the pinned model before the up-count patch, **static-inference** from the model source, not run: in the model `reload − count` rises, so `0x723ffc` takes the wrap branch on the first observed change and returns after about one counter step instead of the requested time; sub-tick values recorded by loader and log writers run the opposite way within the period from hardware. Existing emulated comparisons that bind these values (E-LOADER-ORCHESTRATION-NATURAL-QUALIFIED-150) compare model against model and are unaffected as comparisons.

The retained engine `emulation/xm1-upstream` (timer in `emulator/interrupts.py`) still carries the gating `verify-candidate` routes (`emulation/VerifyCandidate.hs:89-94`). It counts mode `0x23` down as well (`interrupts.py:343,690-695`), records the same 12 MHz/prescaler inference and needs the `kernel-active-scan-order` tie fixture for equal priorities. The FujiMod fix is the default model since 2026-11-03; its adoption assessment and the retained engine's status are in [xm1-timer-upcount-adoption.md](xm1-timer-upcount-adoption.md).

## 8. Unknowns and least-risk experiments

Camera items are designs only and need the owner. "Read-only RAM" means the existing ordinary `read-ram`/`read-log` operations, which read RAM and change no device state. Reading controller registers needs a RAM-push routine; `+0x200` must never be read by an observer, because a read acknowledges.

| # | Unknown | Why it matters | Least-risk experiment |
| --- | --- | --- | --- |
| U1 | Whether `0xfff91004` reads back the written reload, and the counter's exact range (0..29999 or 1..30000) | `now()` arithmetic | Camera, RAM-push, read-only: with IRQ masked, sample `+0x04` once and `+0x00` 4096 times into a RAM buffer; report min, max and the wrap step. No timer write |
| U2 | Whether the `0x23` write restarts the counter at 0 and when a reload write takes effect (immediately or at wrap) | first-period phase; any owned reprogramming | Static first: no loader code reprograms the running kernel timer after `0x40c44`; trace whether image 4's channel driver `0x655e..` is ever called with channel 0. Phase 1 never writes the timer. If an owned driver must, design a RAM-push test on a channel with no live stock user |
| U3 | Physical input clock and whether one line-32 IRQ ends each period | µs conversions; alarm accuracy | Camera, read-only RAM: sample `0x400c0800` and B's domain clock at host-timed intervals over a longer baseline (existing read-log method). The 99.88 Hz figure already fixes the tick |
| U4 | Line-32 fan-out to both banks | B's timeouts; per-domain time drivers | Camera, read-only RAM: two samples, seconds apart, of the domain-B kernel clock that `0xd4d4c` advances by 10 per B tick (inside tick root `0x400e4588`; the exact word is read from `0xd4d4c` before the run). It moves only if core 1 takes line 32 |
| U5 | Control bits 0, 1, 2 and 7 and whether `0x63` raises line 32 | owned timer programming | Static: no further X-M1 source. Only needed if the platform reprograms the timer |
| U6 | Priority sense, preemption gating, equal-priority order | nesting correctness of owned handlers under stock entry | Camera, read-only RAM: repeated snapshots of `0x40054500..0x4005457f` and `0x400545a4`; a snapshot with count ≥ 2 shows which priority slots are simultaneously occupied. Emulator cannot decide this |
| U7 | `+0x80` write-one-to-set or plain; `+0xe0` versus `+0x120` | owned glue that bypasses stock sequences | Avoid: reuse the stock sequences of section 4.4. If needed, RAM-push readback of `+0x80` after a stock enable of an already-masked group |
| U8 | Mode fields `+0x60`, `+0x100`, `+0x140`, `+0x180` (edge/level/polarity) | owned sources with level devices | Static: tabulate every image-4 define packet and the device each serves; then emulator cases per mode |
| U9 | `+0x40` family, `+0x1c0`/`+0x1c4`, `+0x1c8`, `+0xc00` | completeness of a retired-vector platform | Static: trace record 2's use of the `+0x40` read at `0x1e68`, `0x6c3bfc`'s caller (both line 15 on bank 0), and the readers of `0x40054500` (`0x434a4` copies all 16 slots) |
| U10 | Software pend of lines other than 120 | interrupt-mode executor pender | Emulator only models it. Camera RAM-push: define a handler on a line with no stock source, pend it on the own bank, record the handler's entry count in RAM, disable it |
| U11 | WFI wake with CPSR.I set and a disabled-but-pending source | standby backend; idle | Camera, read-only RAM: B93's wait/restore counters after a standby cycle; the producer of B93's flag is still open (E-INITIAL-TASK-BODIES-SOURCE-150) |
| U12 | Exclusive-monitor behaviour of the shared lock `0x400c0804` across cores | correctness of serialization | Covered by the exclusive-monitor gate (manifest draft); no timer/IRQ-specific experiment |
| U13 | FujiMod `0x23` direction | emulated sub-tick and busy-wait timing | Emulator model change upstream of runs (proposed issue below); no camera needed |
| U14 | Instructions executed per `0x23` counter step on each core with the other core runnable (FujiMod declares 24 per board tick shared by all runnable cores, 96 per step for one) | instruction cost of every µs busy-wait and device delay in emulated routes; not cross-core ordering, which `quantum` sets | Camera, RAM-push, with U1's sampler: fixed dependent and independent A32 sequences of K = 64 and 1024 and warmed cached loads between counter reads, IRQ masked with CPSR read back, spans inside one period, placement, cache and ACTLR state and core identity recorded, peer core parked or running; optionally the performance monitor around each span ([audit §4.1](emulator-device-fidelity-audit.md#41-proposed-combined-camera-session-fixes-3-4-and-6-with-u1-u6-and-u14) part D) |

## 9. Proposed issue text

Title: Fix the EXR2 kernel-timer direction in the emulator model

Two hardware console captures show `[0xfff91004] − [0xfff91000]` falling within every tick (4,004 same-tick pairs, none rising; E-XM1-TIMER-IRQ-LAWS-150). With `0xfff91004` reading back the written reload (U1 of reports/xm1-timer-interrupt-laws.md), the X-M1 kernel timer counts up in control mode `0x23`. FujiMod `3d90035d` `emulator-exr2-system/src/timer.rs` counts that mode down from the reload to 1. Under the model the stock µs busy-wait `0x723ffc` returns after about one counter step, and every recorded sub-tick value runs the opposite way. Change: count up in `0x23` with the period equal to the reload, keep the line-32 edge per wrap, and keep the first period equal to the reload value written before start; take the exact range (0..reload−1 or 1..reload) from U1 once observed and declare it as a model assumption until then. Acceptance: a synthetic model test where `reload − count` falls within one period; one reviewed guest run that records `0x723ffc` iteration counts before and after the change; existing emulated comparisons re-baselined where they bind sub-tick values.
