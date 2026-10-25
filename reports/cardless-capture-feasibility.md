# X-M1 1.50 cardless media state and capture-to-RAM feasibility

## Decision

**The archived cardless body capture sampled the slot as card-present.** Run 3 contains the active-low card-detect row in its asserted state, the SD initialization error latch, filesystem error flags, media status 4 and warning `0x13` (CARD ERROR). This is not the stock absent-card state. The firmware has a stock NO CARD route when detection reads absent, including a card-edge reset of SD initialization. No inspected mode or power path makes an unchanged present indication mean absent. A mode change can clear warning presentation, but does not turn this media failure into NO CARD.

**Full-resolution capture-to-RAM is not implemented or ready for a body run.** Stock image processing already uses RAM, and USB can read a retained extent. Neither fact supplies a completed fresh exposure, a buffer lease across the transfer, storage-independent transaction completion or preview restoration. The service imaging jobs are separate diagnostic branches, not a drop-in ordinary-capture implementation. Keep [#105](https://github.com/skeet70/latent-image/issues/105) conditional on a quality question that coherent preview/statistic observations cannot answer.

The next cardless AF decision is therefore between restoring an absent electrical indication, repairing the slot, or separately reviewing the AF-only admission override described in the [observation plan](af-observation-plan.md#cardless-s1-result-and-decision-options). This report does not select or authorize a camera operation.

## Evidence and scope

This is host-only inspection of the X-M1 1.50 decodes and the existing ordinary-boot Run-3 RAM capture, not a new camera experiment. The official DAT identity is SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`. Addresses below belong to those X-M1 images; `0x4xxxxxxx` is the uncached alias of physical `0x0xxxxxxx`. No behavior or address is imported from another model.

- **E-AF-CARD-DETECT-RUN3-150:** observations extracted from the retained body capture, including sampled detect, SD and filesystem flags, registered media destination and pool selector. Run 3 is non-atomic and precedes the failed S1 attempt.
- **E-AF-CARD-MEDIA-PRODUCER-150:** source inspection and targeted A32 disassembly of detection, initialization, status translation and warning refresh. This supplies a control-flow explanation, not the exact failed SD command or an instruction trace of that boot.
- **E-CARDLESS-RAM-CAPTURE-MAP-150:** source inspection of frame descriptors, diagnostic output publication, completion cleanup and save-related boundaries. It establishes no fresh exposure or completed RAM-only transaction.

The starting constraints are [specification §11.0](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration), [#39](https://github.com/skeet70/latent-image/issues/39), [#85](https://github.com/skeet70/latent-image/issues/85), E-AF-CARDLESS-S1-BLOCKED-150 and E-AF-CARDLESS-S1-PREDICATE-150. The failed S1 attempt has no post-attempt RAM or console capture, so Run 3 must not be presented as that attempt's state.

## 1. What makes CARD ERROR instead of NO CARD

### 1.1 The body sampled present without a card

Input row 8 at `0x00828200` describes `KEY_CARD_DET`, input code `0x11`, scanner kind 2, port 12 bit 7 and asserted level 0. `0x006a0888` evaluates kind 2 by reading `0xfff89000 + port*0x40 + 0x14`, comparing the bit with row halfword `+0x12`, and storing the result at row byte `+0x16`. The scanner at `0x006a0a78` also retains the prior result at `+0x17`.

| Run-3 field | Observed value | Meaning and limit |
| --- | ---: | --- |
| Card-detect asserted-level halfword, `0x00828212` | 0 | Low means present in this input row |
| Current and previous scanner bytes, `0x00828216` / `0x00828217` | 1 / 1 | Both retained evaluations are asserted; the stock decode initializes both to 0 |
| Port-12 startup mirror, alias `0x400a0030`, physical `0x000a0030` | `0x00000028` | Bit 7 is low, agreeing with present; this mirror is not a simultaneous live MMIO read |
| Media device type, `0x00917f6c` | 6 | Selects the direct SD GPIO reader in `0x006eddc0` |
| Raw SD status flags, `0x009738c4` | `0x10` | SD error flag set; absent flags `0x1` / `0x2` clear |
| Filesystem media flags, physical `0x021808f8` and `0x0217a3d8` | `0x2` / `0x2` | Matching device-object and exported filesystem error classification |
| Media status, `0x00918af8` | 4 | CARD ERROR classification |
| Cached warning and active mask, `0x00950114` / `0x00950118` | `0x13` / `0x00080000` | CARD ERROR is active, not only a historical label |

The row observation is evidence of the software-sampled input, not proof of a permanently stuck mechanical switch, the damaged conductor, or the electrical cause. Issue #85's reported broken pin does not identify the detect-switch net. Contact damage, a switch held closed, a short or another board-level fault remain physical diagnoses, not firmware findings. Nevertheless, Run 3 contains evidence for a present indication despite the reported empty slot; media status 4 alone would not have proved that.

### 1.2 From detection to the SD initialization failure

`0x006eddc0(device)` dispatches by the type at `0x00917f6c + device*0x14`. For type 6 it returns twice GPIO12.7: low returns 0, high returns 2. In `0x006f4d24`, result 2 takes the absent branch `0x006f68bc(0)`; result 0 enters the type-6 initializer `0x006faa38`.

The SD status setter `0x006f68bc` operates on `0x009738c4`:

| Setter argument | Effect |
| ---: | --- |
| 0 / 1 | OR absent/unavailable bits `0x1` / `0x2` |
| 2 | Clear all status bits after successful initialization |
| 3 | OR `0x4` |
| 4 | OR `0x8` |
| 5 | OR `0x10` on initialization/error branches |

`0x006faa38` checks the initialized latch at `0x008fc7d0`, performs controller/card setup and negotiation, and calls the setter with 5 on several failures. It sets the initialized latch to 1 only on its successful path. Run 3 has that latch 0 and raw status `0x10`. These support failed initialization after the present indication. They do not isolate which command, controller wait or negotiation step failed, and do not establish a card transaction transcript.

`0x00709b10` then clears its device-object flags at `object+0x78` and translates the exported raw status: raw bits `0x3` become filesystem flag `0x1`; raw `0x10` becomes flag `0x2`; raw `0x8` becomes flag `0x4`; raw `0x4` becomes flag `0x10`. The earlier tests take precedence. `0x006f52e4 -> 0x006f6e94` exports the raw SD status block. `0x00701090` reads the device-object status, and `0x0071d66c` copies its flags into the filesystem snapshot.

Run 3 resolves `0x008fc87c = 0x42180880`, making device 0's `+0x78` physical `0x021808f8`, and `0x00910204 = 0x42163800`, making its exported `+0x16bd8` physical `0x0217a3d8`. Both contain 2. This closes the observed error classification through the alias translation; it is not a guessed physical address.

### 1.3 Media status and warning are separate latches

The destination block is registered by `0x00bfb124 -> 0x00db4b34 -> 0x00db4af4`, queue-2 opcode 1. `0x00ec0ab8` stores that request, `0x00ec56b4 -> 0x00ec08b8` registers its media pointer at `0x015b8074`, and setter `0x00ec0528` writes pointer `+0x40`. Run 3 contains `0x015b8074 = 0x00918ab8`, binding that destination to `0x00918af8`.

`0x00ec6988` queries `0x007101e4(0, out)`, which reads the filesystem snapshot, and passes the first word through `0x00ec6934` into `0x00ec0528`. Targeted instructions at `0x00ec69d8`-`0x00ec69e0` verify this forwarded return value; the decompiler omits the setter argument.

| Filesystem flags tested in order | Resulting media status | Warning from `0x00d0c87c` |
| --- | ---: | --- |
| Bit `0x1` set | 2 | `0x11`, NO CARD |
| Else any `0x6` set | 4 | `0x13`, CARD ERROR |
| Else bit `0x8` set | 3 | `0x12`, CARD NOT INITIALIZED |
| Else | 1 | No warning selected by this mapper |

The CARD ERROR warning is activated by `0x00d3bc34 -> 0x00632478(id,0x80)`. Its active bitset and presentation cache are not the source of the SD error. Stock clearing exists: `0x00d3bb54` clears an individual warning, `0x00d4f3c4` clears media warnings, and `0x00d4f4a8` immediately reselects NO CARD / CARD NOT INITIALIZED / CARD ERROR from the media status. Application-mode commit `0x0063975c` also calls warning reset `0x0063266c`. Clearing or dismissing the label therefore does not repair the producer, and a momentarily cleared warning is not a stable NO CARD state.

Do not confuse neighboring word `0x00918afc` with the media status. It is separately maintained, including by `0x00d0af94` and setter `0x00ec055c`; its value is not the input to `0x00d0c87c`.

## 2. Does a stock transition back to NO CARD exist?

**Yes, conditional on the detect input becoming absent. No software-only workaround for the captured present indication is established.** The high GPIO level takes the absent branch before SD initialization, yielding the filesystem absent flag that maps to media status 2. The common still-S1 predicate does not reject warning `0x11`, although other lens/mode/inhibition checks and USB-attached AF remain separate.

The stock card-edge handler `0x006a1c50` handles codes `0x11` and `0x91` by calling `0x0071f68c` and clearing the initialized latch through `0x006fae5c`. Thus a real detect transition has a stock re-detect/reinitialization path. A fresh ordinary boot also starts the initialization decision from its sampled hardware condition. Neither path guarantees NO CARD while GPIO12.7 remains low. Repeating boots or switching shooting modes is not a demonstrated fix for this body, and no mode-specific menu option in the inspected path substitutes absent for present. Video also explicitly refuses NO CARD in its S1 predicate.

Restoring the electrical absent indication would let stock firmware take that route. How to restore it is physical repair work, not a proposed contact manipulation, card insertion, switch-short or GPIO write. The slot remains unavailable for save/playback and SD recovery even if a detect repair alone permits no-card AF.

### Ready H1 plan: idle detect and media state, two passes

The ready plan is `private-data/read-plans/cardless-detect-idle.json`. Its contents are reproduced below because private plan files are ignored by Git. It uses the existing closed `read-ram` operation: identity and stored-gate queries, then five RAM windows per pass, two passes with a 1000 ms pause between the last reply of pass 0 and the first request of pass 1. It performs ten RAM reads, transferring 44 payload bytes. The transport's aggregate timeout budget is 281,000 ms, including that pause; it never retries. This is an H1 read/query plan, not an AF or capture operation.

```json
{"windows":[{"index":0,"address":"0x00828216","length":2,"purpose":"KEY_CARD_DET current and previous asserted-state scanner bytes"},{"index":1,"address":"0x400a0030","length":4,"purpose":"port-12 boot input mirror; bit7 is active-low card detect, not live MMIO"},{"index":2,"address":"0x009738c4","length":4,"purpose":"raw SD initialization/status flags"},{"index":3,"address":"0x00918af8","length":4,"purpose":"registered media status"},{"index":4,"address":"0x00950114","length":8,"purpose":"cached warning and first active-warning mask word"}]}
```

All windows lie in previously captured DRAM or its uncached alias. The mirror is RAM at physical `0x000a0030`, not the live GPIO register `0xfff89314`. Reading it twice does not cause the firmware to refresh it. The scanner bytes provide the current software-sampled detect observation; the mirror supplies the boot comparison. Each pass and each window can still be torn relative to running firmware, so two agreeing passes support a stable sampled state, not an atomic trace or proof of a permanently stuck contact.

#### Conditions and commands

The owner decides whether to run this plan. This draft does not invoke the standing read grant or authorize a run. If chosen, the owner leaves the body idle at the ordinary shooting screen with the slot empty and untouched, attaches USB through the already established ordinary-boot `ff80` route, and gives a go naming this plan and its two-pass limit. Do not enter jig/service imaging, change modes, touch the slot, press S1/S2, or write any gate/configuration/media word. The existing tool requires exactly one `04cb:ff80` device, X-M1 identity and a nonzero stored `0xf7`; refusal is not a reason to change those conditions.

Host-only rehearsal command, which does not open USB:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-CardlessCapture" \
  run --jobs=4 --local_resources=memory=6144 //transport:camera -- \
  rehearse read-ram "$PWD/private-data/read-plans/cardless-detect-idle.json" \
  --repeat 2 --interval-ms 1000
```

Exact live command, only after the owner's decision and the required host rehearsal:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-CardlessCapture" \
  run --jobs=4 --local_resources=memory=6144 //transport:camera -- \
  read-ram "$PWD/private-data/read-plans/cardless-detect-idle.json" \
  --repeat 2 --interval-ms 1000
```

The tool creates a journaled session under `private-data/sessions/`. Decode the two scanner bytes in file order and the remaining fields as little-endian 32-bit words. Each pass is stored as `ram/000-ADDRESS.bin` or `ram/001-ADDRESS.bin`; the warning file contains the cached id followed by the first active-mask word. The session's `pass-start` and `ram-read` records supply timing and window identities. A normal result is `read-ram: 10 read requests;passes=2;no write sent, so the session needs no supply cut`. This is an expected result, not an observed run.

#### Interpretation

| Field | Stable present and initialization error, as in Run 3 | Stable absent, after media refresh |
| --- | --- | --- |
| Scanner current / previous, `0x00828216` | `01 01` | `00 00`; a differing pair indicates an edge or unsettled sampling |
| Port-12 RAM mirror bit 7, `0x400a0030` | Clear; Run 3 word `0x28` | Set if the boot mirror recorded absence; do not require it to track a later transition |
| Raw SD flags, `0x009738c4` | Run 3 `0x10` | Absent branch sets bit `0x1`; other retained bits can coexist, so do not require exactly `0x1` |
| Media status, `0x00918af8` | 4 | 2 |
| Cached warning, `0x00950114` | `0x13`, CARD ERROR | `0x11`, NO CARD, if no higher-priority active warning wins presentation |
| Active mask, `0x00950118` | Bit `0x00080000` set for `0x13` | Bit `0x00020000` set for `0x11`, with CARD ERROR bit clear after warning refresh; unrelated warning bits may coexist |

Stop after the two passes. Any refused reply, timeout, disconnect, unexpected indication or reported behavior change stops further reads and is reported to the owner. Do not automatically re-run, clear warnings, reset the media state, change modes or press the shutter to force a preferred result.

Two agreeing present/error passes would confirm that the current software scanner still sees card-present on the empty broken slot. That leaves physical detect repair versus a separately reviewed software admission route for the owner's decision; it does not identify the damaged contact. Two absent/NO CARD passes would establish the stock media condition for a separately approved S1-only observation, not AF success. If the row is absent but status remains 4, or the two passes disagree, retain the evidence and investigate refresh/timing before selecting a route. No live check was performed to prepare this plan.

## 3. Full-resolution capture-to-RAM

### 3.1 Buffers and processing are present, ownership is not an address

The shared recorder/frame/develop map is documented in [service-shoot feasibility](service-shoot-feasibility.md#what-the-jobs-reach-and-what-returns) and [service rendering](film-simulation.md#service-rendering-without-firmware-writes). It supplies the following boundaries, not a complete ordinary shutter-to-RAM transaction:

| Object / boundary | X-M1 binding | What it establishes |
| --- | --- | --- |
| Per-frame record | `0x00e87ad4(slot)` returns `[0x0157dd3c] + slot*0x326c`, for `slot < 0xc9` | Finite slot namespace, not 201 simultaneous allocated frames |
| Input descriptor | Record `+0x8c` / `+0x90` | A source base/extent used by diagnostic `0xa6`; not a classified fresh full-resolution exposure |
| Processing/output descriptor | Record `+0x184` family, including base `+0x188`, height `+0x190`, width `+0x198` | Diagnostic `0xa5` / `0xa7` publisher computes one or two bytes per pixel depending on `+0x19b4`; the fields do not prove Bayer/RGB/YUV packing |
| Another output pair | Record `+0x2928` / `+0x292c` | Published for `0xa8`; codec/format and freshness are not established |
| Developed output slots | `0x00e883f0(kind,index)`, 0x28-byte slot stride, descriptor at `+0xc`; 17 / 32 slots by kind | `0xa9` publishes descriptor `+4` / `+8` using index at `0x01583e74` |
| Develop parameters | `0x00e55918`, record film field `+0x508`, downstream `0x00d79408` / `0x00d78418` | A shared development path; service-supplied exposure/parameter provenance remains #39 work |
| Pool selection | `0x00e44d6c`, selector `0x015480d4`; Run 3 reads selector 0 and enabled state `0x015480d8 = 1` | Observes the idle catalogue selector, not a capture allocation, DMA completion or physical backing of every catalogue extent |

The [buffer catalogue](ff80-bulk-read.md#71-buffer-pool-descriptors-and-their-consumers) includes mode-0 buffers of `0x01827c00` bytes and `0x0207ca80` bytes, with overlapping ranges and many addresses above the Run-3 capture's physical end `0x05aa0000`. This report narrows the earlier unknown selector to its observed idle value 0. It does not show the next capture's selected descriptor, or establish contiguous populated DRAM in `[0x05aa0000,0x0b2ece00)`.

### 3.2 Where processing and storage diverge

Ordinary acquisition, image processing, encoded-file construction, media permission, save completion and frame cleanup are separate boundaries. The frame interface has a distinct `0x00e67750` request named `Reccf_SvPermitMediaWrite`, posting message `0x1367`. Its consumer family includes `0x00e7ee84` and `0x00e77168`, which resolves the frame associated with that permit. The develop task dispatcher `0x00ea0934` has an explicit wait mechanism `0x00e9e038`; diagnostics in `0x00e9db80` distinguish wait/cancel state `0xec` as `Wait Save` / `Cancel Save`. These identify storage-related scheduling beyond producing pixels. They do not establish that the ordinary save completion can be omitted.

The diagnostic branch is observable at `0x00e51858`: service selectors `0xa5`, `0xa7` and `0xa9` choose modes 1, 2 and 3; ordinary processing chooses 0. For example, `0x00e531ac` only runs its ordinary codec setup, starts `0x011d4a20`, and waits for completion flag `0x10` when this selector returns 0. Its diagnostic arm returns a different result through the caller's output. The corresponding `0x00e9a070` and `0x00e9a36c` stage wrappers also query filesystem flags and take a different branch on the absent flag. Merely getting S2 past an outer predicate therefore does not preserve the entire ordinary processing path.

The host result diverges at `0x00e772ec` for `0xa5`-`0xa8`, and `0x00e6f9a0` for `0xa9`: those sites copy selected descriptor fields into `0x015967a8` / `0x015967ac`. Ordinary selection does not publish through these diagnostic cases. The [existing classification](film-simulation.md#service-rendering-without-firmware-writes) identifies `0xa5` as diagnostic preprocessing that omits color/tone stages, `0xa7` as a raw alias, `0xa6` as input/staging, and `0xa9` as the film-enabled developed-image candidate. These are not interchangeable ordinary JPEG or RAF results.

No safe ordinary capture cut point is established between completed exposure/development and the first filesystem mutation. The exact ordinary save action, every downstream file/media write, its cancellation/error completion and the preview-restart transaction have not been closed by this map. Consequently the report does not claim that the service branch is storage-free or that declining `0x1367` would safely finish an ordinary capture. A new implementation must own that boundary and its completion protocol, not make a generic filesystem operation return success.

### 3.3 Can read-bulk get the image before release?

**Transport access exists; a usable ownership interval does not yet.** `read-bulk` reads an admitted physical DRAM range through at most 1 MiB DMA chunks. It does not acquire a frame slot, increment a buffer reference, pause a producer or delay stock cleanup. Reading the published pair twice only detects some changes; it cannot stop release/reuse between chunks. Alias translation changes the address used by DMA, not the producer's ownership or cache contract.

Reachability must be checked per producer, not inferred from the presence of low-DRAM descriptor pointers. The [AF statistic producer map](xm1-snapshot-producers.md#af-statistic-intake-computation-and-completion), E-XM1-AF-PRODUCER-SNAPSHOT-150, binds alternating banks `0x5ffb0dc0` and `0x5ffb3770`, each allocated `0x29b0` bytes. Masking the alias bits yields physical `0x1ffb0dc0` / `0x1ffb3770`, above read-bulk's admitted physical end `0x0b3f6e00`; the bridge's `0x5ffe...` statistic buffers are outside it too. Existing read-bulk cannot directly retrieve those banks. Their addresses do not move the ordinary image-pool catalogue there or establish where a full-resolution output will reside. They establish a separate reachability failure for direct statistic reads and reinforce the need to classify the chosen capture descriptor. Neither raising the transport limit nor calling the bank DRAM supplies backing, ownership or a safe read. An independently reviewed bounded copy into owned, admitted memory would be a new mechanism, not a transport-only flag change.

The cleanup hazard is visible in the publishers themselves. `0x00e772ec` publishes its pair and then reaches conditional buffer cleanup through `0x00e6dae8`, per-frame descriptor cleanup and `0x00e87d80`. `0x00e6dae8` walks the frame's resource entries and ends with `0x00e46128` when its final-clear argument is zero. `0x00e6f9a0` publishes the `0xa9` output and then has output-slot release loops through `0x00e88448`. That release calls `0x00e46090`, returns the descriptor through the pool bookkeeping and clears its descriptor fields. Exact retained resources depend on branch state; publication alone is not a host lease.

`0x00e67a40` also clears the published pair before subsequent service triggers. Neither the service `0x450006` retrieval nor arbitrary debug RAM reading supplies a lifetime proof. A successful read can still fetch freed, partially reused or stale bytes.

Runs 2 and 3 measured approximately 10.5 MB/s and 5.93 MB/s on different static RAM windows. At those rates a `0x01827c00`-byte extent would need about 2.4-4.3 seconds and a `0x0207ca80`-byte extent about 3.2-5.7 seconds, before fixed session overhead. These are transfer-budget estimates, not image-buffer timings or permission to wait inside a stock real-time callback. An owned small intended-region copy may reduce the requirement, but its allocation, copy completion and release must be bound first.

### 3.4 Feasibility and risks

A bounded RAM capture remains a research candidate because stock sensor/develop machinery and a readable RAM transport exist. The present mapping does not show an unmodified ordinary capture completing into an owned RAM-only result with no SD card. It also does not prove that such a transaction is impossible. The missing contract is capture completion and ownership, not another raw read command.

| Required property | Current evidence / blocker |
| --- | --- |
| Fresh exposure with stock exposure and shutter behavior | Frame/job interfaces mapped; no cardless ordinary full-resolution exposure has been observed |
| Classified output with acquisition provenance | Descriptor candidates mapped; active dimensions, packing, processing stage, exposure generation and lens state at acquisition remain unbound |
| Retain through export, then release once | Cleanup functions mapped; no host-owned lease or reviewed independent copy exists |
| Skip SD without fake success | Diagnostic divergence mapped; no completed ordinary no-save transaction or closed storage side-effect graph |
| Bounded cancel/error/host-loss handling | No reviewed capture adapter or transaction-state contract |
| Restore preview and support repetition | No end-to-end cardless capture/release/restart evidence or leak/accumulation observation |

Risks include sensor/shutter sequencing and cancellation, holding capture tasks across a multi-second transfer, pool exhaustion or reference imbalance, reuse during DMA, stale cache lines, format/extent mistakes, overlapping mode-dependent buffers, unmeasured DRAM backing, accidental storage/calibration operations through the shared service selector, and losing the only running session without the SD recovery route. The broken slot is not repaired by an AF-only override or by RAM image export.

The smallest justified next work is to establish whether the intended optical-quality question needs full resolution, then bind one ordinary still frame's producer, finished descriptor, lease/release and no-save completion in host analysis/emulation. Service jobs remain under #39 with fresh exposure, consumed parameters and adjacent storage hazards evaluated separately. No no-card-check bypass, warning reset, media-ready patch, fake write result, capture payload or live service job is supplied here.

## 4. Reproduction

Inputs remain owner-supplied and private. Use the [firmware extraction](firmware-extraction.md) identities and existing Run-3 provenance in [bulk-read outcomes](ff80-bulk-read.md#64-run-3-outcome). The source slices are `private-data/analysis/x-m1-1.50/af/functions/ADDRESS.c`, with the eight-digit address from each function above. Decompiler types and missing forwarded registers are leads; the status setter's forwarded argument was checked with A32 disassembly.

Representative host commands used in this investigation:

```sh
D=private-data/analysis/x-m1-1.50
S=private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin
xxd -g 1 -s 0x828200 -l 28 "$S"
xxd -g 4 -e -s 0xa0030 -l 4 "$S"
xxd -g 4 -e -s 0x9738c4 -l 64 "$S"
xxd -g 4 -e -s 0x21808f8 -l 16 "$S"
xxd -g 4 -e -s 0x217a3d8 -l 64 "$S"
xxd -g 4 -e -s 0x15b8070 -l 16 "$S"
xxd -g 4 -e -s 0x15480d0 -l 24 "$S"
xxd -g 4 -e -s 0x15967a0 -l 24 "$S"
devenv shell -- llvm-objdump -d --print-imm-hex --triple=armv7a-none-eabi \
  --adjust-vma=0xbf1a00 --start-address=0x2d4f34 --stop-address=0x2d5058 \
  "$D/lz/id5.elf"
```

Searches used the existing small function slices, avoiding incomplete first-4-MiB searches of the large decompiler export. This is a distilled address/control-flow report, not redistribution of the firmware source. No camera, USB, physical slot operation or firmware/configuration/calibration write was performed.
