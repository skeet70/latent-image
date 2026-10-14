# SD-BOOT card entry: strap decode, pad drivers and an observable card outcome

This report retains X-M1 1.50 ROM selector, SD loading, bootstrap, storage-map and reference-lifetime findings. Labels distinguish byte-read, source-inspected, static-inference, emulated, hardware-observed, reported and unproved claims. Host findings and later owner-run observations are scoped separately.

Artifacts: boot ROM `private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin` (16384 bytes, SHA-1 `bed698ed1d91b5c29ce7c5d650156e6100c32cce`) and its listing `bootrom/bootrom.dis`; boot record 1 `decode/boot-40048.bin`; image 4 `lz/id4-0061c400.bin`; the built card `bootrom/diagnostic-card-image/ffsys.bin`, `ffboot.bin` and `diagnostic-sd.img`; the owner's captures cited per item. The sibling checkout `../x70view` is cited by path only and supplies no X-M1 fact.

**Current result:** the [exact v5 physical rollback](#260-physical-rollback-readback-status-20) reports complete RC0 readback; the owner reports ordinary startup, photography, normal-card operation/playback and retained film simulations/settings. [Recovery status](recovery-status.md) owns current coverage and [the recovery kit](../recovery/README.md) owns supported reproduction. Earlier candidate/session sections retain technical evidence, not reusable camera authorization or current instructions. Preserve the successful card; no repeat write or reinstall is justified.

Reading paths: sections 1–2.5 bind the ROM selector and pad inputs; sections 2.37–2.60 trace installed-storage reachability, physical target selection and the successful bounded repair; section 4 binds dual-route configuration hazards. Exact input bytes remain owner-supplied and private.

## 1. The mode-detect chain at 0x80000260, instruction by instruction

The function body is 41 instructions, 0x80000260 to 0x80000300. Raw words were read from the pinned artifact; the listing quotes are `bootrom.dis`. `r0` enters as the caller's mode-detect argument and is overwritten at 0x80000260; the return value in `r0` is the mode, or `-1`.

| Address | Word | Instruction | Effect |
| --- | --- | --- | --- |
| 0x80000260 | e59f0804 | `ldr r0,[pc,#0x804]` | r0 = [0x80000a6c] = `0xfff89000` (GPIO block base) |
| 0x80000264 | e59f27fc | `ldr r2,[pc,#0x7fc]` | r2 = [0x80000a68] = `0xfff98000` (selector/status word) |
| 0x80000268 | e59f3800 | `ldr r3,[pc,#0x800]` | r3 = [0x80000a70] = `0xfff88000` (clock base, used only on the -1 path) |
| 0x8000026c | e59f1800 | `ldr r1,[pc,#0x800]` | r1 = [0x80000a74] = `0x0c000c00` |
| 0x80000270 | e5801008 | `str r1,[r0,#0x8]` | [0xfff89008] = `0x0c000c00` |
| 0x80000274 | e5900014 | `ldr r0,[r0,#0x14]` | r0 = [0xfff89014] (port 0 input word) |
| 0x80000278 | e5921000 | `ldr r1,[r2]` | r1 = [0xfff98000] |
| 0x8000027c | e2000b03 | `and r0,r0,#0xc00` | keep GPIO bits 11:10 |
| 0x80000280 | e2011007 | `and r1,r1,#7` | keep selector bits 2:0 |
| 0x80000284 | e18113a0 | `orr r1,r1,r0,lsr #7` | r1 = (GPIO[11:10])<<3 \| selector[2:0]; bits 4:3 = the high pair |
| 0x80000288 | e3a00007 | `mov r0,#7` | |
| 0x8000028c | e1d00001 | `bics r0,r0,r1` | r0 = 7 & ~r1; Z = 1 iff selector[2:0] = 7 |
| 0x80000290 | 03e00000 | `mvneq r0,#0` | r0 = -1 on that condition |
| 0x80000294 | 0a000015 | `beq 0x800002f0` | first test taken to the shared store |
| 0x80000298 | e3110018 | `tst r1,#0x18` | bits 4:3 both zero? |
| 0x8000029c | 03a00001 | `moveq r0,#1` | |
| 0x800002a0 | 0a000012 | `beq 0x800002f0` | |
| 0x800002a4 | e3110007 | `tst r1,#7` | reaches here only with bits 4:3 nonzero; selector zero? |
| 0x800002a8 | 03a00003 | `moveq r0,#3` | |
| 0x800002ac | 0a00000f | `beq 0x800002f0` | |
| 0x800002b0 | e2010007 | `and r0,r1,#7` | |
| 0x800002b4 | e3500001 | `cmp r0,#1` | |
| 0x800002b8 | 03a00004 | `moveq r0,#4` | |
| 0x800002bc | 0a00000b | `beq 0x800002f0` | |
| 0x800002c0 | e3500002 | `cmp r0,#2` | |
| 0x800002c4 | 03a00005 | `moveq r0,#5` | |
| 0x800002c8 | 0a000008 | `beq 0x800002f0` | |
| 0x800002cc | e3500003 | `cmp r0,#3` | |
| 0x800002d0 | 03a00006 | `moveq r0,#6` | |
| 0x800002d4 | 0a000005 | `beq 0x800002f0` | |
| 0x800002d8 | e3500004 | `cmp r0,#4` | |
| 0x800002dc | 03a00002 | `moveq r0,#2` | |
| 0x800002e0 | 0a000002 | `beq 0x800002f0` | |
| 0x800002e4 | e3500005 | `cmp r0,#5` | |
| 0x800002e8 | 03a00007 | `moveq r0,#7` | |
| 0x800002ec | 13a00000 | `movne r0,#0` | selector 6 falls through here |
| 0x800002f0 | e5821000 | `str r1,[r2]` | [0xfff98000] = the combined 5-bit word, bits 31:5 cleared |
| 0x800002f4 | e3700001 | `cmn r0,#1` | |
| 0x800002f8 | 03a01000 | `moveq r1,#0` | |
| 0x800002fc | 05831084 | `streq r1,[r3,#0x84]` | [0xfff88084] = 0 on the -1 path |
| 0x80000300 | e12fff1e | `bx lr` | |

**Byte-read, the exact strap values.** The combined word is `r1 = ((0xfff89014 >> 10) & 3) << 3 | (0xfff98000 & 7)`, so bits 4:3 hold `0xfff89014` bits 11:10 and bits 2:0 hold `0xfff98000` bits 2:0. Bits outside those two fields are not consulted: no other load or test lies between the write at 0x80000270 and the return.

| First test that fires | Condition on the two registers | r0 returned | Mode and handler |
| --- | --- | --- | --- |
| 0x8000028c-0x80000294 | `0xfff98000` bits 2:0 = 7 | -1 | halt; 0xfff88084 bit 0 cleared |
| 0x80000298 | `0xfff89014` bits 11:10 = 0b00, selector bits 2:0 not 7 | 1 | SD/FAT channel 0, `0x8000082c` |
| 0x800002a4 | high pair nonzero, selector bits 2:0 = 0 | 3 | internal format 4, `0x80000938` r0=0 |
| 0x800002b4 | high pair nonzero, selector = 1 | 4 | internal format 1, `0x80000938` r0=1 |
| 0x800002c0 | high pair nonzero, selector = 2 | 5 | internal format 2, `0x80000938` r0=2 |
| 0x800002cc | high pair nonzero, selector = 3 | 6 | internal format 3, `0x80000938` r0=3 |
| 0x800002d8 | high pair nonzero, selector = 4 | 2 | raw SD channel 1, `0x8000071c` r0=1 |
| 0x800002e4 | high pair nonzero, selector = 5 | 7 | `0xfff56000` receiver, `0x80000ac0` |
| 0x800002ec | high pair nonzero, selector = 6 | 0 | RAM handoff, `0x80000b2c` |

**Mode 1 in exact terms.** `0xfff89014` bits 11:10 both read 0 (the high pair is exactly `0b00`, not any low value) and `0xfff98000` bits 2:0 read 0, 1, 2, 3, 4, 5 or 6. The selector-7 halt is tested before the high pair, so a selector of 7 halts whatever the high pair reads, and the mode-1 condition is `high pair = 0 and selector != 7`. Either field alone selects nothing: a high pair of 0 with selector 7 halts, and a nonzero high pair never returns mode 1.

**Byte-read, once per reset and no fallback.** The sample is one pass with no loop, called once from reset at 0x80000130. The dispatch at 0x8000013c-0x800001f0 compares the returned mode against 0 through 7, calls one handler, and on a nonzero handler return branches to the halt at 0x80000020 (`wfi`) and 0x80000024 (`b .`); a returned -1 falls to `mvn r0,#0` at 0x800001f4 and the same halt. The writeback at 0x800002f0 stores the combined word on every path, so after any reset 0xfff98000 bits 4:3 and 2:0 record what was sampled. On the -1 path 0xfff88084 bit 0 is also cleared.

## 2. What drives the two sampled fields at reset

### 2.1 What the fields are

**Byte-read:** 0xfff89014 is port 0's input word in a 16-port register block at 0xfff89000 with a 0x40 stride; port k's input is read at 0xfff89014 + 0x40k, as the three 16-word snapshot loops do (record 1 at `boot-40048.bin` 0x269c: `ldr r1,[r2],#64` with r2 = 0xfff89014, stored to 0x400a0000+4k). Bits 11:10 are two of that word's pads.

**Byte-read:** this body's own input table binds those two pads to the two controls the documented press names. Image 4's 36-row condition table at 0x828120 has 28-byte rows; row 13 at 0x82828c reads `28 00 00 00 00 00 00 00 00 02 00 00 0b 00 00 00 01 00 00 00 02 00 00 00 00 00 00 00`: code 0x0028 (`KEY_FUNCTION`), kind 2, operand `0x0000000b`, compared level 0. Row 22 at 0x828388 reads code 0x002b (`KEY_CMDL_PUSH`), kind 2, operand `0x0000000a`, level 0. `FUN_006a0888`'s kind-2 arm (0x6a0894-0x6a08e4) computes `(0x03ffe240 + bank) << 6` (`movwls r0,#0xe240` / `movtls r0,#0x3ff`, `addls r0,r2,r0`, `lslls r0,r0,#6`), which is `0xfff89000 + bank*0x40`, loads `[r0,#0x14]`, and extracts bit `operand & 0xf`. So operand 0xb selects bit 11 and operand 0xa bit 10, each asserted at 0.

**Hardware-observed:** the act-logged session `20261009T064945Z-read-log` emits code 0x0028 on the physical Fn press (tick 17087) and code 0x002b on the physical sub-command push (tick 22342, E-PRESS-MAP-150), so at application run time those two pads carry the two controls.

**Byte-read, the same block holds a different card input.** Row 8 at 0x828200 reads code 0x0011 (`KEY_CARD_DET`), kind 2, operand `0x000c0007`, level 0: port 12 (`bank = (operand >> 16) & 0xf = 12`, `0xfff89000 + 0xc0 = 0xfff89300`, read at +0x14 = `0xfff89314`) bit 7. Card presence is not read from 0xfff89014 and is not one of the two sampled fields.

### 2.2 The 0x0c000c00 write at 0x80000270

**Byte-read:** the write is one 32-bit store of `0x0c000c00` to port 0's register +0x08: `e59f1800` at 0x8000026c loads the literal from 0x80000a74 and `e5801008` at 0x80000270 stores it, four instructions before the first read. The value carries bits 11:10 in both halves, and neither instruction establishes whether the store replaces the register or selectively updates those bits. **Reported:** the sibling project's model of this block treats such port writes as `{mask: high halfword, level: low halfword}`; that is a fixture convention, not X-M1 evidence. **Unproved:** the write's update semantics and the electrical function of the bits (output data, enable, pull configuration or function select); both are missing SoC facts.

**Byte-read, the same write appears again.** The mode-7 initialization at 0x800020cc-0x800020f0 reads the register, ORs `0x0c000c00` into it, stores it back, and then polls `[r1,#0x14]` with `movw r3,#0xc00` until both of bits 11:10 read high before it starts the receiver. **Static-inference:** the poll in that path would be vacuous if the store alone fixed the input word's bits to 1, so the two bits are expected to be able to hold a low value while the same control word is in place.

**Byte-read, the loader's own value for the same register.** The post-handoff record 1 restores the whole port file from a table at `boot-40048.bin` 0x1b00 (loop at 0x2644-0x2684, table mapping `[0x1b0c] -> +0x08`, `[0x1b14] -> +0x00`, `[0x1b10] -> +0x04`, `[0x1b04] -> +0x0c`, `[0x1b08] -> +0x10`, `[0x1b00] -> +0x18`, `[0x1b18] -> +0x1c`). The port 0 entry's stored value for +0x08 is `0xffff0fff`, and for +0x00 `0xffff0000`. **Static-inference:** bits 11:10 are set in the +0x08 value, so the loader's restore does not present a different level for those two bits; it does not treat the register as the mode strap.

**Reported:** `../x70view/emulator/boot_devices.py` stores writes to the port data register it models as mask and level, keeps the other port offsets as opaque values, and states in its snapshot limits that GPIO output/mux writes retain state while external pin behavior is not modeled. Its SD-boot board supplies the two sampled bits by forcing the input word through `set_gpio_input_pin` before reset, with the source string "SD-boot buttons held at power-on". The fixture, not the ROM, is what makes that route select mode 1.

### 2.3 What the reset sample shows and what it does not

**Hardware-observed:** the cardless sweep held Function together with every body button, including the sub-command dial press, at power-on and every combination booted the application (E-SD-BOOT-ENTRY-HOLD-150). The only route to the application on this body is mode 4, and mode 4 needs the high pair nonzero with selector 1, so each swept sample read the high pair as 1, 2 or 3, never `0b00`. The captured value 3 comes from a different condition: the idle maintenance boot reads `0xfff89014 = 0x00000d4e` and the reset-stage snapshot word 0 `0x00000d62` (both bits 11:10 = 3, E-STRAP-READ-150), with no mapped control held. The sweep and the idle capture are not one held-control measurement.

**Static-inference, what the sweep does and does not fix:** it fixes that the pair never read `0b00` under the tested holds. One control reading low gives `0b01` or `0b10`, which still selects mode 4 with selector 1 (0x80000298 tests only whether either high bit is set), so the sweep is also consistent with exactly one of the two controls asserting while the other does not. It is not a measurement of either pad's level, and no held capture reads either register with a control held at reset.

**Hardware-observed:** the same two pads follow those controls while the application runs, through the two rows quoted above and the act log. **Unproved:** whether the panel can present `0b00` at the ROM sample at all, and if it cannot, whether one pad path, both pad paths, the +0x08 store's update semantics, or a reset-independent path accounts for it. **Static-inference:** the arrangement in which both switches directly ground both pads at the sample is the one the sweep excludes; every single-control variant stays open.

**Unproved, the precise missing fact:** the board net between the Fn and sub-command switches and GPIO0 pads 10 and 11, including any series device, gate or scanner, plus two register facts: the electrical function of port 0 register +0x08 bits 11:10 (section 2.2) and whether that store replaces the register or updates only those bits. The held service manual identifies assemblies and FPCs; it contains no schematic or pad list. No held capture reads either register with a control held at reset, and no artifact records a pad-level measurement. A schematic, a continuity or pad measurement, or SoC documentation would close this; no firmware byte can.

### 2.4 The low selector, 0xfff98000 bits 2:0

**Byte-read:** the ROM reads the register once at 0x80000278, after GPIO setup and input sampling but before card or internal-media initialization. Its other direct touches are the writeback of the combined word at 0x800002f0 and the config apply's read and write at 0x80000338/0x8000034c, which change bits 17:16 while preserving the low field. No record, loader or image accesses 0xfff98000 at another site in the forms the census decodes, with its caveats in `bootrom/strap-read-safety.md` section 3. The ten-word base table at image-4 0x829144 has no established consumer; that is not a proof that it is dead data. The record and image-5 port snapshots read GPIO inputs, not the selector.

**Hardware-observed:** the held X-M1 maintenance capture reads `0xfff98000 = 0x00000019`, consistent with the ROM's combined word for high pair 3 and low selector 1. This observation belongs to that maintenance reset, not the proposed card-present Fn/sub-command reset.

**Byte-read, card detect is a different signal twice over.** Inside the ROM, the SD path reads card presence from the SD host controller: `0x8000155c` writes byte 1 to host +0x2f, waits for +0x24 bit 17 and refuses without bit 16. In the application, row 8 above reads `0xfff89314` bit 7. Neither path touches 0xfff98000.

**Reported:** the sibling model installs `boot_source=1` at 0xfff98000 as a construction fixture for both its normal and SD-boot boards, keeps card presence on GPIO12 bit 7, labels the source an inferred NAND type-1 normal-boot profile, and names selector bits 2:0 a storage-geometry selector in its mode table. **Static-inference:** the selector is a reset-presented option field, not a card-sense bit.

**Unproved, the precise missing fact:** the electrical source of 0xfff98000 bits 2:0 on the X-M1 board, whether package pins, fuses, a reset-loaded register or a companion register, and whether any board-level path makes it card-dependent. No held capture reads either strap register with a card inserted, so a card-dependent external path to this register can be neither established nor excluded.

**Direct-route verdict:** existing target material does not exclude mode 2 for the proposed actuation. The unresolved table follow-through supplies no established low-bit writer and no reset-source definition; a negative consumer census does not identify pins, fuses or a reset latch. The [platform reference](https://github.com/tiredboffin/fffw/wiki/EXR-II-CPU)'s register value and debug-register description are leads, not an X-M1 reset-source definition.

**Emulated:** 32 complete executions of unchanged ROM `0x80000260`–`0x80000300`, one for each low selector and GPIO pair, return the decoded mode and write the expected combined word. With selector 1 the only modes are 1 and 4; mode 2 occurs only for selector 4 and a nonzero pair. Another 32 prefix cases stop at original PC `0x80000288`, after both reads and the OR, with `r1 = selector | (pair << 3)`. Their only store is GPIO setup; selector writeback and dispatch have not executed. Inputs are ordinary-memory fixtures, not evidence of the selector's physical source. Replayable wire inputs and reports are under `private-data/scratch/selector-decision/`.

**Source-inspected, transport limit:** the existing `probe-straps` operation observes its current maintenance boot and does not reset the camera itself. Its service transport is activated after the application enters mode `0xb`; it cannot observe a ROM SD/halt path that never reaches the application. `FUN_00635280`'s UP/S2/USB predicate does not require other keys released, but adding those controls and USB changes the tested reset condition. S2 is the shutter's second stage. A later maintenance reboot loses the earlier startup's provenance. A standard-card test cannot be cleared by assuming selector 1: raw mode 2 would consume its sectors before this transport exists.

**Conditional guarded-reset design, not an available camera operation:** a qualified early-debug method could halt at executed ROM PC `0x80000288` and inspect CPU `r1`, avoiding assumptions about system-register writeback. Permit continuation only for `r1` in `{0x01, 0x09, 0x11, 0x19}`; otherwise stop without dispatch. These four sampled words have selector 1 and lead only to modes 1/4. Keeping that guard on every diagnostic reset would exclude mode 2 operationally, without proving selector immutability. It would still require the mode-1 configuration/payload review.

The missing prerequisite is target-supported early-debug access capable of a nonpatching halt at this boundary, with the actual ROM execution alias and reset/pin effects verified, or target documentation establishing the reset selector's source. Debug access on this body is not identified or verified. Board access needs an owner-discussed invasive plan; no new camera command is ready. Repeating the existing software censuses is not an acceptance path for this missing hardware fact.

### 2.5 Reconciliation with the cardless sweep

The sweep's normal boots are consistent with the decode only as mode 4: a nonzero high pair with selector 1. The sweep fixes the pair's exclusion of `0b00` and nothing about its exact value; the captured 3 belongs to the idle condition (section 2.3). It says nothing about the selector, because no card was present and the selector read 1 in every capture anyway. Since mode 1 requires the high pair to be exactly zero at the same sample, the card-present Function plus sub-command push condition tests whether card presence or the press itself can move both high bits low, which no decoded main-CPU path does and no held measurement has shown. The file-presence hypothesis has no decoded consumer; route selection precedes every card access, and the card's only decoded input to the reset sample would be the selector's unproved electrical source. **Static-inference:** mode 1 needs both high bits low, so a resolved finding that either switch cannot present a low level at the sample closes the panel route for this body and moves the entry work to non-panel driving of the pads, which this assignment does not cover.

### 2.6 Bounded LED/sound spike

**Byte-read, X-M1 1.50:** `FUN_006bdbc0` alternates `0xfff893c0 = 0x00010001` and `0x00010000` indefinitely, with application sleeps between writes. Decoded image-4 instructions independently confirm the stores at original PCs `0x006bdbf0` and `0x006bdc18`; `mvn r6,#0x76000` plus store offset `-0xc3f` resolves the target address. The function gates the loop on runtime state and otherwise calls a callback. This establishes a target-native toggle sequence, not its physical indicator or early-boot readiness. The application function itself is not callable from the diagnostic payload.

**Source-inspected, sibling corroboration:** freshly fetched [fffw main](https://github.com/tiredboffin/fffw/tree/b084c998b02a60859288c9b6dc7204e6a94f2e13) retains the [X-E2 LED example](https://github.com/tiredboffin/fffw/blob/b084c998b02a60859288c9b6dc7204e6a94f2e13/boot/xe2-led0-main.c). Its disabled red-blink branch uses those same two writes and a bare delay loop, preceded by `0xfff893c4 = 0x00050001`. Its colour experiments also write that adjacent register. That extra setup is not target-bound and must not be imported as X-M1 initialization. Remote branches are main, develop and experimental; none is recovery-named. The scout checked the experimental boot tree against main and reported it unchanged; the README's `ffre` recovery tool has no supplied implementation in the inspected main tree.

**Source-inspected, application dependencies:** fujihack master remains [754acff9](https://github.com/fujihack/fujihack/tree/754acff9aa0322df19446cf7eca699b221c3330d). The [Doom branch's indicator](https://github.com/fujihack/fujihack/blob/1adcee69bf6a7b811ffce93d8de1aa44432e145b/src/doom.c) changes framebuffer pixels and calls `bmp_apply`; it also uses firmware task sleep. Its [X-F1 beeper stub](https://github.com/fujihack/fujihack/blob/754acff9aa0322df19446cf7eca699b221c3330d/model/xf1_101.h) is an application-function address, not a standalone buzzer driver. The target's `FUN_006bbf00` likewise posts message `0x722c` to application port `0x62`. These are not immediate SD-handoff sound mechanisms.

**Spike verdict:** the target-native GPIO sequence is a credible minimal blink-payload candidate, materially stronger than the earlier sibling-only lead. A candidate can inline the two target writes and register-only delays without calling application code, changing `0xfff88084`, adding a card writer or importing sibling pin setup. No such new payload was built in this spike. Physical LED identity, required output enable/pinmux and cadence remain unproved. A distinctive repeating visible signal would establish payload execution; absence would remain inconclusive. This candidate does not remove the existing configuration-apply risks or authorize a live run. The spike ends here rather than extending into GPIO or sound-driver reconstruction.

### 2.7 Built blink candidate

**Built and independently byte-reviewed; residual-risk run authorization is recorded in section 2.8:** `//host/firmware:diagnostic-blink` builds `host/firmware/diagnostic-blink.S` with the existing one-sector linker bounds. The 512-byte payload contains 24 A32 instructions and zero padding. Entry 0 masks IRQ/FIQ, constructs only the target-confirmed GPIO address and values, then alternates the two writes three times per group. Each store is followed by `dsb sy`. Short delays use `0x01000000` decrement iterations; the group ends with an additional `0x08000000` hold of word `0x00010000`. These are instruction counts, not measured seconds or a confirmed LED polarity. It never calls, reads data, uses the stack, returns, writes RAM markers or accesses another peripheral.

The separate candidate directory is `private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-card/`. Its `ffboot.bin` SHA-256 is `e02fa1ee7aecae973ef6934790a1c1cad01247756dc05d4dfeee806bc5880c6a`; image SHA-256 is `827d495a70199ac59505953cf89fe2d123135fa7122a580111c6257b6a218595`. Both raw LBA 2 and FAT LBA 2339 contain that payload. Independent inspection found every image byte outside those two sectors identical to the original halt control. The FAT configuration and raw overlay are unchanged; all three section 4.6 register risks remain.

**Emulated smoke:** the unchanged full payload ran in Unicorn's R5F proxy, relocated to `0x00010000` in privileged A32 mode with an ordinary-memory GPIO fixture. The hook shortened `r4` only on first entry to each delay loop. Two complete groups produced 12 exact alternating stores, 12 shortened pulse delays and two shortened group gaps in 126 executed instructions, with IRQ/FIQ masked and no data reads. This checks full-payload execution/control flow, not hardware behavior or timing. The temporary C driver and executable were removed; observed output is retained in the candidate's `smoke-output.txt`.

**Independent review:** a separate `openai-codex/gpt-6.1-sol` session decoded the actual bytes, checked ELF entry/padding/relocations and compared the two payload sectors and unchanged image spans. It reported no byte-level defects and conditionally approved the diagnostic sequence/packaging. The retained `byte-review.txt` records its verdict and scope. The review explicitly withholds live clearance and did not independently reproduce the smoke.

The owner approved preparation and subsequently authorized one residual-risk attempt on the corrected evidence basis in section 2.8. No card was written or camera operated by the model. A distinctive repeating signal supports execution of the loaded payload, but does not distinguish FAT mode 1 from raw mode 2. Normal boot or absence of a signal does not establish execution. Output readiness, physical LED identity and the configuration risks remain unproved; this is not an RC flash or a demonstrated recovery chain.

Host reproduction for the existing candidate:

```sh
devenv shell -- bazel build //host/firmware:diagnostic-blink
devenv shell -- bazel run //host/firmware:build-boot-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-blink.bin private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-card/diagnostic-sd.img
```


### 2.8 Spare-card preparation and conditional camera procedure

**Owner-authorized residual-risk experiment:** the three modified raw-mode words were observed in host execution, not executing on the physical camera. `0x04` is a standard FAT16 MBR partition type; that does not establish its safety as a controller-register field. After that misconception was corrected, the owner explicitly selected “Go for one SD blink attempt,” knowing the register effects remain unknown and recovery after power removal is not guaranteed. Authorization covered the verified spare-card preparation, one attempt bounded to 60 seconds, power cut and ordinary-operation checks below. It does not establish hardware safety, resolve the register semantics, accept a recovery gate or authorize an RC flash. The reviewed implementation is committed as `0a38986`; unrelated recipe-emitter work was excluded from that commit. The reported outcomes are in section 2.9; this authorization is consumed and does not cover a repeat or button sweep.

The preparation block below selects the independently reviewed startup-restored variant in section 2.12. Both prior blink images remain unchanged and their live authorizations are consumed. The owner requested preparation of this next bounded candidate; a camera run still needs a fresh go naming the startup-restored SD blink.

Use an expendable SD card in a separate host card reader. This replaces its partition/filesystem bytes and destroys access to its existing contents. Leave the normal camera card out of this preparation. Run from the repository root in `devenv shell`. Identify the whole spare-card device from `lsblk`, not a partition. If it is mounted, unmount only its listed partitions through the file manager first. Keep the reader/card connected until readback finishes.

```bash
(
  set -euo pipefail
  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-startup-card/diagnostic-sd.img"
  EXPECTED=4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0
  BYTES=17826304
  printf '%s  %s\n' "$EXPECTED" "$IMG" | sha256sum --check
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole spare SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  [[ "$(lsblk -dn -o RO "$CARD")" == 0 ]] || { echo 'Device is read-only'; exit 1; }
  lsblk -p -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$CARD"
  read -r -p "ERASE this spare card. Type $CARD to confirm: " CONFIRM
  [[ "$CONFIRM" == "$CARD" ]] || { echo 'Cancelled'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted or swap-active device: stop and unmount the spare card only'; exit 1; }
  [[ "$(sudo blockdev --getss "$CARD")" == 512 ]] || { echo 'Reader does not expose 512-byte sectors'; exit 1; }
  CAPACITY="$(sudo blockdev --getsize64 "$CARD")"
  (( CAPACITY >= BYTES )) || { echo 'Card is smaller than the image'; exit 1; }
  sudo dd if="$IMG" of="$CARD" bs=4M conv=notrunc,fsync status=progress
  sudo blockdev --flushbufs "$CARD"
  sudo cmp --bytes="$BYTES" "$IMG" "$CARD"
  echo 'PASS: image readback matches'
)
```

Stop on any error; do not insert a failed or partially written card into the camera. `cmp` is silent on success. After the final PASS, remove the spare card from the reader. Do not mount, format, repair or copy files onto the resulting research layout, and do not run the firmware updater. The block-device, whole-disk, mount, read-only, capacity and typed-confirmation checks reduce host-write mistakes; they cannot identify the owner's intended card for them.

**Host rehearsal:** copying the image into an expendable 32 MiB regular-file fixture with `dd ... conv=notrunc,fsync` and comparing the exact 17,826,304-byte prefix succeeded while retaining fixture capacity. The block-device guard rejected that regular file. This rehearses copy/readback and a rejection path, not a real card write or the `sudo`/reader behavior. The temporary fixture was removed.

**Camera steps, only after go on the corrected risk basis:**

1. Establish a known-good ordinary boot with the normal card and note the existing film-simulation entries/settings. Then power off. If the body is currently in USB maintenance rather than ordinary operation, end that session by its documented power-cut procedure.
2. Disconnect USB. With the camera unpowered, remove the normal card and insert the verified spare card. Keep the lens and other configuration unchanged. Prepare to cut the external supply or remove the battery immediately.
3. Record the LCD and indicator from before power-on. Hold Fn and press the sub-command dial inward, turn power on, keep both held for ten seconds, then release. Do not add UP, shutter S2, dial rotation or another button combination.
4. Observe for at most 60 seconds from power-on. Look for repeating groups of three pulses with a longer hold; polarity and wall-clock timing are unverified. If ordinary UI appears, record that outcome and end the attempt. Do not accept a format/update prompt. Cut power immediately for unexpected actuation, unusual noise or heating.
5. End by cutting the external supply or removing the battery, not relying on the software power switch while the body is dark. Move the switch to OFF while unpowered, remove the spare card, reinstall the normal card, then restore power and boot with no buttons held.
6. Check ordinary boot, settings and film-simulation entries. If normal operation returns, take one ordinary photograph and check playback using the normal card. If it does not return, or any unexpected persistent effect appears, stop; do not retry the SD experiment, format a card or start an update.

| Observation | Interpretation |
| --- | --- |
| Ordinary UI or ordinary card-format prompt | The non-returning diagnostic payload was not reached in the observed attempt; do not claim that the camera never accessed the card. |
| Distinctive repeating three-pulse/long-hold pattern | Strong evidence of payload execution; this does not distinguish FAT mode 1 from raw mode 2 or establish recovery capability. |
| Dark with no signal, a steady indicator or another pattern | Inconclusive: early failure, configuration hang, other boot route, unavailable GPIO output or unobserved cadence remain possible. |

Report the visible outcome, approximate first-change time/cadence, whether normal boot/capture/playback returned after the cut, and any settings change. Retain raw video privately. One attempt is the bound; do not expand it into a button sweep.

### 2.9 Physical observations and current verdict

**Hardware-observed, as reported by the owner:** the planned card-present Fn/sub-command attempt stayed dark with no LED or other response for the full 60-second observation. The owner cut power and moved the switch to OFF. Before completing the return procedure, the owner reported an additional start holding record and Fn, outside the agreed single-attempt procedure; that start reached the normal screen with a message described as “no card initialized” and repeated red-LED blinking. The owner cut power again, returned to the procedure and reported normal boot, settings intact and the film-simulation entries present, including PRO Neg.

| Observation | Evidence bound |
| --- | --- |
| Planned Fn/sub-command start: dark, no LED for 60 seconds | No positive payload witness. SD route, mount/read/apply failure, other ROM halt, GPIO readiness and cadence remain unresolved. This is not a confirmed failure to read the files. |
| Separate record/Fn start: ordinary screen, card-initialization message, red blinking | The non-returning diagnostic payload was not reached on that start. The red LED operates in application state; its behaviour does not establish the diagnostic GPIO pair's physical identity or readiness at SD handoff. |
| Final ordinary start: settings and PRO Neg entries retained | Normal boot returned after the two reported starts and cuts. This does not establish that raw mode 2 executed, that the three changed words were applied harmlessly, or that arbitrary failures are recoverable. |

The owner subsequently confirmed that the card-preparation command finished with PASS and that ordinary photo capture and playback worked after returning to the normal card (E-SDBOOT-POSTCHECK-CONFIRM-150). The intended candidate was the blink image in section 2.7; preparation/readback was owner-reported, not an independently inspected physical-card dump. No video or raw preparation output was supplied. That experiment's post-return checks are complete and its authorization is consumed. The selected mode and payload execution remain unproved; no recovery gate was accepted. The owner's subsequent extra-register request and its separate artifact are in section 2.10.

### 2.10 Extra-register blink variant

**Owner-requested bounded experiment:** “Lets quickly try the extra register version of our blink test via sd card.” This supersedes the no-new-payload condition only for one extra-register attempt. SD boot remains off the RC release path; no passive bus-capture acquisition, button sweep, full firmware boot or persistent install is authorized.

`//host/firmware:diagnostic-blink-setup` builds the existing assembly with `LED_SETUP`, inserting exactly four instructions before the original loop: construct `0x00050001` in `r5`, store it once to `0xfff893c4`, and execute `dsb sy`. This is the [X-E2 example's disabled red-blink setup](https://github.com/tiredboffin/fffw/blob/b084c998b02a60859288c9b6dc7204e6a94f2e13/boot/xe2-led0-main.c), not an X-M1 direction-register definition. The loop, delays and three raw-mode controller differences are unchanged. The default `diagnostic-blink` binary retains its previous hash.

The distinct candidate is `private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-setup-card/`. Its 512-byte `ffboot.bin` is SHA-256 `d0fcad665118b1fcb401343597580650e813e43982f0d339bcb5ea266426591e`; the 17,826,304-byte `diagnostic-sd.img` is `d049413d0b9511bf4e2c8745087c149743d001789aeef874434d7c043cf24ce6`. The image differs from the previous blink card only at raw LBA 2 and FAT LBA 2339. Configurations, metadata and all other bytes are identical.

**Emulated smoke:** the unchanged 28-instruction, 112-byte code ran relocated to `0x00010000` in the Cortex-R5F proxy, with GPIO modeled as ordinary memory and register delays shortened to two iterations at their first loop entry. Two groups completed in 121 instructions: one setup store, twelve exact alternating toggle stores, fourteen shortened delays, no data reads and IRQ/FIQ masked. The trace is retained as `smoke-output.txt`; the temporary C driver/executable were removed. This is instruction/control-flow evidence, not electrical readiness or timing.

**Independent review:** a separate high-reasoning `openai-codex/gpt-6.1-sol` review inspected actual ELF/binary/image bytes and returned APPROVE BYTES without findings. It confirmed entry 0, no relocations, 400 zero-padding bytes, the one new store/barrier, unchanged baseline instructions and exact two-sector packaging differences. `byte-review.txt` retains its verdict and hardware limitations. ROM-parser verification passed; existing boot-card tests passed from cache. The additional GPIO write's physical effects and all three raw-mode effects remain unproved. A power cut cannot be promised to recover arbitrary failures.

**Historical handoff:** at this attempt, section 2.8 selected the setup image in this section; its current block selects the later section 2.12 candidate. The instructed setup procedure required the separate host reader and the six-step camera procedure: USB disconnected, Fn plus inward sub-command-dial press for ten seconds at power-on, observe no longer than 60 seconds, cut power, replace the normal card while unpowered, then verify ordinary boot/settings/PRO Neg/capture/playback. A repeating three-pulse/long-hold pattern would support execution, not recovery or a selected SD mode; darkness remains inconclusive. No physical card or camera operation was performed by the model.

**Hardware-observed, owner-reported outcome:** “No dice, same as last time, didn't do anything. All good after cut and swap back, restart. Everything is still there and working.” The added setup produced no visible execution witness. The owner reports ordinary operation and retained state after cutting power, swapping back and restarting. Exact duration, video and this attempt's card-readback output were not supplied; do not infer them from the planned procedure. This does not establish whether the setup instruction executed, what mode was selected or whether the raw-mode words were applied. The one-attempt authorization is consumed. Stop these blink trials and keep SD boot parked pending concrete new target-bound evidence, not further guessed setup writes.

**Hardware-observed, owner-reported card/actuation contrast:** with the diagnostic card, Fn plus pressed sub-command dial remains dark while other button combinations reach the application's uninitialized-card error; with the normal-operation card, Fn plus pressed dial boots normally into shooting. This is positive evidence of a card-and-actuation-dependent boot outcome, not an ineffective button combination. It does not identify file reads, staging or payload execution. The normal and diagnostic conditions use different physical cards, so contents/layout and card-specific behaviour are not isolated. **Static inference:** the documented combination remains the leading entry candidate for a card-sensitive boot path. If contents rather than card hardware determine the outcome, the decoded ROM selector, which samples before media reads, is not a complete explanation of entry. Preserve this distinction without proposing another LED setup or button sweep (E-SDBOOT-CARD-ACTUATION-CONTRAST-150).

### 2.11 File-first hypothesis and upstream clarification

**Owner direction:** treat our files as the primary suspect. Physical-card differences and layout are deferred unless stronger evidence points there; the contrast in section 2.10 is retained as a limitation, not the active investigation branch. This permits host analysis, not another camera trial.

**Reported external clarification:** tiredboffin says his working X-E2 `ffboot` was custom code: it began as a blinking loop and became a programmer that reads a standard DAT from FAT SD and flashes EEPROM. NAND/DMA behavior, the CPU1-neutralization sequence and bad-block management are partly understood even on that body. Building `ffboot` from DAT firmware is his current development work, not an available renamed-DAT or full-application boot recipe. His X-T4 progress does not establish X-M1 compatibility. This clarifies the earlier recovery account; do not import the custom flasher into an execution-marker experiment (E-SDBOOT-AUTHOR-CLARIFICATION-150).

**Byte-read, startup difference:** the [pinned X-E2 startup](https://github.com/tiredboffin/fffw/blob/b084c998b02a60859288c9b6dc7204e6a94f2e13/boot/xe2-start.S) writes `0xfff88084 = 1` before calling the blink code. Neither the baseline nor the LED-setup variant does; section 2.12 adds a distinct startup-restored variant. The X-M1 ROM itself writes `1` there at `0x800000e4`–`0x800000ec`, but our stock-derived `ffsys.bin` has `config+0x10 = 0x300`: apply at `0x80000328`–`0x80000330` writes the low bit, `0`, before the payload read. Target application routine `0x006568ec` initializes 16 banks at `0xfff89000 + bank*0x40`, then clears the same bit at `0x00656a18`–`0x00656a20`. These facts bind the register to target startup and GPIO initialization, not an exact hardware meaning or a demonstrated requirement for blinking.

**Byte-read, configuration priority:** stock configuration changes `0xfff88028` from ROM channel-0 bring-up's `0x80000` to `0x60000`, writes final `0xfff88030 = 0xa00`, and requests divider bits `15:6 = 0` through `0x800010ac`, all before opening/reading `ffboot.bin`. Target routine `0x006f42d4` masks bits `19:16` of `0xfff88028` and sets that field to `6` for several presets (`0x006f438c`–`0x006f43e0` among them). The final `0xa00` retains bit `0x200`. A changed field is not proof of disabled SD or an invalid frequency; source selection, physical rate and cold-start effects remain unbound.

**Emulated dataflow check:** unchanged captured ROM windows `0x80000328`–`0x80000334`, `0x800003cc`–`0x800003f8` and `0x800006a8`–`0x800006cc` consumed the current setup card's actual `ffsys.bin`. Stores confirmed `0xfff88084 = 0`, the final clock words `0x60000`/`0xa00`, and channel-0 halfword writes `0x503`, `3`, `3`, `7` from an initial `0x507`. The final divider field is zero. This throwaway Unicorn Cortex-A9 A32 interpreter used flat fixture memory and a seeded clock-ready bit. The Cortex-R5F attempts raised `UC_ERR_EXCEPTION` at the first window and supplied no successful replay; this is not an admitted target-core native case or hardware readiness evidence. The bounded successful output is retained under `private-data/scratch/sd-file-entry-smoke-output.txt`; the temporary driver and executable are removed.

**Next bounded action:** inspect the `0xfff88084` startup difference as one candidate change, preserving configuration/layout and existing GPIO stores. A startup-faithful variant would restore the upstream/target-ROM `1` before the loop; whether that produces a visible marker remains unproved. It needs exact-byte review and a fresh operation-specific go before a camera run. Ask tiredboffin for the original blink's `ffsys` preparation and any startup prerequisites, not his NAND programmer. Do not bundle CPU1-neutralization, NAND access, clock changes or a full firmware payload into that candidate (E-SDBOOT-FILE-ENTRY-AUDIT-150).

### 2.12 Startup-restored blink candidate

**Owner-requested host preparation:** “continue to the next bounded candidate.” `//host/firmware:diagnostic-blink-startup` enables `LED_SETUP` and `STARTUP_RESTORE` in the existing assembly. After the unchanged `cpsid if`, five instructions construct `0xfff88084` in `r6`, construct `1` in `r7`, store that word once and execute `dsb sy`. The existing setup and loop bytes follow unchanged. This restores the upstream/target-ROM startup write identified in section 2.11; it does not establish the register's meaning or an LED requirement.

The distinct directory is `private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-startup-card/`. `ffboot.bin` is 512 bytes, SHA-256 `30b8f5c0b53323a78cd2bdf971b2849b7f409b40132af73f328c9ca1e880de03`; `diagnostic-sd.img` is 17,826,304 bytes, SHA-256 `4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0`. `ffsys.bin` retains SHA-256 `79859bdd17234f2e09caa6f229008f6b32bb7238fc10dac499c5e4b4fa4e0f0b`. Both payload sectors equal the new binary; every image byte outside raw LBA 2 and FAT LBA 2339 equals the previous setup card. Configuration, raw overlay, metadata and delay constants are unchanged. Both older payload hashes remain unchanged.

**Exercised verification:** the packager's two ROM-parser paths passed. Existing `//host/firmware:boot-card-tests` ran with cache disabled and passed. The emitted payload's unchanged bytes ran relocated to `0x10000` in the Cortex-R5F proxy: two groups in 126 instructions, one startup store, one setup store, twelve alternating GPIO stores, fourteen shortened delay loops, no data reads and final IRQ/FIQ masking. GPIO registers were ordinary fixture memory; the hook shortened delay registers to two iterations without changing payload bytes. This proves bounded instruction/control flow, not electrical behavior, original-address handoff, physical timing or recovery. `smoke-output.txt` retains the result; temporary driver/executable were removed.

**Independent review:** an explicit high-reasoning `openai-codex/gpt-6.1-sol` read-only review returned APPROVE BYTES with no blocking findings after inspecting the actual ELF, every payload byte and complete image comparisons. It verified entry 0, no relocations, 132 code bytes, 380 zero-padding bytes, the exact five-instruction insertion, unchanged retained code and old hashes, and exact two-sector packaging. `byte-review.txt` retains the verdict and limits. The first reviewer was rate-limited and supplied no review.

**Preparation smoke:** the exact current section 2.8 block passed `bash -n`; its hash check accepted this image, and supplying a disposable regular file stopped with “Not a block device” before `sudo`, `dd` or any device write. This exercises syntax/hash/rejection, not a real card write or reader behavior. The earlier regular-file copy/readback rehearsal remains the copy-path evidence.

**Conditional owner handoff:** use the section 2.8 guarded write/readback block only on the expendable card in the separate host reader. Stop unless it prints PASS. Before camera insertion or power-on, obtain a fresh go naming one startup-restored SD blink attempt. Then use exactly section 2.8's six steps, with a 60-second ceiling and the cut/swap/ordinary-boot/capture/playback return checks. No button sweep, NAND programmer, full firmware boot, card-format prompt or updater action. The extra register's LED effect and the three unchanged raw-mode register effects remain unproved; power removal is not a guaranteed recovery. SD boot remains off the RC release path (E-SDBOOT-BLINK-STARTUP-CANDIDATE-150).

### 2.13 Owner-reported repeating red marker

**Hardware-observed, owner-reported:** after an initial start produced “nothing,” the owner turned the camera off and started again holding Fn, the sub-command-dial push and shutter. The owner observed three approximately three-second blinks, a probably 30-second-plus steady interval, three more blinks and another steady interval. A subsequent report confirms that the second long interval ended, a third triplet occurred and another long steady interval began. All illuminated intervals were red. The owner later clarified that shutter was fully pressed. The first start's exact actuation/duration, the off method, hold/release timing and this attempt's preparation/readback output were not supplied.

**Static inference:** repeated triplets separated by a much longer steady-red interval are strong evidence of the diagnostic payload executing, consistent with inverted LED polarity. With inverted polarity the final illuminated interval spans one short delay plus the eight-unit group gap. Exact ON/OFF phase and duration measurements or video are unavailable; do not treat the owner's word “blinks” as an edge trace. This is a positive marker witness, unlike the earlier dark-only outcomes. It does not establish FAT versus raw entry, exact card-byte identity, why the additional shutter press mattered, or whether the startup store alone enabled the marker. Both payload startup and actuation differ from the previous setup experiment.

**Containment and return:** the second start and added shutter press were outside the planned single-attempt actuation; no fresh named live go is recorded between the candidate handoff and those initial reports. The model instructed an immediate supply cut/battery removal, switch OFF while unpowered, normal-card replacement and ordinary boot/settings/film-simulation/photo/playback checks. The owner subsequently confirmed normal start, everything working and settings intact, closing the qualitative return checks. The exact cut method and per-setting byte readback were not supplied. The marker does not demonstrate a full application boot, an internal-media writer or repair capability (E-SDBOOT-BLINK-STARTUP-OBSERVED-150; E-SDBOOT-BLINK-STARTUP-RETURN-150).

**Failed reproduction, owner-reported:** the prepared single contrast was Fn plus fully pressed shutter without the dial; the owner instead reports a handful of further attempts with battery cuts between them. Fn plus held sub-command-dial push, with or without shutter, produced a dark screen and no LED. The owner reports the other pairs booted normally with an SD-card-uninitialized warning. The pair report is aggregate, not an enumerated controlled matrix; exact attempt count, hold/release timing, retry shutter depth, durations and card readback are unavailable. The owner restored the normal SD card and confirms settings intact and shooting working; playback was not separately reported. The earlier marker remains an isolated witness, not a reproducible button recipe. Darkness cannot distinguish failure to fetch, failure to hand off or indicator failure, and ordinary boot does not identify a specific ROM selector (E-SDBOOT-BLINK-STARTUP-NONREPRO-150).

**Post-attempt readback, owner-reported:** a host-reader snapshot of the first 17,826,304 bytes matches the pinned startup-restored image by SHA-256 and `cmp`. The snapshot is `private-data/analysis/sd-card-readback/readback.uXfGnzcf/card-prefix.bin`; both hashes are `4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0`. This excludes a retained byte difference in the staged region, including both payload copies, configuration and filesystem metadata within it, as the cause of failed reproduction. It does not exclude changes beyond the image, transient changes later reversed or card-controller state, and does not establish what the camera fetched during the marker. No rewrite or further button retry follows (E-SDBOOT-BLINK-STARTUP-READBACK-150).

### 2.14 Failed-boot recovery state

The revision-2 install completed but ordinary startup and updater-menu access failed (E-RECORDER-BOOT-FAILURE-150, E-RECORDER-RECOVERY-UNREACHABLE-150). The supported USB identity was also unavailable on the attempted maintenance entry, before any service transfer (E-RECORDER-USB-UNAVAILABLE-150). These negative observations motivated independent native bootstrap work; they did not identify the failing stage or authorize a repeat. Copying a replacement DAT to a card did not constitute an installed repair. The later bounded v5 physical repair and reported ordinary operation supersede this incident state; sections 2.37–2.60 retain the applicable technical findings.

### 2.15 Stock-native RAM rollback candidate

`//host/firmware:build-recovery-card` emits the current host-verified candidate under `private-data/analysis/x-m1-1.50/bootrom/native-recovery-frame-card/`. It takes only the pinned official 1.50 DAT and the current build's hash-pinned linked rollback stub. It preserves the full 256 KiB native staging span, including both boot records, except for the primary reset literal at `0x20`, the successful post-image-load call at `0x1fdc`, and the new 168-byte code at `0x6000`. The primary BSS ends at `0x5280`; the companion record starts at `0x10000`.

The entry guard admits the FAT SD loader's saved return, as exercised and reviewed in section 2.18, without re-reading live inputs. It refuses the modeled ordinary internal-media and raw SD returns. Raw staging remains in the image, with loader count 512 and the same three MBR-derived controller-register differences; configuration apply occurs before the guard, so refusal does not undo those writes.

After the stock loader succeeds, the rollback accepts only the known recorder receive word or the stock word at `0x0071feb4`. It restores the recorder word to stock, performs local cache maintenance, and continues through the original close callback before peer release. An unknown word or nonzero stock discriminator halts at that checkpoint; this late check does not guard earlier native initialization. The primary reset literal remains patched after kernel startup, although the TCM copy uses the unchanged companion vectors. The current image is not conventionally partitioned and does not contain the 38 MiB restore DAT.

The actual host emitter and saved-image verification passed. The generalized card builder reproduced the existing 512-byte marker card byte-for-byte by its recorded image hash; payload-boundary tests passed. Seven throwaway compiled-payload smoke scenarios exercised entry refusal, recorder/stock/unknown words and the continuation ABI. That smoke excluded native initialization, substituted CPU classification and skipped CP15 operations: it proves neither physical boot nor cache coherence. The ELF has only 168 allocated text bytes and no relocations. Independent reviews approve the bounded byte/layout and callback ABI contracts, not a camera run (E-NATIVE-SD-ROLLBACK-CANDIDATE-150).

Native safety remains conditional on mode-1 entry by the intended discriminator-zero processor, correct peer activation/TCM mapping, and coherent low/bit-30 RAM aliases. This bootstrap still loads installed native segments and id4 and checks the installed completion marker; it cannot repair corrupt loaders, segment tables or a missing marker. Only the injected rollback is RAM-only. Inherited application startup writes retained state and can conditionally program settings media on rejected headers/read failures. Any proposed run must name and accept those inherited effects, not describe the complete boot as nonpersistent.

```sh
devenv shell -- bazel build //host/firmware:build-recovery-card //host/firmware:native-rollback
```

A successful temporary application boot would not repair the installed recorder image. Permanent recovery still needs a readable card carrying the official DAT, a reachable native updater, an operation-specific owner go, and a verified normal cold boot after installation. Do not substitute another blink for that restoration contract.

### 2.16 Bounded native recovery-boot attempt

The owner authorized one native SD recovery-boot attempt on 2026-10-11 with “Okay lets go, walk me through it,” after discussion of inherited retained/settings writes and preserving a successful powered session (E-NATIVE-SD-ROLLBACK-GO-150). This is not a stock-reflash authorization. Mode-1 physical entry, peer startup and cache/alias behavior are trial uncertainties, not byte-review findings already demonstrated on hardware. No updater, format, settings edit, USB service operation or button sweep is included.

The attempt is complete and its authorization consumed (E-NATIVE-SD-ROLLBACK-DARK-150). The owner reported PASS from the 17,826,304-byte card write/readback, then no response after the instructed power-on, no LCD illumination or LED activity at any point, and confirmed power removal. No application foothold or restoration was observed. The dark result does not establish whether either ROM loader read the payload, whether the guard ran/refused, or whether native loading/peer/cache behavior failed. Stop camera operations; do not repeat this candidate or sweep buttons.

The preparation and camera procedure below remain conditional on the fresh named go in section 2.18. They now select the saved-frame candidate, not the image used in the completed dark trial. Prepare only the expendable card in a separate host reader, with the camera disconnected. Run from the repository root in `devenv shell`. This replaces the card's partition/filesystem bytes and destroys access to its contents. Identify the whole card device yourself; do not reuse a previous `/dev/sda` listing. Unmount only the spare card's listed partitions through the file manager first.

```bash
(
  set -euo pipefail
  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/native-recovery-frame-card/native-recovery-sd.img"
  EXPECTED=38271a89abae8e750ad5cc84b842989e9f3cfeee68b2e76ac8aeda766ddd3f6f
  BYTES=17826304
  printf '%s  %s\n' "$EXPECTED" "$IMG" | sha256sum --check
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole spare SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  [[ "$(lsblk -dn -o RO "$CARD")" == 0 ]] || { echo 'Device is read-only'; exit 1; }
  lsblk -p -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$CARD"
  read -r -p "ERASE this spare card. Type $CARD to confirm: " CONFIRM
  [[ "$CONFIRM" == "$CARD" ]] || { echo 'Cancelled'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted or swap-active device: stop and unmount the spare card only'; exit 1; }
  [[ "$(sudo blockdev --getss "$CARD")" == 512 ]] || { echo 'Reader does not expose 512-byte sectors'; exit 1; }
  CAPACITY="$(sudo blockdev --getsize64 "$CARD")"
  (( CAPACITY >= BYTES )) || { echo 'Card is smaller than the image'; exit 1; }
  sudo dd if="$IMG" of="$CARD" bs=4M conv=notrunc,fsync status=progress
  sudo blockdev --flushbufs "$CARD"
  sudo cmp --bytes="$BYTES" "$IMG" "$CARD"
  echo 'PASS: image readback matches'
)
```

Stop on any error. After PASS, remove the spare card without mounting, formatting, repairing or copying files onto it. The prefix size and write/readback sequence match the previously rehearsed preparation; the model performs no physical card write.

1. Leave the body unpowered and USB disconnected. Set aside the normal card, insert the verified spare card, and keep the lens/configuration unchanged. Be ready to disconnect the external supply or remove the battery immediately. Do not require a healthy baseline from the currently failed body.
2. Record the screen and indicator. Hold Fn, the sub-command-dial inward press and full shutter, then power on once. Release all buttons as soon as UI appears or after ten seconds, whichever comes first. This is the combination associated with the isolated marker, not a proven reproducible entry.
3. Observe for at most 60 seconds from power-on. No blink sequence is programmed. Ordinary UI, a stock menu or a card-initialization message is a useful application-startup foothold; report the exact screen immediately. Do not accept a format/update prompt or change settings.
4. If application UI appears, leave the camera powered and the spare card installed while reporting it. The bounded startup trial has reached its observation endpoint; cutting power, rebooting or swapping cards can discard the RAM rollback. Do not flash or connect USB without a separately agreed next operation.
5. If the body remains dark at 60 seconds, loops or shows unexpected actuation/noise/heating, cut supply or remove the battery; do not rely on the software switch. Move the switch to OFF while unpowered and stop. Darkness cannot distinguish strap refusal, native load failure, peer wait or cache/alias failure. Do not repeat or sweep buttons.

Report time to first visible change, exact LCD text, LED behavior and whether the body remains powered. A successful screen alone establishes neither patched-word readback nor permanent recovery.


### 2.17 Loader-selection witness lead

The tried guard re-read the live strap inputs after ROM loading/configuration. A changed input could reject a historically selected mode 1; that possibility is not evidence that it caused the dark trial. The ROM does not forward the decoded mode in `r0`: modes 1 and 4 both feed 1 into the common handoff, which writes 2 to `0xfff98008` and ultimately sets `r0` to the jump destination 0.

The captured instructions supply a narrower witness (E-ROM-LOADER-STACK-WITNESS-150): ROM SP starts at `0xfff00f00`, and each loader leaves its caller return at incoming-SP minus 4 after the successful `pop {...,pc}`. In the high-address disassembly domain, mode 1 leaves `0x8000015c`, mode 2 `0x80000174` and mode 4 `0x800001a4`. In a modeled low reset alias those same relative calls leave `0x15c`, `0x174` and `0x1a4`. Dispatch after return, the common handoff and the primary vector do not push or change SP. The executing camera's caller-PC domain has not been captured; the printed disassembly base does not establish it.

The linked guard in section 2.18 now checks the saved frame and accepts either modeled mode-1 caller tag. `lr` alone is not the saved tag: the loader returns through `pop {...,pc}` after nested calls, without restoring the LR register. The host smoke executes captured frame/handoff instructions with declared storage/decoder/remap substitutions. It is not proof of physical loader selection, complete loader-body execution or native initialization. The old trial remains dark and its go consumed.

### 2.18 Saved-frame guard candidate

The replacement is built and independently byte-reviewed (E-NATIVE-SD-FRAME-GUARD-150). It validates SP equals `0xfff00f00` before reading `[sp-4]`, clears only bit 31 of the saved return, and admits exactly `0x15c` or `0x8000015c`. No live GPIO or selector is re-read. Admission restores `r0=0` and APSR NZCVQ=0 before branching to unchanged stock reset `0x1800`. The 104-byte callback at `0x6040` is identical to the previously reviewed candidate.

The [host smoke](../emulation/README.md#captured-rom-entry-frame-smoke) passed sixteen captured-frame cases and twenty linked-guard cases, over modes 0–7 in both modeled ROM domains. The former live-input guard admitted a non-FAT frame under conflicting late samples and failed the same admission contract. The replacement admits mode 1 despite those samples, rejects every other mode and changed-SP/changed-tag cases, and preserves the tested native-entry ABI. Storage bodies and decoder outcomes are substitutions; no native boot or camera restoration is claimed.

`FrameGuardByteReview` approved the actual ELF, binary, canonical card layout, branch encodings and unchanged callback. `FrameWitnessReview` found no blocker in the frame attribution or bounded smoke claim. These are independent host reviews, not hardware acceptance.

The prepared candidate is `private-data/analysis/x-m1-1.50/bootrom/native-recovery-frame-card/native-recovery-sd.img`, 17,826,304 bytes, SHA-256 `38271a89abae8e750ad5cc84b842989e9f3cfeee68b2e76ac8aeda766ddd3f6f`. Its configuration is unchanged. Only guard/padding bytes differ from the tried image, in both raw and FAT payload copies. Verify it without device I/O:

```sh
devenv shell -- bazel run //host/firmware:build-recovery-card -- verify "$PWD/private-data/firmware/x-m1-1.50/FWUP0002.DAT" "$PWD/bazel-bin/host/firmware/native-rollback.bin" "$PWD/private-data/analysis/x-m1-1.50/bootrom/native-recovery-frame-card/native-recovery-sd.img"
```

The owner-authorized saved-frame attempt is complete and its go consumed (E-NATIVE-SD-FRAME-GO-150, E-NATIVE-SD-FRAME-DARK-150). Guarded preparation printed PASS after writing/readback of 17,826,304 bytes. The owner reported a non-responsive black screen with no LED, then confirmed physical power removal. No application foothold or restoration was observed. No repeat, button sweep, format, updater or USB operation is authorized.

Both native cards produced the same reported dark outcome despite the guard-only byte change. That does not exclude either guard or prove the payload executed: selection/loading failure, rejection of an unexpected saved frame or non-FAT route, and later native/peer/cache failure remain indistinguishable without a stage witness. Another uninstrumented guard variant is not justified by this result.

This removes a host-demonstrated guard failure, not the unknown hardware entry or later native failures. Installed loaders/id4/table/trailer, peer activation and cache/alias visibility remain prerequisites. The card still lacks the restore DAT and same-powered-session restore delivery. Only the injected rollback is RAM-only; inherited startup can write retained state and settings, and configuration application precedes the guard. No recovery level advances.

**Consumed repeat authorization:** on 2026-10-12 the owner said “Yeah, go” for one repeat of the unchanged saved-frame image after disclosure of the unchanged raw-configuration and inherited startup-write risks (E-NATIVE-SD-FRAME-REPEAT-GO-150). Host SHA-256 verification matched the reviewed image, and the owner reported PASS from its guarded preparation. That authorization covered no button sweep, format, flash, USB operation or additional start.

**Repeat outcome, owner-reported:** after reporting PASS from card preparation, the owner reports “Nope, still dark at 60s.” No application foothold was reported. LED behavior and physical power removal have not been confirmed; power removal was instructed. E-NATIVE-SD-FRAME-REPEAT-GO-150 is consumed. Darkness does not establish the selected ROM route, payload non-execution or a particular failed stage (E-NATIVE-SD-FRAME-REPEAT-FAILED-150). Host investigation now checks the deliberately refused raw-mode entry and the FAT configuration's SD-clock transition; no further camera start is authorized.

### 2.19 X-Pro1 factory-package cross-check

The supplied X-Pro1 service manual was reviewed with Sol medium (E-XPRO1-FACTORY-CROSSCHECK). This is sibling documentary evidence, not an X-M1 entry or register definition. References below give printed page / PDF page.

| Source | Documentary fact | Recovery consequence |
| --- | --- | --- |
| 3-9 / 70 | Copy `ffboot.bin` and `ffsys.bin` from SD-BOOT package ZJ01970; rename adjustment-package `X-Pro1_W\SCRIPT\LX10\Firmware\LX10RCS***.bin` to `ffuser.bin`. | The factory firmware file has separate service-package provenance. Its encoding and equivalence to a public DAT are unspecified. |
| 3-2 / 63; 3-16 / 77 | A replacement board lacks firmware; the factory sequence installs it from SD with Fn and the command lever, without USB, before a separate power cycle. | Supports the documented factory use, not execution of our native card or application independence on this body. No hold/release timing or shutter requirement is supplied. |
| 3-16–3-17 / 77–78 | Subsequent USB `[I]` initialization resets flash ROM, formats internal memory and resets adjustment data on an unreplaced board. | Firmware installation and destructive initialization are distinct steps. The manual does not prove that the SD step alone preserves calibration. |
| 3-12–3-15 / 73–76 | USB BOOT/jig setup uses shutter and Up, a green-LED check and the adjustment application. | No separate ROM USB protocol or hardware-forcing procedure is documented. |

The X-M1 equivalents are already recorded in E-SERVICE-MANUAL: SD-BOOT package ZJ02217 and service firmware `X-M1_W\SCRIPT\LX30A\Firmware\LX30ADSC***.bin`. Neither service kit nor service firmware has been obtained or compared with the public DAT. This review adds no new entry candidate; it corroborates the factory-package gap rather than reopening button/layout experiments. Sibling files are not proposed for the X-M1. The two dark native trials and consumed authorizations remain unchanged.

### 2.20 Self-contained carrier and stage journal

**Built host artifact, not a persistent repair:** `//host/firmware:build-recovery-card` emits a 68,159,488-byte image whose mode-1 FAT16 volume carries the exact pinned official 1.50 `FWUP0002.DAT` alongside `ffsys.bin` and the 256 KiB native bootstrap. The current journal-only image is `private-data/analysis/x-m1-1.50/bootrom/native-recovery-journal-halt-card/native-recovery-sd.img`, SHA-256 `7d06fbdef1e773fb95f1be4ede309ad0f0f2881ecc554ce8f7ea875e30297b23`; `ffboot.bin` is SHA-256 `c6d04bcad90211ec52657b09aac5448ee4a2303322aaf78e369e24b9d0ae6f0c`. The builder verifies the pinned DAT hash and container contract, rereads all three files through the modeled ROM FAT parser, checks the raw-mode bootstrap and requires four initially zero report sectors. Raw mode stages the same guarded bootstrap, whose saved-frame check refuses execution outside mode 1.

The DAT is a carrier file only. No target writer opens or consumes it. The bootstrap still enters the installed primary native loader, uses its image table and completion state, restores one RAM call word and closes the installed native image. After the stage-3 report attempt it halts before the stock peer release at `0x1ff8`; application startup and UI are intentionally unreachable on the admitted path. This improves card completeness and observability but does not remove the installed-media dependencies or persistently repair the body.

**Persistent-repair contract:** a future direct writer must enter from the guarded mode-1 payload without calling the installed native reset or trusting the installed native loaders, segment table, completion marker or image records. Before any persistent write it must validate the exact carried DAT and its identity/container structure, independently identify the internal-media layout and current residue, preserve body-specific configuration and calibration, and record a validated pre-write stage. It must then apply an interruption-safe official restore sequence, read back every written region against the selected source and publish completion only after full verification. Any unknown identity, mapping, current state, restore-list interpretation or verification mismatch refuses before the first persistent write. The missing X-M1 storage-boundary/configuration-completion map and a target-bound internal-media writer block implementation; the carrier is not a stubbed writer and makes no R2-R4 recovery claim.

Four raw sectors immediately after the FAT partition provide independent durable stage slots:

| LBA | Stage | Successful record means |
| --- | --- | --- |
| 133120 | payload entry | The saved-frame guard admitted mode 1 and the reporter ran before installed native reset. |
| 133121 | native load complete | The primary discriminator accepted the native load and reached the rollback hook. |
| 133122 | tap restored/coherent | The tap held the stock or expected recorder word, was set to stock and local cache maintenance completed. |
| 133123 | native close complete | The original native close returned before peer release. |

Each 512-byte record has magic `XM1STAGE`, format version, stage and complement, a stage-specific evidence word, mode/ready/CCS/present/clock/status snapshots, `COMP` and an additive zero-sum checksum. A stage has evidence only when its own sector parses completely; a zero sector is absent and a torn or malformed sector is invalid. `build-recovery-card inspect IMAGE` decodes all four without changing the image. Later stages do not replace earlier records.

**Emulated reporter:** the consumed image's helper used inferred CMD24. Current `//emulation:recovery-stage-report-smoke` executes the revised linked ARM reporter at `0x6200` with the X-M1 stock one-block CMD25 grammar: transfer mode `0x0026`, command `0x193a`, block count 1. Success performs CMD13, CMD25, CMD13, CMD17 and exact readback, returning zero with a valid record. No-ready refusal returns 1 without a record; withheld write-ready returns 4 without a record; interruption after 64 FIFO words leaves a torn record without `COMP`; corrupted readback returns 10 after the card received a valid record; CMD25 R1 error returns 3 before FIFO access. The helper rejects the standard R1 error mask, acknowledges only consumed normal-status bits, bounds every poll and performs no retry, controller reset/reinitialization, DMA or internal-media access.

This is host evidence, not hardware execution. X-M1 1.50 target bytes bind CMD25, the PIO FIFO direction and 128-word sector feed ([card report section 6.3](diagnostic-sd-boot-card.md#63-x-m1-stock-cmd25-pio-binding)). Interrupt-free polling equivalence, command/busy completion, post-configuration clock behavior and cache visibility remain unproved. The independent review and authorization cited below apply only to the consumed CMD24 image, not the revised bytes.

### 2.21 Journal-only camera attempt

**Operation boundary:** one cold startup with the exact journal-only image above. The intended admitted path always remains dark: it attempts one write/readback in each of four fixed report sectors, restores the RAM tap, closes the installed native image, attempts the fourth report and halts before peer release. It does not start the application, expose UI or consume the carried DAT. Configuration application still precedes the saved-frame guard, including the three raw-route register differences, and the native loader still depends on installed records. The operation accepts the unproved CMD24/FIFO/clock/cache behavior to obtain the first durable stage witness. No format, updater, USB operation, internal-media writer, repeat or button sweep is included.

**Consumed authorization:** after independent approval and disclosure of the remaining silicon CMD24/FIFO/clock/cache risks, the owner selected “Go for one journal-only attempt” for exact image SHA-256 `7d06fbdef1e773fb95f1be4ede309ad0f0f2881ecc554ce8f7ea875e30297b23` and this section's procedure (E-RECOVERY-STAGE-JOURNAL-GO-150). The go covered the destructive expendable-card preparation, one startup and one post-run raw-card capture. It authorized no repeat or format, updater, USB, internal-media writer or button-sweep operation.

Prepare an expendable SD card in a separate host reader while the camera is disconnected. This destroys access to the card's existing contents. Run from the repository root in `devenv shell`, identify the whole device from the fresh listing and unmount only that card's partitions first:

```bash
(
  set -euo pipefail
  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/native-recovery-journal-halt-card/native-recovery-sd.img"
  EXPECTED=7d06fbdef1e773fb95f1be4ede309ad0f0f2881ecc554ce8f7ea875e30297b23
  BYTES=68159488
  printf '%s  %s\n' "$EXPECTED" "$IMG" | sha256sum --check
  bazel-bin/host/firmware/build-recovery-card inspect "$IMG"
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole expendable SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  [[ "$(lsblk -dn -o RO "$CARD")" == 0 ]] || { echo 'Device is read-only'; exit 1; }
  lsblk -p -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$CARD"
  read -r -p "ERASE this expendable card. Type $CARD to confirm: " CONFIRM
  [[ "$CONFIRM" == "$CARD" ]] || { echo 'Cancelled'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted or swap-active device: stop and unmount the expendable card only'; exit 1; }
  [[ "$(sudo blockdev --getss "$CARD")" == 512 ]] || { echo 'Reader does not expose 512-byte sectors'; exit 1; }
  CAPACITY="$(sudo blockdev --getsize64 "$CARD")"
  (( CAPACITY >= BYTES )) || { echo 'Card is smaller than the image'; exit 1; }
  sudo dd if="$IMG" of="$CARD" bs=4M conv=notrunc,fsync status=progress
  sudo blockdev --flushbufs "$CARD"
  sudo cmp --bytes="$BYTES" "$IMG" "$CARD"
  echo 'PASS: image readback matches'
)
```

Stop on any error. The initial `inspect` must print all four stages as `absent`. After PASS, remove the card without mounting, formatting, repairing or copying files onto it.

1. Leave the body unpowered and USB disconnected. Set aside the normal card, insert the verified expendable card and be ready to disconnect the supply or remove the battery.
2. Record the screen and indicator. Hold Fn, the sub-command-dial inward press and full shutter, then power on once. Release all buttons after ten seconds. No visible marker or UI is programmed.
3. Observe until 60 seconds from power-on. If UI unexpectedly appears, leave the camera powered with the card installed and report the exact screen; do not select anything. Otherwise, at 60 seconds disconnect the supply or remove the battery, move the switch to OFF while unpowered and remove the card. Do not repeat.
4. With the camera disconnected, place the card in the host reader. Do not mount it. Capture and decode the exact written prefix:

```bash
(
  set -euo pipefail
  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/native-recovery-journal-halt-card/native-recovery-sd.img"
  BYTES=68159488
  UNCHANGED=68157440
  SECTORS=133124
  OUT="$PWD/private-data/sessions/$(date -u +%Y%m%dT%H%M%SZ)-recovery-stage-card.img"
  [[ ! -e "$OUT" ]] || { echo 'Output already exists'; exit 1; }
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole recovery SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" && "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole block device'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Card is mounted: stop and unmount it'; exit 1; }
  sudo dd if="$CARD" bs=512 count="$SECTORS" iflag=fullblock status=progress |
    dd of="$OUT" bs=4M conv=fsync status=none
  [[ "$(stat -c %s "$OUT")" == "$BYTES" ]] || { echo 'Captured size mismatch'; exit 1; }
  cmp --bytes="$UNCHANGED" "$IMG" "$OUT"
  bazel-bin/host/firmware/build-recovery-card inspect "$OUT"
  echo "PRESERVE: $OUT"
)
```

`cmp` covers every byte before the four report sectors and is silent on success. Preserve the snapshot regardless of decoder output. A valid stage record means that complete matching sector survived to host readback; it does not prove the helper observed command completion. An absent or invalid later stage narrows progress only after the last valid stage. The zero-before-attempt and no-repeat rules prevent a valid stale record from being attributed to this attempt.

**Hardware-observed outcome:** the owner reported the initial image hash check passed, all four report sectors decoded absent, the 68,159,488-byte expendable-card write/readback printed PASS, and the camera was still dark at the observation endpoint. After the powered attempt, the 133,124-sector capture completed and the unchanged-prefix comparison returned silently; all four sectors again decoded absent. The preserved snapshot is `private-data/sessions/20261012T033427Z-recovery-stage-card.img`, SHA-256 `7d06fbdef1e773fb95f1be4ede309ad0f0f2881ecc554ce8f7ea875e30297b23`, byte-identical by whole-image hash to the prepared image. No report or other byte on the captured image changed (E-RECOVERY-STAGE-JOURNAL-ABSENT-150).

The result supplies no positive stage witness. It does not establish that the saved-frame guard ran: a non-mode-1 route, failure before payload handoff, reporter precondition or R1 refusal, and an unavailable CMD24/FIFO path can all leave stage 0 zero. With no valid earlier stage, absence of stages 1-3 adds no ordering claim. The one-attempt authorization is consumed; do not repeat this image.

### 2.22 Divider-preserving native recovery candidate

The next source-bound candidate changes the FAT configuration, not the native rollback or journal code (E-SD-DIVIDER-CANDIDATE-150). The ROM initializes SD0's divider selector to `0x500`, reads `ffsys.bin`, then applies its optional field at `config+0x33c`. The stock-derived word is `0x80000000`: ROM `0x800006a8`–`0x800006c8` calls the channel-0 helper with selector zero. That helper replaces the divider before the subsequent directory and `ffboot.bin` reads. No measured frequency or physical overclock is claimed.

The candidate clears only that optional gate, `0x80000000 -> 0`, at the FAT configuration's byte `+0x33f`. The ROM then branches directly to the unchanged seven-pair loop at `0x800006cc`, preserving the controller divider installed by its own successful initialization. Global clock-tree application remains, including `0xfff88028=0x60000` and `0xfff88030=0xa00`; this is not full source-clock preservation.

**Exact reviewed artifact:** `private-data/analysis/x-m1-1.50/bootrom/native-recovery-divider-card/native-recovery-sd.img`, 17,826,304 bytes, SHA-256 `ad2541463b16bbb9756bbc8f2f5bba8621d04f44e34b45a5c4923b0b16c05131`. Relative to the reviewed saved-frame card, exactly one byte changes at zero-based offset 1,197,375 (`2337*512+0x33f`). Both FAT copies, root entries, file chains, complete 256 KiB `ffboot.bin`, raw configuration and raw payload are unchanged. This is the application-startup rollback card, not the later journal-only carrier; it contains no restore DAT and no SD reporter.

**Executed host comparison:** a passive clock-write observer over captured ROM shows the unchanged card rewriting SD0 `+0x2c` from selector `0x500` to zero, while the candidate skips that helper. Both primary runs complete original close and restore the recorder tap; all seven full output snapshots compare byte-for-byte. The separate two-CPU candidate run reaches both kernel entries with source-equal kernels and the complete expected post-rollback image 4. Source/device timing, global clock effects, clean media, reset/aliases and coherent caches remain fixtures. The temporary observer build target was removed.

**Independent review:** Sol high returned APPROVE BYTES for the exact candidate hash after checking whole-image confinement, FAT geometry/chains, captured gate/helper control flow and unchanged entry/close ABI. This is not physical clock or recovery approval. The byte change supplies a distinct finite hypothesis for a future attempt; it does not explain the earlier dark outcomes.

**Raw-route closure:** E-RAW-SD-STAGING-150 executes mode 2 with a declared virtual SD1 card and the unchanged saved-frame card. Captured ROM performs 514 sector reads, transfers all 256 KiB, and reaches guard `0x6000` with source-equal staging, `SP=0xfff00f00`, saved return `0x174`, `r0=0` and NZCVQ clear. That guard deliberately admits only `0x15c`; its source therefore rejects the raw frame. The observation stops before executing the guard, not after native loading. Physical SD1 wiring, global-clock effects and the three raw controller-register changes remain unproved.

Simply admitting `0x174` is not the next journal change: raw mode initializes SD1 and shared RCA/CCS state, while the current reporter issues commands through fixed SD0. A raw-capable reporter would need route-bound controller selection. Also, raw configuration applies the same global clock words and hardwired SD0 divider helper before fetching its payload. The declared model does not establish that those transitions work on silicon.

**Raw-native host result:** the reporter mismatch is specific to journal payloads. [The raw-only native candidate](#231-raw-only-native-recovery-host-result) binds SD1 configuration, removes the MBR conflicts and executes captured-ROM loading through both kernel entries with the complete expected RAM rollback. No physical entry or new camera go follows.

**Trial decision:** the one-byte, divider-preserving application-startup card was selected as the smallest distinct attempt and has now been tried without a visible foothold. Its outcome and consumed authorization are below. The journal-only carrier, separate full-RAM candidates and raw-native host lead remain distinct; do not transfer authorizations between their bytes.

**Consumed authorization:** the owner said “go for one divider-preserving native-startup attempt” (E-SD-DIVIDER-GO-150). Host re-verification matched the exact image hash and 17,826,304-byte size. The owner has now reported PASS from preparation and no visible change on startup; this go is consumed.

The authorized procedure was section 2.16's native procedure, not section 2.8's older blink procedure: physically unpowered body, USB disconnected, verified spare inserted, lens/configuration unchanged; hold Fn, inward sub-command-dial press and full shutter, power on once, release at the first UI or ten seconds, and observe at most 60 seconds. No blink was programmed. The go covered no repeat, button sweep, updater, format, USB operation or persistent restore.

**Outcome:** no application foothold was reported. The concurrent recovery thread obtained the owner's 60-second dark report and a preserved snapshot whose entire 17,826,304-byte prefix matches the prepared divider card; independent `cmp` here also passed (E-SD-DIVIDER-MIXUP-DARK-150). There is no visible progress witness or changed byte in that prefix. Physical power removal remains unconfirmed in this thread and has been instructed. Do not repeat the divider card. This result does not establish the selected ROM route, configuration/payload execution or the failed stage; the raw path retains the original divider configuration. It is not a result for the separate full-RAM candidate in sections 2.24–2.27.

### 2.23 CMD25 journal candidate

The fetched current `x70view` SD implementation exposed a stock PIO write lead, then X-M1 1.50's own bytes closed the target binding: `0x006f2bf4` constructs packed request `0x193a0026`, and `0x006f32a4` feeds 512-byte units through `0xfff50020`. The reporter now uses that request as split transfer/command halfwords `0x0026/0x193a`, block count 1, and record attempt identifier 2. No X70 address or target-specific payload byte was copied.

**Built host artifact:** `private-data/analysis/x-m1-1.50/bootrom/native-recovery-cmd25-journal-halt-card/native-recovery-sd.img`, 68,159,488 bytes, SHA-256 `0609a53a6cf887b5f580be0fa454d75883dd63ac7eb0e6a941cc5c7a341abecf`; its 262,144-byte `ffboot.bin` is SHA-256 `a79775e36ad7fdb55375f904ffa6c455a84daa9897891731fa20cab3d9767835`. `build-recovery-card verify` passed and `inspect` reports all four stages absent. The reporter and persistent-write refusal tests pass.

Independent review returned **APPROVE WITH NONBLOCKING LIMITATIONS** for host-byte correctness with no blocking finding. It verified the exact hashes, three changed `ffboot.bin` bytes, matching raw/FAT copies, valid FAT chains, fixed report LBAs, bounded CMD13/CMD25/CMD13/CMD17 sequence and Auto CMD12 selection. It does not close physical mode-1 entry, FIFO/polling/Auto-CMD timing, card busy behavior, post-configuration clock, cache visibility or installed native-media dependencies. The exact image is byte-reviewed but not authorized for camera use; a new owner go must name this image and one bounded operation.

### 2.24 Preloaded full-RAM recovery candidate

Mode 1 completes configuration and reads all of `ffboot.bin` to RAM before jumping through address zero. The low-address handoff is therefore not a jump into zeros when the FAT payload is complete. A large `ffboot.bin` can preload the stock application sections before the native loader runs. This does not create a new entry route: it still depends on the same unproved mode-1 selection, configuration apply, FAT read and handoff.

**Built host artifact:** `private-data/analysis/x-m1-1.50/bootrom/full-image-cmd25-journal-card-v2/full-recovery-sd.img`, 68,159,488 bytes, SHA-256 `f95bae3a1c3cd12eb933ca3cc56b888a2cb946f2bff05d368bee6f1554636ba7`; its 24,248,320-byte `ffboot.bin` is SHA-256 `62309a253b65aef3d4b2a8af53c28ffe1843989dac55a6f8e69e1f5571beaeb8`. Attempt identifier 3 distinguishes its report from the journal-only candidate.

The FAT payload begins with the stock 256 KiB staging span. Its reset vector enters a saved-frame guard at `0x6000`; only stack `0xfff00f00` and FAT caller offset `0x15c` are admitted. Reporter failure is ignored, R0 and APSR are cleared, and an absolute `bx` reaches the stock native entry at `0x1800`. The raw route reads only that guarded prefix and supplies caller offset `0x174`, so it halts before stock startup.

Two native-loader words change after the guard. The call at `0x1f5c` becomes an ARM NOP so the internal-media table read cannot replace the preloaded table at `0x0061c000`. The image-4 load/decompress call at `0x1d5c` becomes `mov r0,#0`, preserving the preloaded stock section at `0x0061c400` while satisfying the caller's zero-success contract. The preloaded table's word at +8 is 1. Stock kernels, resources, decoded sections 4 and 5 and the plain image occupy their target addresses; both application BSS spans are zero.

The captured-ROM entry smoke executes the exact guard for both ROM aliases: mode 1 reaches `0x1800`, modes 0 and 2-7 refuse, and altered stack or saved-return witnesses refuse. The canonical builder and verifier pass; the CMD25 reporter tests pass; all four report sectors are absent in the built image.

This is a materially different payload attempt, not a solution to entry. It bypasses the known broken internal image-4 call instead of loading the broken image and repairing one RAM word. Independent review returned **APPROVE WITH LIMITATIONS**, found no blocking byte defect and independently reconstructed the exact `ffboot.bin`. The reviewed one-attempt envelope still leaves physical mode-1 selection, the 24 MiB FAT transfer, the unchanged configuration's divider override, cache/alias behavior, CPU1, installed kernels/resources and complete application startup unproved. Stock startup may write ordinary retained settings, and the updater is outside clearance. The exact image is independently cleared for one bounded owner-run startup only after a new owner go names its hash and operation.

### 2.25 Full-RAM recovery authorization

The owner selected **Full-RAM recovery** after review disclosure and authorized one bounded startup with exact image SHA-256 `f95bae3a1c3cd12eb933ca3cc56b888a2cb946f2bff05d368bee6f1554636ba7` (E-RECOVERY-FULL-RAM-GO-150). The authorization covers destructive preparation and complete readback of one expendable SD card, then one cold startup holding Fn, inward sub-command-dial and full shutter, USB disconnected, observed for at most 60 seconds.

If UI appears, release the buttons, leave the camera powered with the recovery card installed and report the exact display. Do not enter or confirm the updater, format the card, change settings, connect USB, reboot or remove power. If it remains dark or behaves unexpectedly at 60 seconds, physically cut supply, move the switch OFF while unpowered and stop. Then preserve one exact card snapshot and inspect the report sectors. No repeat or button sweep is authorized.

Prepare only the expendable card in a separate host reader, with the camera disconnected. Run from the repository root. This destroys the card's existing contents. Identify the whole device from the current `lsblk` output; do not reuse an earlier device name.

```bash
(
  set -euo pipefail

  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/full-image-cmd25-journal-card-v2/full-recovery-sd.img"
  EXPECTED=f95bae3a1c3cd12eb933ca3cc56b888a2cb946f2bff05d368bee6f1554636ba7
  BYTES=68159488

  printf '%s  %s\n' "$EXPECTED" "$IMG" | sha256sum --check
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole expendable SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  [[ "$(lsblk -dn -o RO "$CARD")" == 0 ]] || { echo 'Device is read-only'; exit 1; }
  lsblk -p -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$CARD"
  read -r -p "ERASE this expendable card. Type $CARD to confirm: " CONFIRM
  [[ "$CONFIRM" == "$CARD" ]] || { echo 'Cancelled'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted or swap-active device: stop and unmount the expendable card only'; exit 1; }
  [[ "$(sudo blockdev --getss "$CARD")" == 512 ]] || { echo 'Reader does not expose 512-byte sectors'; exit 1; }
  CAPACITY="$(sudo blockdev --getsize64 "$CARD")"
  (( CAPACITY >= BYTES )) || { echo 'Card is smaller than the image'; exit 1; }
  sudo dd if="$IMG" of="$CARD" bs=4M conv=notrunc,fsync status=progress
  sudo blockdev --flushbufs "$CARD"
  sudo cmp --bytes="$BYTES" "$IMG" "$CARD"
  echo 'PASS: image readback matches'
)
```

Stop unless the block prints `PASS: image readback matches`. Remove the card without mounting, formatting, repairing or copying files onto it.

1. Leave the camera unpowered and USB disconnected. Insert the verified recovery card and be ready to remove its battery or external supply.
2. Hold Fn, the inward sub-command-dial press and full shutter, then turn the camera on once.
3. Release all buttons when UI appears or after ten seconds. Observe for at most 60 seconds from power-on.
4. If UI appears, leave the camera powered and the recovery card installed. Report the exact screen and LED behavior. Do not select firmware update, format, settings, USB, power-off or reboot.
5. If the body remains dark, loops, makes unexpected mechanical sounds or heats, physically remove power no later than 60 seconds, move the switch OFF while unpowered and stop. Do not repeat or try another button combination.

Only after a dark result, return the unmounted card to the host reader and preserve exactly 133,124 sectors before inspection:

```bash
(
  set -euo pipefail

  BYTES=68159488
  SECTORS=133124
  mkdir -p private-data/sessions
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole recovery SD device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted device: stop and unmount the recovery card only'; exit 1; }
  OUT="$PWD/private-data/sessions/$(date -u +%Y%m%dT%H%M%SZ)-full-ram-recovery-card.img"
  sudo dd if="$CARD" of="$OUT" bs=512 count="$SECTORS" iflag=fullblock status=progress
  sudo chown "$(id -u):$(id -g)" "$OUT"
  [[ "$(stat -c %s "$OUT")" == "$BYTES" ]] || { echo 'Captured image has the wrong size'; exit 1; }
  devenv shell -- bazel run //host/firmware:build-recovery-card -- inspect "$OUT"
  echo "PRESERVE: $OUT"
)
```

### 2.26 Reported capture does not contain the full-RAM candidate

The owner preserved `private-data/sessions/20261012T044615Z-full-ram-recovery-card.img`, 68,159,488 bytes, after an operation and reported all four journal stages absent. Host comparison gives snapshot SHA-256 `d8d1cc0f53b4e29ddcc3e20903939798e4602720798eeb2cf01d98decf070c1c`, not the authorized full-RAM image hash. Its first 17,826,304 bytes hash to `ad2541463b16bbb9756bbc8f2f5bba8621d04f44e34b45a5c4923b0b16c05131`, byte-for-byte the earlier divider-preserving native recovery image. Its FAT boot sector is labeled `XM1DIAG`, not the full candidate's `XM1RECV`.

This capture cannot be the prepared full-RAM image after a successful complete write/readback. It supplies no outcome for the full-RAM payload and does not consume that exact-image authorization. Preserve the snapshot and physical card without another camera run until the owner identifies whether this was the earlier divider attempt or whether full-image preparation failed or targeted another device.

The owner then identified this as a probable card mix-up and reported that the camera remained dark for the full 60-second bound. The dark outcome belongs to the exact divider-image prefix found on the card, not to the full-RAM candidate. The owner wants to perform the still-unconsumed full-RAM operation. A fresh write/readback plus a second whole-image hash after physically removing and reinserting the card is required before that camera startup.

### 2.27 Service-manual FAT32 full-RAM carrier

The X-M1 service manual's SD-BOOT card preparation specifies FAT32. The earlier project carriers used FAT16 because the captured ROM explicitly parses FAT12, FAT16 and FAT32; FAT16 was sufficient for the ROM contract but did not match the documented factory recipe.

`//host/firmware:build-full-recovery-card` now emits a FAT32 carrier while preserving the reviewed full-RAM payload and raw refusal prefix. Exact image: `private-data/analysis/x-m1-1.50/bootrom/full-image-cmd25-journal-fat32-card-v2/full-recovery-sd.img`, 68,159,488 bytes, SHA-256 `55fb79f1d57b5c26aa2434b7f9f66a88995902c5e56d6e147f0ce9c8dd3ec26b`. Its `ffboot.bin`, `ffsys.bin` and official DAT are byte-identical to the reviewed FAT16 candidate.

The partition occupies LBAs 2048 through 133119 and classifies as FAT32 under the captured ROM's arithmetic. It uses 512-byte sectors, one-sector clusters, 32 reserved sectors, two 1,024-sector FATs and root cluster 2. Primary and backup boot sectors agree; primary and backup FSInfo agree; both FATs agree; all three file chains extract to their pinned hashes; and the four report sectors at LBAs 133120 through 133123 remain outside the partition and zero. `fsck.fat -n` accepts the extracted volume.

Independent review returned **APPROVE WITH LIMITATIONS** with no blocking byte/layout finding. The raw 1 MiB prefix and report sectors are identical to the earlier reviewed carrier. The MBR deliberately retains type `0x04` and its stock-derived length so the raw-mode configuration words do not change; captured ROM checks only nonzero type and start LBA, though ordinary host partition tools may misreport the card.

Filesystem format and physical medium compatibility are separate. A 64 GB card has ample capacity, and the image exposes only a 64 MiB partition, but that card is SDXC. The ROM negotiates CMD8/HCS and OCR CCS and supports block addressing; this does not prove the owner's specific 64 GB SDXC card initializes in factory SD-BOOT. Prefer a known-good 4–32 GB SDHC card if available. The FAT32 artifact has a new hash, so the earlier FAT16 authorization does not cover it.

### 2.28 FAT32 full-RAM authorization

The owner selected a 4–32 GB SDHC card and authorized one bounded startup with exact FAT32 image SHA-256 `55fb79f1d57b5c26aa2434b7f9f66a88995902c5e56d6e147f0ce9c8dd3ec26b` (E-RECOVERY-FULL-RAM-FAT32-GO-150). This replaces, rather than extends, the unused FAT16 authorization.

The operation envelope remains section 2.25: destructively prepare one expendable card, perform one cold startup holding Fn, inward sub-command-dial and full shutter with USB disconnected, and observe for at most 60 seconds. Before camera insertion, physically remove and reinsert the prepared card, identify the whole device again and verify the first 133,124 sectors against the exact image hash. This additional check prevents another card/image mix-up. No repeat, alternate button combination, updater action, format, setting change, USB connection or second startup is authorized.

If UI appears, release the buttons, leave power and card in place and report the exact display and LED behavior. If the body remains dark or behaves unexpectedly, physically remove power by 60 seconds, move the switch OFF while unpowered and preserve the first 133,124 sectors as in section 2.25.

### 2.29 FAT32 SDHC attempt remained dark

The owner completed both pre-run checks on a 7.2 GB SDHC card. The second check followed physical removal and reinsertion and matched exact image SHA-256 `55fb79f1d57b5c26aa2434b7f9f66a88995902c5e56d6e147f0ce9c8dd3ec26b`. One authorized Fn plus inward sub-command-dial plus full-shutter startup then remained dark, with no LED or LCD response.

Post-run capture `private-data/sessions/20261012T050819Z-fat32-full-ram-recovery-card.img` is exactly 68,159,488 bytes and has the same SHA-256 `55fb79f1d57b5c26aa2434b7f9f66a88995902c5e56d6e147f0ce9c8dd3ec26b`. All four report slots decode absent. Therefore no byte in the captured image changed during the attempt, and no successful reporter stage is established.

This closes the FAT16-versus-FAT32 and 64 GB SDXC compatibility variables for this button recipe: matching the manual's FAT32 format on a smaller SDHC card did not produce observable entry. It does not distinguish failure to select mode 1 from a failure before the first successful report write. The exact authorization is consumed; no retry or alternate button combination follows from it.

### 2.30 Compact stock-segment-4 candidate

`//host/firmware:build-seg4-recovery-card` builds a mode-1-only alternative to the 24,248,320-byte full-RAM payload. Its 2,894,848-byte `ffboot.bin` contains the stock 256 KiB native staging span followed by the stock 1 KiB segment table and 2,631,680-byte decoded segment 4. These are X-M1 1.50 bytes extracted from the pinned official DAT and mapped decode; ROM `ffsys.bin` remains the distinct 1,024-byte hardware configuration file.

The reset literal enters a guard at `0x6000`. The guard admits only ROM stack `0xfff00f00` with saved FAT return `0x15c` after clearing its alias bit; raw caller `0x174` and a wrong stack halt. It copies compact source `[0x00040000,0x002c2c00)` to `[0x0061c000,0x0089ec00)`, clears image-4 BSS `[0x0089ec00,0x00bf1a00)`, cleans those data-cache lines, invalidates the instruction cache and enters stock native reset `0x1800`. The staged table has its completion flag at `0x0061c008` set to 1. Native call `0x1d5c` becomes `mov r0,#0` to skip the installed recorder image-4 load, and table-read call `0x1f5c` becomes a NOP so the copied stock table remains authoritative. Native ids 0 and 1 and later sections still load from installed media.

The built artifacts are `private-data/analysis/x-m1-1.50/bootrom/seg4-recovery-card/`: `ffsys.bin` is 1,024 bytes, SHA-256 `79859bdd17234f2e09caa6f229008f6b32bb7238fc10dac499c5e4b4fa4e0f0b`; `ffboot.bin` is 2,894,848 bytes, SHA-256 `178265aedab678587572d2f64a69dbf92fa3837a19c7f191ceecd8916bdbe1ec`; `seg4-recovery-sd.img` is 68,159,488 bytes, SHA-256 `b56ffb0b29c2e5983e19288c53f1adf50e8ba46713af702570e867fe7c0bb05f`. Builder verification remounts the emitted FAT32 image through the project ROM parser and checks exact file bytes, stock input hashes, patch words, compact placement and raw/FAT separation.

`//emulation:seg4-image-guard-smoke` executes the linked guard in the Cortex-R5F proxy with a patterned 2,632,704-byte compact source and poisoned BSS. It observes an exact destination copy, complete BSS zeroing and the `0x1800` handoff; raw-return and wrong-stack controls do not reach the reporter or native entry. `//emulation:seg4-recovery-stage-report-smoke` separately executes the same linked reporter through successful write/readback and five bounded failure scenarios; success returns zero with a valid record.

The isolated two-core recovery continuation executes the exact card under the declared ROM SD0, application SDHC, IRQ arbitration, installed recorder-media and shared-memory/cache fixtures. ROM mode 1 reads 11,323 sectors and DMA-stages all 2,894,848 payload bytes. Its separate application-SD model does not inherit the ROM controller state, so the reporter returns precondition refusal 1 before issuing a card command. The observer validates the exact stage-0, attempt-4 record and checksum, unchanged readback/card bytes and an exact complete-staging comparison with only that 512-byte record substituted; 508 bytes differ from the staged stock content. CPU0 and CPU1 then reach `0x40020` and `0xd0020`; both kernels and the candidate-derived stock table-tail plus segment 4 match their expected bytes, the receive word is stock `0xebffffdc`, and execution reaches stock media receive at `0x0071feb4`. The run continues to 69,163,963 instructions before the application-continuation observer stops on a successful CMD18 request result. Boot completion, display and updater readiness are not established.

Independent review found no remaining byte or evidence-comparison blocker after the observer constructed the complete expected reporter record independently and handled successful readback as a second exact buffer replacement.

This candidate removes the long full-RAM transfer and the installed recorder segment-4 load. It still depends on physical mode-1 entry, ROM FAT transfer, installed native records, ids 0/1 and later sections, companion startup and silicon cache behavior. No camera operation or card-writing authorization follows from the host result.









### 2.31 Raw-only native recovery host result

SD-BOOT-RAW-NATIVE-01 produces a distinct raw-mode candidate through `//host/firmware:build-raw-recovery-card` (E-RAW-NATIVE-CANDIDATE-150). Its 263,168-byte image is the 1,024-byte configuration followed by the exact 256 KiB native staging span at LBA 2. The retained image is `private-data/analysis/x-m1-1.50/bootrom/raw-native-recovery-card/raw-native-recovery-sd.img`, SHA-256 `d8c569215640e8411544634657a40c8d6c6474fc97df7e90e55ecdd358899d60`.

**Configuration binding:** the pinned official DAT supplies both configuration halves. Exactly two words change:

| Configuration offset | Official value | Raw-native value |
| --- | --- | --- |
| `+0x18`, raw payload sector count | `2` | `512` |
| `+0x33c`, optional SD0 divider gate | `0x80000000` | `0` |

Captured ROM dispatch passes channel 1 to the raw loader and leaves saved return offset `0x174`. Channel 1 resolves to `0xfff51000`, with divider encoding `0x4000` at initialization and `0x100` after card initialization. The helper preserves the low six control bits and polls readiness. Generic configuration's optional divider helper hardwires channel 0 even on the raw path; clearing its gate avoids that SD0 transition. Global clock changes remain: bring-up writes `0xfff88028=0` and `0xfff88030=0x400`, then configuration writes `0x60000` and `0xa00`. These encodings do not establish physical frequencies.

The raw-only layout retains official words `+0x1c0=0`, `+0x1c4=0` and `+0x1fc=0x01000101`. Captured-ROM execution writes those exact values to `0xfff010e0`, `0xfff010e4` and `0xfff0111c`, avoiding the dual-path MBR overlay's three conflicts. The target controller field meanings remain unresolved.

**Native bytes:** the 168-byte linked stub at `0x6000` validates SP `0xfff00f00` before reading `[sp-4]`, masks only the caller's alias bit and admits exactly `0x174` or `0x80000174`. It clears R0 and APSR NZCVQ before native reset `0x1800`. Staging changes are the reset literal `+0x20`, the close call `+0x1fdc -> BL 0x6040`, and the stub. The 104-byte callback equals the reviewed no-journal callback; original close, continuation and companion bytes remain stock. The callback refuses a non-primary discriminator or unknown tap, restores the recorder tap to `0xebffffdc`, performs local cache maintenance and tail-calls original close. It has no reporter.

**Executed host boundary:** the isolated upstream runtime `private-data/experiments/x70-target-add/source-raw-native-recovery` consumes the exact card and captured ROM (E-RAW-NATIVE-EXEC-150). Both positives read raw LBAs 0–513 once through SD1, DMA-stage all 262,144 payload bytes, execute the guard, complete original close and reach CPU0 `0x40020` and CPU1 `0xd0020`. Both complete kernels match their selected source at both entries. SD0 commands and clock writes remain zero.

| Logical media / sensitivity | Instructions | Observed boundary |
| --- | ---: | --- |
| Official stock | 13,875,381 | Both kernel entries; complete stock image 4 unchanged |
| Installed recorder revision 2 | 13,875,381 | Both kernel entries; complete expected RAM rollback |
| Recorder, one sector omitted | 13,113,075 | Observer rejects incomplete staging at `0x6000` |
| Recorder, one served payload byte changed | 13,114,079 | Observer rejects unequal staging at `0x6000` |
| Recorder, optional SD0 gate restored | 13,114,079 | Staging equal; observer rejects SD0 clock writes at `0x6000` |

Recorder's whole 2,631,872-byte section at `0x61c340` equals source before the callback. At original-close entry, close return and both kernel entries, it equals source with exactly three changed bytes at `0x71feb4`–`0x71feb6`. Other installed RC0 edits remain: this is receive-tap rollback, not pristine official firmware. The full primary BSS `[0x4300,0x5280)` is checked before initialization and zero at initialization completion; later native state reuses it.

The three sensitivities are host-observer rejections before guard execution, not guest refusals. Separately, the captured-frame smoke passes 20 raw admission/refusal cases across both aliases and the callback smoke passes five tap/discriminator cases. Ten temporary builder controls reject altered inputs/layout and an existing output; that probe was removed.

**Independent review:** `RawByteSolReview` returned APPROVE BYTES with no findings after reconstructing the entire card and checking linked instructions, patch confinement and close ABI. `RawEvidenceSolReview` returned APPROVE BOUNDED EVIDENCE with no blocker after independently decoding source image 4 and auditing ROM transfer, peer execution and the observer (E-RAW-NATIVE-REVIEW-150). Retained checkpoints hold observed hashes rather than intermediate guest RAM; historical equality relies on the audited byte-comparing observer, whose process-start identity is not atomically pinned. These verdicts do not clear camera execution.

Build into a fresh private directory; verify the retained candidate without device access:

```sh
devenv shell -- bazel build //host/firmware:build-raw-recovery-card //host/firmware:raw-native-rollback
devenv shell -- bazel run //host/firmware:build-raw-recovery-card -- build private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/raw-native-rollback.bin private-data/analysis/x-m1-1.50/bootrom/NEW_RAW_NATIVE_CARD
devenv shell -- bazel run //host/firmware:build-raw-recovery-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/raw-native-rollback.bin private-data/analysis/x-m1-1.50/bootrom/raw-native-recovery-card/raw-native-recovery-sd.img
```

Run the prepared upstream experiment from `private-data/experiments/x70-target-add/source-raw-native-recovery` with a fresh output:

```sh
nix-shell ../native-shell.nix --run 'PYTHONPATH="$PWD/native/python:$PWD" FUJI_AF_NATIVE_MEDIA=recorder timeout --kill-after=5s 60s python -m x70_native --rom-sd-profile xm1-raw-native-card --entry rom --limit 30000000 --storage-profile erased --output ../NEW_RAW_RECORDER_OUTPUT'
```

Use `FUJI_AF_NATIVE_MEDIA=stock` for stock. Add `--raw-refusal-control short-transfer`, `source-byte` or `sd0-clock-gate` for sensitivities. Acceptance is the retained `state.json` classification and comparisons; the upstream CLI returns zero for intentional observer stops too.

**Recovery boundary:** virtual SD1 attachment and route inputs do not prove physical entry or slot wiring. Installed native loaders, table, completion marker and application sources remain prerequisites. The Cortex-R5F proxy, low reset alias, peer reset/release, coherent caches, clean media, ideal decompressor and device timing remain declared fixtures; managed configuration storage is erased, not the failed body's captured state. Application/updater readiness remains unproved. Only the injected rollback is RAM-only; inherited startup can change retained settings. This image carries no restore DAT. No recovery level advances.

**Owner approval:** the owner states that camera approval is in place for this unchanged raw image (E-RAW-NATIVE-OWNER-APPROVAL-150). Host verification again reproduces the pinned candidate hash; no card-reader or camera operation occurred. Approval is not the missing prerequisite. No target-supported raw-entry recipe or physical SD1-to-slot binding is identified: mode 2 requires low selector 4 and a nonzero high GPIO pair (section 2.4), while the known Fn/sub-command bindings address only the high pair. The selector's source and card dependence remain unresolved, so raw entry is neither demonstrated nor excluded. No camera command, guessed selector write or pad-forcing procedure is supplied.

### 2.32 Compact stock-segment-4 attempt remained dark

The owner completed both destructive-write readback and post-reinsertion comparison on a 7.2 GB SDHC card; both matched the authorized FAT32 image SHA-256 `b56ffb0b29c2e5983e19288c53f1adf50e8ba46713af702570e867fe7c0bb05f`. The one authorized Fn plus inward sub-command-dial plus full-shutter startup remained dark.

The preserved 68,159,488-byte post-run image at `private-data/sessions/20261012T144651Z-seg4-recovery-card.img` has the same whole-image SHA-256. `//host/firmware:build-recovery-card inspect` reports `payload-entry`, `native-load-complete`, `tap-restored` and `native-close-complete` all absent. No card byte changed.

This consumes E-RECOVERY-SEG4-GO-150. Reducing `ffboot.bin` from the full-RAM candidate to the compact 2,894,848-byte stock-segment-4 candidate did not produce a visible recovery or durable stage witness. The unchanged card does not distinguish failed mode-1 selection, file transfer, handoff, saved-frame admission or a reporter precondition refusal before its first write. It therefore does not contradict the host execution result and does not establish payload execution on the body. No repeat is authorized.

### 2.33 Post-failure warm-toggle blink witness

The owner rewrote the 7.2 GB SDHC card with the exact startup-restored dual-path blink image from section 2.12 and obtained matching comparisons before and after physical reinsertion. While intentionally trying more starts than the proposed bounded comparison, the owner left the battery installed, held Fn plus the inward sub-command-dial press and toggled the power switch repeatedly at an estimated 0.75 second per transition without removing the battery. The repeating marker then began while both controls remained held. Full shutter was not reported as held for this successful transition.

The observed cadence was again three short red intervals followed by a long red interval, repeating. The switch was subsequently left OFF and the card returned to the host. The preserved 17,826,304-byte prefix at `private-data/sessions/20261012T190320Z-blink-success-card-prefix.img` has SHA-256 `4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0` and compares byte-for-byte with the prepared image.

This is a second marker witness and the first after the installed application became non-booting. The exact cadence binds execution to the project-authored diagnostic payload, so the body can still initialize one of the card's two ROM routes, transfer the payload, hand off and execute it independently of the installed application. This satisfies the R2 execution criterion. The card intentionally exposes identical payload bytes through FAT mode 1 and raw mode 2, so the witness does not identify the route, low selector value or exact successful switch edge. Rapid warm toggling may involve retained power/reset state or switch timing; it is evidence for a timing-sensitive entry condition, not a reproducible recipe. The unbounded retries were outside the proposed operation. No further live attempt is authorized.

### 2.34 Dual-route finite-marker RAM rollback candidate

The highest-value follow-up to the post-failure marker witness is one 17,826,304-byte card that exposes the same 256 KiB native payload through both admitted layouts. FAT mode 1 reads `ffsys.bin` and `ffboot.bin`; raw mode 2 reads configuration LBAs 0–1 and the same payload at LBA 2. The canonical image is `private-data/analysis/x-m1-1.50/bootrom/dual-native-recovery-card/dual-native-recovery-sd.img`, SHA-256 `665baff8d4d2428f0ee768505ee5ed92bc1c97bcdd3856d7f3d1da4b5c9177ba`. Its staging SHA-256 is `97cc415350df01905a43f8282e7491351a0d55865040faedcc83f56bff397e4e`; the 296-byte linked stub SHA-256 is `8e2f4ad8e200b7f8d4c9f2c1f41c80ba453d74ace275e3fb1682985b566706e7`.

The guard requires SP `0xfff00f00` and accepts only saved return `0x15c`/`0x8000015c` for FAT or `0x174`/`0x80000174` for raw. It emits one finite marker interval for FAT or two for raw through the LED writes already observed on the body, restores its working registers, clears R0 and APSR NZCVQ, then enters native reset `0x1800`. An unexpected caller or stack halts before native loading. The marker count identifies the admitted route; the marker is not the recovery result.

The staging reset literal at `+0x20` points to the guard. The close call at `+0x1fdc` points to callback `0x60c0`. Native loading therefore installs the recorder image before the callback inspects `0x71feb4`. The callback accepts only the stock word `0xebffffdc` or recorder word `0xeb34a9c9`; it replaces the recorder word in RAM with the stock word, performs local data-clean and instruction-invalidate operations, and invokes original close `0x2044`. It does not move the application call. It delays the repair until segment 4 exists in RAM, then resumes the original loader path before the application can execute that call.

Host execution covers both routes. The finite-guard smoke passes both aliases for FAT with one marker interval and raw with two, preserves the admitted register frame, reaches reset with zero R0/NZCVQ, accepts stock and recorder taps, and refuses a wrong stack, caller, tap or primary discriminator. The exact FAT image under captured-ROM and recorder-media fixtures executes 82,628,183 instructions, restores the tap and reaches CPU0 `0x40020` and CPU1 `0xd0020` with matching loader bytes. The exact raw prefix under the isolated upstream runtime reads SD1 LBAs 0–513, stages all 262,144 bytes, restores the tap and reaches both kernel entries after 13,875,381 instructions with no media programming. This dual raw configuration performs SD0 clock writes but no SD0 commands; that differs from the earlier raw-only image and remains a physical-hardware uncertainty.

Normal LCD/UI startup after the finite route marker is the recovery indication. A marker followed by darkness means entry occurred but recovery did not reach an observable application; it does not authorize another attempt. The rollback is RAM-only for that powered session. Powering off before a persistent stock repair returns to the installed recorder image and its broken call. CPU proxy, physical cache visibility, SD controller timing, exact warm-switch edge, settings effects and application readiness on the failed body remain unproved.

Canonical host verification:

```sh
devenv shell -- bazel run //host/firmware:build-dual-recovery-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/dual-native-rollback.bin private-data/analysis/x-m1-1.50/bootrom/dual-native-recovery-card/dual-native-recovery-sd.img
devenv shell -- bazel test //emulation:dual-native-rollback-smoke --test_output=all
```

Independent review returned **APPROVE WITH NONBLOCKING LIMITATIONS** (E-DUAL-NATIVE-RECOVERY-REVIEW-150). Follow-up review confirmed that the strengthened smoke checks marker ordering/counts, preserved registers, SP, R0/NZCVQ and the original-close register ABI. The full marker delay and CP15 cache operations remain host substitutions; canonical whole-card verification remains an explicit command rather than a Bazel test. The exact bytes are ready to commit. Review does not authorize camera execution.

**Proposed one-operation bound after commit and owner go:** prepare only the expendable SDHC card while the camera is disconnected, using section 2.16's guarded whole-device write and readback with `IMG` set to this section's canonical image and `EXPECTED` set to `665baff8d4d2428f0ee768505ee5ed92bc1c97bcdd3856d7f3d1da4b5c9177ba`. Physically remove and reinsert the card in the host reader, then repeat the exact 17,826,304-byte comparison before camera insertion.

1. Start with a charged battery retained, USB disconnected, verified card inserted and switch OFF. Hold Fn plus the inward sub-command-dial press continuously; do not hold full shutter.
2. Alternate the switch at approximately 0.75 second per transition for at most seven transitions: ON, OFF, ON, OFF, ON, OFF, ON. Leave the final state ON. If the marker begins while ON, stop toggling. If it begins while OFF, make only the next scheduled ON transition and then stop toggling.
3. Keep the controls held through the finite marker, or until ten seconds after final ON if no marker appears, then release. One marker cycle identifies FAT; two identify raw. The marker proves payload entry only.
4. Observe for at most 60 seconds after final ON. Normal LCD/UI is the recovery endpoint. Leave the switch ON, card inserted and USB disconnected, and report immediately; do not navigate, format, update, reboot or remove power.
5. If a marker occurs but no UI appears by 60 seconds, or if no marker occurs, cut power by removing the battery, move the switch OFF while unpowered and stop. Unexpected noise, actuation, odor or heating requires immediate battery removal. Do not repeat.

This is one bounded startup operation, not seven attempts. The transition cap reproduces the only observed warm-entry condition while guaranteeing a final ON state. Its timing remains empirical and unproved. A fresh owner go must name this exact image and procedure.

### 2.35 One-sector entry-latch recovery candidate

The dark dual-recovery attempt did not provide feedback until the ROM had transferred all 262,144 payload bytes and the payload had admitted the saved frame. `//host/firmware:build-staged-recovery-card` replaces that ambiguity with a one-sector first stage. The canonical image is `private-data/analysis/x-m1-1.50/bootrom/staged-native-recovery-card-v6/staged-recovery-sd.img`, 17,826,304 bytes, SHA-256 `c62c4085ad4cbe402300e8171a4d52791b5fdbb6f0b8df7eba0418520daee19e`. Its `ffboot.bin` is exactly 512 bytes, SHA-256 `e8bf3d9678a7e6f6787418e5d76a47d71d21414f9329b1f41aec9cbbe32aff2c`; `ffrecov.bin` is the existing 262,144-byte dual native rollback stage, SHA-256 `97cc415350df01905a43f8282e7491351a0d55865040faedcc83f56bff397e4e`.

Both card routes initially request one sector. The FAT filesystem names the bootstrap `ffboot.bin` and the recovery body `ffrecov.bin`; raw mode reads the bootstrap from LBA 2 and the recovery body from LBAs 3 through 514. The bootstrap admits only SP `0xfff00f00` and saved return `0x15c`/`0x8000015c` or `0x174`/`0x80000174`. It then turns the red LED steadily on, relocates itself to `0x00050000`, and loads the recovery body to the shared high alias at `0x40000000`: eight 32 KiB reads through captured FAT read `0x80001e88`, each requiring its returned 16-bit byte count to equal 32 KiB, or 512 channel-1 raw reads through `0x8000148c`. It invalidates the instruction cache, restores the admitted frame including `[sp-4]`, turns the LED off and branches to the loaded stage at zero.

The visible states are disjoint:

| LED/UI observation | Candidate state |
| --- | --- |
| Steady red | One-sector entry and saved-frame admission succeeded; second-stage loading is in progress. Stop toggling and wait. |
| Continuous equal blinking | The one-sector bootstrap ran but rejected the entry frame or a second-stage read failed. |
| LED off, then one finite pulse | The complete FAT second stage loaded and its dual-recovery guard admitted the FAT frame. |
| LED off, then two finite pulses | The complete raw second stage loaded and its dual-recovery guard admitted the raw frame. |
| Normal LCD/UI after the finite pulse | The RAM rollback continued through native load and original close to application startup. |

`//emulation:staged-recovery-bootstrap-smoke` executes the exact linked bootstrap for FAT success, raw success, wrong-stack refusal, FAT-read failure and FAT short-success refusal. It verifies relocation, all read addresses and sizes, the complete loaded body, LED transitions, restored SP/registers/saved return and zero R0/NZCVQ. Its ROM calls are source-address-bound host substitutions, and its cache is coherent.

The captured-ROM full-path run `private-data/analysis/x-m1-1.50/recovery-bootstrap/recorder-fat-staged-entry-v16` executes the exact canonical card and recorder firmware through FAT mode 1. It transfers the bootstrap and `ffrecov.bin`, observes the bootstrap instruction-cache invalidation, matches the complete 262,144-byte stage, reaches both native kernels, restores the recorder call in RAM, completes original close and stops before the primary rendezvous store. Every retained staging/kernel/segment/image comparison matches. The model holds CPU1 after its release request and substitutes coherent cache visibility; it does not prove physical card entry, SD timing, silicon cache behavior, display startup or a recovered body.

This candidate adds no persistent write. The rollback remains RAM-only and a later persistent stock repair would still be required. Independent re-review of exact v6 bytes returned **APPROVE** with no blocking findings after the FAT short-read check was added (E-STAGED-RECOVERY-REVIEW-150). The review reconstructed both layouts and hashes and found no persistent media-write path. It does not authorize camera use; a fresh operation-specific owner go is still required before card preparation or camera use.

Host verification:

```sh
devenv shell -- bazel run //host/firmware:build-staged-recovery-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/dual-native-rollback.bin bazel-bin/host/firmware/staged-recovery-bootstrap.bin private-data/analysis/x-m1-1.50/bootrom/staged-native-recovery-card-v6/staged-recovery-sd.img
devenv shell -- bazel test //host/firmware:boot-card-tests //emulation:staged-recovery-bootstrap-smoke --test_output=all
```

### 2.36 Entry-latch owner authorization

The owner authorized one staged entry-latch recovery attempt using exact image SHA-256 `c62c4085ad4cbe402300e8171a4d52791b5fdbb6f0b8df7eba0418520daee19e` (E-STAGED-RECOVERY-GO-150). This covers destructive preparation and post-reinsertion verification of one expendable SDHC card, followed by one battery-retained startup operation with Fn plus the inward sub-command-dial press held. It does not authorize another image, a repeat, USB, a format or update prompt, settings changes, or a persistent firmware write.

The operation may use at most 40 approximately 0.75-second switch transitions, ending ON. Stop toggling immediately on any LED response. If the response begins while the switch is OFF, make only the next ON transition and stop. Steady red means the bootstrap admitted entry and is loading the recovery body; continuous equal blinking means refusal; LED off followed by one or two finite pulses means the complete FAT or raw body was admitted. Observe for at most 60 seconds after the final transition. Normal UI is a temporary RAM-recovery endpoint: leave power and card in place and report before any further action. Refusal, no UI after a marker, no response, or unexpected noise, actuation, odor or heating requires battery removal, switch OFF while unpowered and no repeat.

### 2.37 Entry-latch recovery reached application UI

The owner reports that the authorized exact v6 card succeeded after approximately ten warm switch attempts with Fn plus the inward sub-command-dial press held. The red LED first became solid, then emitted a series of blinks, and the camera entered ordinary application UI showing that the recovery card was not initialized. The body is currently powered from a dummy battery (E-STAGED-RECOVERY-SUCCESS-150).

The solid entry latch binds execution to the reviewed one-sector bootstrap. The subsequent finite marker and UI bind complete second-stage loading, native loader continuation and application startup after the RAM rollback. The owner did not count the finite marker intervals, so this observation does not distinguish FAT from raw entry. The uninitialized-card message is expected application handling of the recovery carrier and must not be accepted or formatted.

This demonstrates R3 for the installed recorder-call failure: the independent route reached installed native storage, loaded the relevant image, restored the target call in RAM and resumed to UI. It is not R4. The repair is volatile; power loss or orderly shutdown returns to the persistently broken installed image. Keep the dummy supply stable, leave the recovery card inserted, do not initialize it, do not navigate, connect USB, open the card/battery door, reboot or power off while the persistent-repair operation is prepared and reviewed.

### 2.38 Reviewed one-block persistent repair candidate

The smallest persistent repair is one 128 KiB internal-media write, not a stock-image rewrite. Complementing the pinned RC0-prefix and installed recorder rev-2 DAT media spans yields differences only in logical blocks 14 and 71. Block 14 contains the compressed image-4 changes and differs at exactly seven byte offsets; block 71 contains the 846-byte recorder blob. Replacing installed block 14 with RC0 block 14 restores the pre-recorder image-4 stream and leaves block 71 inert and unchanged. The repair block is 131,072 bytes, SHA-256 `94deaec641a11cf045c8761752b218d9b52af9e2e015e3b8dcc8e33708956bf5`, CRC32 `0x92863e8b`.

The canonical v2 card is `private-data/analysis/x-m1-1.50/bootrom/persistent-one-block-repair-card-v2/persistent-repair-sd.img`, 17,826,304 bytes, SHA-256 `92c1aaf9b22ee951aaf861caf7453bedf00112615406099835a4b3e652c8d883`. Its one-sector `ffboot.bin` is SHA-256 `7f3a3db4e366fbc3573f44888174bcd8cefdac999668df3487067aa62df7eb54`; its 393,216-byte `ffrecov.bin` is SHA-256 `8f5670f9bf7ad283aada4338f3a1e3741d90e40e8472a4e24c434cbe1855667c`. FAT and raw routes carry the same 256 KiB native stage followed by the exact repair block.

The native callback accepts only the recorder call word at `0x0071feb4`, restores it in RAM for the current boot, and temporarily retargets the post-id-6 completion call at `0x0073b564` to stage `0x6200`. The delayed stage restores that displaced call and its cache line before any failure or persistent write, requires storage-ready word `0x00828574` to equal 1, checks the complete carried block against CRC32 `0x92863e8b`, and calls the stock bounded writer `0x006a9268(14,1,0x00040000)`. A zero return emits three long red pulses and tail-calls the original completion helper. Every prewrite mismatch and any writer error emits repeating short red pulses and does not continue.

Host verification passes the canonical card reconstruction, FAT/raw placement, original staged-bootstrap regression, 768-sector/12-chunk extended bootstrap paths, guard/callback ABI, exact code words and cache operations, complete-block CRC gate, all visible refusal paths, and the exact stock `0x006a9268` function under a declared lower-device model. The writer model observes only `(14,1,0x00040000)` and reproduces success, not-ready, out-of-range, lower-check-error and lower-program-error results. Independent v2 re-review returned **APPROVE** with no blocking findings after the shared refusal path was changed from silent WFI to visible blinking (E-PERSISTENT-BLOCK14-REPAIR-150, E-PERSISTENT-BLOCK14-REVIEW-150).

This does not make NAND programming atomic. Remaining device assumptions are delayed code residency at `0x6200`, real cache behavior, valid remap state, media-task exclusivity at the post-id-6 completion point, lower NAND erase/program/verify semantics and power-loss behavior. Success relies on the stock writer's lower verification and zero status; the stage performs no second raw readback.

The current powered UI cannot transform the already loaded v6 RAM-only card into this operation. That card carries neither the repair block nor a persistent writer. The mapped vendor RAM-write service is available only after a maintenance-mode boot, not through ordinary application USB. A debugger/JITL path could in principle inject the same reviewed stage into the current RAM state, but no target-bound X-M1 transport, framing, execution control or reviewed command exists. The x70view and boffin leads therefore provide no ready no-restart operation. Using this candidate requires ending the current volatile session, inserting the exact v2 card and obtaining another SD-boot entry.

Visible contract for a separately authorized run:

| Observation | Meaning and required response |
| --- | --- |
| Initial solid red | One-sector entry admitted; extended stage and repair block are loading. Stop toggling and wait. |
| Repeating short red pulses | Refusal or writer error. Persistent state is not classified. Preserve power and report; do not retry or power-cycle. |
| Solid red later, then exactly three long pulses, then UI | The bounded writer returned zero after its lower verification and application startup continued. Preserve power and report before the confirmation reboot. |

The host build and review do not authorize a persistent camera operation. Section 2.39 records the later operation-specific owner authorization.

### 2.39 Persistent block-14 repair authorization

In direct response to the exact v2 image, one-block mutation, visible outcomes and residual NAND/cache/scheduler risks, the owner said “alright, lets go” (E-PERSISTENT-BLOCK14-GO-150). This authorizes destructive preparation and exact readback of one expendable card using image SHA-256 `92c1aaf9b22ee951aaf861caf7453bedf00112615406099835a4b3e652c8d883`, ending the current volatile recovered session, and one bounded SD-entry operation whose only requested internal-media mutation is `0x006a9268(14,1,0x00040000)`.

The operation permits at most 40 approximately 0.75-second switch transitions with Fn plus the inward sub-command-dial press held, ending ON and stopping immediately on any LED response. Initial solid red means entry admission and loading. Repeating short red pulses mean refusal or writer error: preserve power, do not retry or power-cycle, and report. A later solid-red write interval followed by exactly three long red pulses and UI means the stock writer returned zero after lower verification: preserve power and report before a separately instructed confirmation reboot. No format, update prompt, USB operation, different image, second repair attempt or unreported power cycle is authorized.

### 2.40 Persistent block-14 attempt reached visible refusal

The owner reports that the exact prepared card passed host readback, reached application UI with the card-not-initialized message, and continues to emit equal short red pulses (E-PERSISTENT-BLOCK14-REFUSAL-150). This is the candidate's defined refusal/error signal, not its success signal. The required three long pulses followed by LED-off were not reported. Power remains on pending classification.

The observation proves payload execution at least as far as a repeating failure loop but does not distinguish bootstrap frame/read refusal, callback guard refusal, storage-readiness refusal, source-CRC refusal or a nonzero stock-writer result. In particular, a nonzero writer result may follow an attempted program, so persistent block 14 is unclassified. UI availability does not convert the short-pulse signal into success. No retry, power cycle, card removal, format prompt, USB operation or confirmation boot follows until the powered state has been assessed.

Passive observation counted approximately 14–15 complete on/off cycles in ten seconds. The stage failure loop uses two `0x00800000` decrement delays per cycle; the bootstrap failure loop uses two `0x01000000` delays. The observed faster cadence, together with application UI, is consistent with the delayed native-stage failure loop and not the bootstrap refusal loop. The owner could not distinguish whether a second solid-red interval preceded the pulses because the transition was too quick. The remaining live-compatible branches are therefore storage-ready mismatch, repair-source CRC mismatch or nonzero writer return; whether the writer was called remains unresolved.

### 2.41 Read-only block-14 classifier candidate

No interface available in the powered ordinary UI can read internal logical block 14 without a restart. The bounded follow-up is therefore a separately built read-only SD-stage classifier. It retains the entry latch and volatile recorder-call rollback, restores the delayed completion hook before classification, requires storage readiness, and calls stock adapter `0x006a8ff0(14,1,0x00020000,1)`. The adapter supplies lower reader `0x006a8f1c` with `(14,1,0x00020000,0,1)`. It never calls an internal-media or SD writer.

The canonical v2 card is `private-data/analysis/x-m1-1.50/bootrom/persistent-block14-classifier-card-v2/persistent-block14-classifier-sd.img`, 17,826,304 bytes, SHA-256 `e09c62266ed2c6e3955f53ca585d953a8dca8d0b769199b01b7c654b3426b78e`. Its corrected one-sector bootstrap is SHA-256 `75f56cbf46670f3cb60ca63fa54e56a3bd0c56b7b839b6b7390c51181bef2281`; its `ffrecov.bin` is SHA-256 `e85ec4471d9651705dab6fd400cb7ef0b20778640d4b9786e8d91d3e6e5156c3`; the embedded 904-byte classifier is SHA-256 `13b2e40c0fabf2e5b845f518a3d0b9511a7359e9792c1abb68a88761370407b1`.

The exact classifier smoke executes the stock reader adapter instructions and observes the lower-reader ABI, aligned stack, three CRC outcomes, refusal/error loops, completion restoration and full register/SP restoration. The FAT/bootstrap smoke covers the corrected eight-byte call frame. Independent review initially rejected the candidate because the adapter was called with its invented fifth argument and the FAT reader was called with a four-byte-misaligned stack. Both defects were corrected; re-review independently matched the card hashes, layout, CRCs and reader tuple and returned **APPROVE** with no blocker.

| Indication | Read-only result |
| --- | --- |
| One finite long red pulse, then UI | Block 14 exactly matches installed recorder rev-2, CRC32 `0xd1943bd0`; the persistent attempt did not leave an observable byte change. |
| Two finite long red pulses, then UI | Block 14 matches neither pinned recorder nor RC0 CRC; preserve power because media state is unknown. |
| Three finite long red pulses, then UI | Block 14 exactly matches RC0, CRC32 `0x92863e8b`; the earlier writer changed it to the intended repair bytes despite returning a nonzero status. |
| Repeating fast pulses | Refusal before the read. |
| Repeating slow pulses | Stock reader returned nonzero. |

The card is host-complete and independently approved but not authorized for camera use. It still requires ending the current powered session and obtaining another stochastic SD entry. Remaining assumptions are target cache visibility, delayed-stage residency/exclusivity, stock-reader initialization and DMA visibility, RAM ownership, physical SD entry and visible LED timing.

### 2.42 Conditional read-before-write recovery candidate

The pure classifier would require another SD entry whenever block 14 still matched recorder rev-2. The conditional candidate removes that avoidable restart without writing an unclassified block. It first reads and CRC-classifies logical block 14 through the same stock remap entry used by the writer:

1. Exact RC0 block: make no write, emit three long pulses and continue to UI.
2. Neither pinned CRC: make no write, emit two long pulses and continue to UI.
3. Exact recorder block: independently CRC-check the carried RC0 source, call `0x006a9268(14,1,0x00040000)` exactly once, ignore its status as an oracle, read block 14 again, and decide only from that readback.

After a write, exact RC0 readback emits three long pulses and continues to UI even if the writer returned nonzero. Exact recorder readback emits one long pulse and continues to UI. Unknown readback or read error repeats slow pulses without continuation; a readiness or carried-source refusal repeats fast pulses. There is no retry and no success based only on writer status.

The canonical card is `private-data/analysis/x-m1-1.50/bootrom/conditional-block14-recovery-card-v1/conditional-block14-recovery-sd.img`, 17,826,304 bytes, SHA-256 `361eadd6dc1ad955dc54f229a182da749706d51b6973fc9a42dc2686b6feaccc`. Its fixed-bootstrap SHA-256 is `75f56cbf46670f3cb60ca63fa54e56a3bd0c56b7b839b6b7390c51181bef2281`; `ffrecov.bin` is `e4c15aa5ef9088787f369cc193f5d853baa850144e4b57ae9afb3b459860a18c`; the 1,056-byte conditional stage is `280565af5ebddba9970e4afbe1ab5da7e2f346f428c665ac7533e653dbbf7f82`.

Host smoke passes not-ready, initial read error, preexisting RC0, initial unknown, source refusal, writer status zero/nonzero with repaired readback, unchanged recorder readback, post-read error and unknown post-write data. It executes the real reader adapter and checks both reader calls, the single guarded writer ABI, call counts, pulse counts, completion restoration and final register/SP state. The original bootstrap, classifier, persistent-stage and card-layout regressions also pass.

Independent review returned **APPROVE** with no blocking finding. It independently matched all hashes and CRCs, confirmed reader and writer logical block 14 use the same remap-table entry, and confirmed the synchronous writer releases storage serialization before the post-read reacquires it. Remaining hardware assumptions are physical entry, target cache/DMA visibility, stage residency and exclusivity, RAM ownership, lower NAND behavior, and LED timing.

This candidate is not authorized for camera use. It is a different H3 operation and image from the consumed attempt in section 2.40.

### 2.43 Conditional block-14 recovery authorization

In direct response to the exact conditional state machine, card SHA-256 `361eadd6dc1ad955dc54f229a182da749706d51b6973fc9a42dc2686b6feaccc`, visible outcomes, independent approval and residual hardware risks, the owner said “go” (E-CONDITIONAL-BLOCK14-RECOVERY-GO-150). This authorizes destructive preparation and exact readback of one expendable SDHC card, ending the current powered short-pulse session, and one bounded SD-entry operation.

The operation pre-reads logical block 14. It writes nothing for exact RC0 or unknown contents. Only exact recorder rev-2 plus exact carried RC0 source authorizes one stock writer call `0x006a9268(14,1,0x00040000)`, followed by mandatory stock readback. The operation permits at most 39 approximately 0.75-second switch transitions starting OFF and ending ON, with Fn plus inward sub-command held and immediate cessation on any LED response. Three finite long pulses and UI mean exact RC0 readback. One or two finite pulses, any repeating cadence, no response or unexpected behavior requires preserved power and reporting. No retry, confirmation reboot, card initialization, USB operation or different image is authorized.

### 2.44 Conditional block-14 recovery attempt

The owner ran the authorized exact-image operation and reported application UI with continuing red LED pulses, then supplied `private-data/provenance/owner-files/Downloads/VID_20261012_234225.mp4` while keeping the body powered. The recording contains materially different LED phases. Detailed 10 Hz sheets show the LED continuously visible from about 25.1 through 27.5 seconds despite camera motion, consistent with the bootstrap's solid second-stage-load marker; UI appears during that interval. From 28.3 through 30.63 seconds, nine detected on intervals average 0.133 seconds and eight off intervals average 0.142 seconds, with a 0.275-second mean cycle. At 30.8 seconds the cadence changes. In the stable 31.5–66.1-second interval, 46 complete on intervals average 0.395 seconds (range 0.367–0.433), 46 complete off intervals average 0.357 seconds (range 0.333–0.400), and the mean cycle is 0.752 seconds (range 0.700–0.800). There is no isolated group of one, two or three long pulses followed by cessation; the slow cadence continues through the recording and owner observation.

The terminal cadence matches the conditional payload's repeating-slow loop, not its repeating-fast loop and not any finite one-, two- or three-pulse result. The stage reaches that slow loop after an initial reader error before any write, a post-write reader error, or unknown post-write data. The preceding short cadence cannot be treated as the same invocation's fast branch because both failure loops are terminal; the video does not bind it to bootstrap, application activity or another invocation. A writer-on interval is not uniquely distinguishable among those earlier transitions. This operation therefore may or may not have called the block-14 writer. The current block remains unclassified; RC0 is not verified and R4 remains unclaimed. Preserve power; no retry, confirmation reboot, card initialization, USB operation or card removal is authorized.

### 2.45 Outcome-signal separation

The X-M1 has a physically observed green maintenance-mode indicator, but its exact direct control write is not target-bound. X-M1 1.50 directly establishes only the red sequence `0xfff893c0 = 0x00010001/0x00010000`. The fetched fffw X-E2 experiment assigns green/red pins through `0xfff893c0` and `0xfff893c4`, including `0x00050001/0x00050005`, but that is sibling evidence and cannot supply X-M1 camera bytes. A green outcome marker is therefore the preferred discriminator only after an X-M1-native call or register sequence is bound; using the X-E2 values on this body would violate the target-evidence rule.

The conservative candidate makes every delayed-stage outcome terminal and packet-coded while retaining the target-bound red output. The code requests a four-long-delay `led_on` sync, a four-long-delay `led_off` separator, then one through nine `led_on`/`led_off` pulse pairs and another four-long-delay gap, repeated indefinitely. Physical X-M1 video later established that these helper names are electrically inverted at the visible indicator: `led_on` is dark and `led_off` is lit. The visible packet is therefore a long lit interval, a long dark interval, a long lit separator, then one through nine short dark notches separated by lit intervals before the next long lit interval. Codes distinguish storage-not-ready, initial-read error, initial unknown, preexisting RC0, carried-source refusal, post-write read error, post-write RC0, post-write recorder and post-write unknown. Green remains preferable only after an X-M1-native control is bound.
 
### 2.46 Packet-coded conditional recovery candidate

The packet-coded variant is built without changing the consumed candidate's bytes. Default compilation still emits the 1,056-byte stage SHA-256 `280565af5ebddba9970e4afbe1ab5da7e2f346f428c665ac7533e653dbbf7f82`. `CODED_OUTCOMES` emits a 1,144-byte stage SHA-256 `135d0c7792a6e8e88e461d4c07eba17c1e75612b4a5c7039f83ae312ef036147`. It preserves the pre-read gates, sole writer call and mandatory post-read; only terminal outcome reporting changes.

The canonical card is `private-data/analysis/x-m1-1.50/bootrom/conditional-block14-recovery-coded-card-v1/conditional-block14-recovery-coded-sd.img`, 17,826,304 bytes, SHA-256 `5cf17e43929338fcfc1717d5ac7cdefe6e6d4f95670b4bd2eb43f8b087b95781`. Its fixed bootstrap SHA-256 is `75f56cbf46670f3cb60ca63fa54e56a3bd0c56b7b839b6b7390c51181bef2281`; `ffrecov.bin` is `bfd726af361d77116605f546bfa2eee02846f9e8b6078d96412bc5e45620c150`; the carried RC0 block remains `94deaec641a11cf045c8761752b218d9b52af9e2e015e3b8dcc8e33708956bf5`.

The coded smoke passes all nine outcomes, both writer-status values for repaired readback, exact reader/writer ABI and call counts, exact alternating GPIO values, sync/long/short delay classes, completion restoration and stack alignment. The legacy smoke, boot-card tests, canonical build and exact verify pass. Independent review returned **APPROVE** with no blocking finding. It confirmed codes 1–5 precede the sole writer, codes 6–9 follow the mandatory post-read after exactly one writer call, packet register liveness, linker bounds, builder pinning, card placement and legacy byte preservation.

The packet-coded candidate is now authorized for one exact camera operation. After presentation of image SHA-256 `5cf17e43929338fcfc1717d5ac7cdefe6e6d4f95670b4bd2eb43f8b087b95781`, its conditional write gate, power-loss risk and no-retry boundary, the owner selected “Authorize one exact attempt” (E-RECOVERY-PACKET-CODED-GO-150). Authorization covers destructive preparation and exact readback of one expendable SDHC card, ending the current powered session, and one bounded Fn plus inward sub-command SD-entry attempt. No retry, USB operation, card initialization or confirmation reboot is authorized. Green GPIO values are absent from these bytes.

### 2.47 Packet-coded block-14 classification

The owner ran the authorized exact-image operation and supplied `private-data/provenance/owner-files/Downloads/VID_20261013_004206.mp4`, 56.8 seconds, SHA-256 `fc0b9f1ca967940b4d2952684f634a77704f904bb922dae26fdf20588900705d`. A fixed indicator ROI sampled at 20 Hz contains a complete packet: 6.00 seconds dark, 6.00 seconds lit, then exactly four 1.85-second dark notches separated by 1.85–1.90-second lit intervals. The lit interval after the fourth notch lasts 7.90 seconds because the final pulse-off delay and inter-packet delay are contiguous. The next packet begins with 5.95 seconds dark and 6.05 seconds lit before the recording reaches its first short notch. These timings preserve the assembly's exact 4:1 sync-to-pulse loop-count ratio and establish that the visible red indicator is inverted relative to the helper labels.

The four notches are packet code 4. Code 4 is emitted only after the initial stock read returns block 14 with exact RC0 CRC32 `0x92863e8b`; it precedes the writer branch, so this invocation issued no block writer call (E-RECOVERY-PACKET-CODED-RC0-150). Logical block 14 now reads as the exact carried RC0 block through the stock reader. This resolves the first conditional attempt's unclassified persistent state in favor of restored RC0, but does not identify which earlier invocation performed the restoration.

An ordinary startup with the recovery card removed is the remaining direct recovery check. It gives up the current RAM-restored session, but makes no new persistent write and exercises the repaired stock path. The remaining risk is that another unclassified persistent fault prevents boot despite exact RC0 block 14; the retained reviewed recovery card remains the fallback entry route. R4 remains unclaimed until ordinary startup reaches UI and basic capture/playback succeeds.
After presentation of the video-verified code-4 result and the remaining risk that another persistent fault could still prevent startup, the owner selected “Authorize ordinary startup” (E-RECOVERY-ORDINARY-STARTUP-GO-150). This authorizes ending the current RAM-restored session, preserving and removing the recovery card, inserting a normal initialized card, one ordinary no-button startup, and one capture/playback check if UI appears. It does not authorize an SD-entry retry or another recovery write.


### 2.48 Ordinary startup remained dark

The owner completed the authorized ordinary startup with the recovery card removed and the newly host-formatted conventional MBR/FAT32 card inserted. The camera remained dark (E-RECOVERY-ORDINARY-STARTUP-DARK-150). The card intentionally contained no firmware: ordinary startup loads the installed application from internal media, while an SD firmware file is consumed only by a separately selected updater route. Adding `FWUP0002.DAT` would not make this an ordinary startup and was not authorized.

The packet result contains a stronger same-invocation contradiction than “block 14 plus another fault.” The callback reaches its delayed stage only after RAM word `0x0071feb4` equals the recorder call `0xeb34a9c9`; it then restores that word. Packet code 4 later requires the application stock reader to return exact RC0 CRC32 `0x92863e8b` for logical block 14 before any writer call. The installed native-load result and the application logical-media result therefore exposed different block-14 content in the same staged boot. The staged recovery path can reach UI because it repairs the loaded recorder word in RAM. A no-button ordinary startup then remained dark. The ordinary-startup authorization is consumed. Stop with the body off and supply disconnected; preserve both cards and do not retry.

### 2.49 Boot-time and application media views

The boot record scans physical blocks 0 through `0x139` into the bad-block list at `0x00004310` (`0x00002058`, `0x00003180`, `0x00003464`). Its mapping helper at `0x0000209c` starts from a logical block number and increments the physical result for each sorted bad-list entry not above the current result. The loader publishes that list pointer through the segment table.

Application routine `0x006a90ac` first obtains the loader-published list pointer and constructs the halfword remap table at `0x00828580` without rescanning. Later calls rescan the same physical range through `0x006fb1f4`, rewrite the pointed-to list and rebuild the table. Reader `0x006a8f1c` and writer `0x006a9268` both select `remap[logical]`; a successful writer result with its returned marker flag set calls the rebuild. The boot and application marker classifiers are instruction-equivalent: both treat an all-`0xff` marker as good and the same marker-byte combinations as marginal or bad. Static inspection therefore does not support a claim that they intentionally use different marker formats.

The observed split can still arise if the shared list or remap changes after the native load, if retained warm-start state participates, or if the two reads observe different device state. The owner's no-button startup report does not state that dummy power was physically removed after the code-4 session, so it is not evidence for a post-repair cold start. The smallest next camera check is one true cold ordinary startup with the normal card: no recovery card, no firmware file and no buttons. It requires a new owner go. If it remains dark, another undifferentiated logical block-14 write is contraindicated because it would use the application mapping already shown to return RC0.

### 2.50 Read-only media-view diagnostic candidate

The host-only candidate snapshots the boot list during the early callback, before restoring the recorder call. The delayed stage validates the application list pointer and `remap[14]`, reads the boot-selected physical block through the stock lower reader, reads logical block 14 through the stock adapter, and CRC32-classifies both complete 128 KiB blocks. It issues no internal-media writer and no SD-card writer. Three packet types report status, `boot_physical - 14 + 1`, and `abs(app_physical - boot_physical) + 1`; their sync intervals are four, eight and twelve long delays. Status 9 requires exact recorder CRC at the boot-selected physical block, exact RC0 CRC through logical block 14, and an application mapping exactly one physical block above the captured boot mapping. Statuses 8, 10 and 11 report equal, larger-forward and reverse mapping relations after the same CRC gates.

The final linked 3,852-byte stage is SHA-256 `9a9cf704195929000d199da887972d4235e277be731b5a75af1609eb5ec84971`. The canonical v3 image is `private-data/analysis/x-m1-1.50/bootrom/media-view-diagnostic-card-v3/media-view-diagnostic-sd.img`, 17,826,304 bytes, SHA-256 `0a7468cd4726eb0d0cda278ce345c226643e6b87c91b9cb70353c677a0fbef9f`. Its exact build and verify pass. `//emulation:media-view-diagnostic-smoke` executes the captured physical adapter and covers early-list snapshot, exact `0x006f1440` ABI, both serialization levels, exact recorder/RC0 split, accepted statuses 8 through 11 with ordered packet fields, lock and reader failures, invalid lists, remap mismatch and boot-physical-RC0 classification. The existing bootstrap, classifier and conditional recovery smokes and boot-card tests also pass.

Independent review rejected v1 because its high-level raw reader carried a non-null callback and could skip the requested bad-listed physical block. V2 bypassed that redirect but omitted the lower driver lock. Neither superseded image ran on the camera. V3 directly calls stock physical adapter `0x006fc138` with null callback and exact physical index, bracketed by both the outer storage session and stock object-`0x34` lower lock; every returning reader path releases both. Independent re-review found no remaining blocker and returned **APPROVE**. The operation remains read-only but still requires an operation-specific owner go. No raw physical writer or bad-marker change is built.

### 2.51 Queued media-view operation

The owner authorized this exact v3 operation (E-RECOVERY-MEDIA-VIEW-GO-150). The bounded run uses the preserved 7.2 GB SDHC card, exact host write/readback of image SHA-256 `0a7468cd4726eb0d0cda278ce345c226643e6b87c91b9cb70353c677a0fbef9f`, continuous external video, Fn plus inward sub-command held, and at most 39 approximately 0.75-second switch transitions starting OFF. Stop toggling immediately on the solid bootstrap marker, then wait for the repeating packet sequence without navigating the UI or removing power. Authorization covers one attempt only and is consumed when the camera attempt starts.

The diagnostic result determines whether a repair can be designed. Status 9 plus fields 1 and 2 would identify physical block 14 as the boot-selected exact-recorder copy and physical block 15 as the application-selected exact-RC0 copy. A different accepted status or fields changes that target. Until a reviewed result identifies the exact physical content and mapping, do not write a physical block, alter an OOB marker, run the updater or repeat the logical block-14 repair.

### 2.52 True-cold ordinary startup remained dark

The owner authorized and completed the queued true-cold ordinary startup with the empty conventionally formatted card. The camera remained dark on the single switch-on, and the owner disconnected supply afterward (E-RECOVERY-COLD-START-GO-150, E-RECOVERY-COLD-START-DARK-150). This consumes the cold-start authorization and rules out retained dummy-power state as the explanation for the earlier dark ordinary startup.

The independently approved read-only media-view discriminator subsequently ran under its separate authorization. Section 2.53 records its result.

### 2.53 Media-view diagnostic returned status 12

The owner completed the exact v3 attempt and supplied the captured tail in `~/Downloads/VID_20261013_085744.mp4` (E-RECOVERY-MEDIA-VIEW-STATUS12-150). Frame-level extraction resolves repeating status 12 after accounting for the physical LED's active-low polarity: a four-delay source-on sync, four-delay source-off sync, twelve equal source pulses and the four-delay gap. The second complete cycle begins at 16.230 seconds; its pulse widths are 1.431–1.500 seconds.

Status 12 narrows the split. The boot-list snapshot succeeded, the later application list and `remap[14]` validated, the exact physical read of the boot-selected block succeeded, and that physical block has the recorder CRC. The subsequent stock logical block-14 read returned nonzero. V3 emits no mapping fields on this failure, so the video does not identify either physical index. The authorized attempt is consumed.

The next image must remain read-only and bypass the failed logical-reader step: directly read and CRC-classify both captured physical indices, then report both indices and classifications even when they differ. No persistent repair target is justified until that result is known.

### 2.54 Read-only physical-pair diagnostic

The follow-up removes the logical-reader dependency that returned v3 status 12. It retains the early boot-map snapshot and application list/remap validation, then reads both selected physical blocks through exact stock adapter `0x006fc138`. Each read uses the destination alias, null callback, writable progress storage, cache invalidation, outer storage session and lower object-`0x34` lock. Every acquired lock is released on a returning failure. The operation calls no internal-media or SD-card writer.

The result repeats the same three packet types. Status 9 means the application-selected physical block is one above the boot-selected block and is exact RC0; 10 means a larger forward RC0 split; 11 means a reverse RC0 split; 12 means equal maps with an RC0 second read; 8 means the second physical read failed; 13 means the second block is recorder; 14 means other second-block contents. Statuses 8–14 all emit the boot physical offset plus one and absolute application delta plus one, so a second-read failure still reports both validated indices.

The linked 3,852-byte stage SHA-256 is `9c488f83924da16ae09ca79811bfa4434552daa2b1272080e60c1bd33fbc0966`. Canonical image `private-data/analysis/x-m1-1.50/bootrom/media-view-physical-pair-card-v4/media-view-diagnostic-sd.img` is 17,826,304 bytes with SHA-256 `37576b86a2f924958bd2bd8c44fda85b843a0d1c9829e8a54fba26b931e62c08`. Exact builder verification passes. The behavioral smoke executes the captured stock adapter bytes and covers both physical arguments, both lock levels and cleanup, first/second failure paths, statuses 8–14, mapping fields and packet timing (E-RECOVERY-PHYSICAL-PAIR-DIAGNOSTIC-150).

Independent source and static-byte review found no blocker and returned **APPROVE**. It independently bound the source, generated stage and image hashes; verified snapshot lifetime, map bounds, both physical-reader ABIs, serialization, status/field semantics and absence of a reachable writer; and confirmed no v3 logical-reader reference remains (E-RECOVERY-PHYSICAL-PAIR-REVIEW-150). Hardware behavior remains unproved. The owner subsequently authorized one exact v4 run under the bounded Fn plus inward sub-command procedure (E-RECOVERY-PHYSICAL-PAIR-GO-150); authorization is consumed when the camera attempt starts.

### 2.55 Physical-pair result: status 13, fields 1 and 1

The owner completed the exact v4 run and supplied `~/Videos/video_2026-10-13_13-47-28.mkv` (E-RECOVERY-PHYSICAL-PAIR-STATUS13-150). Fixed-position frame analysis resolves at least two complete cycles after applying the LED's active-low polarity. The first complete cycle contains an 8-delay sync and one pulse, a 12-delay sync and one pulse, then a 4-delay sync and thirteen pulses. The next cycle repeats those values.

This decodes as status 13, boot field 1 and delta field 1. Boot physical is `14 - 1 + 1 = 14`; absolute application delta is `1 - 1 = 0`; both validated maps therefore select physical block 14. Both exact physical reads matched the recorder CRC. The earlier application logical RC0 result was not the current physical content of block 14, and v3's later logical read failed. No physical repair is authorized yet.

### 2.56 Reviewed physical-block 14 recovery candidate

The v4 result supplies the previously missing target evidence: both independently validated mappings select physical block 14 and both direct reads classify its complete contents as recorder. Physical recovery v2 converts that evidence into a bounded write gate (E-RECOVERY-PHYSICAL-BLOCK14-REPAIR-150). It refuses unless the boot and application mappings both equal literal 14, both initial direct reads byte-match a carried recorder reference, and the carried RC0 source byte-matches a separately stored RC0 reference.

The final gate is atomic with target selection. It acquires the stock outer storage lock, rechecks published `remap[14] == 14`, invalidates the data cache, directly rereads physical block 14 under the lower lock and byte-matches recorder again. With the outer lock still held, it cleans the RC0 source and calls stock lower adapter `0x006faf5c` once with physical start 14, count 1 and source alias `0x40040000`. The descriptor and conditional map refresh reproduce the captured stock logical writer below its raced remap lookup. Every actual writer return, including nonzero, proceeds through post-write map validation and direct physical readback when a valid map is available.

V1 never ran and is rejected: independent review found equal non-14 admission, a remap lookup race, skipped readback after writer error and CRC-only identity. V2 resolves all four. Independent re-review returned **APPROVE** with no blocking finding and independently bound the stock reader/writer/lock/cache ABIs, source, disassembly, extended bootstrap, raw/FAT layout and exact hashes (E-RECOVERY-PHYSICAL-BLOCK14-REVIEW-150).

The v2 stage is 3,852 bytes with SHA-256 `46a6edfab37ff71b494e5f2674010bb9a1243ef24d7102c15fab491feb127a93`. Its 512-byte extended bootstrap SHA-256 is `4694c27d9e598292d563d8f51e5ed7059bb29c0a88baca490e3ff0d784f66552`; it relocates to `0x000c0000` and loads the exact `0xa0000`-byte body through either 20 FAT reads or 1,280 raw sector reads. Canonical image `private-data/analysis/x-m1-1.50/bootrom/physical-block14-recovery-card-v2/physical-block14-recovery-sd.img` is 17,826,304 bytes with SHA-256 `1fdda1cc2b670adb51eb705063f4076c859e517f40961dae4b5866a973a31ec6`. Exact builder verification, boot-card tests and four recovery/bootstrap smokes pass.

The repeating four-delay packet is status; the eight-delay packet is boot physical offset plus one; the twelve-delay packet is final physical delta plus one. Status 20 means writer success and exact RC0 readback at unchanged physical 14. Status 21 means writer success and exact RC0 after a validated stock bad-block remap. Statuses 15-16 are pre-write refusals. Statuses 18-23 report a zero writer return followed by invalid map, read failure, RC0, recorder or other content. Statuses 24-29 report the corresponding observations after a nonzero writer return. Status 17 is unused. Any result except 20 or 21 requires preserving power for diagnosis; the candidate loops in the packet emitter and does not continue to UI.

The owner subsequently authorized one exact v2 attempt (E-RECOVERY-PHYSICAL-BLOCK14-GO-150). Authorization covers exact card write/readback and one Fn plus inward sub-command entry attempt with stable dummy power and continuous video. Stop toggling on the solid entry marker and preserve power until the packet is decoded. The authorization is consumed when the camera attempt starts.

### 2.57 Exact recovery attempt refused at status 6

The owner completed the authorized v2 attempt and supplied `~/Videos/video_2026-10-13_15-29-10.mkv` (E-RECOVERY-PHYSICAL-BLOCK14-STATUS6-150). Four complete packets repeat after the camera stabilizes. With active-low polarity, each contains the four-delay sync followed by exactly six pulses; no eight-delay or twelve-delay field packet follows. This is status 6.

Status 6 occurs after the first direct physical read succeeds but before the second physical read and every write gate. The complete 128 KiB boot-selected content byte-matched neither carried recorder nor carried RC0. The stock lower writer was not called, so this attempt requested no persistent-media mutation. The visible UI is compatible with the media-task packet loop. The authorization is consumed; no retry is authorized. The captured-startup investigation below reproduces the reference failure.

### 2.58 Captured startup reproduces the reference failure

The v2 refusal has a concrete software explanation (E-RECOVERY-REFERENCE-LIFETIME-150). Native loading replaces RAM `[0x40000,0x4dae0)` with kernel 0, destroying the old carried repair source. Primary entry then invokes captured zero routine `0x434f0` for `[0x62c54,0x63460)` and `[0x4dae0,0x62c54)`. The combined clear destroys the first `0x3460` bytes of the recorder reference at `0x60000`.

A bounded host replay loads the exact v2 recovery body and captured kernel, executes both clears with alias-normalized pointers, and observes 13,197 changed recorder-reference bytes. With the actual original recorder block supplied by the lower-read model, the real delayed stage emits status 6 after one read and no writer. This reproduces the live refusal without changing the physical block. The previous synthetic smoke reseeded references immediately before the delayed stage and omitted their startup lifetime.

This identifies a candidate defect, not a new fault in the camera. The exact current physical block is still uncaptured. The replay does not execute the whole scheduler, peer or silicon cache behavior.

The contiguous RC0 reference had two additional concrete collisions. Primary idle uses supervisor SP `0xa0000`; executing the captured initial IRQ frame changes 31 bytes of a reference ending there. Moving the reference to `0x70000` instead overlaps the primary log store initialized by `0x41354(0)`; executing that full initializer with shared RAM aliases changes 77,483 reference bytes. Both placements were rejected on the host before a camera run. The initializer's two log spans and the extra startup clear `[0xa0800,0xc0900)` are now exercised, not omitted from the lifetime probe.

### 2.59 Two-buffer physical recovery candidate

V5 retains one logical RC0 reference and the existing physical read/program buffer at `[0x20000,0x40000)`. The reference occupies three fragments, avoiding both boot records' live material, the demonstrated log stores, native NAND scratch and initial idle IRQ frame. The builder refuses unless both native-stage reference holes are entirely `ff` before embedding.

| Logical RC0 bytes | Reference RAM |
| --- | --- |
| `[0x00000,0x08000)` | `[0x08000,0x10000)` |
| `[0x08000,0x10000)` | `[0x16000,0x1e000)` |
| `[0x10000,0x20000)` | `[0x88000,0x98000)` |

Recorder identity remains a complete byte comparison. The expected recorder block is the pinned RC0 reference with substitutions at exactly four word offsets covering the seven changed bytes already verified by the builder. The comparison also requires the RC0 reference's four substituted words to equal their pinned RC0 values. Under the outer lock, the final reread must match that recorder identity. The stage restores those four words in the read buffer from the RC0 reference and compares all 128 KiB to RC0 before cleaning and programming the buffer through alias `0x40020000`. Literal-14 gates, single-program admission and post-readback for every writer return remain unchanged.

The 3,852-byte stage SHA-256 is `00554ef61bbaa2eaf8f3341f75871fc7639c32adc77a6d16f3ae287c014f967e`. The bootstrap remains unchanged. Canonical v5 image `private-data/analysis/x-m1-1.50/bootrom/physical-block14-recovery-card-v5/physical-block14-recovery-sd.img` is 17,826,304 bytes with SHA-256 `15d0cc28fc76d7d78ecf59a455284761333cd0e345929d3f88393fe991f5f343`. V3 and v4 are rejected and must not be deployed.

Canonical verification and five recovery/card suites pass. The [captured-startup command](../emulation/README.md#physical-block-14-recovery-startup-lifetime) uses shared canonical/uncached RAM backing and executes all three known zero calls, the complete zero-arm log initializer with a declared primary discriminator, and the initial idle IRQ frame against the exact v5 body. All reference fragments survive, and the recovery reaches status 20 with complete modeled-media equality to RC0. Refusal cases change a byte inside a rollback word, outside all rollback words, or in the final locked read, or fail that final read. No writer runs in those cases.

Exact v5 byte and expanded-proof reviews both returned **APPROVE** with no blocking finding (E-RECOVERY-FRAGMENTED-V5-REVIEW-150). They independently bind the image/body/stage bytes and complete RC0 reference, inspect fragment transitions and restoration, and confirm the real startup/log/IRQ proof and modeled-media oracle. Earlier v3/v4 approvals are withdrawn (E-RECOVERY-IDLE-LOG-LIFETIME-150). The owner authorized one exact v5 physical-block-14 rollback attempt (E-RECOVERY-FRAGMENTED-V5-GO-150); no result is yet reported. Remaining hardware risks include reference lifetime beyond the modeled startup and initial IRQ frame, real cache/peer scheduling, NAND erase/program/remap behavior and power interruption. Status 20 or 21 is the required exact-RC0 readback result; visible UI alone remains insufficient.

**Owner-authorized operation:** one v5 physical-block-14 rollback attempt using only the exact image above (E-RECOVERY-FRAGMENTED-V5-GO-150). Authorization is consumed when the camera attempt starts; no automatic retry or ordinary restart is authorized. Preparation uses the established expendable-card checks below, from the repository root. Do not overwrite the preserved earlier attempt card if it is the only retained evidence copy.

```bash
(
  set -euo pipefail
  IMG="$PWD/private-data/analysis/x-m1-1.50/bootrom/physical-block14-recovery-card-v5/physical-block14-recovery-sd.img"
  EXPECTED=15d0cc28fc76d7d78ecf59a455284761333cd0e345929d3f88393fe991f5f343
  BYTES=17826304
  printf '%s  %s\n' "$EXPECTED" "$IMG" | sha256sum --check
  [[ "$(stat -c %s "$IMG")" == "$BYTES" ]] || { echo 'Unexpected image size'; exit 1; }
  lsblk -dp -o NAME,SIZE,MODEL,TRAN,RM,RO,TYPE
  read -r -p 'Whole expendable SDHC device in the host reader: ' CARD
  CARD="$(readlink -e -- "$CARD")"
  [[ -b "$CARD" ]] || { echo 'Not a block device'; exit 1; }
  [[ "$(lsblk -dn -o TYPE "$CARD")" == disk ]] || { echo 'Not a whole disk'; exit 1; }
  [[ "$(lsblk -dn -o RO "$CARD")" == 0 ]] || { echo 'Device is read-only'; exit 1; }
  lsblk -p -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$CARD"
  read -r -p "ERASE this expendable card. Type $CARD to confirm: " CONFIRM
  [[ "$CONFIRM" == "$CARD" ]] || { echo 'Cancelled'; exit 1; }
  MOUNTS="$(lsblk -nr -o MOUNTPOINTS "$CARD")"
  [[ -z "${MOUNTS//[[:space:]]/}" ]] || { echo 'Mounted or swap-active device: stop and unmount the expendable card only'; exit 1; }
  [[ "$(sudo blockdev --getss "$CARD")" == 512 ]] || { echo 'Reader does not expose 512-byte sectors'; exit 1; }
  CAPACITY="$(sudo blockdev --getsize64 "$CARD")"
  (( CAPACITY >= BYTES )) || { echo 'Card is smaller than the image'; exit 1; }
  sudo dd if="$IMG" of="$CARD" bs=4M conv=notrunc,fsync status=progress
  sudo blockdev --flushbufs "$CARD"
  sudo cmp --bytes="$BYTES" "$IMG" "$CARD"
  echo 'PASS: image readback matches'
)
```

After an operation-specific owner go and both host checks passing, remove the card without mounting or formatting it. With camera USB disconnected, prepare continuous video of the LED and stable dummy-battery power. Insert this card while unpowered, then hold Fn plus inward sub-command-dial press and use the previously observed warm switch toggles for at most 60 seconds. Stop switching immediately on the solid entry marker; release the buttons and leave the power connection unchanged. Do not cut power, format the card, navigate settings or invoke an updater. Record one complete four-delay status packet and its following mapping fields; a fixed five-minute wait is not required. Preserve power for decoding regardless of visible UI. Only status 20 or 21 proves exact RC0 through this candidate's post-write physical readback; any other status needs diagnosis. If no entry marker appears within the acquisition bound, stop and report no entry. An ordinary-startup check is a separate subsequent decision.

### 2.60 Physical rollback readback: status 20

The owner supplied `private-data/provenance/owner-files/Videos/video_2026-10-13_17-18-21.mkv` after the authorized exact v5 attempt (E-RECOVERY-FRAGMENTED-V5-STATUS20-150). Frame-level red-excess measurements resolve status 20 and mapping fields 1/1. Counts remain identical at thresholds 20, 25 and 30; the physical LED is active-low relative to the emitter's source names.

| Packet | Physical-dark sync | Data pulses |
| --- | --- | --- |
| Boot field | 101.367–113.167 s | One at 119.233–120.633 s |
| Final delta field | 128.167–145.900 s | One at 151.900–153.300 s |
| Status | 160.833–166.700 s | Twenty at 172.700–230.500 s |

Status 20 requires the stock writer to return zero, the post-write map to validate, the direct physical read to succeed and all 128 KiB to equal carried RC0 at the unchanged physical target. Boot field 1 means physical 14; delta field 1 means no physical-index change. This is the first observed successful physical rollback/readback result, not merely a RAM repair or visible UI.

The recovery-attempt authorization is consumed. The owner subsequently authorized one ordinary cold-start check with USB disconnected, no SD card and no buttons held (E-RECOVERY-V5-COLD-START-GO-150). Turn the switch off, disconnect all battery/external power, remove the recovery card while unpowered and wait 30 seconds before reconnecting stable dummy power. This interval separates the checks; it does not prove discharge of every retained state. Switch on once without buttons and observe at most 60 seconds, ending the wait when UI appears. The installed internal firmware supplies the boot; no FW file or card formatting is required. Preserve the recovery card/video. Startup authorization is consumed when the check starts; no automatic retry, writer rerun or firmware reinstall is authorized. Cold-start success and persistence through power removal remain unproved until the owner reports the result.

**Ordinary-startup and functional result:** the owner reports “Started up, with the NO CARD warning” after the authorized no-card, no-button power-disconnect/reconnect check, then confirms photography works without a card, “normal card and playback works” and retained film simulations/settings, including C-slot color +2 (E-RECOVERY-V5-COLD-START-SUCCESS-150, E-RECOVERY-V5-NO-CARD-PHOTOGRAPHY-150, E-RECOVERY-V5-NORMAL-CARD-PLAYBACK-150, E-RECOVERY-V5-RETENTION-150). Ordinary startup without the recovery card after physical rollback supports persistence through the reported power cycle. The failed-boot incident and recovery-adjacent functional/retention checks are complete. Detailed lens/AF and new recipe acceptance remain separate feature work. Both authorizations are consumed. Preserve the successful recovery card unchanged; no recovery writer retry or firmware reinstall is needed.


## 3. The built card's exact bytes

### 3.1 The image

The original halt control is `bootrom/dual-path-diagnostic-card/`, with image SHA-256 `665b2066bafe86e941e2890bd61843b69dc07796d8009ad9e872651278da2b46`. Its stock 1024-byte `ffsys.bin` and 512-byte halt `ffboot.bin` are unchanged from the audited files; the [card report](diagnostic-sd-boot-card.md) records their hashes and host verification. It is not application firmware. The separately built blink candidate in section 2.7 changes only the two payload sectors; this section retains the halt control's exact-byte audit.

The image remains 34817 sectors, with the FAT16 volume at LBA 2048, configuration at 2337–2338, payload at 2339 and unused report sector at 34816. Raw LBAs 0–1 now hold the stock-derived configuration overlay, and LBA 2 holds the same payload. The first MBR entry is `00 00 00 00 04 00 00 00 00 08 00 00 00 00 00 21`: the ROM consumes its type and start LBA, not its stock-derived length or CHS. This is a raw-image-only research layout, not a conventional mountable partition table. Formatting or copying the two files does not preserve the alternate-mode configuration.

### 3.2 ffsys.bin: what the apply does to these exact bytes

**Byte-read:** the 1024 bytes are consumed by `0x80000304` at `0xfff00004`. Ordered writes for these bytes, with the site and the source field:

| Target | Value | Site |
| --- | --- | --- |
| 0xfff88084 | 0 (config+0x10 bit 0 = 0) | 0x80000328-0x80000330 |
| 0xfff98000 bits 17:16 | 3 (config+0x14 bit 4 set, read-modify-write) | 0x80000338-0x8000034c |
| 0xfff98208 | `0x00000011` (config+0x14) | 0x80000350 |
| wait | 0xfff88080 bit 16, no timeout | 0x80000354 |
| 0xfff88000+0x00..+0x20 | `0 0 7 7 7 7 7 0 7` (config+0x20..+0x40, 9 words) | 0x80000360-0x800003c8 |
| 0xfff8802c, 0xfff88030 | 0, 0 | 0x800003cc-0x800003d4 |
| 0xfff88024, 0xfff88028, 0xfff8802c, 0xfff88030 | 0, `0x00060000`, 0, `0x00000a00` (config+0x44..+0x50) | 0x800003d8-0x800003f4 |
| skip | config+0x54 = `0x001a0000` equals the skip constant, so no 0xfffb3000 sequence | 0x800003f8-0x80000400 |
| 0xfff88038 | `0x10000000`, then wait bit 24, no timeout | 0x8000043c-0x80000450 |
| 0xfff99000 | `0x00200101` (config+0x64, bit 0 set) | 0x800005a8-0x800005b0 |
| 0xfff99004 | 9 (config+0x68, bit 0 set) | 0x80000454-0x8000045c |
| 0xfff99008 | 6 (config+0x6c bit 0 cleared, after a bit-0 read-modify-write) | 0x800005b4-0x800005c8, 0x80000460-0x80000468 |
| 0xfff9900c, 0xfff99010, 0xfff99014 | `0x8b330031`, `0x94449443`, `0x40` (config+0x70, +0x74, +0x78) | 0x8000046c-0x80000480 |
| 0xfff99018 | `0x00000101` (config+0x88) | 0x800004ac-0x800004b0 |
| 0xfff9901c | `0x020a0806` (config+0x7c, bit 0 cleared) | 0x80000484-0x8000048c |
| 0xfff990e8, 0xfff990ec | `0x4646`, `0x4646` (config+0xa8, +0xac) | 0x800006d8-0x800006ec |
| 0xfff990f8 | `0x80000000` (config+0x8c, bit 8 cleared) | 0x800006d8-0x800006dc |
| 0xfff01000 words +0x00..+0x244 | the config+0xe0..+0x324 words (146), first word bit 0 cleared, last byte +0x247, then bit 0 set and a wait on 0xfff01090 bit 28 | 0x800005cc-0x80000674 |
| 0xfff52008, 0xfff52030, 0xfff52050, 0xfff52078, 0xfff5207c | `0x00002000` (config+0x328 masked with `0x00302000`), 0, `0x22312131`, `0x00000605`, 0 | 0x80000678-0x800006a4 |
| SD clock helper | config+0x33c = `0x80000000` has gate bit 31 set, so `0x800010ac` is called with channel 0 and selector `config+0x33c & 0xffc0 = 0` | 0x800006a8-0x800006c8 |
| optional pairs | config+0xb8/+0xc0/+0xc8/+0xd0 all zero, so all four are skipped | 0x80000510-0x80000538, 0x80000600-0x80000628 |
| pair table | count 7 at config+0x348, then (0xfffb2004,`0x5f5f`), (0xfffb2008,1), (0xfffb200c,`0x11`), (0xfffb2010,`0x1111`), (0xfffb2014,`0x1111`), (0xfff98100,`0x50000100`), (0xfff98104,0), each followed by `dsb sy` | 0x800006f4-0x80000714 |

**Byte-read:** `0x80000304` returns 0 on every decoded path; a config image fails only by hanging one of the unbounded waits. **Static-inference:** the bytes are the same ones the mode-4 boot applies at every power-up (E-CONFIG-APPLY-150), but mode 1 enters without the mode-4 internal-media initialization, so the waits' incoming state is not proven identical.

**Two corrections to the card report's section 6.1.** First, the optional SD-clock field `+0x33c` is not zero: it reads `0x80000000`, so the helper at 0x800010ac is called, with selector argument 0 taken from the field's bits 15:6. The helper clears the register's bit 2, writes `selector | (register & 0x3f)`, sets bit 0, waits for bit 1 and sets bit 2 (0x800010ac-0x800010f0), so the channel-0 divider field the init left at 0x500 is rewritten to 0 rather than left in place. Second, the payload's `movw sp,#0x200` places the stack pointer one byte past the one-sector staged span at 0x40000000, not inside it; nothing pushes or calls, so no stack location is written either way.

### 3.3 ffboot.bin: the payload and its write set

**Byte-read:** the 512-byte file is 15 A32 instructions in 60 bytes followed by zeros: 0x00 `cpsid if` (0xf10c00c0), 0x04 `movw sp,#0x200`, 0x08 `movw r0,#0x100`, 0x0c/0x10 `movw/movt r1` = `0x44314d58` (bytes 58 4d 31 44), 0x14/0x18 r2 = `0x544f4f42` (42 4f 4f 54), 0x1c/0x20 r3 = `0x00000150`, 0x24/0x28 r4 = 1, 0x2c `stmia r0,{r1-r4}` (0xe880001e, registers r1-r4 stored to 0x100-0x10f), 0x30 `dsb sy`, 0x34 `wfi`, 0x38 `b .`. The write set is the one 16-byte store. **Byte-read:** the staged span is one 512-byte DMA sector at 0x40000000, the alias of RAM 0, so the store lands inside the payload's own staged bytes; the ROM's SRAM buffers, the FAT context and the SD state are untouched.

### 3.4 Why the built card cannot show execution

**Byte-read:** the payload halts in the ROM's own halt shape (`wfi` with IRQ and FIQ masked; `b .` if released), so the only visible outcome is a dark body. The signature at RAM 0x100 lies inside the first staging span of the next mode-4 boot, `[0, 0x40000)`, and is overwritten before any reader can run; the reader itself needs the running application, which the halt prevents (E-SDBOOT-RAM-SIGNATURE-150, `boot-card-detection.md`). A dark body from this card is consistent with payload execution, a config-apply hang and a mount or read failure, so it establishes no entry. This is the audit's finding, confirmed on the exact bytes.

## 4. An observable-outcome design

### 4.1 The channel

Only code the ROM stages from the card can leave a durable mark: every ROM failure path writes volatile state alone (0xfff88084 bit 0, the selector word), and the ROM's SD and internal-media code issues no write command. A payload that writes to the card is therefore the one observable that no earlier stage can produce. The built image already reserves LBA 34816 outside its MBR partition for this.

### 4.2 Payload revision 2, the reporting payload

**Static-inference design, host work, not built here.** Enter at offset 0 of the staged sector as today, and keep the whole sequence straight-line and bounded.

1. Do not reset, re-clock or re-initialize the controller. The mode-1 path leaves channel 0 selected with the card in transfer state and the FAT context at 0xfff00404 (card report section 6.1).
2. Issue one single-block write against that state: block size 512 and count 1, the report LBA as the argument with the CCS scaling the ROM's own block layer used (E-SDBOOT-CARD-IMAGE-150 records the two forms), then the transfer-mode and command halfwords in the order the ROM uses at 0x80001120-0x80001174, PIO through the data window with DMA disabled.
3. Bound every wait with an iteration counter and a distinct halt per failure. No retry, no second write, no command other than the write and one bounded read-back.
4. Record content: a fixed magic, an attempt nonce chosen per run, the payload revision, the entry-state words the payload reads without side effects (the SD ready flag at 0xfff00434+0x10, the CCS flag at +0x0c, the ROM's mode word at 0xfff98000), a checksum over the record's leading bytes, and a completion marker written last.
5. On success, one bounded read of the same LBA and a byte compare, then halt. On any failure, halt without a second write.

**Unproved and blocking:** the controller's transmit path, FIFO direction and pacing, completion and busy semantics, and the actual post-config clock rate. A modeled controller cannot establish these on silicon (card report sections 6.1-6.2). The write helper needs the same independent byte review and owner go as any write-capable native code.

### 4.3 What the host observes

Read LBA 34816 as a raw sector on a card reader, never as a file: magic, nonce and checksum must validate. The nonce rejects a stale record from a previous attempt, the checksum rejects a torn one, and the completion marker distinguishes a finished record from a partial one. The card is disposable; a valid record is the only claimed positive evidence.

| Observation | Interpretation | Bound |
| --- | --- | --- |
| application boots normally, report absent or unchanged | the sample did not select mode 1; the card was never read | decisive for the tested actuation |
| dark body, valid fresh report | mode-1 entry taken; mount, apply and both file reads succeeded; the staged payload executed | decisive; a valid record cannot be produced by an earlier stage, and no fallback or re-sample exists after the halt |
| dark body, no or invalid report | no mode-1 entry can be claimed: the sample was not mode 4, and the candidates include a selector-7 halt, mode 2 reading raw blocks without consulting the files, another non-mode-4 handler, or a mode-1 failure before the payload's write | inconclusive; not evidence of entry or execution |

### 4.4 The no-ffboot control card is inconclusive in the dark

**Byte-read:** removing `ffboot.bin` cannot identify the selected mode. A selector-7 sample goes to the same halt whatever the high pair reads: `mvneq r0,#0` at 0x80000290 with `beq 0x800002f0` at 0x80000294 feeds the -1 path, and the dispatch's `bne` at 0x800001fc reaches the halt at 0x80000020. A selector-4 sample with a nonzero high pair calls mode 2 at 0x80000170, which reads raw blocks and never opens `ffboot.bin`, and modes 3, 5, 6, 7 and 0 also ignore the file. **Static-inference:** dark on the control card is consistent with several non-mode-1 outcomes. The control card rules out a mode-1 sample when it boots normally: only mode 4 reaches the application on this body, and mode 1 with this card always ends at the halt. When it is dark it establishes nothing, and a later boot or reset samples the straps again and says nothing about the first attempt. The reporting record, the one code path that only mode 1 stages and runs, is the only positive proof, as section 4.3 states.

### 4.5 Residual ambiguity

**Unproved and stated as a bound:** the ROM's only failure writes are volatile, and its stage order (mount, ffsys read, apply, ffboot read, handoff) offers no injection point a payload could occupy before the apply. Darkness without a valid record is inconclusive at two levels: it does not establish mode 1 at all (the selector-7 halt and mode 2 both leave the body dark, and section 4.4's control card cannot separate them), and within mode 1 it cannot separate a config-apply hang from a read failure or a failed payload write. A valid record is the only positive proof, and no later observation reconstructs an earlier sample.

### 4.6 The mode-2 safety envelope

**Byte-read:** config offset x is written to `0xfff01000 + (x - 0xe0)`. The indexed loop at `0x800005dc`–`0x800005ec` stops after word index 145: target words +`0x00` through +`0x244`, last byte +`0x247`. The old FAT-only image supplied ordinary MBR bytes and a zero second sector as mode-2 configuration; this changed many applied values and supplied a zero staging count. The current verifier rejects that image.

The hardening is built: raw LBAs 0–1 retain stock configuration bytes except the loader count at +`0x18` and required MBR fields. The count at SRAM `0xfff0001c` is one, and LBA 2 supplies the halt payload. Exactly three configuration-apply words differ: `0xfff010e0` takes `0x00040000` instead of zero; `0xfff010e4` takes `0x08000000` instead of zero; `0xfff0111c` takes `0xaa550101` instead of `0x01000101`. Their effects remain unproved. Matching the other words does not establish a controlled halt, since configuration apply precedes payload staging and has unbounded waits. Mode-2 channel-1 physical-slot wiring and the selector source remain unbound.

The x70view `PHY` label and simulated completion are leads, not target register definitions. The local investigation below does not depend on an author reply.

**Static-inference:** `0xfff01000` is likely a packed DDR memory-controller bank, with a DATABAHN-family relationship plausible. The target's stock word 0 is `0x600`; the ROM clears bit 0, copies 145 following words, sets bit 0, then polls word 36 bit 28. The stock table contains packed small values and repeated groups with ascending tags 0–13. A Fujitsu-origin [MB8AC0300 DATABAHN header](https://raw.githubusercontent.com/hypoxic/stubby-1.60.1/d670fe97469f4fbac35fcf67fbab81c7ea84f153/src/ddr3c.h) and [initialization source](https://raw.githubusercontent.com/hypoxic/stubby-1.60.1/d670fe97469f4fbac35fcf67fbab81c7ea84f153/src/ddr3c_init.c) independently define DDR3 class `6 << 8` and START at bit 0. This comparison supports a family interpretation, not target field names.

The reference layout differs: it declares 115 words, places interrupt status/ack/mask at words 40/41/42, and differs in packing by word 2. The target copies 146 words and polls word 36. Reference fields at numeric offsets `+0xe0`, `+0xe4` and `+0x11c` cannot be transplanted. Its shifted preset values also do not supply exhaustive writable or reserved-bit masks. Controller-local interrupt fields remain compatible with a DDR-controller identity; a standalone interrupt-bank label is not.

**Byte-read, independently checked runtime dataflow:** image 5 routine `0x00ec7688`–`0x00ec7828` (`private-data/analysis/x-m1-1.50/lz/id5.dis`, lines 736491–736595) sets target `+0x48` bit 16 before WFI and clears it afterward. It saves original `+0x98` and `+0x00` into SRAM `0xfff00690`/`0xfff00694`, manipulates `+0x98` bit 13, clears `+0x00` bit 0, then restores those original words. Target `+0x94` receives a post-bit-13-set `+0x98` readback OR `0x007fff00`; after WFI it receives the saved `+0x98`-derived word without that OR. It never reads original `+0x94`, so this is not restoration of that register's original value. Companion controls are partly saved and rewritten; clock `0xfff88038` bit 28 is cleared before WFI, set afterward, then bit 24 is polled. **Static-inference:** coordinated memory-controller/power-state handling is strongly supported. ACK/MASK names for `+0x94`/`+0x98` remain hypotheses. This routine does not access any of the three overlay words.

The held configuration comparison is stock 1.50 versus the project-authored overlay, not two independent target configurations. The held session-window inventory contains two `fff*.bin` captures, both four-byte strap reads (`0xfff89014` and `0xfff98000` in `20261008T061646Z-probe-straps`); it contains no controller-bank or SRAM-config capture. A 1.00 boot configuration, cold/warm controller comparison and target chip/DRAM markings were not found in the held material. Those missing observations cannot be replaced by a simulated ready bit.

**Emulated, bounded instruction-window evidence:** replayable cases under `private-data/scratch/sd-register-window/` execute unchanged captured ROM bytes relocated to `0x00010000` under the host Cortex-R5F proxy. Register seeds bind config, target and companion addresses; the consumed companion word `+0x1c` is seeded from the preceding ROM assignment, `config[0x7c] & ~1`. Device registers are ordinary fixture memory. Both stock and raw write cases execute 883 instructions with 148 stores: 146 initial target words, companion `0xfff9901c = 0x020a0807`, then target word 0 enabled to `0x601`. Their three differing target stores occur intact at original PC `0x800005e4`, before readiness is read.

The cases deliberately stop at relocated `0x00010668`, before the polling instruction, with runner status `unknown_call`; that status is host containment, not a successful apply return. Separate polling fixtures execute the unchanged `0x8000065c`–`0x80000674` window: bit 28 clear at target `+0x90` exhausts 100 instructions; bit 28 seeded set reaches the deliberate next boundary after seven instructions. A fixture-set ready bit is not hardware initialization evidence. Independent review checked ROM/config identity, retained complete provenance, exact stores, terminal PCs and scope.

The raw overlay sets target `+0xe0` bit 18 and `+0xe4` bit 27. At target `+0x11c` it sets mask `0xaa550000` and clears stock mask `0x01000000`, including bit 24. These are full-word software writes. Existing zero values do not establish reserved or write-ignored bits, and readback or bus-level writes alone would not establish harmless cold-start effects. The exact target field meanings and access classes remain the live-clearance boundary.

### 4.7 Status

The startup-restored marker establishes application-independent execution through a ROM SD route (E-SDBOOT-BLINK-WARM-OBSERVED-150). FAT/raw selection, low selector, successful switch edge and controlled entry remain unbound. The marker alone establishes neither internal-media reachability nor repair. Later bootstrap/UI and v5 writer/readback findings establish scoped R3 and a defined physical repair; formal R4 acceptance remains unclaimed. [Recovery status](recovery-status.md) owns the current scope.

### 4.8 Route decision and discriminating follow-up

The passive-capture design below is a possible route discriminator, not a ready camera operation. Acquisition was declined. The successful v5 repair does not bind a reproducible selector/timing recipe or validate every configuration-register effect above. No further camera recovery attempt is justified; any new experiment needs its own scope, reviewed bytes and owner authorization.

**Static-inference, optional hardware experiment:** a qualified passive capture of the SD command, response and data lines could identify how far the tested actuation progressed without modifying payload or configuration bytes. Decode addressing from the card's negotiated CCS state, not by assuming that command arguments are LBAs. Bind the actual image/readback and distinguish a request from a successful response and complete data transfer.

| Bus observation | Narrowed result |
| --- | --- |
| No payload transfer under a complete, qualified capture | The staged payload was not successfully fetched on the observed card interface. The preceding command trace can distinguish no SD traffic, initialization/mount failure and configuration-read progress; it does not identify an unobserved ROM mode or channel. |
| FAT path successfully reads configuration LBAs 2337–2338, but not payload LBA 2339 | Failure lies after configuration reads and before successful payload staging; configuration apply, the subsequent directory/read path and clocks remain candidates. |
| Successful complete transfer of payload LBA 2339, or raw-path LBA 2 after configuration LBAs 0–1 | Payload bytes reached the card interface. The remaining problem is after the transfer or in the indicator path; this does not prove DMA delivery, handoff or instruction execution. |

Prerequisite: a high-impedance SD breakout/capture setup qualified for the actual I/O voltage, clock rate and bus width, with a known-good ordinary-boot trace establishing that responses and data are captured correctly. A partial or unqualified trace cannot turn missing traffic into negative evidence. The electrical setup and any new camera run need their own review/owner go. No such setup is established here. The capture route is not an active release task. The owner's subsequent extra-register request in section 2.10 is a separate bounded exception, not a replacement passive-capture plan or a button sweep.

**Owner decision:** the owner has neither capture device nor breakout and declines acquiring them (E-SDBOOT-BUS-NOT-PURSUED-150). Do not plan or dispatch passive SD capture. SD boot stays parked; reopen on concrete new target-bound evidence, not a repeated dark-only test or an unrequested hardware plan. The release dispatch below remains the active route.

**Release dispatch:** finish F9 stock navigation/rendering and regression acceptance on the integrated candidate; resolve or explicitly disposition the separate placement/lifetime risks; rebuild the complete final container with boot-store metadata, recipe changes and only compatible instrumentation; run the real consumers and corruption/fallback paths on those emitted bytes; independently review that exact artifact. Preserve the body-specific backup and named updater interruption risk, then request an operation-specific flash go. Source-level closure, kernel acceptance and RC0's successful install do not accept the new candidate.

**Host route check:** `rebuild-update diff` against the pinned stock DAT and the previously installed RC0 prefix DAT passed 42/42 contract checks and found identity, selector, fill and both restore lists unchanged. The CLI required absolute input paths under `bazel run`; the first relative-path invocation did not find the DAT and performed no validation. This check concerns the RC0 reference, not the currently integrating recipe RC, attached decoded-image verification or a new physical installation.


## Reproduction

Run from the repository root:

```sh
devenv shell -- bazel run //host/firmware:build-boot-card -- verify private-data/firmware/x-m1-1.50/FWUP0002.DAT private-data/analysis/x-m1-1.50/bootrom/diagnostic-halt.bin private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card/diagnostic-sd.img
sha256sum private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card/{ffsys.bin,ffboot.bin,diagnostic-sd.img}
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi bazel-bin/host/firmware/diagnostic-halt.elf
```

`od -A x -t x1 -j 446 -N 16 private-data/analysis/x-m1-1.50/bootrom/dual-path-diagnostic-card/diagnostic-sd.img` reads the current MBR entry. The mode-1 and mode-2 handler bodies are in `bootrom/bootrom.dis` at `0x8000082c`–`0x80000934` and `0x8000071c`–`0x80000828`; the card rows are at image-4 file offsets `0x20be8c` and `0x20bf88` (base `0x61c400`).

Related: [the window map](bootrom-window.md), [the entry analysis](sd-boot-claim-test.md), [the card record](diagnostic-sd-boot-card.md), [the detection trace](boot-card-detection.md), [hardware routes](recovery-hardware-routes.md).
