# Hardware forcing sheet: the boot ROM's SD-BOOT strap sample (X-M1 1.50, XM1-body-01)

**Question.** Which physical pads or test points drive the strap registers the ROM samples at reset, and what does it take to force mode 1 or mode 2 by hand?

**Answer in one paragraph.** The mode is a pure function of two fields read once per reset: `0xfff89014` bits 11:10 and `0xfff98000` bits 2:0. Mode 1 requires the first field to be exactly `0b00` while the second reads 0 through 6; mode 2 requires a nonzero first field while the second reads 4; a second field of 7 halts whatever the first reads. The first field is bound by this body's own bytes to two controls (Fn and the sub-command dial push, active-low), but no pad, test point, resistor or connector pin that carries it is identified in any held artifact, and the second field has no identified physical source at all. The sheet can therefore specify an executable mode-1 forcing procedure only after an owner observation binds the two nets; it cannot promise mode 2 until the second field's source is identified. A forced entry is proven only by a positive payload marker; a dark body proves nothing.

Labels follow the recovery reports: **byte-read** names artifact bytes, **hardware-observed** names existing reviewed captures, **reported** names owner/manual/sibling statements, **static-inference** names deductions the bytes support, **unproved** names what nothing here establishes. Host-only: no camera, USB device, card reader or probe was touched, no card was written and nothing here is authorized.

**Current disposition, 2026-10-12:** the owner backburnered pad forcing because board access and electrical probing are too invasive for now. This does not weaken the earlier hardware observation that both controls work: the act-logged session recorded the physical Fn press and sub-command-dial push separately at application runtime. The unresolved question is narrower, whether both corresponding GPIO inputs can be made low simultaneously at the ROM's one reset-time sample. No continuity work or powered forcing is authorized.

## 1. Register side, exact

### 1.1 The two reads and the decode

**Byte-read**, pinned listing `private-data/analysis/x-m1-1.50/bootrom/bootrom.dis` and artifact `bootrom-0x80000000-4000.bin` (SHA-1 `bed698ed1d91b5c29ce7c5d650156e6100c32cce`):

| PC | word | instruction | effect |
| --- | --- | --- | --- |
| 0x80000130 | eb00004a | `bl 0x80000260` | mode detect called once from reset, before the dispatch |
| 0x80000260 | e59f0804 | `ldr r0,[pc,#0x804]` | r0 = 0xfff89000 (GPIO block base) |
| 0x80000264 | e59f27fc | `ldr r2,[pc,#0x7fc]` | r2 = 0xfff98000 (strap/status word) |
| 0x80000268 | e59f3800 | `ldr r3,[pc,#0x800]` | r3 = 0xfff88000 (clock base, halt path only) |
| 0x8000026c | e59f1800 | `ldr r1,[pc,#0x800]` | r1 = 0x0c000c00 |
| 0x80000270 | e5801008 | `str r1,[r0,#0x8]` | [0xfff89008] = 0x0c000c00 |
| 0x80000274 | e5900014 | `ldr r0,[r0,#0x14]` | sample 0xfff89014, 32-bit port 0 input word |
| 0x80000278 | e5921000 | `ldr r1,[r2]` | sample 0xfff98000, 32-bit |
| 0x8000027c | e2000b03 | `and r0,r0,#0xc00` | keep GPIO bits 11:10 only |
| 0x80000280 | e2011007 | `and r1,r1,#7` | keep selector bits 2:0 only |
| 0x80000284 | e18113a0 | `orr r1,r1,r0,lsr #7` | r1 = (pair << 3) \| selector; bits 4:3 pair, bits 2:0 selector |
| 0x80000288-0x80000294 | e3a00007 / e1d00001 / 03e00000 / 0a000015 | `mov r0,#7`; `bics r0,r0,r1`; `mvneq r0,#0`; `beq 0x800002f0` | selector 7 -> -1 halt, whatever the pair reads |
| 0x80000298-0x800002a0 | e3110018 / 03a00001 / 0a000012 | `tst r1,#24`; `moveq r0,#1`; `beq` | pair 0b00 -> mode 1 |
| 0x800002a4-0x800002ec | e3110007 / 03a00003 ... 13a00000 | selector 0,1,2,3 -> modes 3,4,5,6; 4,5 -> 2,7; 6 -> 0 | pair nonzero mapping |
| 0x800002f0 | e5821000 | `str r1,[r2]` | write the combined word back to 0xfff98000 |
| 0x800002f4-0x80000300 | e3700001 / 03a01000 / 05831084 / e12fff1e | `cmn r0,#-1`; `moveq r1,#0`; `streq r1,[r3,#0x84]`; `bx lr` | on the -1 path, 0xfff88084 bit 0 = 0 |

