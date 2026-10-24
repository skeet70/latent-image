# Diagnostic SD-BOOT card, first payload and observable outcomes

Delivered host-side, never run: the card builder `//host/firmware:build-boot-card`, the first bounded payload `host/firmware/diagnostic-halt.S`, the built image with its hashes, the payload write audit, the specification of the reporting payload that would make a run observable, and the per-outcome table. Labels: **byte-read** names ROM instructions or data, **static-inference** names a deduction, **hardware-observed** names an existing reviewed capture of this body, **unproved** names what nothing has yet established. Nothing here was carried on a card, inserted into a camera or executed; no camera or USB device was touched.

## 1. What the card establishes if it is never run

It establishes that the factory SD route's prerequisites are constructible from held material without the unobtained service package: the stock two-half config concatenated into a 1024-byte `ffsys.bin`, a project-authored 512-byte `ffboot.bin` and a FAT volume that the ROM's own parser accepts. It bounds the payload: one sector, one 16-byte store at RAM `0x100`, no MMIO, no media write, no call, and a halt that matches the ROM's own halt. It gives a future payload a verified staging target and a reserved report sector to write to.

This report describes the original halt control, not the later blink variant. The owner has now attempted the card-present blink variant and reported darkness without an execution witness, followed by normal-operation restoration and preparation/readback PASS ([current physical observations](sd-boot-card-entry.md#29-physical-observations-and-current-verdict)). The three MBR-derived configuration effects and selected mode remain unbound. SD boot is parked off the RC release path; a qualified passive bus capture is the optional discriminating follow-up ([route decision](sd-boot-card-entry.md#48-route-decision-and-discriminating-follow-up)). Neither the original RAM signature nor the negative blink result accepts R2.

## 2. The tool

`devenv shell -- bazel run //host/firmware:build-boot-card -- build PINNED_DAT PAYLOAD_BIN PRIVATE_DATA_SUBDIRECTORY` builds and verifies; the `verify` subcommand re-checks an existing image. Inputs are the pinned official `FWUP0002.DAT` (SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, checked as a hash before use and parsed with the container contract's own checks) and one 512-byte payload file. The tool refuses a container that fails its checks, a payload that is not one sector, a relative path outside the workspace, an output directory that exists, and any image that fails its own ROM-model verification.

`build-stage-marker POSITION` and `verify-stage-marker POSITION` otherwise take the same arguments and add an LED marker to the stock configuration; both set `+0x10` = `0x302`, whose bit 1 makes the `ffboot` failure teardown write 1 to `0xfff88084`. `after-apply` appends `(0xfff88084,1)`, `(0xfff893c4,0x00050001)` and `(0xfff893c0,0x00010000)` to the `+0x348` pair table, which runs last in the apply, after the `+0x33c` SD-divider step and before `ffboot.bin` is opened. `before-memory-wait` enables `0xfff88084` in optional slot `+0xb8` (`0x80000510`, after the `0xfff88080`, `0x80000b50` and `0xfff88038` waits and the `0xfff99000` register writes) and sets up and lights the LED in slots `+0xc8`/`+0xd0` (`0x80000600`-`0x80000628`, after the `0xfff01000` register writes, before the `0xfff01090` wait); everything between those two points is straight-line or cycle-bounded. Slot `+0xb8` also enables the `+0x158` delay of `0x0a010200` CPU cycles before the LED lights, so watch at least 30 s. A cold start that stays dark stopped before the marker; a lit LED that never enters the payload's pattern means the boot passed the marker but the payload did not run. Images: `stage-marker-blink-card/` (`after-apply`, SHA-256 `a53eb1664645821bc6dbbd0456b1457b830a65d30e7a6346e3bd17d99f429e52`) and `memory-marker-blink-card/` (`before-memory-wait`, SHA-256 `a1792d566b5547de6aabe916bae86a6ff832b4750ccfc9b5021281fbc68eaf87`), both under `private-data/analysis/x-m1-1.50/bootrom/` with the startup-restored blink payload. On this body both cards stay dark on cold starts and reach the blink payload under fast power toggling ([entry timing](sd-boot-card-entry.md#261-entry-timing-and-stage-markers), E-SDBOOT-STAGE-MARKER-150).

A leading `--power-hold` applies to every subcommand and sets `+0x10` bit 0 in the configuration both ROM routes read, so the apply's store at `0x80000330` keeps the 1 the ROM wrote to `0xfff88084` at `0x800000ec` through the `ffboot.bin` read and handoff. On a stage-marker configuration it gives `+0x10` = `0x303`. Without the option every image is byte-identical to before; `verify` and `verify-stage-marker` still accept the startup-restored (`4f1c632c…`), after-apply (`a53eb166…`) and before-memory-wait (`a1792d56…`) images. The built power-hold card and the reason for it are in [card-entry section 2.62](sd-boot-card-entry.md#262-the-fffw-unbricking-note-and-the-power-hold).

Current candidate: `private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card/`. It supersedes the FAT-only image, which the current verifier rejects.

| File | Size | SHA-256 |
| --- | --- | --- |
| `ffsys.bin` | 1024 | `79859bdd17234f2e09caa6f229008f6b32bb7238fc10dac499c5e4b4fa4e0f0b` |
| `ffboot.bin` | 512 | `ddeb66c4db346dcc1f00c7871e071ddb8ccdf9bff60157cdc77fd3d00e054a20` |
| `diagnostic-sd.img` | 17826304 | `665b2066bafe86e941e2890bd61843b69dc07796d8009ad9e872651278da2b46` |

`ffsys.bin` is a byte-read match to the two documented spans: extracting `payload.bin[0x48,0x248)` and `[0x848,0xa48)` with `xxd` and concatenating them gives the same SHA-256 as the tool's output, and the tool derives the same bytes independently from the DAT's complemented main range (`B.map complement` of `DAT[0x214..]`). Both derivations were run; they agree.

Static-inference for the card as a whole: a card written from this image is not a repair medium and carries no writer. It is the read-only half of the diagnostic design plus the payload that halts.

## 3. The image and how it was verified against the ROM's parser

The FAT16 volume starts at LBA 2048 and declares 32768 sectors, 512-byte sectors, one reserved sector, two 128-sector FATs, 512 root entries and one sector per cluster. `FFSYS   BIN` occupies clusters 2 and 3; `FFBOOT  BIN` occupies cluster 4. LBA 34816 is the reserved report sector, currently unused.

Raw LBAs 0 and 1 contain the stock 1024-byte configuration with only the mode-2 sector count and mandatory MBR type/start/signature fields changed. LBA 2 contains the same 512-byte halt payload as the FAT file. Mode 2 reads config+`0x18`, at SRAM `0xfff0001c`, masks it to 18 bits and stages exactly one sector from LBA 2. Mode 1 still reads the unchanged stock `ffsys.bin`.

The ROM ignores MBR CHS and partition-length fields. These and the other partition entries retain stock configuration bytes; this is not a conventional host-mountable partition table. File copying or formatting does not preserve the raw-mode overlay. Any future preparation must write the whole verified image to an expendable card and verify its readback before insertion.

The raw overlay changes five stock bytes and four configuration words. The count change at +`0x18` is not consumed by configuration apply. Three applied register words remain different:

| Register | Stock word | Raw-mode word |
| --- | --- | --- |
| `0xfff010e0` | `0x00000000` | `0x00040000` |
| `0xfff010e4` | `0x00000000` | `0x08000000` |
| `0xfff0111c` | `0x01000101` | `0xaa550101` |

Their target-specific effects are unproved. Matching all other configuration words and staging a halt payload does not establish that those writes are harmless.

Verification is by parsing the emitted bytes the way the ROM parses them, implemented from the decode of `0x80001ae0`-`0x80002090` rather than by a filesystem library:

- `mountRom` reproduces `0x80001ae0` (read 510 bytes at the sector, require `55 aa` at `+510`, require `FA` at `+54` or `+82`, otherwise take the first MBR entry at `+0x1be`, require a nonzero type byte at `+4` and use its start LBA at `+8`), `0x80001ba0` (read 36 bytes at BPB `+13` into the context: sectors per cluster at `+13`, reserved at `+14`, FAT count at `+16`, root entries at `+17`, total sectors at `+19` or `+32`, sectors per FAT at `+22` or `+36`; data clusters `N = (total - reserved - fats * sectorsPerFat - rootEntries / 16) / sectorsPerCluster`, then `N + 2 < 0xff7` is FAT12, `< 0xfff7` is FAT16, else FAT32; FAT32 root cluster from BPB `+44`) and `0x80001dc4` (walk the 32-byte root or FAT32 root chain, skip entries with attribute bit 3, refuse entries with bit 4, compare all 11 short-name bytes).
- `nextCluster` follows the chain with the ROM's own FAT widths and end-of-chain values, `chainBytes` refuses a short or cyclic chain, and `readRomFile` returns the bytes the ROM's read helper would return for the directory-declared size.
- `readRawRom` models the two raw configuration sectors and the masked sector count; `verifyCard` requires the exact stock-derived overlay and the same one-sector payload through both loading paths.

The CLI build and verify runs report the four configuration-word differences above. The six FAT metadata locations remain those of the existing volume: boot sector 2048, FATs 2049 and 2177, root 2305, configuration 2337–2338 and payload 2339. The raw payload is duplicated at LBA 2.

## 4. The payload and its write audit

`ffboot.bin` carries no header, so byte 0 is the entry the handoff reaches after the ROM stages the file to `0x40000000` (the alias of RAM 0) and jumps to `0`. The payload is 15 A32 instructions in 60 bytes, zero-padded to 512 to match the ROM's sector-rounded DMA destination span.

| Offset | Instruction | Effect |
| --- | --- | --- |
| `0x00` | `cpsid if` | masks IRQ and FIQ; the ROM already leaves them masked in SVC mode, so this is idempotent |
| `0x04` | `movw sp, #0x200` | defines a stack at `0x200`, one byte past the one-sector staged span at `0x40000000`; nothing pushes or calls, so no stack location is written |
| `0x08` | `movw r0, #0x100` | base register for the one store |
| `0x0c`-`0x28` | four `movw`/`movt` pairs | builds `0x44314d58`, `0x544f4f42`, `0x00000150`, `0x00000001` |
| `0x2c` | `stmia r0, {r1-r4}` | the single memory write: 16 bytes at `0x00000100`-`0x0000010f` |
| `0x30` | `dsb sy` | orders the store before the halt |
| `0x34` | `wfi` | the ROM's own halt instruction (`0x80000020`) |
| `0x38` | `b .` | the ROM's own self-loop (`0x80000024`) |

Write audit, static-inference over the pinned bytes with the host checks in section 7 as the executable form:

- Total memory writes: one instruction, 16 bytes, at RAM `0x00000100`-`0x0000010f`. That address is inside the payload's own 512-byte staged span at `0x40000000`, so it overwrites payload bytes, not the ROM's SRAM, not the config buffer at `0xfff00004`, not the FAT context at `0xfff00404`, not the SD state at `0xfff00434` and not the sector buffer at `0xfff00448`.
- Signature content: `58 4d 31 44` "XM1D", `42 4f 4f 54` "BOOT", `50 01 00 00` (the 1.50 version word) and `01 00 00 00` (payload revision 1). A later capture of RAM `0x100` while the body is still halted would read `XM1DBOOT` followed by those two words.
- No MMIO access, no CP15 access, no supervisor call, no load instruction, no call, no push or pop, no cache or TLB maintenance. The read set is the payload's own instruction fetches at `0x00000000`-`0x0000003b`.
- The payload never returns: it writes the signature, orders it, halts and self-loops, so nothing downstream runs and no camera state depends on its registers.
- `wfi` with IRQ and FIQ masked leaves the core halted until a reset; should an unmaskable event release it, the next instruction is the self-loop.

## 5. What the owner would see, outcome by outcome

The ROM has no write path to the card, so **the card's own contents are the same after every outcome**; only the ROM's execution path and RAM change. The two channels that exist are the halt behaviour and the ROM's `0xfff88084` status bit, and the second one does not separate these outcomes: the init sets bit 0 to 1 at reset (`0x800000ec`), the config apply overwrites the whole register with `config+0x10` bit 0, which is 0 for the stock config (`0x80000328`-`0x80000330`), and the mode-1 failure path also writes 0 (`0x80000858`-`0x8000085c`). No X-M1 binding from that register to a visible indicator is established, so it is reported as a register value, not as an LED prediction.

| Outcome | What the ROM does (byte-read) | What is visible from outside | What it does not prove |
| --- | --- | --- | --- |
| Entry refused; the application boots | the mode selection at `0x80000274`/`0x80000278` never presents the mode-1 strap pair, so the SD handler never runs and the card is never read | a normal boot: LCD UI, ordinary key and shutter response, USB enumerates when connected; the card's contents are irrelevant | nothing about the card. This is what every prior attempt produced (E-SD-BOOT-ENTRY-HOLD-150), and it is the only outcome that is positively identifiable |
| Entry taken; the payload runs | mount, `ffsys.bin` read to `0xfff00004`, apply `0x80000304` returns 0, `ffboot.bin` opened, 512 bytes staged, handoff writes 2 to `0xfff98008` and jumps to 0; the payload masks interrupts, sets SP, writes the signature, halts and self-loops | dark body, no LCD image, no USB enumeration, no response to any control, indefinitely until the battery is pulled; `0xfff88084` = 0 | nothing distinguishable from the two rows below by any channel available today. The signature at RAM `0x100` is the only positive evidence, and no held route can read it after a halt, because the jig/service read path needs the running application, which this outcome prevents; a reboot does not preserve it either, since the next boot stages its first block over RAM `0x00000000`-`0x0003ffff` before the handoff ([detection trace](boot-card-detection.md), E-SDBOOT-RAM-SIGNATURE-150) |
| Entry taken; the config apply hangs | the apply writes `0xfff88084` = 0 at `0x80000330` and then polls with no software timeout: `0xfff88080` bit 16, `0xfff88038` bit 24, `0xfff01090` bit 28, the optional `0xfffb3000` sequence and the pair table | dark body, halted indefinitely, no signature written because the payload never runs; `0xfff88084` = 0 | the same as the row above; also nothing here identifies which wait hung. Least likely of the dark rows, because the stock config is byte-identical in effect to the apply this body survives at every mode-4 boot, but mode 1 enters without the mode-4 controller initialization, so it is not excluded (E-CONFIG-APPLY-150) |
| Entry taken; a file or the volume is unreadable | the mount retries four times, the `ffsys.bin` open and read retry four times, exhaustion writes 0 to `0xfff88084` and returns -1, and the dispatch halts at `0x80000020` with no fall-through | dark body, halted indefinitely; `0xfff88084` = 0 | the same as the rows above. The retry loops make this row slower to reach the dark state than an immediate hang, but a dark screen gives the owner no reliable timing |
| Some other outcome | an actuation that moved the strap pair off mode 4 sends the boot into mode 0, 2, 3, 5, 6 or 7; modes 0, 3, 5 and 6 need no card, mode 7 waits for its receiver, and any of them may also fail or halt | dark body, or a normal boot, depending on the mode | a dark body does not identify mode 1: modes 3, 5, 6 and 7 can also end without the application. Only a normal boot is decisive in the other direction |

The honest summary: the card converts an unobservable attempt into a two-way signal, normal boot against dark body, where dark means some ROM mode was entered and did not hand off to the application. It cannot, by itself, distinguish payload execution from a config hang or a failed read, and it cannot prove payload execution at all. The reserved report sector exists so that a reporting payload can close that gap by leaving bytes the owner can read on a host afterwards.

**Host follow-up (2026-10-10):** [the card-entry review](sd-boot-card-entry.md) re-verified these exact bytes and specifies the reporting payload, the outcome table and the mode-2 safety envelope required before an attempt. It also corrects two details here: the optional SD-clock field `+0x33c` reads `0x80000000`, so the helper at `0x800010ac` is called with selector 0 and rewrites the channel-0 divider field rather than leaving the init's `0x500` in place; and the payload's `movw sp,#0x200` places the stack pointer one byte past the one-sector staged span, not inside it.

## 6. SD write feasibility, inherited state and recommendation

**Static-inference, not a hardware write binding:** a successful mode-1 handoff leaves a selected SD card and an enabled SDHCI-shaped host controller, so a one-block write should be able to reuse the state that just read the payload. The ROM provides no writer. Its read path plus the SDHCI field map gives a CMD24 candidate, not executable bytes approved for this body. **Unproved and blocking:** the controller's transmit behavior, completion/busy semantics and actual post-config SD clock frequency. No helper is implemented here, and no camera run is authorized.

**Corrections to the earlier section 6:** the claim that the reserved LBA means “a wrong write cannot damage the FAT, the root directory or either file” is withdrawn. Only a correctly addressed write is outside those structures; a bad address or addressing-unit bug can overwrite them. The earlier requirement to derive the command “by experiment or by emulation against a modeled controller” did not establish hardware behavior. A controller model exercises assumptions supplied by the project; it cannot prove those assumptions about silicon. The prior start-token/CRC16/CMD59 description is also superseded: this is native SD through a host controller, not a demonstrated SPI interface.

### 6.1 What the ROM leaves at a successful handoff

**Byte-read sources:** the captured ROM artifact and `private-data/analysis/x-m1-1.50/bootrom/bootrom.dis`, particularly `0x800010ac`-`0x80001674`, the command table at `+0x22e0`-`+0x232f`, mode 1 at `0x8000082c`-`0x80000934`, the sector dispatcher at `0x80001ffc`-`0x80002090` and handoff at `0x80002298`-`0x800022c0`. The config is the pinned `ffsys.bin` in section 2. Register meanings below are **static-inference** where they use the [Linux SDHCI register definitions](https://github.com/torvalds/linux/blob/master/drivers/mmc/host/sdhci.h), read 2026-10-09, rather than just naming the loads and stores.

| Item | What the ROM bytes establish | Meaning and limit |
| --- | --- | --- |
| Channel and pins | **Byte-read:** mode-1 mount calls `0x80001ff4` → `0x80001308(0)`; channel 0 uses `0xfff50000`, writes `0xf000a000` to `0xfff8930c`, `0x80000` to `0xfff88028` and `0x200` to `0xfff88030` | **Static-inference:** card-slot pin/clock bring-up. Mode 2 uses channel 1 at `0xfff51000`, different pins and clocks; a channel-0 helper is not a mode-2 helper |
| Reset and presence | **Byte-read:** `0x8000155c` writes byte 1 to host `+0x2f`, waits for bit 0 to clear, waits for `+0x24` bit 17 and refuses without bit 16 | **Static-inference:** full host reset, stable card detect and card present |
| Power and timeout | **Byte-read:** `+0x29` is ORed with `0x0f`; `+0x2e` low nibble becomes `0xa`; `+0x34` and `+0x36` get halfword `0x1fff` | **Static-inference:** native-SD power/voltage selection, data timeout and status enables. An absolute timeout duration is **unproved** |
| Bus width | **Byte-read:** `+0x28` is read, bits `0x22` cleared, bit `0x02` set; the last init record is CMD6 argument 2 immediately after CMD55 | **Static-inference:** four-bit host bus and ACMD6 four-bit card bus, not CMD6 high-speed switching. No later SD-host width write appears before payload entry |
| Initial SD clock | **Byte-read:** channel 0 passes `0x440` to `0x800010ac`; that helper disables bit 2, preserves low six bits, installs the selector, sets bit 0, waits for bit 1, then sets bit 2 | **Static-inference:** internal clock enable/stable and SD-clock output enable. Selector encoding is not a measured frequency |
| Final SD clock | **Byte-read:** after all ten init records, channel 0 passes `0x500` to the same helper; channel 1 passes `0x100` | **Static-inference:** channel-0 selector bits `[15:6]` become `0x500`; enable/stable bits 0, 1 and 2 are set on successful return. No hertz value is established; controller version, source-clock rate and post-config clock-tree effect are **unproved and blocking** for a live writer |
| RCA, OCR and addressing | **Byte-read:** SRAM `0xfff00434+0x00` holds CMD3 response with low 16 bits cleared; `+0x04` holds ACMD41 OCR; `+0x08` holds HCS `0x40000000` after accepted CMD8 or 0 for the legacy path; `+0x0c` holds OCR bit 30; `+0x10` becomes 1 only at init completion | **Static-inference:** selected card's RCA, OCR, SD-v2 negotiation and CCS. The ready word is a software completion marker, not a fresh card-state observation |
| Last transfer | **Byte-read:** the payload destination `0x40000000` selects DMA read `0x8000148c`; `+0x00` gets the RAM address, `+0x04=0x200`, `+0x06=1`, `+0x08` gets the scaled LBA, `+0x0c=0x11`, `+0x0e=0x113a`; status 1 is acknowledged, status 2 awaited and bits `0x0a` acknowledged | **Static-inference:** successful CMD17 single-block DMA completion before entry. The current one-sector payload's final LBA is 2339, argument `0x923` with CCS or `0x124600` without CCS |
| Successful exit | **Byte-read:** mode 1 returns 0 after consuming the file; no CMD0, deselect, controller reset or SD-power-off call occurs on success; handoff stores 2 to `0xfff98008` and jumps to RAM 0 | **Static-inference:** no normal-path teardown discards the selected card or controller state. Failure-path teardown is not the successful handoff |

**Byte-read:** the ten init records start with the zero CMD0 record at ROM `+0x22e0`, then CMD8/`0x1aa`, CMD55/0, ACMD41/`0x00100000` OR HCS, CMD2/0, CMD3/0, CMD7/saved RCA, CMD16/`0x200`, CMD55/saved RCA and ACMD6/2. ACMD41 repeats CMD55/ACMD41 until OCR bit 31 is set or the PMU-derived budget expires. This corrects the window-map shorthand that places CMD0 at `+0x22e8` and gives ACMD41 argument `0x10`: `+0x22e8` contains CMD8 and ACMD41's literal is `0x00100000`. The table pointer itself is low address `0x000022e0`; boot-time visibility of that ROM table at the low alias is **unproved** by the post-boot ROM capture.

**Static-inference:** CMD7 selection, accepted ACMD6 and a successful completed CMD17 put the intended path in SD transfer state. There is no observed CMD13 reply at payload entry, so transfer state is not **hardware-observed**. A helper must check `+0x10=1`, CCS in `{0,1}`, enabled/stable clock, four-bit host control, card presence and clear command/data inhibit, then query CMD13 with the saved RCA and require READY_FOR_DATA and CURRENT_STATE=TRANSFER before writing. If these preconditions fail, halt without a write, not reinitialize.

**Byte-read:** `ffsys.bin` apply runs between card initialization and the later directory/payload reads. It replaces global clock words including `0xfff88028=0x00060000` and `0xfff88030=0x00000a00`. The stock config's optional SD-clock field `+0x33c` reads `0x80000000`: gate bit 31 is set, so the helper at `0x800010ac` is called for channel 0 with selector argument 0 and rewrites the divider field (bits 15:6) to 0 rather than leaving the init's `0x500` in place. Its four optional address slots are zero, and its seven pair writes target none of the SD-host registers or `0xfff8930c` ([window map section 3.1](bootrom-window.md#31-the-0x80000304-config-apply-its-field-map-and-the-stock-config-block)). **Static-inference:** the bus width and card selection persist, while the SD clock divider and the upstream clock tree are rewritten. Reaching the payload proves that this particular card/controller combination could read under that post-config state; it does not establish frequency margins or successful programming.

**Recommendation, static-inference:** reuse the successful channel-0 state; do not introduce reset, CMD0, voltage changes, bus-width changes, new pin writes or clock changes merely to report entry. The necessary path already initialized and read the card. Reinitialization adds unrelated MMIO and failure modes, and blindly calling the ROM initializer after staging is especially unjustified when its low-address table may now be RAM. A normal mode-4 boot leaves no such channel-0 SD initialization contract for this payload.

### 6.2 What one CMD24 would take

**Static-inference candidate, not byte-read X-M1 write behavior:** the ROM's offsets and bit uses match SDHCI for transfer mode `+0x0c`, command `+0x0e`, response `+0x10`, FIFO `+0x20`, present state `+0x24`, host control `+0x28`, clock `+0x2c`, reset `+0x2f`, normal/error status `+0x30/+0x32` and enables `+0x34/+0x36`. On that interpretation, PIO CMD24 uses `+0x0c=0x0000` (single block, write, no DMA/auto-command) and `+0x0e=0x183a` (CMD24, data present, index/CRC checks, short response). CMD13 uses `+0x0c=0`, `+0x0e=0x0d1a` and the saved RCA. These values are strong leads, not a binding of this controller's undocumented write path.

The specification of a future helper is:

1. **Static-inference requirement:** embed the one channel-0 base, one report LBA and one internally built 512-byte record. Carry an attempt-specific identifier, payload revision and integrity check so a host can reject an old or torn record. Reserve separate owned code, stack, report buffer and readback buffer spans within the newly staged payload extent; enlarge `ffboot.bin` if necessary rather than fitting a writer into the current signature/stack locations. No pointer, MMIO address, command or target LBA comes from the card.
2. **Static-inference requirement:** check inherited preconditions and CMD13 as above, clear only the status bits needed for this transaction, and bound every poll. A software iteration budget bounds instruction count, not elapsed seconds; clock/timing evidence must support a suitable wall-time budget before a live proposal.
3. **Byte-read addressing rule, static-inference write application:** report LBA 34816 (`0x8800`) is argument `0x8800` for CCS=1 or `0x01100000` for CCS=0. Set block size 512 and count 1, write argument, then transfer mode and command in that order. Never enable DMA: the final ROM read left a DMA destination register, but PIO with DMA disabled does not use it.
4. **Static-inference candidate:** await command completion at `+0x30` bit 0, acknowledge it and reject R1 error bits. Await Buffer Write Ready at `+0x30` bit 4 (`0x10`), not the ROM's Buffer Read Ready bit 5 (`0x20`), then store 128 little-endian words to `+0x20`. Honor FIFO readiness as required by the resolved controller contract and acknowledge only consumed status. Transmit-FIFO stores and their pacing remain **unproved and blocking** on this controller.
5. **Static-inference candidate:** await Transfer Complete bit 1 and busy release, reject error status throughout, and use bounded CMD13 checks to require card-ready transfer state after programming. Native SD start/end framing and lane CRC are the host controller's job on the SDHCI interpretation; do not add SPI start tokens, CMD59 or software CRC bytes to the 512-byte data. Actual transmit framing, CRC and busy behavior remain **unproved and blocking**.
6. **Static-inference requirement:** make one bounded CMD17 PIO readback and compare all 512 bytes, then halt on either success or failure. ROM `0x80001618`, its inhibit waits and its clock-stability wait are unbounded, so copying/calling the stock read path unchanged would defeat the helper's bounds. No retry, CMD25, CMD12, erase, write-protect command, partition edit or filesystem update belongs in this operation.

**Unproved implementation prerequisites:** bind the SDHCI write candidates to this controller through target-specific documentation or decoded shipped code using its transmit path, and resolve the actual post-config clock rate and limits for the chosen card. A native helper would then need a machine-code write-set/control-flow audit, independent review and host failure-injection tests. A modeled controller can demonstrate the helper's decisions for modeled responses, including lost readiness or completion; it cannot remove either hardware blocker. Neither a working ROM read nor a passing host model authorizes trying the candidate values on this body.

### 6.3 X-M1 stock CMD25 PIO binding

**Source-inspected lead, then byte-read on X-M1:** fetched `../x70view` revision `a7dd99af5805b4bd50b895ea2b8353c769c3a032` with its Git LFS objects. Its recovered stock SD path identified a finite PIO writer using packed request `0x193a0026`. That sibling source is only the lead. The binding comes from this project's pinned X-M1 1.50 image: `0x006f32a4` selects its PIO branch for an unaligned source, calls `0x006f3160(0x200)`, and calls `0x006f2bf4`; instructions `0x006f2c0c`-`0x006f2c14` construct `0x193a0026`; `0x006f2838` sends 512 bytes as 128 little-endian words through `0x006dea04`, whose only store is to `0xfff50020`. The same function repeats by 512-byte units and waits for buffer-ready mode 5 and final transfer completion.

**Static-inference:** the packed word supplies transfer mode `+0x0c=0x0026` and command `+0x0e=0x193a`: CMD25 with block-count enable and automatic stop, not the report helper's inferred CMD24 `0x0000/0x183a`. With `+0x06=1`, it is the shipped application's one-block transaction grammar. This supersedes CMD24 as the preferred reporter transaction. The existing CMD13 guards, one-sector FIFO feed, bounded polls, CMD17 readback and no-retry policy remain applicable.

This closes the missing target-code binding for command choice, FIFO direction and 512-byte feed. It does not prove that an interrupt-free polling port at ROM handoff observes the same readiness/completion sequence, the post-configuration clock rate, card busy timing or cache visibility. The fetched X70 emulator supplies a software model for those responses, not physical X-M1 evidence. The journal-only attempt's all-zero readback therefore remains inconclusive: no entry and early rejection of its CMD24 transaction are both compatible with the observation.


### 6.4 Placement and interrupted-write outcomes

**Byte-read:** the built image's first MBR partition is `[2048,34816)`; the dedicated report sector is LBA 34816, byte offset `0x01100000`, outside that partition but inside the image. `ffsys.bin` occupies LBAs 2337 and 2338, and the current `ffboot.bin` occupies LBA 2339. **Static-inference:** a correctly addressed one-sector write at 34816 changes none of the MBR, BPB, FAT copies, root directory or either boot file. The owner would need a disposable card with at least 34817 sectors, no valuable contents or other retained partitions, the image written and verified, and the report sector read/cleared on the host before each attempt. No such card preparation was performed here.

**Static-inference:** do not put the report into an arbitrary “free FAT cluster” or after a file's logical end; those locations can belong to the filesystem or later allocation. The out-of-partition sector avoids filesystem metadata updates and is already reserved by this builder. A host should read it as a raw sector, not expect a report file to appear.

| Failure stage | Persistent result and next action |
| --- | --- |
| Guard, status query or command rejected before data acceptance | **Static-inference:** no new report expected; stop without a write or retry. Unchanged bytes do not distinguish no entry from a helper failure |
| FIFO/data transfer fails or CRC rejects the block | **Static-inference:** old contents or an invalid/torn record are possible; no success claim. Halt without a second write. The hardware's exact residue is **unproved** |
| Data accepted, card programming busy, controller timeout or power lost | **Unproved:** a valid new sector, the old sector or corrupted contents may remain. A software timeout does not cancel card-side programming; halt is not rollback, and cutting power while the card is busy adds risk |
| Completion reported, readback fails | **Static-inference:** the write may already be durable; classify it as uncertain rather than retry. Only later host inspection can determine whether the attempt's full record exists |
| Wrong argument, wrong CCS scaling, unintended extra block or command | **Static-inference:** the target bound is lost; boot files, partition/FAT metadata or any other addressed card data can be destroyed. A constant in source is not proof of correct machine-code address arithmetic |

**Static-inference risk bound:** even a correctly addressed logical-sector write is not a proof that only one physical flash sector is modified. The SD card owns flash translation, erase units and metadata. Its power-loss behavior is **unproved**; conservative loss is the entire disposable card, including loss of usability, not just one report sector. Replace/reimage the card rather than promising filesystem repair. A host-validated full record with a fresh attempt identifier is positive entry evidence; zero, stale or invalid bytes are inconclusive.

### 6.5 Risk judgment against the passive RAM detector

**Recommendation:** if a passive RAM signature can be recovered through the next maintenance boot, do not build the write helper. It supplies no needed observation and introduces a write-capable native program plus card/controller timing dependence. The current `0x100` signature is within the next ordinary boot's staging region, so unchanged electrical RAM retention cannot preserve it through that overwrite; [the detector analysis](boot-card-detection.md) owns the survival finding and any other passive location's verdict.

**Recommendation:** the consumed CMD24 image should not be reused. A replacement can now use the X-M1 stock CMD25 command grammar, but actual clock rate, polling equivalence, controller write/busy behavior and executable confinement remain blockers. The unresolved entry trigger means this finding alone does not justify another card attempt.

**Static-inference, conditional risk bound:** a reviewed helper that disables DMA, uses only the resolved channel-0 command/transfer/FIFO/status registers, sends only CMD13/CMD25/CMD17, writes only its owned RAM and one correctly addressed card sector, and halts on error has no specified route to internal firmware, calibration or the lens. The SD commands address the removable card, not the separate `0xfff52000`/`0xfff52800` internal-media controller. Under that verified write set, conservative persistent loss is the expendable card and temporary loss is a halted body until reset. Physical/electrical failure independence is **unproved**, not a guarantee.

**Static-inference:** an arbitrary native-code defect is not confined by the intended removable medium. A wrong MMIO base, corrupted control flow or accidentally enabled DMA can reach RAM or other controllers; the ROM environment provides no demonstrated sandbox or recovery supervisor. The target writer closes command grammar, not the early polling port or silicon behavior.

**Governance:** this persists to an SD card and executes new write-capable bytes on the camera. A revised executable needs independent byte review and an owner's go naming the operation under the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). This investigation accessed no camera, card reader or USB device.

## 7. Test results

`devenv shell -- bazel test //host/firmware:boot-card-tests //tests:research-contracts --test_output=all`: **both targets passed; 36 behavioral card checks**. Coverage includes both modeled loading paths, exact configuration differences, fragmented FAT reads, sector-zero FAT mounting, masked/zero/raw counts, short raw input, corrupted raw configuration and payload, legacy-image refusal and malformed FAT files. The two nonzero FAT16 cluster-high cases reject images that the old model falsely approved. Instruction-word and incidental geometry assertions were removed; the emitted payload's executable write audit remains an independent review obligation.

The actual `build` command emitted the candidate in section 2, and `verify` passed using a workspace-relative image path. The same command rejected the old FAT-only image with `raw-mode configuration differs`. The CLI reports the unresolved register differences and does not claim live clearance.

The CLI also refused a sibling-prefix output directory outside `private-data/`; the empty smoke fixture was removed. Independent emitted-byte reviews accepted the host layout and the payload's explicit write set. Live clearance was withheld on the unidentified raw-mode register effects and controller wiring/state, not on the lack of a reporting payload.

These are host results. They are a research-contract and format check, not hardware, card-reader, SD-controller or camera evidence.

## 8. Ledger claim text as recorded

The initial build is recorded in **E-SDBOOT-CARD-IMAGE-150** and the overwritten-signature finding in **E-SDBOOT-RAM-SIGNATURE-150**. **E-SDBOOT-DUAL-PATH-HARDENING-150** records the current stock-derived raw overlay, host verification and remaining register-effect boundary. No SD payload execution, recovery level or installation acceptance follows from these host results.

## Reproduction

```
devenv shell -- bazel test //host/firmware:boot-card-tests --test_output=errors
devenv shell -- bazel run //host/firmware:build-boot-card -- build private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-halt.bin private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card
devenv shell -- bazel run //host/firmware:build-boot-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-halt.bin private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card/diagnostic-sd.img
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi bazel-bin/host/firmware/diagnostic-halt.elf
```

The retained candidate already exists, so reproduction of `build` requires a different new directory under `private-data/`; existing output directories are refused. The payload source is unchanged. Neither this reproduction nor the byte review authorizes card insertion or power-on.
