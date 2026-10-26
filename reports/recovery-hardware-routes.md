# Recovery doors on the X-M1 hardware

## Scope and result

**Static-inference:** the captured ROM has eight entry mechanisms, not one documented button procedure: RAM handoff, SD/FAT, raw SD on a second channel, four internal-media formats, and an unnamed config-plus-bulk receive engine. It selects one mechanism and never falls back after failure. No alternate ROM entry has been demonstrated on this body.

**Hardware-observed:** E-SD-BOOT-ENTRY-HOLD-150 records Function held with every tested body button, including the sub-command dial press, and application boot on every attempt. **Byte-read:** the computed input evaluator maps Function to GPIO0 bit 11 and sub-command push to GPIO0 bit 10. **Static-inference:** the sweep included the two controls the shipped application associates with the ROM's high pair; it was not a sweep of unrelated inputs. It establishes no usable SD entry under the tested cardless conditions, not that those nets cannot move at reset.

**Unproved:** the electrical availability of those controls during ROM sampling and the physical source of `0xfff98000` bits 2:0 remain unresolved. No test pad, resistor position, connector pin or reset-retained software selector is identified. The writable-latch claim remains withdrawn: the two fixed ROM stores preserve bits 2:0. The generic service byte writer can address this register, but an instruction that requests a store does not establish which silicon bits accept it.

**Byte-read:** mode 7 supplies the ROM's one unresolved non-SD/non-internal-media input mechanism, at `0xfff56000`, after mode selection. **Unproved:** its wire protocol, whether its source is a host or another on-board component, and any connection to the USB socket. A boot-ROM USB interface is not established; blanket absence of every serial or host-facing interface is not established either. Identifying this block is worth real work.

This is host-only research over the captured ROM and official 1.50 decode. No camera, card reader or USB device was accessed; no camera operation is authorized. `byte-read` names instructions/data, `static-inference` names deductions, `hardware-observed` names the existing reviewed body captures, `reported` names owner/manual/public statements, and `unproved` names unresolved hardware or execution claims. No sibling-model behavior supplies an X-M1 fact. E-RECOVERY-HARDWARE-ROUTES-150 records the bounded conclusions; the [window map](bootrom-window.md) remains the detailed ROM address reference and [UPDATE-01](update-validation-and-boot.md) remains the record/writer reference.

## 1. What the selector actually samples

### 1.1 The complete input and decode

**Byte-read:** every traversal of ROM reset `0x80000000` → `0x80000028` calls `0x80000260` at `0x80000130`. Before the reads, `0x80000270` writes `0x0c000c00` to GPIO `0xfff89008`. It then reads GPIO `0xfff89014` at `0x80000274`, reads `0xfff98000` at `0x80000278`, and forms `s = ((GPIO >> 10) & 3) << 3 | (selector & 7)`. It stores `s` to `0xfff98000` at `0x800002f0`.

| GPIO bits 11:10 | Low selector bits 2:0 | Mode | Evidence |
| --- | --- | --- | --- |
| any | 7 | -1, halt | byte-read; this test precedes all others |
| 0 | 0 through 6 | 1, SD/FAT | byte-read; includes low selector 0 |
| 1, 2 or 3 | 0 | 3, device format 4 | byte-read |
| 1, 2 or 3 | 1 | 4, device format 1 | byte-read |
| 1, 2 or 3 | 2 | 5, device format 2 | byte-read |
| 1, 2 or 3 | 3 | 6, device format 3 | byte-read |
| 1, 2 or 3 | 4 | 2, raw SD channel 1 | byte-read |
| 1, 2 or 3 | 5 | 7, unnamed receive engine | byte-read |
| 1, 2 or 3 | 6 | 0, RAM handoff | byte-read |

**Hardware-observed:** the maintenance capture E-STRAP-READ-150 reads `0xfff89014 = 0x00000d4e` and `0xfff98000 = 0x00000019`, giving high input 3, low input 1 and mode 4. The reset-stage port snapshot reads `0x00000d62`; its high input is also 3. **Static-inference:** normal display boot after a tested hold supports continuing through mode 4, but is not a capture proving every selector bit was numerically unchanged: all three nonzero high values select mode 4 when the low value is 1.

### 1.2 GPIO input is not a proven software latch

**Byte-read:** the port array is 16 registers at `0xfff89014 + 0x40*k`; boot-record snapshots and runtime users treat `+0x14` as a read register. Firmware writes neighboring GPIO control words, including mask-shaped values with a high-half enable mask and low-half value, and the ROM configures the two sampled pins before reading them. No recovered fixed-address or GPIO-descriptor store targets `0xfff89014`; the arbitrary-address service writer is separate. **Hardware-observed:** snapshot and live register differ in bits 5, 3 and 2, so the whole port value is not frozen at reset.

**Static-inference:** `0xfff89014` is a port input/status word; the reset sample's high bits are hardware-presented signals, not a value the ROM constructs. Image 4 also tests bit 10 at `0x006a3c34` to gate sub-command rotation. Its table-driven button evaluator supplies the missing physical-control binding in section 1.5. The neighboring control writes do not prove that output configuration can override the sampled input.

**Unproved:** package-pin numbers, intervening circuitry, pull resistors and reset-time power/enable state. The application binding identifies user controls, not a schematic net or a continuity measurement. The firmware contains no card-file or button parser before ROM selection; a service-card-dependent electrical condition would have to be supplied outside that main-CPU flow.

### 1.3 The low selector is stored, but its input bits may not be writable

**Byte-read:** the store at `0x800002f0` clears bits 31:5 and writes the derived high input into bits 4:3, but writes bits 2:0 back unchanged. Config apply `0x80000338`-`0x8000034c` reads the same register and changes only bits 17:16, preserving the low selector. Handoff writes 1 or 2 to a different register, `0xfff98008`.

**Static-inference:** runtime `0x19` agrees with a recorded combined boot value. It does not distinguish a fully writable low selector from read-only strap bits sharing an address with writable status/config fields. A successful store instruction is not a hardware readback demonstrating that a changed low bit sticks.

**Unproved:** what establishes the low selector at power-on, whether it comes from pins, fuses, a reset-loaded register or a companion, whether a changed low selector can be read back, and which reset domains replace or retain it. The ten-base table at `0x00829144` contains `0xfff98000`, but neither the full recovered decompiler export nor the aligned pointer/materialization census establishes a consumer. Section 1.6 separates those bounds from the computed GPIO reads the former census missed.

### 1.4 What would remove the button gate

**Static-inference, conditional:** if software can change low input 1 to 4 and reach ROM reset while the changed value survives, nonzero GPIO high input selects raw SD mode 2 without pressing anything. Low input 6 similarly selects RAM mode 0; low input 5 selects mode 7. Getting mode 1 instead requires GPIO high input 0 or deliberately selecting/calling the SD/FAT handler from an existing execution foothold. The cold-reset origin of the low input does not decide the warm-reset case.

**Unproved:** no mapped CPU-reset request plus retained-selector contract closes that chain. The owner’s button result therefore neither blocks nor establishes this proposed software mechanism. If the low selector is proved read-only or reset-reloaded for every accessible hardware reset, that closes selector-plus-reset, not direct software entry into an already mapped ROM handler or physical GPIO forcing.

**Static-inference:** direct A32 control transfer from an owned RAM foothold can bypass the selector altogether. Calling an SD handler or entering a purpose-built loader is a different mechanism from resetting the silicon. It requires an executable foothold, interrupt/cache/MPU/stack and peripheral-state reconciliation, and a defined return or halt. It can support a healthy-body transient experiment; by itself it cannot recover a cold-starting body with no usable application. No such camera operation is implemented or proposed here.

**Static-inference:** direct ROM-handler entry and the application S-record writer share the prerequisite of a running body but offer different capabilities. `FUN_00db8ab0` is an application-resident persistent firmware-span writer using the application's services; a ROM SD-handler entry would be a transfer to a staged payload whose execution and media access can be independent of those services after handoff. The first supplies a writer, the second supplies an execution/loading mechanism. Neither supplies a proven cold-failure entry, and neither demonstrates the other's contract. They are not two established ways to invoke the same operation.

### 1.5 Computed access and the physical controls

