# The X70 emulator as a reference: terms, distance, SoC cross-check, revised recommendation

This assessment compares a privately accessible X70 1.10 emulator checkout with the X-M1 implementation: architecture, shared ROM behavior, device-model transfer and bounded target-addition experiments. Inspection occurred on 2026-10-09; permission and fixture disposition were updated from the owner's report on 2026-10-14. Primary credit for the inherited emulator belongs to jitl (justjake) and [x70view](https://github.com/justjake/x70view). Statements about that project are X70-reported or source-inspected; conclusions from X-M1 bytes are byte-read or static-inference. Sibling-derived interpretations remain unproved on X-M1 until target decoding or capture binds them. The assessment itself copied no source into the repository and authorizes no camera operation. [The licensing review](../LICENSING.md) distinguishes the reported redistribution grant from remaining downstream terms and artifact obligations.

## 1. What the material permits

### 1.1 What the checkout states

The inspected checkout has no root `LICENSE`, `COPYING`, `NOTICE` or SPDX file and no emulator/native source license header. `research/af-scenes/LICENSES.md` lists third-party scene and lens-data terms. `native/README.md` reports dependency terms, including a “BSD bindings” label; that note is an upstream assertion, not a verified classification of the retained files. [The licensing review](../LICENSING.md) owns component-by-component terms and release decisions; absence of a checkout license does not negate the separately reported redistribution permission.

`AGENTS.md` describes a private, single-developer repository that includes the supplied X70 firmware and camera RAM captures, and asks that upstream licences and attribution be kept. `README.md` names the private GitHub repository that backs the project up and gives its restore procedure. Both are working rules for that checkout and a backup statement; neither grants rights to anyone else, and the author's grant, which is outside the checkout's files, is recorded in section 1.2.

The checkout also holds the X70 1.10 `FPUPDATE.DAT` and per-unit RAM captures. Those are Fujifilm's firmware and one camera's state, and the author cannot grant them; they are not inputs to our work.

### 1.2 The author's grant

The owner asked the author the questions this assessment raised and reports his answers of 2026-10-09:

- The author gave the owner a personal grant to use the project as he wishes. The owner's stated understanding is that work derived from it may be published. The grant is informal and undated in writing; that is its form, and no scope limitation was stated.
- Whether the emulator can take another body's DAT by adding a manifest and model instances is unknown to the author. That makes it an experiment on our side rather than a question (section 5.6).
- A port belongs on our side rather than contributed into his repository, which he describes as already a mess.
- The measured-versus-inferred status of the media geometry is asked and awaiting a response; the boot-ROM capture procedure question has a reported answer (section 1.4).

The initial 2026-10-09 report is preserved as historical evidence `E-X70-GRANT-150`. On 2026-10-14 the owner reported: “jitl just gave explicit permission for redistribution as long as he's credited.” This is the current credited redistribution grant for jitl (justjake)'s x70view work, recorded as `E-X70-REDISTRIBUTION-PERMISSION-150`. The report supplies no author-provided SPDX license, sublicensing authority or downstream modification terms. Do not describe the inherited implementation as AGPL on the strength of the project's original-code license.

The reported permission does not grant rights to Fujifilm's X70 1.10 `FPUPDATE.DAT`, per-unit RAM captures or other proprietary inputs. Vendored components retain their own terms, but this report's upstream dependency note does not settle the license of each retained binding, header or source file. [The licensing review](../LICENSING.md) owns the actual component classification and remaining artifact obligations.

On 2026-10-14 the owner chose to retain the exact sixteen-byte remembered MCU packet in `emulation/xm1-upstream/emulator/captured_mcu_profile.py` publicly as a scoped observed fixture (`E-MCU-OBSERVED-FIXTURE-PUBLIC-150`). This disposition does not label the packet synthetic or authorize redistribution of other firmware, dumps or camera state.

Public redistribution of the author's work is expressly reported as permitted with credit. Preserve that credit and upstream notices, and assess downstream terms and dependency compatibility for the actual release artifact without reinstating an absence-of-redistribution-permission blocker. The pinned public GitHub tree returned 404 in the publication audit; this limits public reproducibility of the source inspection, not the reported permission.

This section is a reading of the facts as reported, not legal advice.

### 1.3 What the grant changes in our practice

Reading for method and cross-checking target conclusions remain distinct from redistributing implementations:

- Device models and board structure are useful research and credited redistribution inputs for register semantics, fixture straps, release rules, fail-closed device contracts, board factories, checkpointing and replay. Preserve upstream terms and distinguish the author's reported redistribution grant from unreported downstream licensing terms.
- The labels do not move. A statement about the X70 stays X70-reported, and a reading their model supplies for our bytes is ours only through our own bytes or a capture. The grant changes what may be used, not what is evidenced.
- The boot-ROM capture stays redundant (section 3.1), and the X70 DAT and RAM captures stay unused: our own bytes, captures and decode supply the X-M1 side.

The current credited redistribution permission is owner-reported. This report records it without inventing a standard license or expanding it to third-party material.

### 1.4 What remains to ask

Two items, both facts rather than grants, were put to the author:

1. The measured-versus-inferred status of the type-1 media geometry (2 KiB pages, 64 per 128 KiB block, 1024 blocks), of the per-unit "captured" storage profile, and of the fixture values their media model carries. A measured answer makes their media model a measured sibling reference; an inferred answer leaves it a model reading like ours. Our own geometry is byte-read either way, and our media questions (`+0x404`'s identity, spare-prefix semantics, the device's physical behaviour) stay ours to settle.
2. The boot-ROM capture procedure behind their README's two matching reads. Reported 2026-10-03 (X70-reported): the processor ROM was read directly over the `ff80` diagnostic USB interface, 16 KiB at `0x80000000`, in requests of at most 256 bytes; two complete reads matched, the SHA-1 matched the upstream FF4227 ROM reference, and the decode is `direct-flow.asm` with the capture record preserving the method and hashes. The two blocks' identity is already settled by the recorded SHA-256 (section 3.1). No independent capture proof accompanies the answer: the method is reported, not observed here.

Nothing in this list gates the experiment of section 5.6 or a build. The target-addition question is not on this list because the author does not know the answer; it is our experiment. The port direction is not on this list because it is settled: separate.

### 1.5 What should be copied