**Byte-read:** the two fields are the entire input. `and r0,r0,#0xc00` and `and r1,r1,#7` mask everything else, and no other load or test lies between the write at 0x80000270 and the return at 0x80000300. The sample is one pass, called once from reset at 0x80000130. The dispatch (0x8000013c-0x800001f0) runs one handler per boot and branches to the halt at 0x80000020 (`wfi`) and 0x80000024 (`b .`) on any nonzero handler return (`0x800001f8 cmp r0,#0`; `0x800001fc bne 0x80000020`). There is no fallback, no loop and no second sample.

### 1.2 What each mode needs

| 0xfff89014 bits 11:10 (pair) | 0xfff98000 bits 2:0 (selector) | mode | handler |
| --- | --- | --- | --- |
| any | 7 | -1 halt | 0x80000020 / 0x80000024 |
| exactly 0b00 | 0 through 6 | 1 SD/FAT channel 0 | 0x8000082c |
| nonzero | 0 | 3 internal format 4 | 0x80000938, r0=0 |
| nonzero | 1 | 4 internal format 1 | 0x80000938, r0=1 |
| nonzero | 2 | 5 internal format 2 | 0x80000938, r0=2 |
| nonzero | 3 | 6 internal format 3 | 0x80000938, r0=3 |
| nonzero | 4 | 2 raw SD channel 1 | 0x8000071c, r0=1 |
| nonzero | 5 | 7 config/bulk receiver at 0xfff56000 | 0x80000ac0 |
| nonzero | 6 | 0 RAM handoff | 0x80000b2c |

Mode 1 in exact terms: pair exactly `0b00`, selector not 7. Mode 2 in exact terms: pair nonzero, selector exactly 4. Mode 4, what this body boots: pair nonzero, selector exactly 1.

**Static-inference:** the two fields are not equal candidates for forcing. The pair can be moved by the two mapped controls; the selector has no mapped control, package pin, fuse, resistor, companion register or card path. With the selector reading 1, the only value any held capture shows, the pair alone toggles between mode 4 (nonzero) and mode 1 (zero). No physical action reaches mode 2, 0, 3, 5, 6 or 7 without first identifying what drives bits 2:0.

### 1.3 What the later stores do to a forced value

The question is whether a value forced at the sample survives the two later stores. It does, instruction-wise, because neither store writes a different value into the sampled field; both write back what they read.

1. **Writeback 0x800002f0, byte-read:** `str r1,[r2]` stores the combined word from 0x80000284, bits 4:3 the sampled pair and bits 2:0 the sampled selector, and clears bits 31:5. It runs on every path, including the selector-7 halt, which branches to the same store. The instruction writes back the values that were read, not a default, so it cannot overwrite a forced sample with a different value; whether the silicon accepts the store is covered by caveat (ii) below.
2. **Config apply 0x80000334-0x80000350, byte-read:** `ldr r1,[config+0x14]`; `ldr r2,[0xfff98000]` at 0x80000338; `bic r2,r2,#0x30000` clears only bits 17:16; `orreq r2,r2,#0x20000` or `orrne r2,r2,#0x30000` selects 0b10 or 0b11 from config bit 4; `str r2,[0xfff98000]` at 0x8000034c; then `str r1,[0xfff98000,#0x208]` writes 0xfff98208 at 0x80000350. Bits 4:0 are preserved by this sequence, so it also cannot clobber a forced sample. This is where the config-apply change at bits 17:16 lives.
3. **Handoff 0x80002298-0x800022c0, byte-read:** writes 1 (mode 0) or 2 to `0xfff98008`, a different register, then jumps to 0x00000000. The mode-1 handler itself never touches either strap register; it mounts the card, reads `ffsys.bin` to 0xfff00004, applies it, stages `ffboot.bin` to 0x40000000 and returns 0.
4. **Byte-read:** `0xfff89014` has no store in any checked artifact. The only writes into its block are the `+0x08` control word and the pin-mux/control offsets; the firmware port-snapshot loops read it.
5. **Caveats, unproved.** (i) The config apply's count-bounded `(address,value)` loop at 0x800006f4-0x80000700 can request a store to any address, either strap register included, from card bytes; whether either register accepts the write is unproved, and a non-reviewed card can therefore make the ROM issue such a store request. (ii) **Hardware-observed:** the held maintenance capture reads `0xfff98000 = 0x00000019` while that same boot's apply writes bits 17:16 to 0b11, since the pinned stock config's `+0x14` = 0x11 sets bit 4. The capture shows bits 17:16 as 0. **Static-inference:** the written field does not read back in the held read; the read-back semantics of written bits are unproved, so a software write of the low selector must not be assumed to read back even if the instruction retires.