**Byte-read:** image 4's 36 input rows begin at `0x00828120`, stride 28. `FUN_006a0888` kind 2 reads operand `+0x0c`, computes `0xfff89000 + ((operand >> 16) & 15)*0x40`, loads `+0x14`, extracts bit `operand & 15`, and compares it with the halfword at row `+0x12`. Instructions `0x006a08c4`-`0x006a08d4` materialize `0x03ffe240`, add the bank, shift left six and load. A census requiring the final address to appear as a literal or `movw`/`movt` pair cannot see this access.

| Row | Address | Code and name | Kind / operand / asserted level | Physical binding and status |
| --- | --- | --- | --- | --- |
| 13 | `0x0082828c` | `0x0028`, `KEY_FUNCTION` | byte-read: 2 / `0x0000000b` / 0, GPIO0 bit 11 | hardware-observed: the owner's Fn act emits this code |
| 22 | `0x00828388` | `0x002b`, `KEY_CMDL_PUSH` | byte-read: 2 / `0x0000000a` / 0, GPIO0 bit 10 | hardware-observed: the sub-command dial push emits this code |
| 21 | `0x0082836c` | `0x0034`, `KEY_MOVIE_REC` | byte-read: 1 / ADC channel 9 / cache thresholds `0x37a2` and `0x37a0` | hardware-observed: the MOVIE act emits this code; static-inference: it is not the GPIO0/11 control |

**Byte-read:** the kind-2 compare makes both GPIO inputs active-low. `FUN_006a0a78` scans these rows, and `FUN_006a16bc` also evaluates them during input initialization. **Hardware-observed:** [the input map](input-map-150.md) binds the controls to their codes in the act-logged `20261009T064945Z-read-log` session, Fn at tick 17087 and sub-command push at 22342. **Static-inference:** these two normal-operation control paths can influence the same bitfields ROM samples. **Unproved:** that the direct pin levels are available before ROM sampling, or that they were both low at `0x80000274` on any swept reset; the ring records application input codes, not that earlier pin sample.

### 1.6 Register consumers and producers, including indirect forms

**Byte-read:** the following inventory combines the ROM, both records, both loaders, images 4/5/plain, the full 32 MiB recovered `filmsim/ghidra/all.c` export, direct instruction re-reads and the descriptor tables. Addresses for record 2 use its `0x10000` load base, not the older census's `0x50000` base. A producer below means a firmware store request, not proof that a hardware input changes.

| Access family | Consumer or producer | Target and bound |
| --- | --- | --- |
| ROM selection `0x80000274`, `0x80000278` | byte-read: consumes both target words | high bits 11:10 and low bits 2:0 select one mode |
| ROM `0x800002f0`, `0x80000338`/`0x8000034c` | byte-read: writes selector; later reads/modifies/writes it | first store preserves low three bits; second changes bits 17:16 only |
| ROM mode-7 poll `0x800020e0` | byte-read: reads GPIO0 input | waits for bits 11:10 both high after changing GPIO control `+0x08` |
| Record-1 `0x269c`; record-2 `0x122cc`; image-5 `0x00ec7db8` | byte-read: post-indexed port-mirror readers | sixteen `+0x14` words, stride `0x40`; port 0 reaches `0xfff89014`, stores go to RAM `0x400a0000` |
| Record-2 `0x11d8c`, `0x11dd0`, `0x11ddc` | byte-read: direct GPIO0 input reads | bits 4, 4 and 7, not the ROM high pair |
| Image-4 `0x6a30ec`, `0x6a3c34`, `0x6b8508`/`0x6b851c`, `0x6b8544`/`0x6b8558`, `0x6c2c2c`, `0x6c3390`/`0x6c3398`, `0x6c357c`, `0x6c4b20`, `0x6d5c10` | byte-read: direct GPIO0 input reads | bits 5, 10, 7, 7, 7, 4/7, 4, 4 and 1 respectively; no direct input-word store |
| Image-5 `0xec7a3c`, `0xec7e94` | byte-read: direct GPIO0 input reads | bit 2 on the stop path, not the ROM high pair |
| `FUN_006a0888`, kinds 2/3/4 | byte-read: row-driven computed input reader | `+0x14` of selected port; rows 13/22 reach target bits 11/10, shutter rows reach port-0 bits 2/3 |
| `FUN_00656ff4`, through `FUN_00741a00` | byte-read: caller-descriptor bank reader | computed `port*0x40+0x14`; can reach port 0; returns selected bits in RAM |
| `FUN_007416c8`, table `0x0081aa98`, 98 entries | byte-read: table-driven pin reader | entries 29/30 select GPIO0 bits 9/6; no entry selects GPIO0 bits 10/11 |
| `FUN_006a3b4c`, `FUN_006a406c` | byte-read: RAM-descriptor computed reads | descriptors copied from `0x007cb1e4` select port 3 bits 8/9; do not reach the target; `0x6a3c34` separately reads port-0 bit 10 |
| `FUN_00dba85c`, caller `FUN_00ecc414` | byte-read: cache-selected whole-port reader | switch accepts ports 0-15, computed `+0x14`; selection 0 reaches target and copies low/high input bytes to adjustment-cache fields |
| `FUN_00647b1c` / `FUN_00647b78`, service handler `FUN_00d803cc` functions 2/3 | byte-read: request-address byte reader/writer | register-indirect arbitrary byte address can reach either target; existing strap capture used function 2; no selector-changing write/readback is held |
| ROM config `0x80000304` optional address slots and final address/value loop `0x800006f4`-`0x80000700` | byte-read: config-driven indirect store requests | can request either address after mode selection; stock optional slots are zero and its seven pairs target neither register |
| GPIO control/output/mux families | byte-read: computed neighboring-register writes | `FUN_00656788`, `006568ec`, `00656a34`, `00656c68`, `00656d98`, `00656ec8`, `0065cbd8`, `0064bf40`, `0064c1cc`, `006a4788`, `006d1c68`, `00741604`, `00741b00`, `00741b94`, `00741c7c` write port offsets `0`, `4`, `8`, `0xc`, `0x10`, `0x18` or `0x1c`, not `+0x14` |
| Ten MMIO bases `0x00829144`-`0x00829168` | byte-read: contains GPIO/system bases; unproved consumer | no recovered source reference, aligned pointer into `[0x00829100,0x0082916c)` or paired A32 `movw`/`movt` reference found in the eight artifacts; an address formed by other arithmetic remains outside that negative bound |

**Byte-read, census correction:** `strap-scan.c`'s previous literal-load recognizer did not require base register `pc`. It could treat a non-PC `ldr` as a literal fetch. The earlier alleged GPIO copies at `0x6c9344`, `0x6c9788`, `0x6df9e0` and `0x6dddb4` actually read `0xfff6b014`, `0xfff6e014`, `0xfff65014` and `0xfff52014`; `0x6d6d04` is also not a GPIO input read. **Static-inference:** that earlier scanner's hit list is neither an exhaustive inventory nor a reliable exact count without instruction-level address reconstruction. Its omissions do not justify a no-computed-access conclusion.

**Source-inspected, scanner correction (2026-10-09):** the private scanner now recognizes only conditional-space A32 word loads with an immediate offset, `Rn == pc`, preindexing, no writeback and destination `r0`-`r14` as literal materializers. It applies the encoded add/subtract direction to `PC + 8` and accepts only aligned, complete words within the supplied file; trailing partial words are excluded. Hand-derived positive/negative literal, register-relative rejection and boundary fixtures form the 19-case smoke suite in `private-data/analysis/x-m1-1.50/bootrom/strap-scanner-smoke/SmokeFixtures.hs`. This closes the known literal-recognition defect, not a firmware inventory. The caller still requires the materialized base itself in the requested range, searches at most 20 following words and reports at most eight uses per materialization; the scan does not evaluate conditions, establish reachability or track computed/table-derived addresses and complete register state. No match remains bounded negative evidence. Earlier ledger records and census counts are not rerun or silently revised by this fix.