The assessment runs an external scratch checkout. Any copied or adapted model proposed for publication must satisfy section 1.2 and preserve vendored-component terms. The boot-ROM capture is redundant (section 3.1), since the project holds the same bytes from its own body; the X70 DAT and per-unit RAM captures are not inputs.

## 2. Architecture

### 2.1 What it models and how it is built

Layout, X70-reported: `emulator/` holds the Python models and the board factory (`board.py`, `boot.py`, `interrupts.py`, `loader_devices.py`, `graphics_devices.py`, `producer_devices.py`, `image_work_devices.py`, `sd_card.py`, `control_bus.py` and 137 Python files in all, 3.2 MB); `native/` holds a Rust PyO3 extension with a Python adapter (`x70_native`); `decomp/` holds firmware mods; `research/` holds notes and JSON proof receipts; `firmware/` holds mapped sections and the manifest; `tests/` and `tools/` hold the suite and tooling.

Rust owns the two Unicorn engines, one physical DRAM backing, shared SRAM and vector storage, instruction counts, exclusive monitors and deferred translation invalidation. Python owns the scheduler, the peripheral models, checkpoints, the renderer and the browser viewer. The README states that ordinary instructions and RAM stores make no Python callbacks.

The boot path is the captured 16 KiB ROM executed against modelled devices, then the DAT's sections decoded through the same Fuji LZ grammar our project decoded independently, with every loaded section size- and SHA-256-checked against the manifest. Kernel and RTOS instructions are stock; the project states that no RTOS return is substituted on the paths it claims.

### 2.2 Device models

A model declares a containment test plus an `mmio_ranges` superset for dispatch, and read/write handlers; any access outside the modelled offsets raises `UnsupportedDevice` and stops the machine. The model's register set is the firmware's observed access set: `interrupts.py` states that its families are inferred from actual kernel accesses rather than a hardware manual. A different firmware's accesses are gaps to be filled from that firmware's own census, which is the property that matters for a port.

Unmeasured behaviour is an explicitly named profile fixture, and each result reports the profile text with its limits. Examples from the boot command line: `--control-bus-profile endpoint7-stationary-and9-heartbeat-counter`, `--input-processing-profile stock-mode0-81-packed-writes`, `--producer-stop-profile stock-idle-stop-handshakes-unmeasured`, `--spi0-input-profile absent-external-flash-fixed-low-miso`. Iteration uses checkpoints of the whole board state with identity checks, plus a warm REPL that restores a boundary and replays a suffix in a few hundred milliseconds.

### 2.3 The two CPUs and exceptions

Two Unicorn Cortex-R5F contexts stand in for the camera's R4 with an explicit CP14 identity fixture. Their own limits text says R5F is an ISA proxy and R4 semantics are not fully validated, which is the conclusion our engine spike reached independently. This is the same gap our bounded runner documents, and both projects place the R4 identity in a declared fixture rather than in the engine.

CPU1's release is a board rule: an MMIO write to the second bank's software-request register releases CPU1 after its high vectors are copied from `0x3fff0000`; their README names it a source-120 pulse and labels the release protocol a board assumption. Interrupts are delivered architecturally (CPSR and SPSR banking, vector fetch, LR) with the firmware's own handlers and returns, over two 128-source banks with per-source priorities, an acknowledge read, an end-of-interrupt write and an optional nesting profile. There is no data-cache simulation: the RAM and its `0x40000000` alias are two views of one backing, cache maintenance is invisible to the scheduler, and hardware cache coherence is stated as a model limit.

### 2.4 Firmware loading

The ROM comes from the captured block; the DAT is parsed into mapped sections; each section's size and SHA-256 is checked against a manifest generated from the X70 DAT. A `captured` storage profile supplies one unit's logical configuration bytes as a fixture, and the README states that the public DAT does not contain them and they must be present separately; an `erased` profile makes them optional.

### 2.5 Tests

`tests/` holds 208 files beside `native/tests/`, on `unittest`, with the project's own harness helpers; a saved suite report in their stock progress note records 328 reference and 57 native tests passing at one baseline. The checkout's rule is that tests live next to the code they test and write into ignored build directories, that research notes keep JSON receipts, and that unchanged sources and binary are fingerprinted around a run.

### 2.6 Their frontier, as their documents state it

The stock path executes the ROM, the loader, both kernels and the RTOS, reaches the REC UI state (mode 3 installed) and serves firmware-programmed RAM compositions through a viewer with ordinary key input. REC initialization is incomplete (`REC_INIT` state 5, busy 1), both display frame counts are zero, and sensor acquisition and physical HDMI remain unresolved; the menu pixels come from declared graphics and backdrop laws. Their own words: a firmware-produced normal shooting screen is a separate hardware-model milestone.

The 2026-10-09 report that the emulator "boots unmodified X70 firmware into record mode" with HDMI output is therefore stronger than the repository's own statements. What is established is the REC UI state with declared-law pixels; the HDMI work is separate hardware research using a capture card.

### 2.7 Distance from our runner, and whether ours could grow into theirs

Ours is one Unicorn R5F engine in user-mode A32 over an admitted subset, sixteen regions, 1 MiB of logical bytes, thirty-two functions and stubs, fail-closed MMIO, no exceptions, no scheduler, no peripherals and no checkpoints. Its contract is containment: one function closure over declared memory, with the result checked against an independently derived expectation.

Theirs is a board whose default is that everything executes unless a device refuses, with two engines, privileged modes, exception entry and return, interrupt delivery, a virtual-tick scheduler, device models, checkpoints and a renderer.

The runner cannot be widened into that board. The system layer inverts the default, and the missing pieces (privileged modes, exception plumbing, interrupt injection, the `mrc p14` intercept, a scheduler, device models) are new work rather than more admitted instructions; growth by admitting instruction forms reaches more closures, not a machine. The two contracts are different tools, and the bounded runner stays the right tool for closures.