**Byte-read:** no later path re-reads either field for selection, so a forced level may be released after the sample without changing this boot's dispatch. The one exception is mode 7's own wait, which polls `0xfff89014` bits 11:10 high at 0x800020e0 and cannot complete with a pad held low; mode 7 is reachable only with selector 5.

### 1.4 What forcing can and cannot select

- Mode 1: driving both pads to read `0b00` at the sample selects it if the selector reads 0 through 6. That is the sheet's whole opportunity.
- Mode 2: driving the pair does nothing for mode 2 while the selector reads 1; mode 2 needs selector 4, whose source is unidentified. Do not attempt it by driving the pair.
- Context to explain first, or the attempt is blind: the pair reads 3 with no control held (idle capture), and the sweep read it nonzero with the mapped controls held. Any forcing hypothesis must account for both.

## 2. Physical side, honest

### 2.1 What is established

- **Byte-read:** both sampled pads are in port 0 of the 16-port register block at 0xfff89000, stride 0x40, input word at `+0x14`. Bits 11:10 are two of its pads. Card detect is a different signal (SD-host present state in the ROM, port 12 bit 7 in the application) and is not either sampled field.
- **Byte-read:** image-4 row 13 at `0x82828c` is code 0x28 (`KEY_FUNCTION`), kind 2, operand 0xb, asserted level 0, which resolves to GPIO0 bit 11; row 22 at `0x828388` is code 0x2b (`KEY_CMDL_PUSH`), operand 0xa, level 0, GPIO0 bit 10. The kind-2 compare asserts while the bit reads 0.
- **Hardware-observed:** the act-logged session `20261009T064945Z` emits those codes on the physical Fn press (tick 17087) and sub-command push (tick 22342), so at application run time those two pads follow those two controls.
- **Hardware-observed:** the only strap capture (E-STRAP-READ-150, maintenance boot) reads `0xfff89014 = 0x00000d4e`, pair 3, and `0xfff98000 = 0x00000019`, selector 1, with no mapped control held; the same boot's port-mirror word 0 reads `0x00000d62`, pair 3. **Static-inference:** any nonzero pair with selector 1 identifies mode 4, the normal display boot.
- **Byte-read:** the ROM writes `0x0c000c00` to `0xfff89008` four instructions before the read; mode-7 initialization repeats an OR-and-store of the same value and then polls bits 11:10 high (0x800020cc-0x800020e8), so the write alone does not force the input word's bits high.
- **Hardware-observed:** the cardless press sweep (E-SD-BOOT-ENTRY-HOLD-150) held Fn together with every body button including the sub-command dial push at power-on, and every combination booted the application; the exception, Fn plus DISP/BACK, reached the upgrade screen, an application surface, so it also ran mode 4. **Static-inference:** every tested hold left the pair nonzero at the sample; the sweep fixes only that. One control reading low would give 0b01 or 0b10, which also boots mode 4, so the exact value under hold is unmeasured.

### 2.2 What the service jig connects to