**Reported, host-only synthetic smoke:** the independent runtime worker ran `devenv shell -- bash private-data/analysis/x-m1-1.50/bootrom/strap-scanner-smoke/smoke.sh before` and the same command with `after`. Both compiled with `clang -std=c11 -O2 -Wall -Wextra` without compiler diagnostics. The preserved earlier scanner returned exit 1 with 14 passes and five failures: negative literal, first-word literal, register-relative load, unconditional instruction space and PC destination. The corrected scanner returned exit 0 with 19 passes and no failures. The register-relative fixture's spurious strap access disappeared, and the positive/negative literals retained the hand-derived access sites. Logs are `strap-scanner-smoke/before-runtime-result.json` and `after-runtime-result.json`. These results cover the declared fixtures, not exhaustive instruction semantics, firmware absence or hardware behavior.

**Static-inference:** none of the recovered fixed or table-selected application stores changes the target input word or the selector low field. The real computed readers establish the GPIO/control binding; the arbitrary-address service/config writers establish only that software can issue bus writes there. **Unproved:** unrecognized code, externally supplied arbitrary pointers and hardware behavior cannot be closed by this static inventory. No result here proves low-bit read-only semantics or a reset-retained writeable selector.

### 1.7 What the X70 board assumes

**Reported:** `../x70view/emulator/boot_devices.py`, `BootControls.__init__`, initializes `0xfff98000` from `boot_source=1` and GPIO0 input from `boot_gpio=0xc00`. `BootControls.write` rejects writes to GPIO input `+0x14` but accepts the system word as an ordinary stored register. `BootControls.snapshot` labels boot straps and initial register values simulation fixtures; GPIO output/mux writes do not model external pin behavior. The modeled system-word writeability is not a measurement of our low bits.

**Reported:** `../x70view/emulator/sd_boot_board.py`, `make_sd_boot_board`, drives `SD_BOOT_PINS = ((0,10),(0,11))` low through `BootControls.set_gpio_input_pin` before ROM reset; it keeps the normal low selector 1 and releases the pair at handoff. Its module description labels card identity, latency, button timing and electrical behavior fixtures, not measurements. **Byte-read:** our identical ROM's selection arithmetic accepts exactly that fixture as mode 1. **Static-inference:** a successful modeled SD boot proves the supplied input values satisfy this ROM, not that any X-M1 reset supplies them.

**Reported:** `../x70view/research/sd-boot.md`, “Open requirements”, and `../x70view/research/button-monitor/README.md`, “What it shows”, associate X70 GPIO0/10 with `KEY_CMDL_PUSH` and GPIO0/11 with `KEY_FUNCTION3`. The former records an owner report identifying the rear dial push and red movie-record button, but states that no pin-word read was saved and the confirmation method was not recorded. This is a static key-table binding plus reported physical identification, not a saved reset-time electrical measurement. **Byte-read:** our X-M1 table instead assigns GPIO0/11 to `KEY_FUNCTION`; its MOVIE control is analog. The X70 physical pair cannot replace the X-M1 pairing.

### 1.8 Physical access and the refutation

| Possible external source | Finding | Status |
| --- | --- | --- |
| User controls for GPIO0 bits 11:10 | Fn and sub-command push, active-low, through the computed row reader and this body's act/code binding | byte-read plus hardware-observed binding; static-inference for electrical influence; reset-time availability unproved |
| Test pad, resistor position or connector pin for GPIO0 bits 11:10 | No numbered pad, resistor or pin identified | unproved |
| Source for selector bits 2:0 | No control, package pin, fuse, resistor or companion-register binding identified | unproved |
| Service SD card or USB socket driving the pair | No mapped connection; ROM selects before card access, and USB jig entry is application code | byte-read for ordering; outside-main-CPU electrical influence unproved |

**Reported:** the held X-M1 service manual ZM01086-107 has disassembly, board/FFC assembly identities and procedures, not a GPIO/reset schematic or connector pinout. Section 5-2 names `BUTTON FN` M206 and `Y29971_MAIN-TOP_FPC` M223; 5-3 names `Y29971MAIN_ASSY` M343; 5-4 names `Y29971_REAR_FFC` M405; 5-5 lists fuses and a battery. These assembly names assign no strap pad or resistor. Section 3-8 prescribes the Function/dial press with a prepared SD card and no USB, but supplies no separate service-card electrical interface.

**Static-inference:** “cold reset is closed” remains valid only as “no demonstrated, usable alternative cold entry on this body.” It is not established as physical impossibility: the two requested high bits are associated with physically accessible controls, and their reset-time circuit state remains unproved. The owner's sweep is relevant negative evidence for the mapped Fn/sub-command pair, not a demonstration that no button could reach it. It bounds the tested cardless conditions, not every reset-time board condition. No invalid-application fallback exists in the decoded ROM/record chains.

**Unproved:** low-selector software writeability and warm-reset retention remain separate missing facts; no computed access found here restores the withdrawn latch claim. A schematic or equivalent target-specific electrical record could identify intervening enable logic and low-selector source. Opening the camera, touching a board, asserting a net or trying a selector/reset write is outside this assignment. No such operation is proposed.


## 2. Reset and execution-entry map

**Byte-read:** the captured block contains one reset entry and one mode-detect call. There is no reset-reason branch that decides to skip sampling. Thus the instruction-level answer is definite: any entry that actually traverses `0x80000028` reaches both reads again. Whether a particular reset source gets there, and whether the register contents were reset first, are separate hardware facts.