Integration uses Haskell host tooling and Haskell/Rust models and execution code under [current principles](../docs/principles.md#implementation-and-build). C is limited to technically necessary engine or ABI boundaries; retained Python responsibilities are migration debt. Running the external upstream tool for local research does not resolve the publication requirements for adapted code.

What their checkout supplies is now more than method: the device models, board structure and harness shapes are usable inputs (§1.3). Their `DriverHarness` is the mirror image of our native-case runner, a routine-level harness with declared substitutions; it confirms the shape of the tool we already have.

## 3. SoC evidence

### 3.1 The two boot ROMs are the same bytes

Our artifact `private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin` is 16,384 bytes with SHA-256 `4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be` and SHA-1 `bed698ed1d91b5c29ce7c5d650156e6100c32cce` (byte-read; the capture history is E-BOOTROM-PROBE-150 and E-BOOTROM-DISCRIMINATOR-150).

Their capture path `research/hardware/ff80-bootrom-20261003T190308Z-d56f5032/bootrom.bin` is a Git LFS pointer in this clone, recording `oid sha256:4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be` and size 16384; their README states two full reads matched byte for byte, `JASM_A_1` at offset `0x3ff0`, and a SHA-1 matching the upstream FF4227 ROM reference (X70-reported).

Their file is not present locally, so no byte-diff was run; the equality of a recorded content hash with our local content hash settles that the two 16 KiB blocks are the same bytes. What that does and does not establish is in section 4.

### 3.2 Block by block

Our side of this table is a constant-address access scan (`private-data/analysis/x-m1-1.50/bootrom/strap-scan.c`) over our ROM, both boot records, both loaders, images 4 and 5 and the plain image, together with the decodes in [the window map](bootrom-window.md) and [firmware extraction](firmware-extraction.md). The scanner decodes movw/movt, mov/mvn with rotated immediates and pc-relative literal loads, computes the effective address of a plain load or store with an immediate offset, and skips register-offset, post-indexed and table-driven forms, so the census is a lower bound. Their side is the model code and its comments, X70-reported.

| Block | X70 model | Our bytes | Reading |
| --- | --- | --- | --- |
| `0xfff88000` clock | offsets `0x00`-`0x38` dividers plus `0x40`, `0x48`, `0x50`-`0x5c`, `0x80`, `0x84`; `+0x38`/`+0x40` request bit 28 then read ready bit 24; `+0x80` bit 16 ready, read-only; `+0x48` alone settles after a virtual delay | ROM: poll-then-write over `+0x00`-`+0x20` with an unbounded wait on `0xfff88080` bit 16, then `0xfff88038` bit 28 set and an unbounded wait on its bit 24, direct stores `+0x24`-`+0x30`, and `0xfff88084` bit 0 cleared on the halt path | Same block and the same ready convention at `+0x38`/`+0x48` and `+0x80`; agreement |
| `0xfff89000` GPIO | 16 ports at `0x40` stride, `+0x00` output with high-half write masks, `+0x14` input; the boot fixture sets port 0 bits 11:10 | ROM: `+0x08` write `0x0c000c00`, `+0x14` read (bits 11:10 in the mode detect, bits 1, 4, 5, 7, 10 polled at runtime), pin-mux writes `+0x8c`-`+0x110`, `+0x18c`/`+0x30c`, `+0x208`; three 16-register mirrors at stride `0x40` into RAM `0x400a0000` | Same layout; both bodies take the boot source from port 0 bits 11:10; agreement |
| `0xfff98000` system | `{0, 8, 0x100, 0x104, 0x208}` | ROM: strap read and mode word at `+0x00`, handoff value at `+0x08`, pair writes `0xfff98100` = `0x50000100` and `+0x104` = 0, config `+0x208` | The same five offsets; agreement |
| `0xfff99000` DRAM (model label) | 20 offsets, `{0,4,8,c,10,14,18,1c,40,44,c0..f8}` | ROM: programmed from the config block at `+0x04`-`+0xf8`, polled at `+0x04`/`+0x08`, read-modify-write `+0x1c` | Configuration and polling offsets overlap; the block's exact X-M1 function remains unbound |
| `0xfffb2000` reset | `{4, 8, 0xc, 0x10, 0x14}` | ROM: pair writes the same five offsets, values `0x5f5f`, 1, `0x11`, `0x1111`, `0x1111` | Same five; agreement |
| `0xfff01000` PHY (model label) | any offset below `0x248` accepted; writing bit 0 to `+0x00` starts it; `+0x90` bit 28 reads as ready once started | ROM: `+0x00` orr #1 then an unbounded wait on `0xfff01090` bit 28, and 146 indexed words written over `+0x00` through `+0x244`, last byte `+0x247`; image 5 read-modify-writes `+0x00`, `+0x48`, `+0x94`, `+0x98` | [INFERENCE] A memory-controller family interpretation fits the target programming sequence and packed values better than a standalone interrupt bank or PHY-only identity. Controller-local interrupt fields remain plausible. The sibling label and completion fixture do not bind target field meanings; see [the local register investigation](sd-boot-card-entry.md#46-the-mode-2-safety-envelope) |
| `0xfffb3000`, `0xfffb4000` interrupt banks | two 128-source banks; families at `0x60`, enable set `0x80` and clear `0xa0`, pending set `0xc0` and clear `0xe0`, source config `0x100`, pending clear `0x120`, `0x140`, `0x180`, current source `0x1c0`/`0x1c4`, reset `0x1c8` and `0xc00`, acknowledge `0x200`, active bitmap `0x230`, handler vectors `0x400+4*line`, source priorities `0x800+4*line` | ROM: conditional config writes at `+0x8c`, `+0xac`, `+0x10c`, `+0x12c`, `+0x15c`, skipped on this body (config field `+0x54` = `0x001a0000`); boot record 1 writes `0x01000000` to bank-1 `+0x8c`, `+0xac`, `+0xcc` and `+0xec` at sites `0x1ec4`-`0x1ed0`; both records write `+0xc00` in both banks; image 5 read-modify-writes bank-0 `+0x80`-`+0x8c` and `+0xa0`-`+0xac`, writes `+0x319c`, and snapshots `+0x3000`-`+0x300c` and `+0x3060`-`+0x306c` into RAM | Our accesses land inside their families, reset register and vector/config families included; bank 1's bit-24 write at `+0xcc` is the address and value the X70 board treats as the second CPU's release; the two bases as the controller banks is a sibling-derived reading that our accesses support. Coverage gap: our reads at offsets `0x00`-`0x0c` are outside the families their model accepts |
| `0xfff91000` timers | offsets `0`/`4`/`8` free-running loader timer, `0x10`-`0x2f` and `0x30`-`0x3f` channels, divider `1 << ((control >> 4) & 7)`, channel lines `0x20+n`, no claimed physical frequency | both records write `+0x00`/`+0x04`/`+0x08` (record 1 at `0x26d4`, record 2 at `0x52304`); both loaders and image 4 read `+0x00`/`+0x04` and write `+0x08`/`+0x04`; reload 30000; the log tick is `[0xfff91000]/1875` measured at 99.88 Hz; the increment callback is registered as id 32 | Same block and register triple; the divider and physical source are declared unknowns on their side and measured on ours, so the counter model transfers and the time base does not. Their channel lines sit at `0x21`-`0x23` while our evidence names a kernel callback id 32, an open item until our own kernel decode is read |
| `0xfff43000` decompressor | Fuji LZ engine: start `+0x04`, source `+0x40`, length `+0x44`, destination `+0x48`, output count `+0x4c` read-only, completion acknowledge `+0x88`; 16 KiB job pages; the first submission skips the 20-byte stream header | ours: the same base and the same start/source/length/destination/ack/count registers, `0x4000`-byte feeds after a `0x14`-byte header, an implementing grammar and a byte-exact encoder | Same engine and register set; agreement |
| `0xfff52000` media and `0xfff52800` companion | type-1 geometry, 2 KiB pages, 64 pages per 128 KiB block, 1024 blocks; a 512-byte aperture at `+0x400`; spare prefix from registers `0x40`-`0x4c`; DMA at `+0xa0` (tag `0x401000`), `+0xa4`, `+0xa8`; done and acknowledge at `+0x54`; commands `0x800007`, `0x800033`, `0x802001`, `0x802362`; erased or clean spare, ECC and bad-block fixtures; one whole-page program command writes a volatile backend | ours: the controller at `0xfff52000`/`0xfff52800` with device select `+0x00`, command and status `+0x04`, control `+0x08`, parameter `+0x1c`, unit offset and page `+0x20`/`+0x24`, done and acknowledge `+0x54`, `+0x58`, the companion `+0xa0`/`+0xa4`/`+0xa8` carrying `0x401000` with destination and count, a 512-byte window at `+0x400` whose second word `+0x404` the page scan tests, `0x210` unit stride (512 data plus 16 spare), 128 KiB blocks and a software bad-block remap; device size and technology unproved | Same geometry and the same DMA and acknowledge registers; `+0x404`, unnamed in our window map, is the second word of their 512-byte aperture, and their program model fills the aperture's first 16 bytes from registers `0x40`-`0x4c`, a candidate reading for our spare-prefix question (sibling-derived, unproved here) |
| `0xfff02000` fill and copy | a channel-0 loader fill and application fill/copy model with a register set at `{0,4,8,c,10,14,24,28,2c,30,34,300}` | ours: every call site held (the image-4 bss zeroing, the mode-7 path), register map not decoded | The one block where their model supplies what our assessment listed as missing |
| per-body peripherals | USB, SDIO, lens, HDMI, audio, accelerometers, the graphics and image pipelines | our companion register bank at `0xfff84040` and the `(9,2)` bank, the mode-7 block at `0xfff56000`, SD at `0xfff50000`/`0xfff51000`, image blocks at `0xfff18000`-`0xfff1a000` | Bases differ or are unmapped on our side; this is where a port's work lives |

### 3.3 Chip, revision, or firmware

The byte-identical ROM and matching access conventions support a shared ROM/controller lineage. They do not establish the same chip or silicon revision. This body's config block skips the `0xfffb3000` sequence (gate field `0x001a0000`, byte-read); its peripheral set differs; and each model's register coverage is its own firmware's access set. Our uses of interrupt-bank offsets `0x00`-`0x0c`, the model's DRAM-labelled block at `0xfff99000` and the `+0x404` accept field appear as gaps on their side rather than conflicts.

## 4. The boot-ROM comparison

Comparable is too weak a word for these two artifacts: they are the same bytes. Our local file's SHA-256 equals the content hash recorded in their capture's LFS pointer and stated in their README, over the same 16 KiB length; their file is not present in this clone, so the comparison was made by hash and no diff was run.

What the comparison tells us:

- The ROM content both projects model is one program, so a boot model validated against one ROM exercises the other, and any divergence in a boot model on this content is a model bug rather than a firmware difference. That is the property the previous assessment wanted and could not have.
- The ROM-side device obligations are shared and now named on both sides: the two strap registers and the mode word, the config apply's five block writes with its three unbounded waits (`0xfff88080` bit 16, `0xfff88038` bit 24, `0xfff01090` bit 28), the media controller's init, six-unit page scan and `+0x404` accept condition, the load's DMA descriptor words (`+0xa0`, `+0xa4`, `+0xa8`), the handoff stub and the timer-programming tail.
- The X70 project's boot is evidence about our ROM rather than an anecdote, with its remaining assumptions labelled in its own notes.

What it does not tell us: anything about our loader, kernel, RTOS, images, media content, strap values or peripherals; anything about the media device's physical identity; and nothing about silicon revision or die identity beyond the ROM mask, which is the strongest claim this material supports about the chip.

## 5. Revised recommendation

### 5.1 What the reference changes

The shared ROM and source-bound device contracts make a separate X-M1 port technically feasible. The bounded target-addition experiment (§5.6) tests the remaining implementation gap. The owner's 2026-10-14 report establishes explicit credited redistribution permission for the author's work; downstream modification and sublicensing terms were not reported (§1.2). Sibling interpretations in section 3.2 retain their evidence scope; a common engine solution does not establish X-M1 silicon fidelity.

### 5.2 What it does not change

- The output question. Their renderer shows firmware-programmed RAM compositions under declared laws; sensor acquisition and the camera's own display path are unresolved on their side too. Film-recipe acceptance still needs the body.
- The application-side mass. Their models are keyed to X70 addresses and X70 firmware; the equivalent models for our menu, develop and service paths have to be recovered from our own bytes, and that is the bulk of the cost.
- The language rule. Their runtime is Rust and Python; project-authored tooling and models use Haskell/Rust, with C only at necessary engine or ABI boundaries and no new project-authored Python.
- The upstream terms. The reported credited redistribution permission concerns the author's work, not Fujifilm firmware or per-unit captures. Vendored components retain their own licenses; downstream modification and sublicensing terms were not reported (§1.2).

### 5.3 Smallest useful target and cost with the reference in hand

T1, the mode-4 boot handoff of this body's ROM and container, accepted against the captured dump, stays the smallest checkable target. With the reference, every wait the ROM blocks on has a named counterpart: clock `+0x38`/`+0x48` and `+0x80` bit 16, PHY `+0x00` and `+0x90` bit 28, DRAM `+0x04`/`+0x08`, media `+0x54` and `+0x404`, the reset block's five offsets, and the second core's release write in our own record 1. With the grant, the models and the board structure are direct inputs rather than method: their boot fixture's strap and GPIO values, the clock and PHY ready transitions, the media read path and its `+0x404` accept, the fill/copy register set, the interrupt banks' enable, pending and acknowledge semantics, and the release rule adapt into the plan's S2 to S7. Revised estimate: the plan's split was engine two to three days, models two to four, comparator one to two, integration one to three; the model and board-design share becomes adaptation, so days with about a week as the outside figure, against days to two weeks with method only and a few person-weeks without the reference. The engine work (privileged modes, exception entry, the `mrc p14` intercept, one or two contexts), the dump comparator, and the acceptance of the media read model against our dump are unchanged: the checkout cannot supply them. If the experiment of section 5.6 returns yes, the same instrument can be had on their framework at the experiment's cost plus its gap list, as a separate tool; the estimate above then prices the instrument integrated with this repository's build and tests.

T2, kernel up, is better specified but not proportionally cheaper: the kernel-layer devices have shapes to adapt directly and an access census to re-derive as a coverage check, so one to two person-months stands with its upper tail lower, and the validation risk is lower because their model passed a working boot over the same controller block and ROM.

T3, menu and develop in context, remains several person-months for the reason in section 5.2: its cost is our application devices, not the framework. The reference shortens it by removing unknown-unknowns and by supplying the renderer and iteration approach, and it does not remove the device recovery work.

### 5.4 What it would unblock

- The parameter-provenance trace, stalled on flat state a service session supplies (`0x01529fb8` through the service action): a board reaches that state only when the service path itself is modelled, which is T3 work on our side. Reached at T3, not at T1 or T2.
- The develop path, never executed: reached at T3, and the X70 still-processing and producer models show the shape and volume of that work.
- The static menu work: T3 makes the menu executable in context with a declared-law renderer; what it shows is the emulator's laws, not the camera's rendering.
- The boot-state and boot-timing camera sessions and the writer-attribution passes: T1 and T2, with a better-specified path than the previous assessment could offer.
- Three of our own open readings: the `+0x404` field and the spare-prefix registers have a sibling-derived candidate; the interrupt-controller bases are identified in a way our accesses support; and the `0x3fff00xx` vector literals in record 2 match the second core's high-vector region in the X70 model. Each becomes our fact only through our own kernel decode or a capture.
- The port route: a separate X-M1 target can reuse the architecture as a local research reference. Public distribution of copied or adapted implementation remains unresolved under section 1.2. The experiment tests DAT acceptance and names peripheral gaps; it does not supply target fidelity.

### 5.5 What remains open

1. The one fact still with the author (section 1.4): the media geometry's measured-versus-inferred status. It does not gate the work below. The boot-ROM capture procedure is answered as reported (section 1.4).
2. The target-addition experiment (section 5.6), executed to its positive record-1 boundary in section 5.8. The framework accepts our pinned DAT and executes its ROM-to-record handoff without a device-offset or engine change. A separate port remains an option for the boot-handoff instrument; the later loaders and the T1 dump comparison remain unexecuted.
3. The owner's build decision after that: the repository-integrated T1 at the revised estimate, or a separate port if the experiment returns yes. The acceptance is the dump comparison of [the T1 plan](full-system-emulation.md#73-the-dump-comparison-criterion), and the second core's peer side stays a declared substitution.

### 5.6 The target-addition experiment

The author does not know whether his framework runs another body's DAT, and the surface is undocumented, so the answer is an experiment (X70-TARGET-ADD-01): host-only, in a scratch copy of the checkout, and run before any port or system-layer build. What it must find: whether our 1.50 body's boot, on the same ROM bytes, with our media content and strap values, runs on their board by supplying a mapped-section manifest, model instances and board options, and where it first stops if not.

What this read shows about the surface, all X70-reported:

- `tools/unpack_x70.py` produces `firmware/mapped/*.bin` plus a manifest in a generic shape (source DAT SHA-256; per-section name, file, load address, size, flash and DAT offsets, SHA-256; plus `unmapped_BSS`), but is hard-coded to the X70 1.10 DAT hash and the X70 flash layout.
- The identity surfaces are narrower than the initial assessment stated: `emulator/runtime.py` checks a fixed `EXPECTED_FIRMWARE`, but that is the routine-level display emulator, not the native boot constructor. The native board reads the manifest, checks the ROM digest, verifies `boot` against its manifest digest only for direct loader entry, and compares `kernel0`, `kernel1` and `section4` memory with their manifest digests at `0x40020`; those section digests are not hard-coded. The driver harness requires `DAT_SHA256`, and `native_flash_from_dat` requires the supplied DAT's fixed digest. `emulator/board.py`'s `ROM_HASH` passes unchanged because our captured block has the same content.
- The strap and GPIO inputs are parameters rather than constants: `BootControls(boot_source=..., boot_gpio=..., gpio_inputs=...)` behind `make_board(entry='rom', storage_profile=...)`.
- The checkout's payload files are Git LFS pointers as cloned: the ROM capture, the DAT and every mapped section. It cannot run without a fetch or substitutes, which is the experiment's first environment step and nothing else's concern.

The experiment:

1. A working copy of the checkout outside this repository, the original untouched. Supply the payloads the run needs from our side: our captured ROM block at their capture path, which passes their `ROM_HASH` check because it is the same content, and our 1.50 DAT where their board feeds its NAND model. Their own payload files are LFS pointers as cloned and are not needed for this run; a fetch matters only if the X70 path is wanted alongside.
2. Build the per-body mapping from our own artifacts: a mapped-section export of our DAT and a manifest in their format, from the decode we hold (the container rebuild, the byte-exact decoder and the segment-table correspondence, E-DUMP-RAM-150), with our DAT's SHA-256 as `source_sha256`. Their `native_flash_from_dat` framing (staging header, complemented payload, the media image at payload `+0x48`) matches our container's framing, where raw `0x25c` starts the complemented media; the section layout inside is ours.
3. Run the boot from reset with our mode-4 strap values (E-STRAP-READ-150) and the `erased` storage profile, and move or parameterize each identity pin the run trips. Record every stop: a manifest or section-hash refusal, a fail-closed device offset (their `UnsupportedDevice` names the block and offset), or a board rule that does not fit.
4. Bounded push: fill gaps in run order until either record 1 executes (the loader stage, T1's neighbourhood) or the next stop needs a change outside the manifest, model and board-option surface. That stop is the negative answer and its location is the finding.
5. Report: whether the DAT was accepted, the first and terminal stops, the gap list classed (manifest, model offset, board option, engine), and the work remaining before a boot. The expected stops have candidates in our own census ([section 3.2](#32-block-by-block)): our reads at interrupt-bank `+0x00`-`+0x0c`, the DRAM block's `+0x04`-`+0xf8`, and whatever our config block's skipped `0xfffb3000` sequence does to a board built on the X70 data.

### 5.7 Prepared X-M1 scratch run

Preparation and implementation are source-inspected; the separately executed run is recorded in section 5.8. The scratch source is `private-data/experiments/x70-target-add/source`, selectively exported from upstream revision `52a4e8f019ec27d5cb1f4ac143796f519334bd55` with LFS smudging disabled. It contains the upstream emulator, native package and build helper, not X70 DAT or RAM inputs. No original source-worktree file was edited. The checked-in Haskell exporter `//host/firmware:export-x70-map` defaults to our pinned stock 1.50 DAT and also accepts an explicit caller-pinned candidate digest; it supplies the selected DAT, our captured ROM and a manifest under `firmware/mapped-xm1/`. It parses and checks the container contract, invokes the bounded C decoder on both DAT records and requires byte equality with the supplied decoded artifacts before writing the scratch inputs.

The manifest's `boot` is native flash `[0x40000,0x60000)` at RAM 0, including both supplied boot records. Its kernels and resources use their segment entries; its section-table destination is `0x0061c000`; image 4 and image 5 use their actual decoded lengths. The plain image uses the extent descriptor's destination `0x01100000` and total `0x360000`, not entry 6's duplicate image-5 load/size words. Entry 7 has no resident load extent and is omitted. The original DAT, not the mapped sections, supplies NAND bytes including the trailer; the manifest does not substitute preloaded code for the ROM's transfers.

The supported exporter CLI is host-only and touches neither camera nor USB:

```text
export-x70-map [--expected-sha256 SHA256] DAT DECODE_ID4 DECODE_ID5 ROM FFLZD SCRATCH_SOURCE
```

Without the option, `DAT` must hash to stock 1.50 (`43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`) and the pinned stock checks run. With the option, the DAT must equal the supplied digest and the structural and carried-field checks run without requiring stock checksum equality. `ROM` must be the captured 16 KiB block with SHA-256 `4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be`. The decoded files are accepted only by whole-byte equality with the C decoder's outputs from that DAT. `FFLZD` names the Bazel-built `//lz:fflzd` executable. `SCRATCH_SOURCE` must resolve below this workspace's ignored `private-data/`; its `firmware/mapped-xm1` must not exist. All exported binaries, intermediate records, manifest, DAT and ROM remain below that scratch root. The tool exports inputs for an external emulator, not an installable firmware image, and has no camera API.


Scratch-only upstream adaptations move the DAT and manifest identity pins to our digest, validate every exported section before boot, use our ROM at the existing ROM path, and supply mode-4 low strap bits 1 plus GPIO bits 11:10 = 3. The GPIO words are the captured maintenance reset-stage mirror from E-STRAP-READ-150, held fixed as a fixture, not a claim about ordinary-boot pin values. Storage is `erased`; no captured MCU, user journal or other X70 per-unit profile is selected. The X70 UART-program fixture is unavailable: its fixed `boot[0x14a28:0x14abc]` slice lands in an X-M1 descriptor table, so the constructor no longer reads that slice and a UART-program DMA request refuses without a bound X-M1 profile. This peripheral is beyond the record-1 boundary. The Python adaptations stay in supported upstream files; no project-authored Python tool or Rust engine change is added.

The bounded positive stop is the native entry milestone at `0x1854`, the target of the reset-entry branch `0x1800: bl 0x1854`. The first record-1 instruction executed is the vector word `ldr pc,[pc,#0x18]` at `0x0`, where the ROM handoff lands; `0x1800` is record 1's reset entry, not its first instruction, and reaching `0x1854` establishes that the reset-entry branch executed. The stop compares the manifest `boot` section, RAM `[0,0x20000)`, with the exported DAT boot bytes and with the hash-pinned NAND media bytes at flash `[0x40000,0x60000)`. The ROM loaded media `[0x40000,0x80000)` into RAM `[0,0x40000)`, so the compared section is half of the loaded span and RAM `[0x20000,0x40000)` is loaded but not compared. The positive classification is emitted only when the run entered from the ROM, the low ROM alias has been replaced with DRAM and the BootControls ROM handoff value is 2 (storage); the same boundary reached any other way, including a preloaded `--entry loader` run, stops with classification `non-positive-entry-gate`. A mismatching boot span is a negative stop instead. Its intentional snapshot `failure` has message `Experiment boundary: record-1 first branch executed` and classification `positive-record1`. No application initialization, kernel handoff or T1 dump comparison is claimed by this boundary.

Build and bounded run, from this repository's root, against a fresh prepared scratch source. The export was executed once; repeat native boot only with a new output directory:

```sh
devenv shell -- bazel build //host/firmware:export-x70-map //lz:fflzd
devenv shell -- bazel-bin/host/firmware/export-x70-map \
  "$PWD/private-data/firmware/x-m1-1.50/FWUP0002.DAT" \
  "$PWD/private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" \
  "$PWD/private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin" \
  "$PWD/private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin" \
  "$PWD/bazel-bin/lz/fflzd" \
  "$PWD/private-data/experiments/x70-target-add/source"
nix-shell private-data/experiments/x70-target-add/native-shell.nix --run \
  'export CARGO_HOME="$PWD/private-data/experiments/x70-target-add/cargo-home"; python3 private-data/experiments/x70-target-add/source/tools/build_x70_native.py'
nix-shell private-data/experiments/x70-target-add/native-shell.nix --run \
  'export PYTHONPATH="$PWD/private-data/experiments/x70-target-add/source/native/python:$PWD/private-data/experiments/x70-target-add/source"; timeout 120s python3 -m x70_native --entry rom --storage-profile erased --limit 1000000 --quantum 1000 --output "$PWD/private-data/experiments/x70-target-add/run-next"'
```

`native-shell.nix` imports the same locked `nixpkgs-src` as devenv, with overlays disabled, and supplies Rust, Cargo, Maturin, the native C toolchain and Python with Unicorn, NumPy and pip. Maturin requires the pip executable even with `--skip-install`; adding it to the Nix environment does not run an installation. The upstream build helper uses `develop --skip-install --locked --release`, so it builds the extension in scratch without package installation. Nix's Unicorn Python binding and shared library replace the wheel layout through an explicit `X70_UNICORN_LIBRARY` adapter option; the 2.1.4 binding-version check remains. Export requires a new `mapped-xm1` directory and refuses to overwrite one. Each rerun must select a fresh output directory.

Export source with LFS smudging disabled and supply only owner-held X-M1 inputs. The experiment needs no X70 DAT or per-unit capture payload.

### 5.8 Executed target-addition result

The actual upstream Rust/Unicorn runtime was built and run on our stock X-M1 1.50 DAT and captured ROM. The terminal result is positive at the experiment's bounded record-1 execution boundary: `run-02/checkpoint-5d5454a54a4b/state.json` records 166,046 instructions and virtual ticks, CPU 0 at `0x1854`, record 1's reset-entry milestone at `0x1800` on tick 166,045 and the reset-entry branch target on tick 166,046. The intentional stop is `Experiment boundary: record-1 first branch executed`, classification `positive-record1`. Its 131,072-byte manifest `boot` comparison, half of the 256 KiB the ROM loaded into RAM `[0,0x40000)`, passed against SHA-256 `09fd75ed2dc8f444754f0cfa2b19229808efc17660912b32c3bb6b7272076ace`, the exported DAT boot bytes; RAM `[0x20000,0x40000)` is loaded but not compared. The classifier at that boundary now additionally binds the same span to the hash-pinned NAND media bytes at flash `[0x40000,0x60000)` and gates the positive on the ROM entry (section 5.7); run-02's state records all three gate facts (entry `rom`, `low_rom_alias_active` false, `ROM memory mapping request` value 2), so the executed positive passes the gated classifier. This is emulated evidence for this input and declared board profile, not hardware-observed device semantics.

The inherited X70 UART payload fixture was not applicable to the X-M1 descriptor table and was disabled with UART-program DMA fail-closed. No native engine/device-offset change was needed before the record-1 boundary. Environment setup failures supplied no target evidence.

Classifier gate verification, executed 2026-10-09 after review (emulated; `run-gate-positive/checkpoint-d24e0d1ddf8b` and `run-gate-loader/checkpoint-37c135aefc73`, logs beside the output directories). The `--entry rom` rerun reproduced run-02: 166,046 instructions, `ROM memory mapping request` value 2 and `ROM low alias replaced with DRAM` at tick 166,037, `low_rom_alias_active` false and `rom_handoff_mode` 2 at the stop, classification `positive-record1` with boot SHA-256 `09fd75ed…`. The `--entry loader` rerun reached the same `0x1854` boundary in 3 instructions through the constructor's preload and stopped `non-positive-entry-gate`, message `Record-1 boundary reached without a ROM-entry media handoff`, with entry loader, `low_rom_alias` false, `rom_handoff_mode` null, no handoff events and zero NAND transactions. The substitute-bypass positive is excluded in execution, not only in code. An independent media recompute binds the compared half: the pinned DAT `43d67afa…` yields inverted media SHA-256 `5badefdd…`, the NAND model's `native_data_sha256`, whose `flash[0x40000,0x60000)` is byte-equal to `boot.bin` and SHA-256 `09fd75ed…`, while the loaded `flash[0x60000,0x80000)` differs from the compared section in 23,979 bytes and is not compared; the ROM's transfer union covers exactly `flash[0x40000,0x80000)` to `0x40000000+(off-0x40000)`, and the ROM block rechecks to `4ecedfbf…`. CPU 1 executed zero instructions in both runs and `loader_memory_validation` stayed empty.

The native boot constructor accepted the X-M1 manifest and checked every mapped artifact; its NAND source accepted the pinned X-M1 DAT. ROM instructions then performed the actual mode-4 configuration and media-load path against the inherited device models, with our bytes, our mode straps, a declared fixed maintenance GPIO mirror and erased storage. The manifest did not preload the boot record. CPU 1 did not execute, the kernel-entry `loader_memory_validation` list is empty and no application state was reached.

What remains before a useful T1 boot instrument: record 1's later loader sequence, its release/rendezvous with record 2, our companion-register inputs, decompression and bss fill, the kernel handoff and the dump comparison of [the T1 plan](full-system-emulation.md#73-the-dump-comparison-criterion). The UART-program profile stays unsupported if a later path requires it. The positive target-addition result establishes that this first handoff takes the existing framework; it does not establish all later X-M1 devices or a successful full application boot.

### 5.9 Recorder-residency reproduction

The recorder extension uses a separate ignored scratch, `private-data/experiments/x70-target-add/source-recorder`, and the installed recorder revision-2 container, SHA-256 `72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8`. The exporter decoded that container's records and required whole-byte equality with `rev-2/id4-recorder.bin` and the retained stock image 5. The resulting manifest sets image 4's load address to `0x0061c340`; image-4 offset `0x103b74` is `c9 a9 34 eb`, the little-endian tap word `0xeb34a9c9` targeting `0x0144a5e0`. The 1,896 bytes at that address in the mapped plain-image extent hash to `3badfd4640bce898fdbf634a047e08deccc430aa721615f5e7a8f193f6d0fd03`, equal to `rev-2/af-recorder.bin`.

Removing only the experiment's intentional `0x1854` stop first exposed a stock record-1 dependency: at `0x18fc`, record 1 stores through `r0 = 0x70000000` while clearing 8 KiB immediately after programming CP15 `c9,c1,1`. The inherited board did not map that X-M1 DTCM window and failed closed after 166,089 instructions. The recorder scratch maps a private 8 KiB fixture at that exact address for each emulated core. This is an instruction-bound fixture, not a measured X-M1 TCM implementation.

With that fixture and the exact recorder DAT, both stock kernel milestones were reached (`0x00040020` at tick 926,186 and `0x000d0020` at tick 927,347). Execution then reached the recorder tap at `0x0071feb4` on CPU 0 after 10,911,337 instructions. The entry hook stopped before executing the patched branch and compared the target span in RAM with the payload carried by the candidate. RAM `0x0144a5e0..0x0144ad47` was still 1,896 zero bytes, SHA-256 `43a994de8e61bfab3e4e655ff5d064edd5459c4c3bccc3c114794d193d46930d`, not the expected payload hash `3badfd46…`.

**Emulation:** the recorder container reaches its patched call before the id-6 plain-image load has populated the call target. This reproduces the residency-prerequisite violation with the exact installed candidate and establishes the fix requirement: the pre-load path must remain stock-equivalent until an always-resident trampoline can prove the payload is present. It does not prove that the physical body followed every modeled device input or that this was its only failure. The next emulator acceptance is the corrected candidate reaching this same tap with harmless pre-load behavior, then reaching the payload only after the load is resident.

Revision 3 leaves `0x0071feb4` stock in the container and retargets the id-6 case-3 completion call at `0x0073b564` from `0xebffa12a` (`bl 0x00723a14`) to `0xeb343d0d` (`bl 0x0144a9a0`). The 52-byte `rec_stage` trampoline publishes `0xeb34a9c9` at the receive site, performs data-cache clean and instruction-cache invalidate operations for that word, restores the completion-call arguments and link register, and tail-calls the displaced helper. The emitted DAT is 38,405,148 bytes, SHA-256 `426ee4ec5cf83fef492e719bbfae94fa247549cc02d1eda099af55c44fd4d0e5`; final container verification passed 42/42 checks.

The exact revision-3 candidate was exported and run from ROM with the DTCM fixture above, an explicitly absent blank SD-card model and the emulator's declared equal-priority IRQ selection fixture. The early receive at tick 25,124,489 retained stock word `0xebffffdc` while the payload was still zero. The stage call at tick 45,730,641 found the 1,896-byte payload resident with SHA-256 `23998f7a3e82e51f7352252d15c72af2347c80607df956a9fdcff9e9402dd31d` and the receive word still stock. At the trampoline tail at tick 45,730,654, the receive word was `0xeb34a9c9` and `r0-r12` plus the completion-call link register were preserved. Execution continued without failure to the 100,000,000-instruction bound. No later media receive occurred before that bound, so this proves post-load publication and ABI preservation, not a later hook invocation, successful file carry or camera recovery.

**Emulation:** revision 3 closes the reproduced pre-load branch defect under the declared full-system fixtures. It remains non-installable: a failed id-6 read may still continue to an unstaged trampoline, and the padding carrier is not ownership-proved. Later exact v5 recovery evidence does not validate this recorder candidate.



## 6. Files and labels

Rows: E-X70-EMULATOR-TERMS-150, E-BOOTROM-X70-IDENTITY-150, E-X70-SOC-CROSSCHECK-150, E-X70-SOC-LABELS-150 and E-X70-REFERENCE-ASSESS-150 from the first assessment; E-X70-GRANT-150 records the author's grant and replaces the use-only reading E-X70-EMULATOR-TERMS-150 took before his answers arrived, whose file facts stand; E-X70-ASKS-REVISED-150 records the two asked facts, whose capture-procedure answer is now reported (section 1.4), E-X70-T1-COST-REVISION-150 the revised estimate, E-X70-TARGET-ADD-PLAN-150 the experiment, E-X70-TARGET-ADD-150 the executed result of sections 5.7 and 5.8, E-AF-RECORDER-RESIDENCY-EMU-150 the installed-candidate reproduction and E-AF-RECORDER-STAGE-EMU-150 the revision-3 activation result of section 5.9. Existing reports are not rewritten from a sibling model.

## 7. Limits

- The X70 firmware boot was not run. The upstream runtime was built and executed only with our X-M1 DAT, ROM and decoded artifacts, as recorded in section 5.8. Sibling application claims elsewhere in this report remain X70-reported, not findings from this run.
- The grant of section 1.2 is owner-reported from the conversation of 2026-10-09; it is informal and undated in writing, and its scope is the owner's statement of it. No instrument was read.
- Their boot-ROM capture bytes are not in this clone (a Git LFS pointer); the identity claim rests on the recorded content hash, and the capture procedure is the author's reported method of section 1.4 (`ff80` diagnostic USB read of the processor ROM on 2026-10-03, requests of at most 256 bytes), with no independent capture proof.
- Our access census covers constant-address instruction forms only; register-offset, post-indexed and table-driven accesses are outside it, so every "our bytes" cell is a lower bound.
- No X-M1 behaviour is inferred from the X70 here; the sibling-derived readings stay labelled and unproved until our own bytes or a capture settle them. The grant changes what may be used, not what is evidenced.
- The initial target-addition experiment stopped at record 1. The separate recorder extension reached both kernel entries, the early media receive and revision 3's post-id-6 stage activation using the declared fixtures in section 5.9. It did not complete application boot, reach a later post-activation media receive or meet the T1 dump-comparison criterion.
- Host-only throughout: no camera or USB device was touched. Initial experiments used ignored scratch. Later interactive emulation retains a selected runtime closure at `emulation/xm1-upstream`, based on commit `40406daf52228ace99cb1a3e322f3193f96b0283`, with X-M1 adaptations and a Haskell/Bazel launcher. Firmware, ROM, captures and outputs remain private except the separately approved exact sixteen-byte remembered MCU observed fixture (§1.2). The [full-system report](full-system-emulation.md#119-interactive-x-m1-application-emulation) owns its current execution boundary; [licensing](../LICENSING.md) owns the recorded credited redistribution grant and remaining terms and artifact questions.