- **Reported** (service manual, E-SERVICE-MANUAL): the PC path is the camera's USB Micro B socket through the jig driver package, ZJ00684-100 (Win2k/XP) or ZJ02197-100 (Win7, BULLETIN 2013-058); ZJ02217 is the SD-BOOT card package holding `ffboot.bin`, `ffsys.bin` and `ffuser.bin`; ZJ01936-100 is a 7.2 V battery jig used for battery-voltage adjustment; the camera fixing jig is a mount. The documented jig entry (shutter plus UP with USB attached) runs application code (E-JIG-ENTRY-KEY-BIND-150), so it is not a strap contact either.
- **Reported** (held manual ZM01086-107): it has disassembly, board and FFC assembly identities (`BUTTON FN` M206, `Y29971_MAIN-TOP_FPC` M223, `Y29971MAIN_ASSY` M343, `Y29971_REAR_FFC` M405) and fuses, not a GPIO or reset schematic, connector pinout or pad list.
- **Reported and source-inspected** (ZM01086-107 and E-XM1-SCHEMATIC-SEARCH-150): the manual's raster board views on PDF pages 22–27 and 42–43 and its electrical-parts view identify the MAIN PWB, MAIN-TOP FPC, REAR FFC, FLASH FFC, WLAN FPC and SD-related assemblies. They do not label GPIO nets, test-point functions, connector pins, reset straps or debug contacts. A public-source sweep found no X-M1 schematic, board view with net data, FCC internal exhibit, JTAG map or FF4227 datasheet. Public fffw pages bind lower-three-bit boot-source semantics and FF4227 family context only on sibling bodies; they do not close an X-M1 physical net.
- **Reported** (owner, this task's brief): pogo-pin connectors under a sibling camera are a lead shape. **Unproved:** whether the X-M1 exposes equivalent sprung contacts and what any of them carry; nothing in-repo records an X-M1 pogo contact, pinout or net.
- **Static-inference:** the held service material establishes three external connections only, USB, the SD slot and the battery contacts. None is shown to carry either strap field, and no jig fixture can be assumed to probe them.

### 2.3 Unproved, and the specific owner observation each needs

| Fact | Status | Observation that binds it |
| --- | --- | --- |
| Package pin, pad, test-point or net identity of GPIO0 bits 11:10 | unproved; no numbered pad, resistor, pin or net identified | unpowered board photos, both sides and under every FPC, including sprung contacts; then continuity from each switch contact to candidate points |
| Whether a switch closure directly grounds a pad or passes through a series device, matrix or scanner | unproved; the sweep's normal boots are consistent with either | the same unpowered continuity work: switch open and closed, net resistance to ground and to the positive supply terminal; a net that beeps to the switch only when pressed and reads a resistor, not a short, to one rail is a candidate signal net, and direct continuity to a rail means stop |
| Whether any panel actuation can make the pads read 0b00 at the sample | unproved; the sweep excludes it only for the tested holds | a powered idle-level measurement at a bound candidate point with the control held, on a booting body; the body is currently failed, so only the unpowered part is available today, and no held capture reads either register with a control held at reset |
| The electrical function and update semantics of the 0xfff89008 write (data, enable, pull or function select; replace versus masked) | unproved; the sibling mask/level convention is a fixture, not X-M1 evidence | SoC or board documentation, or a powered measurement showing the pad level follows the switch while that control word is in place |
| The physical source of 0xfff98000 bits 2:0 (pins, fuses, a reset-loaded register, a companion) | unproved; no control, pin, fuse, resistor, companion register or card-dependent path identified | unpowered continuity from the SD slot and card-detect and from companion/FPC pins to candidate strap pins, plus SoC documentation; a card-dependency test needs a paired card-present/absent register capture, which nothing held provides |
| Low-selector writeability and reset retention (SELECTOR-WRITE-01, RESET-RETENTION-01) | unproved; the ROM stores only unchanged bits, and the captured readback shows a written field reading 0 | the selector's source must be identified first; then a guarded software experiment, not a pin measurement |
| LED identity and readiness behind the payload marker | unproved; the marker witness exists but the physical indicator is not bound | a photo of the marker attempt; not blocking for forcing |

The currently failed body rules out run-time pad measurements, so two steps remain available: the unpowered continuity work and the reset attempt itself. Both are below.

## 3. Forcing procedure

Style notes: this follows the camera-run conventions, one bounded operation per go, no new bytes, stop and report. It is an invasive operation (board access) and needs an owner-discussed plan and a fresh go before any probe touches the body. Nothing here is authorized by this sheet; the steps are written for the owner to execute or reject.

### Stage A: observation (body unpowered, battery out, card out, nothing soldered)

1. Photograph both sides of the main board and every FPC at a magnification that reads designators, including any sprung or pogo contacts. Set the photos beside the reported sibling-camera pogo connector if one is available. Keep a photo index.
2. With the meter in continuity mode, locate the two switch contacts, Fn and the sub-command dial push. Confirm each closes to a distinct net and opens otherwise.
3. Probe every candidate service point, FPC pin and sprung contact against both switch nets, and record which points beep to which net.
4. For each candidate, measure resistance to ground and to the positive battery or supply terminal, and classify it. A few ohms to a rail is a rail: stop, do not drive it. A resistor to one rail and open to the other is a plausible pulled input.
5. Decision gate. Proceed to Stage B only if both pads' candidates are classified as signal nets and at least one candidate sits on the SoC side of any series element found in step 3. If not, stop, re-derive from a schematic or SoC documentation, and record that the switch-side press was already evidence-negative.

### Stage B: one forced attempt (fresh go naming this operation, a reviewed card, probes in place)

1. Use the reviewed diagnostic card whose bytes passed their recorded readback check: the startup-restored marker image (`private-data/analysis/x-m1-1.50/bootrom/dual-path-blink-startup-card/diagnostic-sd.img`, SHA-256 `4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0`) or the reviewed stock-native rollback image if that trial is reopened. Do not write a new image for this attempt.
2. Body off, battery or supply removed. Connect one probe per bound candidate net, each through a series resistor sized from Stage A step 4 so the net can be pulled below its input threshold; if the net shows a hard pull-up, stop instead of driving. Never use a bare short, never bridge two nets.
3. USB disconnected. Insert the card. Apply power with both probes holding the nets low, and keep the hold at least 10 seconds after power-on.
4. Record the LCD and indicator from before power-on, and observe for at most 60 seconds. Note the first visible change and the cadence.
5. Cut the external supply or remove the battery immediately at the 60-second mark or on any unexpected actuation, noise or heat. Do not rely on the software switch while the body is dark. Remove the probes only after the supply is cut.
6. Report the outcome, the photos, the step 4 classifications and the cut method, and keep the card readback with the record.

### What is visible, and what it proves

| Observation | What it proves |
| --- | --- |
| repeating cadence of about three-second pulses separated by a much longer steady interval, all illuminated intervals red | the staged payload ran, so at least one early ROM path selected and loaded it. FAT mode 1 versus raw mode 2 is not distinguished by the cadence (E-SDBOOT-BLINK-STARTUP-OBSERVED-150). On the reviewed startup-restored card, mode 1 is the intended reading, not an established one |
| a validated report record at reserved LBA 34816 in the post-attempt readback | if the reporting-design card is used instead, entry plus execution, and it survives a reset; darkness without a record is explicitly not evidence (E-SD-BOOT-ENTRY-ANALYSIS-150) |
| ordinary application UI | the pair did not read 0b00 at the sample; useful only on a booting body |
| update or format prompt, or any unexpected screen | stop; the update screen is application code reached after a normal boot, and accepting a prompt destroys state |
| dark, no LED | inconclusive: mode 1 selected with any card or file stage failure returns -1 into the same WFI halt as a dead application boot, or another mode ran, or the application is dead. Nothing about the sample is proven |

**Byte-read, why dark proves nothing:** the mode-1 handler's failure paths return -1 (the mount and file-open failures also write 0 to 0xfff88084; the read-retry exhaustion calls the teardown 0x80000b3c), and the dispatch halts at 0x80000020 on any nonzero return. Any later reset re-samples both fields and rewrites 0xfff98000 at 0x800002f0, so a later session cannot reconstruct this attempt's sample, and a halted body has no session at all.

### Stop conditions

1. Before the attempt: stop if the Stage A gate has not passed, if any candidate is classified as a rail, if the card has not passed its readback check, or if no fresh go names this operation.
2. During: cut supply immediately on heat, smell, unusual noise, an unexpected screen, a format or update prompt, or a sustained supply-current anomaly if the supply shows one. The 60-second ceiling and one attempt per go are absolute.
3. After: supply cut first, then probes, then the power switch OFF while unpowered. Do not retry, sweep combinations or swap cards for a second attempt under the same go; a repeat needs a new named condition, a fresh go and, for new bytes, a new independent review.
4. If the body enters or appears to enter an update or format flow, do not interact with prompts; cut supply.

### Named risks

1. **Electrical drive on unidentified nets, unproved hardware risk.** The Sol byte review of the SD-boot candidates retains the three MBR-derived raw-mode register writes as unproved hardware risks; driving a net whose function, pull strength and driver state are unknown is that class on the electrical side. A wrong-direction drive, a pad shorted to a driven output, or a bridged rail can damage the SoC or the panel. Mitigations: the Stage A classification, series resistors, battery supply, measuring one pad at a time, and no bare shorts.
2. **Raw mode-2 state application.** If the selector reads 4 at the attempt, its source being unproved, a nonzero pair runs mode 2 at 0x8000071c, which reads card blocks 0 and 1 and applies them as hardware configuration through 0x80000304 (the config window maps to 0xfff01000+(x-0xe0); correction recorded in E-SD-BOOT-ENTRY-ANALYSIS-150) before staging. The reviewed card carries a matching raw overlay, but the apply is not a no-op.
3. **Selector-7 halt.** If the selector reads 7, the attempt can only halt, whatever the pair reads.
4. **No guaranteed recovery.** There is no ROM fallback, no demonstrated SD repair on this body, and a forced-attempt failure may leave the body dark. Power removal is not a promised recovery for arbitrary register effects.
5. **Failed body.** Ordinary return checks, boot with capture and playback, cannot be run as written while the body is in the recorder incident. The only interpretable outcome today is a positive marker; a dark result on this body should not trigger repeats.

## 4. Lead disposition

The documented controls are functional, but their operation at application runtime does not establish the reset-time pair value. Cardless and card-present holds did not produce a reproducible ROM SD entry; the native SD rollback trial was dark. The owner has backburnered direct pad forcing because its required board access and electrical probing are too invasive. Host-only emulator and mode-7 receiver research may continue without reopening that decision.

| Lead | State | Reopen condition |
| --- | --- | --- |
| Direct pad forcing at reset | Backburnered, not authorized | Owner explicitly reopens invasive board work; Stage A then binds both signal nets before any powered attempt |
| Documented Fn plus sub-command push | Evidence-negative for ROM entry; both controls independently known functional | A new condition that distinguishes reset sampling from another repetition of the same holds |
| Emulator and mode-7 receiver research | Host-only, active as separately prioritized | Continue with exact bytes and fail-closed device/protocol evidence; no camera implication |

**Static-inference:** if the owner later reopens Stage A and no SoC-side signal net is found, pad forcing closes. Remaining research would be the preventive resident mode-4 record design and the mode-7 receiver identity, routes 3 and 4 of [recovery-hardware-routes.md](recovery-hardware-routes.md) section 6, not more button attempts.

## 5. Named open inputs

Every unproved input this sheet refuses to fill in:

1. Pin, pad, test-point or net identity for GPIO0 bits 11:10 and any series element between the switches and the pads.
2. Whether the panel path can present `0b00` at the sample at all, and if not, which of the two pad paths, the `+0x08` write semantics, or a reset-independent source keeps the pair nonzero.
3. The electrical function and update semantics of the `0xfff89008` control word's bits 11:10.
4. The physical source of `0xfff98000` bits 2:0, its idle value's origin, and whether any card-dependent path reaches it.
5. Whether the low selector accepts a changed value and reads it back.
6. Which reset sources reach ROM reset and what they re-latch, warm-reset retention included.
7. Whether the register's written bits read back at all; the held capture reads back none of the applied 17:16 field.
8. The physical indicator identity behind the payload marker.
9. Whether the X-M1 exposes pogo or sprung service contacts and what they carry (the sibling report is a shape, not a net).

## Reproduction

The instruction-level quotes above were re-read from the pinned listing `private-data/analysis/x-m1-1.50/bootrom/bootrom.dis` and the artifact `bootrom-0x80000000-4000.bin`; the disassembly command form and artifact provenance are in [recovery-hardware-routes.md](recovery-hardware-routes.md) and the private note `private-data/analysis/x-m1-1.50/bootrom/strap-read-safety.md`. The register under test, the press record and the path decode are in [the window map](bootrom-window.md) section 1 and [the card-entry analysis](sd-boot-card-entry.md) sections 1 and 2. No build, test, formatter, camera, USB device, card reader or probe was used for this sheet.

Related: [the window map](bootrom-window.md), [SD-BOOT card entry](sd-boot-card-entry.md), [hardware routes](recovery-hardware-routes.md), [recovery status](recovery-status.md), [service manual procedures](service-manual-procedures.md).