| Entry or event | Where the held evidence leads | Reads the ROM mode again? | Evidence and missing prerequisite |
| --- | --- | --- | --- |
| Ordinary cold power-up | mode-4 staging and record-1 vector at RAM 0 | yes if it follows ROM reset | hardware-observed working boots and straps; reset-to-ROM wiring is static-inference, not a captured reset trace |
| Warm CPU/SoC reset | no reset strobe or reset-domain map established | unproved; yes if delivered to ROM reset | unproved target/reset-domain, low-selector retention, RAM/controller retention and reset vector mapping |
| Watchdog expiry | no identified X-M1 watchdog block, armed reset path or expiry trace | unproved; yes if delivered to ROM reset | unproved watchdog existence/configuration, reset target and domains; no induced timeout is proposed |
| Software branch to `0x80000000` or `0x80000028` | same ROM initialization and dispatch | yes | byte-read control flow; accessible privileged execution and safe machine state are unproved |
| Software branch to RAM `0x00000000` | live first-record vector points to `0x1800` | no | hardware-observed vector, byte-read record; not a hardware reset and not a ROM entry |
| Second-record reset `0x10000` → `0x11800` | companion initialization, peer wait and shared id-1 entry | no | byte-read; actual companion reset/release mechanism and aliases remain unproved |
| Undefined/SWI/abort/IRQ/FIQ vectors in ROM `+0x04`..`+0x1c` | NOP fallthrough to WFI at `+0x20`, self-loop at `+0x24` | no | byte-read; exceptions are not alternate recovery dispatchers |
| Live application exceptions | rewritten low vectors enter loaders/image 4 | no on the mapped vector entries | hardware-observed targets; downstream handler-specific dispatch is outside the ROM selector |
| Readback aliases at `0x80004000`, `0x80008000`, `0x80010000`, `0x80100000` | the same captured ROM content | unproved execution; base-reset entry would sample if executable | hardware-observed reads only; read aliasing is not proof of execution aliasing or hardware reset vectors |
| USB bus reset, service opcode `0x1d`, C4 or application mode change | USB/session/application-state machinery | no on the mapped chains | byte-read private bus-reset findings `private-data/analysis/x-m1-1.50/end-setting/followup/bus-reset.md` and connect-edge findings `private-data/analysis/x-m1-1.50/end-setting/followup/connect-edge.md`; public [lifecycle limits](service-query-and-jig-reset.md#lifecycle-limits-and-session-implication) and [mode map](service-shoot-feasibility.md); none establishes CPU reset |
| Mapped orderly stop terminal | `0x00ec5804` calls retained-register/GPIO shutdown routines and spins | no | byte-read terminal; any later rail/reset event belongs to unidentified hardware |
| SRAM suspend/wake | image-5 `0xec7688` copied to `0xfff00700`, WFI, restore and return | no on its decoded wake continuation | byte-read; external reset while asleep remains a different unproved event |

**Byte-read:** stop `0x006568ec` clears 16 ports' control words and `0xfff88084` bit 0; it does not store to either selector read register. `0x00ec5804` then loops. The staged stop-side routine `0x00ec797c` copies `0x300` bytes from image 5 beginning `0x00ec7688` into SRAM `0xfff00700`-`0xfff009ff`; its source pointer is the word `0x00ec7684` at `0x00ec7f8c`, and the copy preincrements the source before the first load. The executed function uses `0xfff98200` status, saves/changes control words, executes WFI at source `0x00ec776c`, restores state and returns with `bx lr` at `0x00ec7828`. Neither selector is read in that continuation.

**Static-inference:** this is a suspend/resume door, not a discovered warm-reset door. The presence of `0xfff88088` or `0xfff8808c` programming beside WFI does not justify naming either a watchdog or CPU reset strobe. The companion retained register `0x60`, sampled into `0x400a00c0`, selects application startup behavior, not ROM mode; confusing those two domains would manufacture a software strap route.

**Unproved:** an exhaustive list of physical reset sources cannot be derived from ROM instructions alone. The missing artifact is an X-M1/FF4227 reset-domain and pin/register description, or equivalent target-specific board evidence. Searches of the full decompiler export and constant-address scans of both decoded code images found no established selector-changing CPU-reset routine; this is bounded negative evidence, not proof that the die lacks one.

## 3. The boot-ROM USB claim and the real unresolved receiver

**Reported:** the public survey's [S27 blog](https://danielc.dev/blog/2-years-of-fujihack) claims a low-power boot-ROM USB shortcut across Fujifilm bodies. That cross-model claim supplies no X-M1 controller binding. The project’s ROM dump was obtained through the loaded application’s service memory-read handler, not through a ROM USB endpoint.

**Byte-read:** before mode detect, ROM initialization programs CPU execution state/MPU, its initial flag at `0xfff88084`, SRAM initialization and the two GPIO controls. It initializes no USB endpoint, descriptor table, request parser, UART console or debug monitor. All direct calls and the two Thumb helper calls stay in the captured block; the final external code transfer is to RAM 0. Non-reset ROM exceptions halt.

**Byte-read:** modes 1/2 reach SD command, FAT or raw-block code; modes 3-6 reach the `0xfff52000` media interface; mode 0 only writes GPIO and hands off. These closures contain no request-serving host loop. The known application USB controller uses `0xfff54000`-`0xfff542fc` plus `0xfff55100`-`0xfff5588c` (private bus-reset findings `private-data/analysis/x-m1-1.50/end-setting/followup/bus-reset.md`; public lifecycle context is in [the service/reset report](service-query-and-jig-reset.md#lifecycle-limits-and-session-implication)). The ROM contains no access to that identified register map. **Static-inference:** the ordinary socket's known application USB stack is absent from the captured ROM routes. **Unproved:** that absence does not identify `0xfff56000` or exclude a different hardware boot interface.

### Mode 7 is config plus a bulk input, not just an init hang

**Byte-read:** its handler `0x80000ac0` initializes `0xfff56000` with `0x800020a8`, reads 1024 bytes through `0x80002144` into SRAM `0xfff00004`, applies the same `0x80000304` register config, takes `config+0x18 & 0x3ffff` as a count of 1024-byte transfer units, and calls `0x800021d4` to stage that count at `0x40000000`. Success reaches RAM 0 through the shared handoff. Count zero returns success without any payload transfer; there is no record checksum or model/version gate here.

**Byte-read:** initialization enables clock bit `0x2000` in `0xfff88030`, sets GPIO bits 11:10 through `0xfff89008`, waits until both input bits 11:10 read high, writes engine control `0x83100106`, waits for `engine+4 & 7 == 6`, and waits for control bit 31 to become set. The configuration transfer uses its `+0x100` data window, 256 words. The bulk transfer supplies destination at `+0x10`, 1024 at `+0x14`/`+0x1c`, total byte count at `+0x18`, zeroes `+0x20`/`+0x24`/`+0x28`, and triggers control bit 0. Status `+0x40` bit 0 completes; bits 7:1 signal failure. Its waits carry no software timeout.

**Static-inference:** unlike SD and paged flash, this closure sends no card command or media address. It is compatible with an externally supplied sequential config/payload stream, and deserves investigation as an independent loader. It is not a bound USB implementation, a UART implementation, a debug command monitor or a proven socket-facing host service. The GPIO-high requirement also matters: selection accepts any nonzero high value, but initialization requires high value 3, or it waits for the inputs to change.

**Unproved:** block identity, pinmux electrical semantics, framing, remote producer and access connector. These facts determine whether a host can feed it, a companion can feed it, or only board equipment can feed it. Naming it USB from the public claim or SPI/UART from the register shape would substitute a guess for that missing identification. There is no identified host interface preceding selection; the one unresolved receiver follows selection.

### Mode-7 receiver identification pass

**Byte-read:** mode 7 is a two-phase inbound engine, not a command service. Phase 1 sets control bit 2, requests exactly `0x400` bytes, receives them in the controller's `+0x100` window and copies 256 words into SRAM. Phase 2 clears control bit 2, writes destination `0x40000000`, writes `(config+0x18 & 0x3ffff) << 10` as the byte count and starts another receive. The largest encodable second phase is `0x0ffffc00` bytes. The handler transmits no request, address, command, descriptor or response through the block.

| Receiver fact | Recovery consequence |
| --- | --- |
| The first 1 KiB is applied through the shared register-config interpreter before phase 2 | Its bytes are executable hardware configuration, not a self-describing file header |
| `config+0x18` supplies an 18-bit count in 1 KiB units | A producer must know the transfer length before bulk input starts |
| `config+0x10` bit 1 is retained for failure cleanup | At least one config flag affects the error path, but it does not identify the transport |
| Phase 2 writes directly to `0x40000000` and success reaches the shared RAM-0 handoff | The received payload can supply a boot image without FAT, raw-sector or record parsing |
| Status `+0x40` bits 7:1 fail and bit 0 completes, with no timeout | Missing or incompatible hardware can hang before software regains control |

**Source-inspected:** upstream `fffw` names enum value 7 `BOOT_DEV56` in its X-E2 4.11 BootROM header; [the introducing commit](https://github.com/tiredboffin/fffw/commit/7f313c704dee7df98a5ea19abe3bf8113e6ead90) adds no controller or transport identity. The name records the `0xfff56000` address family, not a decoded device. Public later-generation Milbeaut Linux drivers also do not match this register block: [`milbeaut_usio.c`](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/milbeaut_usio.c) uses a compact byte/halfword UART register set at offsets `0x00` through `0x0e`, while [`sdhci-milbeaut.c`](https://github.com/torvalds/linux/blob/master/drivers/mmc/host/sdhci-milbeaut.c) wraps standard SDHCI registers with vendor registers at `0x200` and above. Those later-family sources exclude direct reuse of those two published layouts; they do not exclude an unpublished FF4227 stream/DMA controller or a different integration.

**Static-inference:** the block is not the known application USB controller because its base, widths and transfer layout differ from the decoded `0xfff54000`/`0xfff55100` USB driver. That does not prove it is non-USB: a separate ROM-facing USB engine, companion-fed stream or board interface remains possible. The configuration-window and direct-DMA phases are compatible with several slave receivers, so register shape alone cannot choose among them.

**Recovery implication:** mode selection needs low selector 5 and any nonzero GPIO0 high pair. Initialization then waits for the high pair to become 3. A high pair of 1 or 2 could therefore select the route while a pressed active-low control is held and let initialization continue when it is released; a high pair already at 3 continues immediately. The known working Fn and sub-command presses address only that high pair. They do not supply low selector 5, whose source remains unidentified. Direct pad forcing is owner-backburnered and no camera operation is proposed.

**Disposition:** there is no fixed mode-7 recovery image to deploy. The software-side content is bounded as a 1 KiB hardware-configuration block followed by a raw payload, but entry, physical endpoint, electrical protocol, framing and producer remain unknown. The existing reviewed SD recovery image targets mode 1 and is not interchangeable with this stream. The next discriminating artifact is an FF4227 register/pinmux description or a factory package/source that names `0xfff56000`/`BOOT_DEV56`; another payload build cannot resolve that hardware contract.

### Mode-7 provenance correction and offline reproduction

`E-MODE7-RECEIVER-PROVENANCE-CORRECTION-150` withdraws the unsupported DAT identity association in `E-MODE7-RECEIVER-BOUND-150`; it does not replace the historical hash. The original record remains unchanged. Its `firmware_sha256`, `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dc944b91ece0751203c5d`, already conflicted with the manifest in private research commit `f1f88e6edd5b606905f4a83a5b6437f36768b117`, which first committed that record. That contemporaneous manifest identifies the 1.50 DAT as `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, as does the current pin. Neither manifest proves which DAT, if any, the mode-7 analysis actually used. No preserved input or contemporaneous artifact binding was found for the conflicting hash; its origin remains unknown.

The ROM source has an independent, recoverable identity. `E-BOOTROM-MODE4-150` in that same private commit and the [initial public ledger](https://github.com/skeet70/latent-image/blob/75abf4831241128a2280ed7627b6f030434bd760/docs/evidence.jsonl#L100) records the 16 KiB ROM SHA-256 `4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be` and listing SHA-256 `19500aabaa1cf41dd4084c515076d873b807908af7ef50bb9ab6ddfe86ee91ab`. Both retained files match those historical hashes. The ROM also matches the capture's recorded SHA-1 `bed698ed1d91b5c29ce7c5d650156e6100c32cce`; the retained `E-BOOTROM-PROBE-150` journal and first/last windows match their recorded SHA-256 values. This binds the ROM bytes and historical listing, not the conflicting DAT field, a new acquisition or a different camera.

The private research history is retained in `private-data/provenance/fuji-af-retirement/fuji-af-all.bundle`, not in the public Git history. The public snapshot preserves the ROM identities and the conflicting record, but does not distribute the ROM, capture journal, listing or bundle. An independently supplied official DAT can reproduce the [public firmware inspection and extraction procedure](../recovery/README.md#firmware-triage-for-other-failures); it cannot supply the acquired mask ROM. Reproducing the ROM analysis requires an independently supplied ROM matching the identity above. If it differs, treat it as a new input rather than reproducing this finding.

These are current host-only reproduction commands, not a reconstruction of the original invocation. Supply the ROM independently, retain the input hash, and use the public development shell's LLVM tools instead of a historical private `.elf` wrapper:

```sh
ROM=private-data/independent/bootrom-0x80000000-4000.bin
OUT=private-data/analysis/mode7-independent
sha256sum "$ROM"
stat --format=%s "$ROM"
mkdir -p "$OUT"
devenv shell -- llvm-objcopy -I binary -O elf32-littlearm \
  --rename-section .data=.text,alloc,load,code,contents "$ROM" "$OUT/bootrom.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi \
  --adjust-vma=0x80000000 --start-address=0xac0 --stop-address=0xb50 \
  "$OUT/bootrom.elf"
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi \
  --adjust-vma=0x80000000 --start-address=0x20a8 --stop-address=0x22cc \
  "$OUT/bootrom.elf"
```

Require the ROM hash above and a size of 16384 bytes before comparing the handler at `0x80000ac0` and receiver helpers at `0x800020a8`, `0x80002144` and `0x800021d4`. LLVM's range arguments here are file offsets before VMA adjustment. Current LLVM output need not reproduce the historical listing's formatting or hash. The historical listing is recoverable privately by its recorded hash, and `E-BOOTROM-MODE4-150` records its LLVM wrapper recipe. The mode-7 record supplies no invocation or tool revision. S32 supplies no Linux commit identities for its 2026-10-12 `master` reads.

The public development shell's LLVM 21.1.8 successfully wraps the matching 16384-byte ROM and disassembles both documented ranges with these exact triple, VMA and offset arguments (`E-MODE7-RECEIVER-PUBLIC-LLVM-150`). The output starts at `0x80000ac0` and `0x800020a8`, contains the helpers at `0x80002144` and `0x800021d4`, and resolves the handler's calls at `0x80000ac4`, `0x80000acc` and `0x80000b10` to those three helpers. This is host disassembly, not firmware execution or recovery verification.

For the external-source comparison, [S32's addendum](../spec/09-sources.md#s32) supplies an immutable `fffw` header and versioned Linux references checked for the same enum/register facts. Those Linux links are current reproduction references, not recovered historical identities. The original private scanner and Ghidra export are not public dependencies for these bounded ROM slices; their broader application/reset-caller search is not reproduced by this procedure. The DAT association, original Linux revisions, endpoint, framing, producer and low-selector control remain unresolved. No recovery acceptance follows from this correction.

## 4. Every mode assessed as an entry

| Mode | What it offers | What could supply a recovery payload | What the project lacks | Assessment |
| --- | --- | --- | --- | --- |
| 0 | byte-read: GPIO config and jump to RAM 0, no staging or RAM clear there | static-inference: retained executable RAM after a reset that preserves it, software-preloaded RAM before direct ROM entry, or a board/debug agent that supplies RAM | unproved mode entry, RAM/alias/controller retention and initialized memory, external RAM load access | useful RAM door, not a cold-boot loader |
| 1 | byte-read: SD channel 0, FAT12/16/32, `ffsys.bin` and `ffboot.bin`, unvalidated A32 handoff | static-inference: host-buildable SD config and project-owned diagnostic | unproved mode entry and electrical channel-to-slot binding; bounded output/write-set audit and hardware exercise | strongest decoded removable-media candidate |
| 2 | byte-read: raw SD channel 1, config blocks 0/1, payload from block 2 | static-inference: host-built raw card or externally driven channel-1 media | unproved low-selector write/retention, physical channel-1 wiring, config execution and output | best software-selector candidate if that latch contract closes |
| 3 | byte-read: same internal-media engine, argument 0 sets device select 4, separate byte/offset-style addressing and count shift 14 | unproved: compatible attached media populated with the marker/config/record | unproved format/device identity, board presence, removable/writable attachment and entry | real format door; no identified usable device |
| 4 | hardware-observed ordinary route; byte-read: device select 1, 2 KiB units, 128 KiB groups and stock boot-record staging | static-inference: a deliberately installed application-independent record, or externally repaired existing records/media | no installed recovery record, no independent execution/output/writer demonstration, no external media programmer/layout binding | nearest preventive resident route; no current fallback |
| 5 | byte-read: device select 2, 4 KiB units, 512 KiB groups | unproved: compatible populated media on the same engine | unproved device identity/presence/removability and whether this is another chip or a format choice on the installed chip | lower priority than identifying the existing medium |
| 6 | byte-read: device select 3, 8 KiB units, 2 MiB groups | unproved: compatible populated media on the same engine | same physical questions as mode 5 | lower priority, not a proven second bank |
| 7 | byte-read: unnamed `0xfff56000` receiver, 1024-byte config then bulk staging | static-inference: a producer feeding its data stream | unproved producer/protocol/connector and mode entry | worth block-identity work, not dismissed as a hang |
| -1 | byte-read: low selector 7 disables the initial flag and halts | none in the captured control flow | nothing here supplies a loader | closed as a ROM software door |

### Mode 0's RAM contents and reachability

**Byte-read:** common reset zeroes ROM SRAM `0xfff00004`-`0xfff00647`; it does not zero low RAM. Mode 0 runs no DRAM config apply, stages no bytes and sets `0xfff98008 = 1` rather than the other modes' 2. It then jumps to `0x00000000`.

**Hardware-observed:** after an ordinary mode-4 boot, RAM zero holds first-record vectors with reset target `0x1800`; the exception pointers have been changed to loaders/image 4. **Static-inference, conditional:** a warm event preserving that RAM and its mapping could make mode 0 run the existing first record again. That record still needs valid internal media, so keeping the stock RAM image is not a repair for a damaged application by itself. A preloaded replacement at RAM zero could instead run a diagnostic independent of application media.

**Unproved:** cold RAM content and whether DRAM is accessible before any config apply on this board. No evidence says it is zero, that it contains a diagnostic, or that its power survives a specific reset. Therefore mode 0 on cold power-up is arbitrary/unavailable memory execution, not a justified recovery test. A prepared warm/direct-entry case needs the payload, RAM/controller retention, entry and reporting contract; these are hardware and execution prerequisites, not service-package dependencies.

### Modes 3, 5 and 6 are controller formats, not proven extra storage chips

**Byte-read:** the selects 4/2/3 and their distinct address/geometry logic exist. **Static-inference:** the selects distinguish controller device classes or formats; their numbers do not prove three extra connected devices, independent chip selects, alternate firmware banks or removable sockets. The established medium is mode 4's select 1, whose application updater is capable of writing it. The other selects have no body-observed medium or writer binding.

**Unproved:** EEPROM, NAND, eMMC or another technology name for mode 3, and the actual physical attachments for modes 5/6. Their page geometries support page-flash interpretations, but do not identify the board's chips. No evidence says any is removable or independently writable by the owner. The owner's SD slot is a removable/writable medium; that fact cannot be reassigned to these controller selects.

## 5. The two boot records do not supply a recovery fallback

**Byte-read:** ordinary ROM mode 4 stages media `[0x40000,0x80000)` into low RAM, including record 1 at 0 and record 2 at `0x10000`, and jumps to record 1's vector. No ROM checksum authenticates those record bytes. A skipped accepted page can compact the staged load without returning failure ([window map](bootrom-window.md) section 9).

**Byte-read:** record 1's `0x1e4c` initializes media, scans/rebuilds bad-block mapping on the primary path, checks the trailer's `0xaaaa` halfword through `0x1cc0`, then loads segment ids 0/1 and image 4 through `0x1d08`/`0x2250`. Storage initialization, bad-block init, trailer failure and returned loader failure call `0x1de8` then self-loop. Hardware decompressor `0x3d08` self-loops on `0xfff43088 & 0xf00` error and waits without a finite timeout for completion. Missing id 4 loops; missing ids 0/1 are dereferenced without a null check. None selects SD, mode 7, the other record as a spare, or a USB service. The ROM side matches: the dispatch runs one handler and halts on any nonzero return (`0x800001f8`-`0x800001fc`), and no validation pass exists before the handoff, so there is no application-validity fallback at the selection layer either ([claim test](sd-boot-claim-test.md), E-SDBOOT-FALLBACK-150).

**Static-inference:** 'application fails to validate' needs this narrower reading: the boot path is not the updater's full-container checksum validation. The traced boot checks a completion marker, segment presence and transfer/decompressor conditions. Arbitrary damaged code may pass those gates and fault or run; a bad image need not produce the mapped validation halt. The trailer helper's delivered transfer unit remains unproved in UPDATE-01, so the claimed trailer failure is conditional on the compared bytes being the trailer's first 32 bytes.

**Byte-read:** record 2 initializes companion peripherals, reads a small checksum/marker-bearing companion record into `0x400a00d0`, sets/clears its validity flag, and continues rather than selecting an alternate application on that check's failure. Some GPIO/peripheral predicates likewise join common continuation. It waits on peer sentinel `0x40020000 == 0x22b`, then looks up shared segment id 1, dereferences it without a null check, and publishes the entry through `0x14c80` before its `mov pc,r3` handoff. Its exception literals `0x3fff00xx` have no established ROM alias mapping. It has no traced application marker/checksum/decompression gate and no alternate media-selection path.

**Static-inference:** reciprocal synchronization supports companion startup, not redundant recovery banks. A first-record halt can strand the peer in its wait; the second record does not rescue it. The statements 'both records halt on application validation failure' and 'the second is a backup loader' would both misstate the bytes: the first has terminal gate failures, while the second shares the first's images and has peripheral-failure continuations and waits.

### Reachable or influenceable from outside

| Input or control | What is influenceable | Recovery value and bound |
| --- | --- | --- |
| Internal-media record/config/trailer/segments | byte-read: ordinary updater and application service S-record writer program the firmware span | static-inference: can install a resident first-record recovery branch while the body works; no host command reaches those writers after a cold application failure |
| Companion registers/GPIO | byte-read: record 2 reads companion register `0x60`, retained header bytes and physical predicates | unproved board-side influence; no input mapped to selecting an independent loader |
| Peer cells/entry globals in RAM | byte-read: both records read/write synchronization and entry cells | static-inference: an existing RAM/debug foothold can change them; not an external command parser and not available on a proven bricked-body path |
| SD card or USB plugged into a failing ordinary boot | byte-read: no corresponding fallback consumer in the traced record chains | closed as an automatic rescue in those chains; a different ROM mode or preinstalled resident branch is required |
| Board debug access or an external media programmer | unproved target access could replace records/media or supply RAM | theoretical external recovery; pinout, enablement, voltage, geometry/ECC/OOB and backups missing |

**Static-inference:** a resident recovery branch before record 1's trailer/application checks is technically distinguishable from the current stock behavior. It could check an independently mapped physical request or try a bounded SD diagnostic before the stock path, using ROM or project-owned loaders. It would not depend on changing the power-up straps. Installing it modifies boot-critical persistent bytes and is a preventive design, not a recovery operation that can be sent after the application is already gone. The stock whole-span update erases/programs the record area too; interrupted record programming can destroy that new branch. A resident route does not cover its own corruption unless an independent board/ROM path is established.

## 6. Ranked routes and missing prerequisites

Rank is research value toward a usable route, not an earned recovery level. Every route remains **unproved** end to end; R1 remains accepted and no result here accepts R2, R3 or R4.

| Rank | Route | Evidence-backed door | What we lack | Worth real work? |
| --- | --- | --- | --- | --- |
| 1 | Reset-time selection of SD/FAT mode 1 | byte-read independent loader and active-low Fn/sub-command GPIO bindings; only high pair must be zero while low input is not 7 | electrical availability of those inputs before application initialization; no usable entry in the tested cardless sweep | host-only circuit/document investigation; board opening or net assertion is outside this assignment |
| 2 | Software low-selector change plus retained warm reset into raw SD mode 2, or RAM mode 0 | byte-read low-value decode and common reset resampling | proof low bits accept changed writes, reset strobe/vector/domain and retention, channel-1/DRAM wiring, executable foothold | yes as a bounded contract investigation; cannot be reported as available or impossible today |
| 3 | Resident recovery entry in mode-4 record 1 | hardware-observed ordinary entry, byte-read unchecksummed record staging and early gate position | designed bounded early loader/report/writer, instruction-state audit and independent persistent-byte review; installation risk and self-corruption coverage | yes as a preventive alternative when early SD selection is unavailable; different risk from external recovery |
| 4 | Mode-7 config/bulk receiver | byte-read complete config-plus-stage-to-RAM path | controller/protocol/pin/producer identity, entry and host/electrical access | yes: highest-value unidentified ROM block; no USB command guessing |
| 5 | Board debug or external programming of established internal medium | static-inference can bypass firmware selection and broken application if the hardware exposes access | X-M1 pinout/debug enablement or chip access, voltages, storage geometry/spare/ECC/remap and durable raw backup/restore | named possible invasive route only; no board work is proposed |
| 6 | Warm/direct RAM entry and direct calls into ROM loaders from a healthy-body execution foothold | byte-read mode-0 handoff and fixed-address loaders | owned executable trigger, machine-state/DRAM contract, transient payload/output | useful for transient demonstration and offline closure; not standalone cold-failure recovery |
| 7 | Modes 3/5/6 with alternate media formats | byte-read selects and loader geometries | physical device presence/identity, attachment, format and writable/removable source | retain, but defer until controller/device identity supplies a concrete attachment |
| 8 | Application S-record writer, ordinary updater, jig/script channels | byte-read write routes while the application exists | working application, and per-operation review/authorization | useful installation tools, not recovery for invalid application |

**Static-inference:** there are three independent ways the lane could stop depending on a button: a board-selectable source, a software-retained selector/reset contract, or a resident mode-4 early loader. The first two need hardware facts; the third needs a deliberately installed design and accepts boot-record risk. No lack of vendor files closes any of them.

**Unproved:** the immediate discriminating prerequisites are (1) reset-time electrical availability of the mapped Fn/sub-command inputs, an X-M1 reset-domain map and `0xfff98000` low-bit semantics, (2) the identity of `0xfff56000`, and (3) whether an owner-acceptable resident record design has independent enough corruption coverage to justify its risk. Host models can specify retained-reset and stream behavior; they cannot discover the board wiring or which reset clears a latch. No board opening, net assertion, selector store, reset request or camera execution is proposed.

### Named open contracts

- **Unproved, SELECTOR-WRITE-01:** determine whether `0xfff98000` bits 2:0 accept a changed software value and read it back, independently of its writable neighboring fields. The ROM stores only the unchanged input bits; neither mode 1's GPIO pair nor mode 2's low selector is shown software-writable.
- **Unproved, RESET-RETENTION-01:** identify a target-specific reset source, its vector target and the domains it resets, then determine whether it reaches `0x80000028` while retaining the changed selector and any required RAM/controller state. The decoded suspend continuation returns to its caller and closes neither this contract nor the watchdog/companion alternatives.
- **Unproved, ROM-DIRECT-ENTRY-01:** establish an owned executable foothold and the machine-state contract for calling a ROM loader without hardware reset. This is distinct from the S-record writer and shares its healthy-body prerequisite, not its persistence capability.

## 7. External-source discriminators

**Source-inspected, E-RECOVERY-SOURCE-DISCRIMINATORS-150:** the retained X-M1 manual is `~/Documents/fujifilm-x-m1-service-manual.pdf`, matching S15's SHA-256. Sections 1-3 and 3-4-4 name [the Global CS Support host](https://cs-support-eid.fujifilm.co.jp/), not the older Denhin host. Section 3-4-4, printed pages 3-9–3-10, instructs copying all three files from the extracted ZJ02217 folder to a FAT32 card: `ffboot.bin`, `ffsys.bin`, `ffuser.bin`. The X-Pro1's split between its SD-BOOT and adjustment packages must not be imported into this X-M1 archive-content claim. The X-M1 adjustment package ZJ02216 remains a separate acquisition lead for `LX30ADSC***.bin`; equivalence to the packaged `ffuser.bin` is unproved.

**Source-inspected:** the X-M1 host timed out in both reader and direct HTTPS checks; [Wayback's host inventory](https://web.archive.org/cdx/search/cdx?url=cs-support-eid.fujifilm.co.jp/*&output=json&collapse=urlkey) returned no rows. [AlienVault OTX passive DNS](https://otx.alienvault.com/api/v1/indicators/hostname/cs-support-eid.fujifilm.co.jp/passive_dns) records only Microsoft Azure addresses `104.41.190.11` in 2019 and `20.46.182.38` in 2021; a direct TLS connection to the historical address also timed out. [URLScan](https://urlscan.io/api/v1/search/?q=domain:cs-support-eid.fujifilm.co.jp&size=100) and OTX's URL inventory have no captures. [Sur.ly's retained metadata](https://sur.ly/i/cs-support-eid.fujifilm.co.jp/) names “GCS(Global CS Support) System” and “Redmine”, not downloadable package contents. Exact-package Internet Archive metadata searches and the inspected neighboring repositories/history yielded no authentic X-M1 factory files. Common Crawl queries returned HTTP 504 and supply no negative archive-coverage evidence.

**Source-inspected, acquisition leads:** the reachable [Monitor repair forum](https://forum-monitor.net.ru/threads/400451/) has a 2011 S3 Pro thread where `Fprime` offered approximately 40 MB of model-related files through a hidden link. Its public Fujifilm and camera-software catalogs supplied no requested X-M1 package. The collector is a sibling-software lead only. [SMPCShop's X-M1 listing](https://smpcshop.com/FujiFilm-X-M1-Service-Manual.html) declares a PDF document packed in ZIP, matching its manual-reseller catalog; it is not a qualified factory-package lead. No purchase, registration or message was made.

**Source-inspected, excluded substitutions:** the [historical fujihack Makefile](https://github.com/fujihack/fujihack/blob/c4c969728ae56ffb8b24a3801615121116be2348/etc/boot/Makefile) compiles for Cortex-A5 and copies the same generated `hack.bin` to all three factory filenames; these are not recovered vendor files. X70's system-bank inventory includes functions accessing `0xfff98500/504`, not selector offset zero, and its emulator's `RESET` name at `0xfffb2000` supplies no silicon reset contract. [Fujitsu patent US8769255B2](https://patents.google.com/patent/US8769255B2/en) describes DMA-capable stream firmware loading through a parallel interface, but gives no established correspondence to `0xfff56000`. [Stubby](https://github.com/hypoxic/stubby-1.60.1) targets other named Milbeaut parts and supplies no FF4227 contract. All three target discriminators remain open: factory loader contents, changed selector plus retained reset, and receiver endpoint/pinmux/framing/producer. No new camera experiment follows from these sources.

### 7.1 Factory-package acquisition coverage

**Source-inspected:** the complete public [2016 Fujifilm ArchiveBot CDX manifest](https://archive.org/download/archiveteam_archivebot_go_falconk_fujifilm-dsc_com_20160730/archiveteam_archivebot_go_falconk_fujifilm-dsc_com_20160730.cdx.gz) has 10,433 capture rows. A decompressed full-manifest search found no ZJ02217/ZJ02216, LX30ADSC, factory boot filenames, HyperFFW, Denhin or Global CS Support reference. This bounds captured URLs, not uncrawled links or page bodies. The inspected DS-7, Digital Camera Manuals Series 2, S8200/S8500 and AX600 optical-disc member listings contain consumer software/manuals and no target filenames. Nested installers were not exhaustively unpacked. The retained service PDFs have no embedded attachments, and the X-M1 PDF has no URL annotations supplying a hidden package download.

**Source-inspected:** all 15 SMPCShop Fuji category pages were inspected, covering 290 advertised product titles, without a target-kit listing. The X-M1 product is a 118-page PDF packed in ZIP. Older product descriptions mention adjustment software or firmware while declaring PDF format, which does not establish binary inclusion. The site is excluded as a ZJ02217 acquisition lead absent an explicit original-package listing.

**Source-inspected:** [VLab's public collection](https://vlab.su/viewtopic.php?t=660) names A340, A350 and A360/A370 reflashing/calibration software, not X-M1; its attachments and collector profile require authenticated access. Sony/Casio repair-tool archive collector inventories supplied no named Fuji kit. Internet Archive metadata searches covering the target package numbers, target member names, `LX30ADSC`, “SD BOOT Soft” and the intersection of Fujifilm/FinePix with service/repair/adjustment/factory/jig returned no camera-service target; metadata does not exhaustively index archive members. Sourcegraph and Hugging Face repository/dataset searches also returned no target reference. The recovered Fujifilm USA ePartners manifest leads to consumer driver downloads, not the X-M1 manual's Global CS Support server. No authentic target archive or explicit target-kit availability claim was obtained. The qualified inquiry requests the original complete `ZJ02217-***.zip`, version/provenance and a member listing showing all three vendor files; ZJ02216 and another manual are not substitutes. No purchase or outbound inquiry occurred.

### 7.2 Raw-mode entry source investigation

**Source-inspected and byte-read, E-RAW-ENTRY-SOURCES-150, E-SD1-WLAN-CONTEXT-150:** this host-only investigation obtained target board identities and a compiled SD1 device context, not a deployable raw-entry procedure. Mode 2 still requires `0xfff98000[2:0] = 4`, a nonzero GPIO high pair and a block-media source on SD1. Existing raw-image approval does not establish those physical prerequisites.

| Source | Obtained fact | Boundary |
| --- | --- | --- |
| X-M1 service manual ZM01086-107, printed 5-5 / PDF p115 | MAIN PWB photograph directly identifies FF4227 and `Y29971_MAIN / XB2624-0101` | No SoC ball map, selector net, reset domain or connector pin functions |
| [Photo-parts X-M1 board album](https://www.photo-parts.com.ua/parts/imgbrowse.php?folder=IMG_STORE/Fujifilm%20X-M1%20PCB) | Both board faces; FF4227 and `Y29971-MAIN / CB2427-0100` are readable | A different revision from the manual photograph; visible traces do not bind hidden BGA nets or test-pad functions |
| X-M1 1.50 plain image at `0x01100000` | WLAN initialization/probe reaches an SDIO driver whose effective MMIO base is SD1, `0xfff51000`, using CMD5/52/53 | Static software reachability, not observed execution or fitted-device wiring |
| [Official X-M1 owner manual](https://dl.fujifilm-x.com/support/manual/x/fujifilm_xm1_manual_en.pdf), printed vii; [original FCC internal photograph](https://fccid.io/m/43880202be95f11353e1170a2f17473a3fc94662408d63a1e794595a8c80cf04.pdf); [MediaTek RT5370 specification](https://www.mediatek.com/products/broadband-wifi/rt5370) | X-M1 names FCC ID `W2Z-02000002`; the original module photograph identifies RT5370, documented as USB 2.0 | Regulatory association does not prove the fitted service assembly's identity or interface |
| [Upstream Route 0xff80](https://github.com/tiredboffin/fffw/wiki/Route-0xff80#booting-without-permission) and retained public wiki history | Author describes an X-E2 bare-metal SD/FAT proof of concept; the 214-commit mirror has no boot-named page in tracked filename history | No published physical entry sequence, raw selector-4 method or X-M1 electrical contract |

**Byte-read:** the SD1 binding is reconstructed from instructions, not a matching literal. At `0x012919f0/0x012919f4`, `mvn r3,#0x000ae000; str r0,[r3,#-0xfff]` accesses `0xfff51000`. Argument-halfword helpers at `0x01291a44` and `0x01291a6c` use complemented base plus register offsets to access `0xfff51008/0xfff5100a`; `0x01291b18/0x01291b1c` publishes the command at `0xfff5100c`. CMD5 builder `0x01290f30` produces `0x05020000`; CMD52/53 builders `0x01291264/0x012912c0` produce the corresponding `0x341a…/0x353a…` words.

**Source-inspected:** `WLAN_Start` context at `0x013e40cc` calls `0x013e215c`, whose case 0 calls host/card initialization at `0x0128ec78/0x0128eec4`, with `lanInit_sdioInit_Error` and `lanInit_sdioInitCard_Error` failures. Case 1 probes through `0x012ad0c4 -> 0x012af3ac -> 0x012d88d0`, where CMD52 accesses configure the SDIO function and 512-byte transfer size. Register and transfer clients at `0x012d8e94` through `0x012d93c4` pack SDIO register addresses and CMD52/53 requests, not LBAs. Logical instance 0 in this API is not physical SD controller 0.

**Unproved, physical conflict:** the compiled SDIO WLAN path and the certified USB-module source chain do not resolve to one authenticated fitted topology. The target service manual separately names `DWM-W094` at M320 and `FFZ10167-100 / Y29971_WLAN_FPC` at M319, whereas the [original FCC OEM manual](https://fccid.io/m/7f2f4944c319807b592851081de83d9eca1c7e779c275f1cd9fb00faa2f299ab.pdf) names `FZ09913-200`. No obtained document explicitly equates those service and regulatory module names. Do not claim SD1 is the fitted WLAN module, that it is not the removable slot, or that a slot card can supply raw mode 2. Generic retained code, another device and mux/share circuitry remain hypotheses.

**Source-inspected:** the [original signed FCC confidentiality request](https://fccid.io/m/4636d6a04949f0661e9c25276f22d1de9cb2b5f5bf44f5af88c8ee6c3cadec6c.pdf) names withheld Schematics, Block Diagram and Theory of Operation. These identify missing original module documents, not an obtainable X-M1 mainboard schematic. Exact board/module searches supplied no authenticated net or pin document; search fallbacks make this a bounded acquisition result, not an exhaustive absence claim. The FF4227 catalog profile contains no datasheet.

**Byte-read/source-inspected, selector follow-through:** no new changed-low-bit readback or retained-reset-to-ROM chain was recovered. The `0xfff98008/0x7ff98008` values in `0x012905e4` are response masks, not dereferenced reset registers. Upstream `fftlib.py`'s `1d -- reset` comment has no reset implementation; `ffjlib.py`'s download-status-reset tag is not a CPU reset contract. The unexplained MMIO-base table at `0x00829144` still has no established consumer. These observations do not exclude unrecognized computed accesses, companion logic or a card-dependent external path.

**Next source requests, SD-BOOT-RAW-ENTRY-SOURCE-01:**

1. Ask the existing upstream contact for the exact X-E2 proof-of-concept entry mechanism: cold power-on versus injected/direct ROM branch, FAT mode 1 versus raw mode 2, reset-time GPIO handling, any observed selector 4, and the source of FF4227 boot-pin/reset-register information. Request the entry mechanism, not the abandoned recovery tool; X-E2 details remain sibling evidence until target-bound.
2. Acquire `Y29971_MAIN` circuit/net documentation for `CB2427-0100` or `XB2624-0101`, with FF4227 boot-select balls, reset semantics, SD slot/WLAN connector nets and any mux. The service assembly identifier is `FCB2427-A100`.
3. Acquire a `DWM-W094` electrical/interface document or an authenticated target module photograph to reconcile it with `FZ09913-200 / W2Z-02000002`. An FPC or assembly photograph without pin functions does not close routing.

**Reported, host verification:** original plain-image bytes were wrapped with `devenv shell -- llvm-objcopy -I binary -O elf32-littlearm --rename-section .data=.text,alloc,load,code,contents` and freshly disassembled with `llvm-objdump --adjust-vma=0x01100000` over file offsets `0x1919e0..0x191b28`, `0x190e58..0x190f88`, `0x191264..0x191478` and `0x2e2180..0x2e21c0`. These reproduced the effective-address operations, command builders and initialization calls. Original board JPEGs and the FCC photo PDF are retained under ignored `private-data/sources/xm1-entry-source/`; the manual and FCC pages were rendered and inspected locally. No camera/card-reader/USB/board operation, external inquiry, purchase or account creation occurred. No entry or recovery level advanced.

## Reproduction and contract check

**Byte-read:** fresh selected disassemblies used the existing `.elf` wrappers with `devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=<base> --start-address=<offset> --stop-address=<offset>`. ROM ranges were `0x0`-`0x354`, `0xac0`-`0xb50` and `0x20a8`-`0x22cc` at base `0x80000000`; image-5 SRAM stop source was offsets `0x2d5c88`-`0x2d5e2c` at base `0x00bf1a00`. LLVM filters these wrappers before VMA adjustment, so these range arguments are file offsets, not the printed virtual addresses.

**Byte-read:** additional host research used the existing `bootrom/strap-scan.c` constant-address scanner on both decoded images for `0xfff88000`-`0xfff88fff`, `0xfff98000`-`0xfff98fff` and `0xfff56000`-`0xfff56fff`, the full Ghidra `filmsim/ghidra/all.c` export sliced for the search tool's size cap, existing first/second-record listings, and an aligned A32-branch/literal/movw-movt candidate census across both code images, both loaders, both records and plain image. The census supplied no established reset-to-ROM caller; numeric values in the ROM address range alone are not code pointers. Scanner blind spots and register-indirect calls prevent an exhaustive absence claim.

**Byte-read, computed-access follow-through:** the full recovered export was searched without the search tool's 4 MiB cap for direct register names, scaled GPIO constant `0x03ffe240`, selector-equivalent constants and MMIO-table references. The row and table bytes were read from the pinned image-4 artifact, including `xxd -g 4 -s 0x20bd20 -l 1008 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin` and the ten-base table at file offset `0x20cd44`. Existing `lz/id4.dis` instructions at `0x6a0888`-`0x6a09c0`, `0x656ff4`-`0x6570e0` and `0x7416c8`-`0x741734` bind the computed readers. The eight artifacts were scanned for aligned pointer words in `[0x00829100,0x0082916c)` and A32 `movw`/`movt` materializations into that interval, with no hit. That finite reference check does not exclude every other way to compute a pointer.

**Reported:** X70 reference statements were read from the cited private paths and function names; no X70 file, code or artifact was copied and its emulator was not run. **Byte-read:** only existing host bytes were inspected. No build, test, formatter, camera access, USB-device access or firmware write was run for E-SD-STRAP-COMPUTED-150 or E-SD-STRAP-PHYSICAL-150; the older contract-check result below belongs to E-RECOVERY-HARDWARE-ROUTES-150.

**Reported, host-check result:** after appending `E-RECOVERY-HARDWARE-ROUTES-150`, `devenv shell -- bazel test //tests:research-contracts --test_output=errors` passed: 1 of 1 test, 26.0 seconds. This is a repository research-contract check, not hardware, reset, USB or recovery verification.
